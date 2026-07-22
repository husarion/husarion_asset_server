# husarion_asset_server

**A lightweight ROS 2 node that serves a component's `package://` assets — meshes, URDFs, textures — over the ROS graph, and announces which packages it owns.** The reference implementation of the [`husarion_asset_msgs`](https://github.com/husarion/husarion_asset_msgs) standard: run one per robot, sensor, or container, and any conforming client (or a [`husarion_rosbridge`](https://github.com/husarion/husarion_rosbridge) server) can fetch your assets without baking meshes into an image.

## What it does

- Hosts a uniquely-named **`GetAsset`** service. On request it resolves
  `package://PKG/REL` to a filesystem path via the ament index, reads the requested
  byte range, and returns it with `total_size` + a `content_hash` so the client can
  chunk and cache.
- Publishes **`AssetProviderInfo`** latched (TRANSIENT_LOCAL) on `/asset_providers`
  and re-announces on a heartbeat timer, so a router/bridge converges on the live
  provider set and drops a crashed provider.
- Owns exactly the packages referenced by the **`robot_description`** it is
  co-located with (auto-derived by scraping `package://` mesh URIs out of the URDF),
  or an explicit `owned_packages` override.

### Security (normative)

Only `package://` URIs; no `..` traversal; only packages in the announced owned
set; the resolved real path must stay inside the package's share directory.

## Configuration

The provider is a **first-class ROS 2 node**: every knob is a CLI flag, an
`ASSET_SERVER_*` env var, **and** a ROS 2 parameter, with precedence
**flag > ROS param > env > default**.

| Flag | ROS param / env | Default | Meaning |
| -- | -- | -- | -- |
| `--owned-packages` | `owned_packages` / `ASSET_SERVER_OWNED_PACKAGES` | _(empty)_ | Explicit owned set (comma-separated). When empty, derive from the description. |
| `--description-topic` | `description_topic` / `ASSET_SERVER_DESCRIPTION_TOPIC` | `robot_description` | Latched URDF source for auto-derivation. |
| `--providers-topic` | `providers_topic` / `ASSET_SERVER_PROVIDERS_TOPIC` | `/asset_providers` | Where `AssetProviderInfo` is announced. |
| `--heartbeat` | `heartbeat` / `ASSET_SERVER_HEARTBEAT` | `5.0` | Re-announce period (seconds). |
| `--max-chunk` | `max_chunk` / `ASSET_SERVER_MAX_CHUNK` | `524288` | Response chunk ceiling (keep under the RMW service limit). |
| `--node-name` / `--namespace` | node identity (`-r __node:=` / `-r __ns:=`) | `husarion_asset_server` / _(global)_ | ROS node identity (unique per provider). |

Configuration is startup-only: a runtime `ros2 param set` is logged as
"restart to apply".

## Run as a ROS 2 node

Once built into a colcon workspace (see **Build**), the provider drops in as a
standard node — `ros2 run`, a launch `Node(...)`, params, and remaps all work:

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

Drop this `node` block into your robot's bringup launch to run the provider
alongside the rest of your stack. It lands on `/<namespace>/get_asset` and
auto-derives its owned packages from `/<namespace>/robot_description`, so it just
works next to your `robot_state_publisher`:

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

Then `ros2 launch my_bringup.launch.yaml`. The package must be on your
`AMENT_PREFIX_PATH` — build it into your workspace (see **Build**), or run the
published image, which *is* the ament-installed node:

```bash
# The image is a standard ROS 2 node — run it, or `ros2 run`/`ros2 launch` inside it.
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

Configuration is via CLI flags (`--help`): `--node-name`, `--namespace`,
`--owned-packages`, `--description-topic`, `--providers-topic`, `--heartbeat`,
`--max-chunk`.

Fetch an asset (the client chunks via `offset`/`length`; `0,0` = whole asset):

```bash
ros2 service call /husarion_asset_server/get_asset husarion_asset_msgs/srv/GetAsset \
  "{uri: 'package://rosbot_description/meshes/rosbot_xl/body.dae', offset: 0, length: 0}"
```

## Build

A Rust / [r2r](https://github.com/sequenceplanner/r2r) node — it hosts a **typed**
`GetAsset` service, so r2r generates the bindings from a sourced
`husarion_asset_msgs` (no untyped-server FFI). Build the contract, then the node:

```bash
# 1) the message contract (colcon, into a sourced workspace)
colcon build --packages-select husarion_asset_msgs   # from a ws with husarion_asset_msgs
source install/setup.bash
# 2) the node (cargo, with ROS + the contract sourced)
cargo build --release
```

For `ros2 run` / launch, build it as the **ament_cargo** package it ships as
(`package.xml`) — it installs the binary where `ros2 run husarion_asset_server
asset_server` resolves it:

```bash
# needs cargo-ament-build (cargo install cargo-ament-build) + colcon-ros-cargo
colcon build --packages-select husarion_asset_server
source install/setup.bash
```

Or just build the image (the `husarion_asset_msgs` contract comes from a build
context, no network):

```bash
docker build -t husarion/asset-server \
  --build-context husarion_asset_msgs=../husarion_asset_msgs .
docker run --rm --network host --ipc host --env-file /etc/husarion/ros.env \
  husarion/asset-server asset_server --owned-packages rosbot_description
```

The image is **universal + mesh-less** (all RMWs, no robot descriptions): a
deploy layers the driver's meshes on top. It's published multi-arch to Docker Hub
as `husarion/asset-server:<version>` + `:latest` by `.github/workflows/image.yml`
on a `vX.Y.Z` tag.

## License

Apache-2.0.
