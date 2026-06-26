# husarion_asset_server — AI context

Reference **ROS 2 provider** (Rust / r2r) for the `husarion_asset_msgs`
`package://` asset standard. Serves a component's meshes/URDF resources over a
typed `GetAsset` service and announces which packages it owns on
`AssetProviderInfo`. Two bins: `asset_server` (the node) + `asset_conformance`
(an untyped `GetAsset` client = the provider conformance suite).

## Build model

r2r generates typed `GetAsset`/`AssetProviderInfo` bindings, so the build needs
a **sourced ROS 2 Jazzy env + libclang + the `husarion_asset_msgs` colcon
contract** — there is **no host-cargo path** (the dev host has no ROS). Build it
two ways:

```bash
# Docker (named build context supplies the contract, no network):
docker build -t husarion-asset-server --build-context husarion_asset_msgs=../husarion_asset_msgs .
# CI (ros:jazzy-ros-base): colcon-build the contract, source it, cargo build — see .github/workflows/ci.yaml.
```

## Behaviour + invariants

- **Ownership** — `--owned-packages a,b` (explicit) OR auto-derived from a
  co-located latched `/robot_description` (parse its `package://` URIs). Run
  **one per published robot_description**, co-located with the description
  publisher so its packages are on `AMENT_PREFIX_PATH`.
- **Security (don't weaken):** `package://` only · no `..` traversal · owned-set
  only · resolved realpath confined to the package share dir.
- The service is `{node_fqn}/get_asset` (namespaced via `ROS_NAMESPACE`); the
  announce is `/asset_providers` (latched). The bridge reads the real provider
  name from the announce, not a guess.

## Releasing (binary, consumed by the rosbot snap)

`just release` → bumps `Cargo.toml`/`Cargo.lock` + CHANGELOG (`.release/apply-release.py`),
tags `vX.Y.Z`, pushes. The tag triggers `.github/workflows/release.yml`, which
builds the binary for **amd64 + arm64 on native runners** (in `ros:jazzy-ros-base`)
and attaches `asset_server-<ver>-linux-<arch>` (+`.sha256`) to the GitHub
Release. The **rosbot snap fetches that prebuilt binary** (no in-snap compile) —
the binary has **no RPATH**, so it resolves `librcl`/`rmw`/`rosidl` + the
`husarion_asset_msgs` typesupport via the snap's ros2-extension
`LD_LIBRARY_PATH` at runtime. So: cut a release **before** rebuilding the snap.

## Conventions

Commit as the operator with **no `Co-Authored-By` Claude trailer**; never push.
The full fmt/clippy/test gate runs in CI (needs ROS), so `just release` doesn't
gate locally — CI is the gate on the tag.
