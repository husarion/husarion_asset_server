# husarion_asset_server

**A lightweight ROS 2 node that serves a component's `package://` assets — meshes, URDFs, textures — over the ROS graph, and announces which packages it owns.** The reference implementation of the [`husarion_asset_msgs`](https://github.com/husarion/husarion_asset_msgs) standard: run one per robot, sensor, or container, and any conforming client (or a [`husarion_rosbridge`](https://github.com/husarion/husarion_rosbridge) server) can fetch your assets without baking meshes into an image.

## What it does

- Hosts a uniquely-named **`GetAsset`** service. On request it resolves `package://PKG/REL` to a filesystem path via the ament index, reads the requested byte range, and returns it with `total_size` + a `content_hash` so the client can chunk and cache.
- Publishes **`AssetProviderInfo`** latched (TRANSIENT_LOCAL) on `/asset_providers` and re-announces on a heartbeat timer, so a router/bridge converges on the live provider set and drops a crashed provider.
- Owns exactly the packages referenced by the **`robot_description`** it is co-located with (auto-derived by scraping `package://` mesh URIs out of the URDF), or an explicit `owned_packages` override.

### Security (normative)

Only `package://` URIs; no `..` traversal; only packages in the announced owned set; the resolved real path must stay inside the package's share directory.

## Configuration

The provider is a **first-class ROS 2 node**: every knob is a CLI flag, an `ASSET_SERVER_*` env var, **and** a ROS 2 parameter, with precedence **flag > ROS param > env > default**.

| Flag | ROS param / env | Default | Meaning |
| -- | -- | -- | -- |
| `--owned-packages` | `owned_packages` / `ASSET_SERVER_OWNED_PACKAGES` | _(empty)_ | Explicit owned set (comma-separated). When empty, derive from the description. |
| `--description-topic` | `description_topic` / `ASSET_SERVER_DESCRIPTION_TOPIC` | `robot_description` | Latched URDF source for auto-derivation. |
| `--providers-topic` | `providers_topic` / `ASSET_SERVER_PROVIDERS_TOPIC` | `/asset_providers` | Where `AssetProviderInfo` is announced. |
| `--heartbeat` | `heartbeat` / `ASSET_SERVER_HEARTBEAT` | `5.0` | Re-announce period (seconds). |
| `--max-chunk` | `max_chunk` / `ASSET_SERVER_MAX_CHUNK` | `524288` | Response chunk ceiling (keep under the RMW service limit). |
| `--node-name` / `--namespace` | node identity (`-r __node:=` / `-r __ns:=`) | `husarion_asset_server` / _(global)_ | ROS node identity (unique per provider). |

Configuration is startup-only: a runtime `ros2 param set` is logged as "restart to apply".

## Run as a ROS 2 node

Once built into a colcon workspace (see **Build**), the provider drops in as a standard node — `ros2 run`, a launch `Node(...)`, params, and remaps all work:

```bash
# Run it directly (launch `namespace=` lands it on /<ns>/get_asset):
ros2 run husarion_asset_server asset_server \
  --ros-args -r __ns:=/rosbot -p max_chunk:=262144

# Or from a params file / launch file (examples/ ships both, tested per release):
ros2 run husarion_asset_server asset_server \
  --ros-args --params-file examples/asset_server.params.yaml
ros2 launch examples/asset_server.launch.yaml

# Introspect the effective config:
ros2 param list /rosbot/husarion_asset_server
ros2 param get  /rosbot/husarion_asset_server heartbeat
```

### Add it to your own launch file (YAML)

Drop this `node` block into your robot's bringup launch to run the provider alongside the rest of your stack. It lands on `/<namespace>/get_asset` and auto-derives its owned packages from `/<namespace>/robot_description`, so it just works next to your `robot_state_publisher`:

```yaml
# my_bringup.launch.yaml
launch:
  - node:
      pkg: "husarion_asset_server"
      exec: "asset_server"
      name: "husarion_asset_server"
      namespace: "/rosbot"        # same namespace as your robot_state_publisher
      output: "screen"
      param:
        # Leave owned_packages unset to auto-derive from the description, or pin it:
        # - name: "owned_packages"
        #   value: ["husarion_ugv_description"]
        - name: "heartbeat"
          value: 5.0
        - name: "max_chunk"
          value: 524288
```

Then `ros2 launch my_bringup.launch.yaml`. The package must be on your `AMENT_PREFIX_PATH` — build it into your workspace (see **Build**), or run the published image, which *is* the ament-installed node:

```bash
# The image is a standard ROS 2 node — run it, or `ros2 run`/`ros2 launch` inside
# it. NB: the image ships mesh-less; see "Run in Docker" for providing the assets.
docker run --rm --network host --ipc host --env-file /etc/husarion/ros.env \
  husarion/asset-server:latest \
  asset_server --ros-args -r __ns:=/rosbot
```

