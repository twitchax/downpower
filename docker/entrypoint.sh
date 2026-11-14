#!/bin/sh
# The container starts as root only to hand the data directory to the server's user: Fly mounts
# volumes root-owned, and files put there over `fly ssh` are root's too. It then drops to uid 10001
# for good (docs/DESIGN.md, "Deploy").
set -eu

data="${DOWNPOWER_DATA_DIR:-/data}"
if [ "$(id -u)" -eq 0 ]; then
    mkdir -p "$data"
    # Only what the server doesn't own yet, so a restart over a settled volume changes nothing.
    find "$data" ! -user 10001 -exec chown 10001:10001 {} +
    exec setpriv --reuid=10001 --regid=10001 --clear-groups -- "$@"
fi
exec "$@"
