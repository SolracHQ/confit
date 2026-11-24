#!/usr/bin/env bash
# Regenerate committed stubs: ./scripts/stubs.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$ROOT"
cargo run -q -p confit-cli -- init --stubs
