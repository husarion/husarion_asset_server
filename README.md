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

## Configuration (CLI flags)

| Flag | Default | Meaning |
| -- | -- | -- |
| `--owned-packages` | _(empty)_ | Explicit owned set (comma-separated). When empty, derive from the description. |
| `--description-topic` | `robot_description` | Latched URDF source for auto-derivation. |
| `--providers-topic` | `/asset_providers` | Where `AssetProviderInfo` is announced. |
| `--heartbeat` | `5.0` | Re-announce period (seconds). |
| `--max-chunk` | `524288` | Response chunk ceiling (keep under the RMW service limit). |
| `--node-name` / `--namespace` | `husarion_asset_server` / _(global)_ | ROS node identity (unique per provider). |

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

Or just build the image (the `husarion_asset_msgs` contract comes from a build
context, no network):

```bash
docker build -t husarion-asset-server \
  --build-context husarion_asset_msgs=../husarion_asset_msgs .
docker run --rm --network host --ipc host --env-file /etc/husarion/ros.env \
  husarion-asset-server asset_server --owned-packages rosbot_description
```

## License

Apache-2.0.
