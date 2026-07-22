# examples

## `asset_server.launch.yaml`

A ROS 2 **YAML launch file** that starts the provider as a standard node under a namespace, with the common parameters set inline. Run it directly or copy it into your own package's `launch/` dir:

```bash
ros2 launch asset_server.launch.yaml
# or: ros2 launch your_package asset_server.launch.yaml
```

## `asset_server.params.yaml`

A ROS 2 **parameters file** covering the knobs. Every operator setting is a ROS 2 parameter (`ros2 param list/get/describe`); precedence is `CLI flag > ROS param > ASSET_SERVER_* env var > default`. Load it with a params file or from a launch file:

```bash
ros2 run husarion_asset_server asset_server \
  --ros-args --params-file asset_server.params.yaml
```

The node name and namespace are node identity, not parameters — set them via the launch `name=` / `namespace=` (or `-r __node:=…` / `-r __ns:=…`).
