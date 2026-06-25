# husarion_asset_server — runnable provider image (Rust / r2r).
#
# r2r generates typed GetAsset / AssetProviderInfo bindings from a sourced
# husarion_asset_msgs, so the build first colcon-builds the contract, then cargo-
# builds the node. The contract comes from a named build context (no network):
#
#   docker build -t husarion-asset-server \
#     --build-context husarion_asset_msgs=../husarion_asset_msgs .
FROM ros:jazzy-ros-base AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential curl clang libclang-dev python3-colcon-common-extensions \
    && rm -rf /var/lib/apt/lists/*
# husarion_asset_msgs (the contract r2r binds).
WORKDIR /msgs
COPY --from=husarion_asset_msgs . src/husarion_asset_msgs
RUN bash -c "source /opt/ros/jazzy/setup.bash && \
        colcon build --packages-select husarion_asset_msgs --merge-install"
# Rust toolchain + the node.
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN bash -c "source /opt/ros/jazzy/setup.bash && \
        source /msgs/install/setup.bash && \
        cargo build --release"

FROM ros:jazzy-ros-base AS provider
COPY --from=build /msgs/install /msgs/install
COPY --from=build /build/target/release/asset_server /usr/local/bin/
COPY docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh
ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["asset_server"]
