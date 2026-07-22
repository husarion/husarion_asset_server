# Changelog

All notable changes to `husarion_asset_server`. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this crate uses [Semantic Versioning](https://semver.org/).

A `vX.Y.Z` tag (cut via `just release`) publishes prebuilt `asset_server`
binaries (amd64 + arm64) on the GitHub Release — consumed by the rosbot snap via
fetch-by-version, so the snap never compiles the r2r node from source.

## [Unreleased]

### Added
- **First-class ROS 2 node.** `asset_server` now ships as an **ament_cargo** package (`package.xml`), so it drops into a colcon workspace and runs as `ros2 run husarion_asset_server asset_server` or from a launch `Node(...)`. A trailing `--ros-args` block (remaps, params, log config) appended by `ros2 run` / launch is stripped before argument parsing, and launch `namespace=` / `name=` (i.e. `-r __ns:=…` / `-r __node:=…`) are honored.
- **Every operator knob is also a ROS 2 parameter** (`owned_packages`, `description_topic`, `providers_topic`, `heartbeat`, `max_chunk`) — set from a launch `parameters=`, a `--params-file`, or `-p name:=value`. Precedence is **flag > parameter > `ASSET_SERVER_*` env > default**. The node serves the standard parameter services and publishes the resolved values back, so `ros2 param list/get/describe` reflect what the provider is running; configuration is startup-only (a runtime `ros2 param set` is logged as "restart to apply"). Example params + launch files ship in `examples/`.

### Changed
- **Cargo package renamed `husarion-asset-server` → `husarion_asset_server`** so the crate name equals the ament `package.xml` name (which can't contain dashes) — required for `ros2 run` / colcon resolution. The **binaries** (`asset_server`, `asset_conformance`) keep their names, so the rosbot snap's fetch-by-version path is unaffected. `package.xml`'s `<version>` is hand-maintained and now bumped in lockstep by `just release`.

## [0.2.0] — 2026-06-26

### Added
- `asset_conformance` provider conformance suite — an untyped `GetAsset` client that validates a live provider against the [`husarion_asset_msgs`](https://github.com/husarion/husarion_asset_msgs) standard (its **Conformance** section), passing 9/9 checks.
- Binary release pipeline: `just release` cuts a `vX.Y.Z` tag that publishes prebuilt `asset_server` binaries (amd64 + arm64) on the GitHub Release, so the rosbot snap fetches by version instead of compiling.

### Fixed
- Resolve the namespaced `{node_fqn}/get_asset` service correctly and drive sync `GetAsset` calls via spin.
- Wait for provider service discovery before running conformance checks.

