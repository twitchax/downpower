#!/bin/sh
# Smoke test for the image: over a root-owned volume, as Fly mounts one, the entrypoint hands the
# data directory to uid 10001 and the server comes up healthy. Usage: docker/smoke.sh <image>
set -eu

image="$1"
name="downpower-smoke-$$"
volume="$name"
port="${SMOKE_PORT:-3999}"

cleanup() {
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker volume rm "$volume" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# A root-owned file and directory, like a raw store put over `fly ssh`.
docker volume create "$volume" >/dev/null
docker run --rm -v "$volume:/data" --entrypoint sh "$image" -c 'mkdir -p /data/raw && touch /data/raw/shipped'

docker run -d --name "$name" -v "$volume:/data" -p "127.0.0.1:$port:3000" "$image" >/dev/null
if ! curl -fsS --retry 30 --retry-delay 1 --retry-all-errors "http://127.0.0.1:$port/healthz"; then
    docker logs "$name"
    echo "smoke: /healthz never answered" >&2
    exit 1
fi
echo

# Nothing under /data is left to root, including what the server wrote after starting.
foreign="$(docker exec "$name" find /data ! -user 10001)"
if [ -n "$foreign" ]; then
    echo "smoke: not owned by 10001:" >&2
    echo "$foreign" >&2
    exit 1
fi
# The server itself runs as 10001.
uid="$(docker exec "$name" sh -c 'for status in /proc/[0-9]*/status; do grep -q "^Name:.downpower" "$status" && awk "/^Uid:/ { print \$2 }" "$status"; done')"
if [ "$uid" != "10001" ]; then
    echo "smoke: the server runs as uid '$uid'" >&2
    exit 1
fi
echo "smoke: ok"
