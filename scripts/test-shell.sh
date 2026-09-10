#!/usr/bin/env bash
# Open an interactive shell in the test container as `tester`.
# Starts the container first if needed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTAINER="${CONFIT_TEST_CONTAINER:-confit-test}"

"$ROOT/scripts/test-up.sh" >/dev/null

exec docker exec -it --user tester -w /home/tester "$CONTAINER" bash "$@"
