# husarion_asset_server — AI context

Reference **ROS 2 provider** (Rust / r2r) for the `husarion_asset_msgs` `package://` asset standard. Serves a component's meshes/URDF resources over a typed `GetAsset` service and announces which packages it owns on `AssetProviderInfo`. Two bins: `asset_server` (the node) + `asset_conformance` (an untyped `GetAsset` client = the provider conformance suite).

## First-class ROS 2 node

The repo is one **ament_cargo** package (`package.xml` at the root), mirroring `husarion_rosbridge`'s pattern:

- **The cargo package name MUST equal the `package.xml` `<name>`** (`husarion_asset_server`, no dashes) — ament_cargo installs to `lib/<cargo-name>/` and `ros2 run <pkg> <exe>` resolves `lib/<ament-name>/<exe>`. The **binaries** keep their names (`asset_server`, `asset_conformance`), so the rosbot snap's fetch-by-version is unaffected by the crate rename.
- **`--ros-args` handling** (`src/ros_args.rs`, pure, unit-tested): the launch block is stripped before clap parses, and the `__node` / `__ns` remaps are mirrored into the r2r node identity (launch `name=` / `namespace=` win over `--node-name` / `--namespace` / env).
- **Every knob is a ROS 2 parameter** (`src/params.rs` `KNOBS` registry: `owned_packages`, `description_topic`, `providers_topic`, `heartbeat`, `max_chunk`; param name = env var minus `ASSET_SERVER_`, lowercased). Precedence **flag > param > env > default** via the env-overlay trick: startup overrides are read off `node.params` and written into their env vars *before* the second clap parse. The node serves the standard param services (`make_parameter_handler`, spawned on the same `LocalPool` as the handlers) and publishes resolved values back, so `ros2 param list/get/describe` are accurate. Config is **startup-only** — a runtime `ros2 param set` logs "restart to apply". Examples in `examples/` (launch YAML + params YAML).

## Build model — Docker only, ros-core symmetry

r2r needs a sourced ROS 2 Jazzy env + libclang + the colcon-built `husarion_asset_msgs` contract — there is **no host-cargo path** (the dev host has no ROS, no cargo). Everything builds in Docker:

- **`just check`** — the full gate in a cached `has-builder:jazzy` toolchain container: fmt, clippy `-D warnings`, tests, the ament_cargo colcon build, and `ros2 pkg` resolution. Needs `../husarion_asset_msgs`. `--platform linux/amd64` is forced (this host caches arm64 ros images; QEMU would take hours).
- **`just image`** — the provider image (`husarion/asset-server:dev`).

**INVARIANT (build/runtime symmetry):** r2r binds **and links** the typesupport of every rosidl package on `AMENT_PREFIX_PATH` at build time, so the build and runtime environments must expose the **same rosidl package set**. Both Dockerfile stages and the check gate therefore use `ros:jazzy-ros-core` (building on ros-base links `rosbag2_interfaces` + `tf2_msgs` — the measured base-vs-core delta — and the binary then fails to load on a core runtime). `ci.yaml` runs in `ros:jazzy-ros-core` for the same reason. Exception: `release.yml` builds the standalone binaries in **ros-base** deliberately — the rosbot snap's ros2-extension runtime provides that superset.

## The image — universal + mesh-less

`Dockerfile`: build stage (ros-core + tooling) colcon-builds the ament package; runtime stage = ros-core + **all three RMWs** (cyclonedds / fastrtps / zenoh) + the msgs install + the ament prefix (entrypoint sources it, so `ros2 run` / `ros2 launch` work inside the container). **No robot descriptions baked in** — the deploy layers the driver's `share/<desc-pkg>` on top and extends `AMENT_PREFIX_PATH` (see husarion-cockpit `deploy/husarion-ugv/asset-server/`). One image serves any robot, independent of driver version. Published multi-arch by `.github/workflows/image.yml` (native runners, digest → manifest) as `husarion/asset-server:X.Y.Z` + `:latest`.

## Behaviour + invariants

- **Ownership** — `--owned-packages a,b` (explicit) OR auto-derived from a co-located latched `robot_description` (parse its `package://` URIs). Run **one per published robot_description**, co-located with the description publisher so its packages are on `AMENT_PREFIX_PATH`.
- **Security (don't weaken):** `package://` only · no `..` traversal · owned-set only · resolved realpath confined to the package share dir.
- The service is `{node_fqn}/get_asset` (namespaced via launch `namespace=` / `-r __ns:=` / `ROS_NAMESPACE`); the announce is `/asset_providers` (latched). The bridge reads the real provider name from the announce, not a guess.

## Releasing

`just release` (gated on `just check`) bumps **`Cargo.toml` + `Cargo.lock` + `package.xml` + CHANGELOG in lockstep** (`.release/apply-release.py`; the package.xml `<version>` drifting is the bug husarion_rosbridge hit at 0.8.1), folds an `## [Unreleased]` CHANGELOG section into the release section automatically, tags `vX.Y.Z`, pushes. The tag triggers **two** workflows:

- `release.yml` — prebuilt `asset_server` binaries (amd64 + arm64, native runners, ros-base) on the GitHub Release; **the rosbot snap fetches these** (no in-snap compile; no RPATH — the snap's `LD_LIBRARY_PATH` resolves rcl/rmw and the typesupport). Cut a release **before** rebuilding the snap.
- `image.yml` — the universal provider image to Docker Hub (`DOCKERHUB_USERNAME` / `DOCKERHUB_TOKEN` secrets; also manually runnable via workflow_dispatch with an explicit version).

## Conventions

Commit as the operator with **no `Co-Authored-By` Claude trailer**; never push. `just check` is the local gate (Docker); CI re-runs the same gate per push, and the image build on tag is the final arbiter. Markdown is mdformat-gated (`.mdformat.toml`, `wrap = "no"` — **single-line paragraphs, no hard wraps**; `pre-commit install` to enable), same style as husarion-cockpit.
