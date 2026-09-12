default:
  @just --list

# Build the host binary (mounted :ro into the test container).
build:
  cargo build

build-release:
  cargo build --release

# Build the Fedora-minimal test image.
test-build-image:
  docker build -f Containerfile.test -t confit-test:latest .

# Start (idempotent) the persistent test container.
test-up: build
  ./scripts/test-up.sh

# Open an interactive shell as `tester` inside the container.
test-shell:
  ./scripts/test-shell.sh

# One-shot run of the mounted binary: `just test-run -- --help` / `just test-run -- plan ...`
test-run *ARGS="--help":
  ./scripts/test-run.sh {{ARGS}}

# Stop + rm container, keep home volume.
test-down:
  ./scripts/test-down.sh

# Stop + rm container AND delete home volume (fresh HOME next up).
test-nuke:
  ./scripts/test-nuke.sh

# Plan the basic_tool fixture (host smoke; writes only ./target).
plan-example:
  cargo run -- plan --profile examples/0-basic_tool/profile.lua --root examples/0-basic_tool -o ./target/plan-example.json

# Show the living spec as of a sealed tag.
show-spec VERSION="0.1":
  git show v{{VERSION}}:docs/spec.md

# Full local verification.
check:
  cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
