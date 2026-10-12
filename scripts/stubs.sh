#!/usr/bin/env bash
# Regenerate committed stubs: ./scripts/stubs.sh [dir] (default: stubs)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${1:-stubs}"

cargo run -q -p confit-engine --example gen -- "$ROOT/$DIR"
