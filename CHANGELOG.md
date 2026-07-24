# Changelog

All notable changes to `husarion_asset_server`. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this crate uses [Semantic Versioning](https://semver.org/).

A `vX.Y.Z` tag (cut via `just release`) triggers two workflows: `release.yml` publishes prebuilt `asset_server` binaries (amd64 + arm64) on the GitHub Release — consumed by the rosbot snap via fetch-by-version, so the snap never compiles the r2r node from source — and `image.yml` publishes the universal provider image `husarion/asset-server:X.Y.Z` + `:latest` to Docker Hub. An `## [Unreleased]` section here is folded into the release section automatically by `just release`.

## [Unreleased]

### Fixed

- **`asset_server` now handles SIGTERM/SIGINT and exits cleanly.** The container entrypoint execs the node as in-namespace PID 1, which gets no default signal dispositions — an unhandled SIGTERM was silently discarded, so `docker stop` (and every host shutdown, e.g. a robot power-button poweroff) hung for the full kill timeout before SIGKILL. Found live on a Lynx, where the provider added ~90 s to `systemd-shutdown` and pushed the poweroff into the power board's hard-cut window.

## [0.3.0] — 2026-07-22

### Added

- **First-class ROS 2 node.** `asset_server` now ships as an **ament_cargo** package (`package.xml`), so it drops into a colcon workspace and runs as `ros2 run husarion_asset_server asset_server` or from a launch `Node(...)`. A trailing `--ros-args` block (remaps, params, log config) appended by `ros2 run` / launch is stripped before argument parsing, and launch `namespace=` / `name=` (i.e. `-r __ns:=…` / `-r __node:=…`) are honored.
- **Every operator knob is also a ROS 2 parameter** (`owned_packages`, `description_topic`, `providers_topic`, `heartbeat`, `max_chunk`) — set from a launch `parameters=`, a `--params-file`, or `-p name:=value`. Precedence is **flag > parameter > `ASSET_SERVER_*` env > default**. The node serves the standard parameter services and publishes the resolved values back, so `ros2 param list/get/describe` reflect what the provider is running; configuration is startup-only (a runtime `ros2 param set` is logged as "restart to apply"). Example params + launch files ship in `examples/`.
- **Universal ROS 2-native provider image** (`husarion/asset-server:X.Y.Z` + `:latest`, multi-arch on Docker Hub). The runtime is `ros:jazzy-ros-core` plus all three RMWs (CycloneDDS / Fast DDS / Zenoh), and the entrypoint sources the ament prefix, so `ros2 run` / `ros2 launch` work inside the container. **No robot descriptions are baked in** — the deploy layers the driver's `share/<desc-pkg>` onto `AMENT_PREFIX_PATH`, so one image serves any robot, independent of driver version.

### Changed

- **Cargo package renamed `husarion-asset-server` → `husarion_asset_server`** so the crate name equals the ament `package.xml` name (which can't contain dashes) — required for `ros2 run` / colcon resolution. The **binaries** (`asset_server`, `asset_conformance`) keep their names, so the rosbot snap's fetch-by-version path is unaffected. `package.xml`'s `<version>` is hand-maintained and now bumped in lockstep by `just release`.
- **`just check` is now the full Docker gate** — fmt, clippy (`-D warnings`), tests, the ament_cargo colcon build, and `ros2 pkg` resolution all run in a cached `has-builder:jazzy` container; no host ROS or cargo needed.

### Fixed

- **Build/runtime rosidl symmetry.** r2r links the typesupport of every rosidl package on `AMENT_PREFIX_PATH` at build time, so both Dockerfile stages (and CI) now build on `ros:jazzy-ros-core` — building on ros-base linked `rosbag2_interfaces` + `tf2_msgs` and the binary then failed to load on a core runtime. (`release.yml` deliberately keeps ros-base: the rosbot snap's runtime provides that superset.)
- Conformance docs now point at the open [`husarion_asset_msgs`](https://github.com/husarion/husarion_asset_msgs) standard's **Conformance** section instead of a dangling SPEC §16 reference.

## [0.2.0] — 2026-06-26

### Added

- `asset_conformance` provider conformance suite — an untyped `GetAsset` client that validates a live provider against the [`husarion_asset_msgs`](https://github.com/husarion/husarion_asset_msgs) standard (its **Conformance** section), passing 9/9 checks.
- Binary release pipeline: `just release` cuts a `vX.Y.Z` tag that publishes prebuilt `asset_server` binaries (amd64 + arm64) on the GitHub Release, so the rosbot snap fetches by version instead of compiling.

### Fixed

- Resolve the namespaced `{node_fqn}/get_asset` service correctly and drive sync `GetAsset` calls via spin.
- Wait for provider service discovery before running conformance checks.
