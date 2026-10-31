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

# Regenerate committed stubs from the live surface.
stubs:
  ./scripts/stubs.sh

# Bundle the basic_tool fixture (host smoke; writes only ./target).
bundle-example:
  cargo run -- bundle examples/0-basic_tool -o ./target/bundle-example

# Preview the basic_tool fixture (host smoke; writes nothing).
check-example:
  cargo run -- check examples/0-basic_tool

# Show the living spec as of a released tag.
show-spec VERSION="0.1":
  git show v{{VERSION}}:docs/book/src/spec/index.md

# Full local verification.
check:
  cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace

# Bump the workspace to one version.
set-version VERSION:
  sed -i '/^\[workspace\.package\]/,/^\[/ s/^version = ".*"/version = "{{VERSION}}"/' Cargo.toml
  cargo check --workspace --offline >/dev/null 2>&1 || cargo check --workspace >/dev/null
  grep -A2 '^\[workspace\.package\]' Cargo.toml

# Format docs at width 80: `just format-docs` formats all of docs/, `just format-docs <file>` formats one file.
format-docs FILE="":
  @if [ -z "{{FILE}}" ]; then mdformat --wrap 80 docs; else mdformat --wrap 80 "{{FILE}}"; fi
