#!/usr/bin/env bash
# Record the readme demo inside the prepared image.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${CONFIT_DEMO_IMAGE:-confit-demo:latest}"

cd "$ROOT"
cargo build --release
docker run --rm \
	--user demo \
	--security-opt label=disable \
	-e HOME=/home/demo \
	-v "$ROOT:/repo:ro" \
	-v "$ROOT/target/release/confit:/usr/local/bin/confit:ro" \
	-v "$ROOT/assets:/out" \
	-w /home/demo \
	"$IMAGE" vhs /repo/assets/demo.tape
ls -lh "$ROOT/assets/confit-demo.gif"
