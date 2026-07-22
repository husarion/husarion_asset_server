#!/usr/bin/env bash
# Source ROS + the message typesupport r2r links against + the ament install
# prefix (so `ros2 run husarion_asset_server asset_server` / a launch Node(...)
# resolve the node), then exec it (honours the live RMW env from --env-file,
# like the rest of the stack).
# NB: not `set -u` — ROS setup scripts reference unbound vars.
set -eo pipefail
source /opt/ros/jazzy/setup.bash
[ -f /msgs/install/setup.bash ] && source /msgs/install/setup.bash
[ -f /opt/husarion_asset_server/install/setup.bash ] && source /opt/husarion_asset_server/install/setup.bash
exec "$@"
