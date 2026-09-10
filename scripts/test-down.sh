#!/usr/bin/env bash
# Stop + remove the test container. The home volume is kept.
set -euo pipefail

CONTAINER="${CONFIT_TEST_CONTAINER:-confit-test}"
VOLUME="${CONFIT_TEST_VOLUME:-confit-test-home}"

if docker ps -a --format '{{.Names}}' | grep -qx "$CONTAINER"; then
  docker stop -t 5 "$CONTAINER" >/dev/null 2>&1 || true
  docker rm "$CONTAINER" >/dev/null
  echo "down: $CONTAINER removed, volume $VOLUME kept"
  echo "re-enter with: just test-shell   |   fresh start with: just test-nuke"
else
  echo "$CONTAINER does not exist (volume $VOLUME untouched)"
fi
