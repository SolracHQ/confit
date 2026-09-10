#!/usr/bin/env bash
# One-shot run: ./scripts/test-run.sh [confit args...] (default: --help)
# Ensures the container is up, then execs the :ro-mounted binary.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTAINER="${CONFIT_TEST_CONTAINER:-confit-test}"
# Prefer debug, fall back to release.
BIN="${CONFIT_TEST_BIN:-/opt/confit/debug/confit}"

"$ROOT/scripts/test-up.sh" >/dev/null

if [[ $# -eq 0 ]]; then
  set -- --help
fi

# Fall back to release binary if debug build is absent inside the mount.
if ! docker exec --user tester "$CONTAINER" test -x "$BIN"; then
  BIN="/opt/confit/release/confit"
fi

exec docker exec --user tester -w /home/tester "$CONTAINER" "$BIN" "$@"
