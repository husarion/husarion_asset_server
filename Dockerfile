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

# ---- build: ros-base has the build tooling (colcon, libclang for r2r) ---------
FROM ros:jazzy-ros-base AS build
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

# ---- runtime: ros-core is the minimal ROS 2 base; add every selectable RMW ----
FROM ros:jazzy-ros-core AS provider
# fastrtps is the ros-core default; add cyclonedds + zenoh so the provider loads
# whatever RMW ros.env selects (a plain ros-core crash-loops on a non-default
# RMW). std_msgs is the typesupport the description subscription links at runtime
# (ros-core omits it; husarion_asset_msgs comes in via /msgs/install below).
RUN apt-get update && apt-get install -y --no-install-recommends \
        ros-jazzy-rmw-cyclonedds-cpp \
        ros-jazzy-rmw-fastrtps-cpp \
        ros-jazzy-rmw-zenoh-cpp \
        ros-jazzy-std-msgs \
    && rm -rf /var/lib/apt/lists/*
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
