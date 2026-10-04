# husarion_asset_server — universal, ROS 2-native provider image.
#
# The image is built as a STANDARD ROS 2 node (ament_cargo): colcon installs the
# `asset_server` executable into an ament prefix, so `ros2 run
# husarion_asset_server asset_server`, `ros2 launch`, and a launch `Node(...)`
# all work against the shipped image. r2r generates the typed GetAsset /
# AssetProviderInfo bindings from a sourced husarion_asset_msgs, which comes from
# a named build context (no network):
#
#   docker build -t husarion/asset-server \
#     --build-context husarion_asset_msgs=../husarion_asset_msgs .
#
# UNIVERSAL + MESH-LESS by design: the runtime carries every RMW the cockpit can
# select (cyclonedds / fastrtps / zenoh) but NO robot descriptions — the driver's
# meshes are layered on top at deploy time (see the cockpit's asset-server
# combine). So this one image resolves package:// for any robot once its
# descriptions are added, independent of the driver version.

# ---- build: SAME baseline as the runtime (ros-core) + build tooling -----------
# INVARIANT: build and runtime must expose the SAME rosidl package set. r2r binds
# and LINKS the typesupport of every rosidl package on AMENT_PREFIX_PATH at build
# time — building on ros-base would link rosbag2_interfaces + tf2_msgs (the
# base-vs-core delta, verified against the jazzy images), which a ros-core
# runtime lacks, and the binary fails to load. ros-base has no build tooling
# advantage anyway (colcon/clang are apt-installed either way).
FROM ros:jazzy-ros-core AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential curl clang libclang-dev git \
        python3-colcon-common-extensions python3-pip \
    && rm -rf /var/lib/apt/lists/*

# husarion_asset_msgs (the typed contract r2r binds).
WORKDIR /msgs
COPY --from=husarion_asset_msgs . src/husarion_asset_msgs
RUN bash -c "source /opt/ros/jazzy/setup.bash && \
        colcon build --packages-select husarion_asset_msgs --merge-install"

# Rust toolchain + the ament/colcon cargo plumbing so the node builds as a
# standard ament_cargo package (colcon installs the executable into an ament
# prefix, exactly like any ROS 2 node). The ament_cargo build needs
# cargo-ament-build + colcon-cargo/colcon-ros-cargo; r2r binds msgs via its own
# build.rs bindgen, so none of the rosidl_generator_rs machinery a typed rclrs
# node needs is required here.
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-ament-build && \
    pip3 install --no-cache-dir --break-system-packages --ignore-installed \
        colcon-cargo colcon-ros-cargo

# The repo is one ament_cargo package (package.xml at the root). Copy only build
# inputs, under src/, so doc/entrypoint/CI edits don't bust the cargo cache.
WORKDIR /ws
COPY Cargo.toml Cargo.lock package.xml ./src/husarion_asset_server/
COPY src ./src/husarion_asset_server/src
# Source ROS + the msg contract so r2r's build.rs binds GetAsset/AssetProviderInfo,
# then colcon-build + install the ament_cargo package into its own ament prefix
# (installs asset_server + asset_conformance to lib/husarion_asset_server/).
RUN bash -c "source /opt/ros/jazzy/setup.bash && \
    source /msgs/install/setup.bash && \
    colcon build --packages-select husarion_asset_server \
        --install-base /opt/husarion_asset_server/install \
        --cargo-args --release"

# ---- runtime: ros-core (same rosidl set as the build stage) + every RMW -------
FROM ros:jazzy-ros-core AS provider
# fastrtps is the ros-core default (kept explicit for self-documentation); add
# cyclonedds + zenoh so the provider loads whatever RMW ros.env selects (a
# single-RMW image crash-loops the moment the operator switches). RMW packages
# add no rosidl packages, so build/runtime symmetry holds; std_msgs + the rest
# of the typesupport the binary links is already in ros-core (verified), and
# husarion_asset_msgs comes in via /msgs/install below.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ros-jazzy-rmw-cyclonedds-cpp \
        ros-jazzy-rmw-fastrtps-cpp \
        ros-jazzy-rmw-zenoh-cpp \
    && rm -rf /var/lib/apt/lists/*
# Patched Fast DDS (husarion/fastdds-patched). Stock Fast DDS has a shared-memory
# transport bug (every release since 2.0.0): after two unclean process deaths the
# robot's SHM world stops accepting NEW participants. The fix only holds if EVERY
# process in that world loads the patched library, so this provider carries it
# too. It must come after the RMW install above (which pulls stock
# ros-jazzy-fastrtps), and is held so no later apt step can replace it. Nothing
# in the build stage links Fast DDS (r2r links rcl/rmw_implementation; the msgs
# typesupport links fastcdr only), so the runtime is the only place it is needed;
# the find below fails the build if any other copy exists.
ARG FASTDDS_PATCHED=v2
ARG FASTDDS_SHA256_AMD64=d40d2f66309f36bb7f4fda46626ba4c492347b8e0d340c742703875c16aa74ae
ARG FASTDDS_SHA256_ARM64=267a7b18834309f5ea17888fe37be4ae53371a2ddb8f9950e7c00be1b474fde9
RUN set -eu; arch=$(dpkg --print-architecture); \
    case "$arch" in amd64) sum=$FASTDDS_SHA256_AMD64 ;; arm64) sum=$FASTDDS_SHA256_ARM64 ;; *) exit 1 ;; esac; \
    apt-get update; \
    apt-get install -y --no-install-recommends curl ca-certificates; \
    curl -fsSL -o /tmp/fastdds.deb \
      "https://github.com/husarion/fastdds-patched/releases/download/${FASTDDS_PATCHED}/fastrtps-${ROS_DISTRO}-${arch}.deb"; \
    echo "${sum}  /tmp/fastdds.deb" | sha256sum -c -; \
    apt-get install -y --no-install-recommends /tmp/fastdds.deb; \
    pkg=$(dpkg-deb -f /tmp/fastdds.deb Package); apt-mark hold "$pkg"; rm -f /tmp/fastdds.deb; \
    dpkg-query -W -f='${Version}' "$pkg" | grep -q '+husarion'; \
    dpkg --verify "$pkg"; \
    test "$(find / -xdev \( -name 'libfastrtps.so*' -o -name 'libfastdds.so*' \) -type f -not -path "/opt/ros/${ROS_DISTRO}/lib/*" | wc -l)" = 0; \
    rm -rf /var/lib/apt/lists/*
# The message typesupport r2r links against at runtime + the ament_cargo install
# prefix (the executable + package.xml, so `ros2 run`/`ros2 launch` resolve it).
COPY --from=build /msgs/install /msgs/install
COPY --from=build /opt/husarion_asset_server/install /opt/husarion_asset_server/install
COPY docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh
# The ament exe dir on PATH so the entrypoint can `exec asset_server` directly
# (clean PID-1 signal handling) while it's still a proper ament-installed node.
ENV PATH="/opt/husarion_asset_server/install/husarion_asset_server/lib/husarion_asset_server:${PATH}"
LABEL org.opencontainers.image.source=https://github.com/husarion/husarion_asset_server \
      org.opencontainers.image.description="Universal ROS 2 package:// asset provider (GetAsset); all RMWs, meshes layered at deploy."
ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["asset_server"]