`examples/` ships a ready-to-run `asset_server.launch.yaml` + `asset_server.params.yaml`.

## Run

```bash
# Auto-derive ownership from a co-located robot_state_publisher's /robot_description:
asset_server

# Or pin the owned set explicitly:
asset_server --owned-packages rosbot_description,husarion_components_description
```

See **Configuration** above — the same knobs are available as CLI flags, `ASSET_SERVER_*` env vars, and ROS 2 parameters.

Fetch an asset (the client chunks via `offset`/`length`; `0,0` = whole asset):

```bash
ros2 service call /husarion_asset_server/get_asset husarion_asset_msgs/srv/GetAsset \
  "{uri: 'package://rosbot_description/meshes/rosbot_xl/body.dae', offset: 0, length: 0}"
```

## Run in Docker — serving assets from an existing robot image

The published image (`husarion/asset-server:X.Y.Z`, multi-arch) is **universal and mesh-less**: the ament-installed node plus every selectable RMW (cyclonedds / fastrtps / zenoh), but **no robot descriptions** — on its own it has nothing to serve. You provide the assets one of two ways.

### 1. Layer them from your robot's image (the deployment template)

If your driver image already carries the description packages, overlay them onto the provider with a few-line Dockerfile: a **donor stage dereferences the share dirs (`cp -rL`) into one `/assets` prefix**, and the final stage copies real files. The dereference matters — a `--symlink-install` colcon tree (e.g. the `husarion/rosbot` image) holds symlinks into its `src/` tree, and a plain `COPY --from` would carry them over dangling. **This is the template the [husarion-cockpit](https://github.com/husarion/husarion-cockpit) Panther/Lynx deployment uses** — keep the arg names if you adapt it.

**Panther / Lynx** — `husarion/husarion-ugv` ships a *merged* install, so one `cp` of the whole share dir suffices:

```dockerfile
ARG ASSET_SERVER_IMAGE=husarion/asset-server:latest
ARG DRIVER_IMAGE=husarion/husarion-ugv:jazzy-<tag>
FROM ${DRIVER_IMAGE} AS donor
RUN mkdir -p /assets && cp -rL /ros2_ws/install/share /assets/share
FROM ${ASSET_SERVER_IMAGE}
COPY --from=donor /assets /assets
# The entrypoint's ROS sourcing PREPENDS to an existing AMENT_PREFIX_PATH, so
# this survives and package:// resolves against /assets/share/<pkg>:
ENV AMENT_PREFIX_PATH=/assets
```

**ROSbot / ROSbot XL** — `husarion/rosbot` ships an *isolated*, `--symlink-install` tree (per-package prefixes), so the description packages are dereferenced one by one; the jazzy `rosbot_description` carries the meshes for **both** ROSbot 2R and ROSbot XL, so this one donor covers both robots. Verified end-to-end against `husarion/rosbot:jazzy-1.1.1-20260629`: `asset_conformance` **9/9**, plus ranged fetches of `meshes/rosbot_xl/body.dae` (COLLADA, 2.4 MB) and `meshes/rosbot/body.glb` (glTF binary, 1.4 MB):

```dockerfile
ARG ASSET_SERVER_IMAGE=husarion/asset-server:latest
ARG DRIVER_IMAGE=husarion/rosbot:jazzy-1.1.1-20260629
FROM ${DRIVER_IMAGE} AS donor
RUN mkdir -p /assets/share && \
    cp -rL /ros2_ws/install/rosbot_description/share/rosbot_description /assets/share/ && \
    cp -rL /ros2_ws/install/husarion_components_description/share/husarion_components_description /assets/share/
FROM ${ASSET_SERVER_IMAGE}
COPY --from=donor /assets /assets
ENV AMENT_PREFIX_PATH=/assets
```

Run it (`--ipc host` matters: Fast DDS uses shared memory between same-host participants, which dies across container IPC namespaces):

```bash
docker build -t rosbot-asset-server .
docker run -d --network host --ipc host rosbot-asset-server \
  asset_server --owned-packages rosbot_description,husarion_components_description \
  --ros-args -r __ns:=/rosbot
# From any node on the graph:
ros2 service call /rosbot/husarion_asset_server/get_asset husarion_asset_msgs/srv/GetAsset \
  "{uri: 'package://rosbot_description/meshes/rosbot_xl/body.dae', offset: 0, length: 0}"
```

built by the compose file that runs it:

```yaml
services:
  asset-server:
    image: my-robot-asset-server:jazzy-<tag>   # local tag — built, never pulled
    build:                                     # ← BY DESIGN, see below
      context: ./asset-server
      args:
        DRIVER_IMAGE: husarion/husarion-ugv:jazzy-<tag>   # = the driver this stack runs
        ASSET_SERVER_IMAGE: husarion/asset-server:X.Y.Z
    network_mode: host
    ipc: host
    env_file: [./ros.env]    # same RMW / namespace / domain as the driver
    restart: unless-stopped
```

**The `build:` in compose is by design — not a missing published image.** The meshes must come from the *exact driver image the robot runs* (they are that image's description packages), while the provider version is independent — so a registry would need every (driver × asset-server) combination, an N×M image matrix nobody should maintain. Instead the deploy combines the two images it has **already pulled** in a seconds-long, two-`COPY` local build: both `FROM`s resolve from the local image store, so it works fully **offline** (air-gapped installs included). Bump `DRIVER_IMAGE` in lockstep with the driver tag.

### 2. Bind-mount an install tree

If the descriptions live on the host (a colcon workspace, an unpacked package), mount them and point `AMENT_PREFIX_PATH` at the prefix:

```bash
docker run --rm --network host --ipc host --env-file /etc/husarion/ros.env \
  -v /opt/robot_ws/install:/assets:ro -e AMENT_PREFIX_PATH=/assets \
  husarion/asset-server:latest asset_server
```

Either way the provider is namespaced like any node (`-r __ns:=/rosbot`, or `ROS_NAMESPACE` from the env file) and auto-derives its owned packages from the co-located `/<ns>/robot_description`.

## Build

A Rust / [r2r](https://github.com/sequenceplanner/r2r) node — it hosts a **typed** `GetAsset` service, so r2r generates the bindings from a sourced `husarion_asset_msgs` (no untyped-server FFI). Build the contract, then the node:

```bash
# 1) the message contract (colcon, into a sourced workspace)
colcon build --packages-select husarion_asset_msgs   # from a ws with husarion_asset_msgs
source install/setup.bash
# 2) the node (cargo, with ROS + the contract sourced)
cargo build --release
```

For `ros2 run` / launch, build it as the **ament_cargo** package it ships as (`package.xml`) — it installs the binary where `ros2 run husarion_asset_server asset_server` resolves it:

```bash
# needs cargo-ament-build (cargo install cargo-ament-build) + colcon-ros-cargo
colcon build --packages-select husarion_asset_server
source install/setup.bash
```

Or just build the image (the `husarion_asset_msgs` contract comes from a build context, no network):

```bash
docker build -t husarion/asset-server \
  --build-context husarion_asset_msgs=../husarion_asset_msgs .
docker run --rm --network host --ipc host --env-file /etc/husarion/ros.env \
  husarion/asset-server asset_server --owned-packages rosbot_description
```

The image is **universal + mesh-less** (all RMWs, no robot descriptions): a deploy layers the driver's meshes on top. It's published multi-arch to Docker Hub as `husarion/asset-server:<version>` + `:latest` by `.github/workflows/image.yml` on a `vX.Y.Z` tag.

### Why Rust (r2r) — build & distribution FAQ

Fair questions come up about a Rust node in a C++-dominated ROS 2 world. The short answers:

- **Nobody downstream ever runs cargo.** Both distribution channels ship compiled artifacts: the multi-arch Docker image above, and the prebuilt `asset_server` binaries (amd64 + arm64) attached to every GitHub Release — the rosbot snap fetches those directly, with no in-snap compile. The Rust toolchain exists only inside the `just check` / `Dockerfile` build containers; the dev host needs Docker, not cargo.
- **No apt package — by design, not omission.** `bloom` cannot release cargo packages into the ROS apt repos today, so a `ros-jazzy-husarion-asset-server` deb is not possible; that is a real, known gap in ROS 2's Rust story. This node was never distributed via apt, though: its deploy surface is the image + release binaries. If a deb channel ever becomes a hard requirement, that ecosystem gap — not performance — is the honest cost of Rust here.
- **The client library is [r2r](https://github.com/sequenceplanner/r2r), not `rclrs`.** r2r binds directly to `rcl` and the rosidl typesupport and builds with plain cargo against a sourced ROS env (plus optional ament_cargo integration for `ros2 run`). Benchmarks of `rclrs` — a different client library with a different executor — don't transfer to this node.
- **Per-message latency is not a factor for this node.** `GetAsset` is a cold-path, on-demand chunked file service hit when a client fetches meshes (typically once, at startup): the work is disk I/O plus DDS transfer of megabyte-sized chunks, so a sub-millisecond client-library overhead disappears behind moving one chunk of one 2.4 MB mesh. For hot-path nodes (tf pipelines, control loops) client-library latency is a legitimate selection criterion — this is not one of those nodes.
- **`husarion_asset_msgs` lives in its own repo on purpose.** It is the wire contract that multiple parties implement (this reference provider, the `husarion_rosbridge` client, any third-party provider) — the contract must be dependable without pulling in one vendor's implementation, mirroring how ROS keeps `common_interfaces` separate from the nodes that use them.

## License

Apache-2.0.
