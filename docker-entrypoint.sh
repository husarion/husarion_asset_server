#!/usr/bin/env bash
# Source ROS + the message typesupport r2r links against, then exec the node
# (honours the live RMW env from --env-file, like the rest of the stack).
# NB: not `set -u` — ROS setup scripts reference unbound vars.
set -eo pipefail
source /opt/ros/jazzy/setup.bash
[ -f /msgs/install/setup.bash ] && source /msgs/install/setup.bash
exec "$@"
