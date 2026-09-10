#!/usr/bin/env bash
# Idempotent start of the persistent `confit-test` container.
# HOME inside the container is a named volume; ./target is mounted :ro.
set -euo pipefail

CONTAINER="${CONFIT_TEST_CONTAINER:-confit-test}"
VOLUME="${CONFIT_TEST_VOLUME:-confit-test-home}"
IMAGE="${CONFIT_TEST_IMAGE:-confit-test:latest}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mkdir -p examples target

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "error: image $IMAGE not found. Run: just test-build-image" >&2
  exit 1
fi

if [[ ! -f target/debug/confit && ! -f target/release/confit ]]; then
  echo "warning: no binary at target/debug/confit yet. Run: just build" >&2
fi

if ! docker volume inspect "$VOLUME" >/dev/null 2>&1; then
  docker volume create "$VOLUME" >/dev/null
  echo "created volume $VOLUME (fake HOME, persists across test-down)"
fi

if docker ps --format '{{.Names}}' | grep -qx "$CONTAINER"; then
  echo "$CONTAINER already running (volume $VOLUME kept)"
  exit 0
fi

if docker ps -a --format '{{.Names}}' | grep -qx "$CONTAINER"; then
  docker start "$CONTAINER" >/dev/null
  echo "started stopped container $CONTAINER (volume $VOLUME kept)"
  exit 0
fi

# :ro = binary/fixture inputs can't be clobbered from inside.
# :Z  = SELinux relabel needed on Fedora hosts (bind mounts only).
docker run -d --name "$CONTAINER" \
  --user tester \
  -v "$VOLUME:/home/tester" \
  -v "$ROOT/target:/opt/confit:ro,Z" \
  -v "$ROOT/examples:/home/tester/examples:Z" \
  "$IMAGE" sleep infinity >/dev/null

echo "up: $CONTAINER (fake HOME in volume $VOLUME)"
echo "next: just test-shell  |  just test-run -- --help"
