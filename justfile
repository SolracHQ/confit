default:
  @just --list

build:
  cargo build

build-release:
  cargo build --release

test-build-image:
  docker build -f Containerfile.test -t confit-test:latest .

test-up: build
  ./scripts/test-up.sh

test-shell:
  ./scripts/test-shell.sh

test-run *ARGS="--help":
  ./scripts/test-run.sh {{ARGS}}

test-down:
  ./scripts/test-down.sh

test-nuke:
  ./scripts/test-nuke.sh

demo-build:
  docker build -f Containerfile.demo -t confit-demo:latest .

demo:
  ./scripts/demo.sh

stubs:
  ./scripts/stubs.sh

bundle-example:
  cargo run -- bundle examples/0-basic_tool -o ./target/bundle-example

check-example:
  cargo run -- check examples/0-basic_tool

check:
  cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace

