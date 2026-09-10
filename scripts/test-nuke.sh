#!/usr/bin/env bash
# Full reset: stop + remove container AND delete the home volume.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTAINER="${CONFIT_TEST_CONTAINER:-confit-test}"
VOLUME="${CONFIT_TEST_VOLUME:-confit-test-home}"

"$ROOT/scripts/test-down.sh" || true

if docker volume inspect "$VOLUME" >/dev/null 2>&1; then
  docker volume rm "$VOLUME" >/dev/null
  echo "nuked volume $VOLUME (fresh HOME on next test-up)"
else
  echo "volume $VOLUME already gone"
fi
