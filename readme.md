# ConfIt

ConfIt (configure it!) is the spiritual successor of my old dotfiles repo.

I format my computers a lot. Partly because I like a clean system, partly
because I am curious and install software I only use once. That is why my
dotfiles were born (and they are not dotfiles at all, apart from the
starship config): set up a PC to be productive with a few commands. It
never completely fit.

I have goals that are clear but not easy to achieve:

- Idempotent changes
- Easy to check diffs
- Flexibility in configuration
- Reproducibility
- Reusability (profiles without redoing all the config)
- LSP help (no spec I forget in a week, no forgotten key names, no configs
  in the wrong place that fail, or worse, get silently ignored)

Single Rust binary, Lua configuration, plan before apply. Two-phase
workflow borrowed from Terraform: preview and diff before touching
anything.

Spec: `docs/specs/current.md` (living record of what is implemented).
Intent per version: `docs/design/`. Changes: `docs/changelog.md`.

Old setup for reference: [SolracHQ/dotfiles](https://github.com/SolracHQ/dotfiles).

## Non-goals

- Not a system package manager, not Nix or Home-Manager. User-space only.
- Not a general provisioner: no root, no services, no secrets management.

## Build

```sh
cargo run -- --help
just check   # fmt + clippy (-D warnings) + test
```

Rust + vendored Lua 5.4 via `mlua` (`lua54` + `vendored`), minijinja for
template rendering at apply time. Dependencies go through `cargo add` only.

## Use

```sh
confit plan --profile profiles/desktop.lua [-o ./plan.json] [--root .] [--format toml] [--state ./state.json] [--conflicts]
confit status --profile profiles/desktop.lua [--root .] [--state ./state.json] [--conflicts]
```

`--root` defaults to the profile file's parent directory. Fixture profiles
under `examples/` (`0-basic_tool`: bat via mise + alias, `just plan-example`
to smoke it; writes only `./target`).

Status: `plan` and `status` over Lua tools, JSON plans on disk with TOML
export. `apply` does not exist yet.

## Testing in docker

Manual testing runs in a persistent Fedora-minimal container as user
`tester`. HOME inside the container is the `confit-test-home` volume.
Host `./target` is mounted `:ro` at `/opt/confit`; `./examples` is mounted
at `~/examples` for fixture profiles.
Requires the Docker daemon (`sudo dnf install -y docker-cli moby-engine`,
`sudo systemctl enable --now docker`) and your user in the `docker` group
(log out/in once after `usermod -aG docker`).

```sh
just test-build-image  # once: build confit-test:latest
just test-up           # cargo build + start `confit-test` (idempotent)
just test-shell        # interactive bash as tester
just test-run -- --help # one-shot run of the mounted binary
just test-down         # stop+rm container, keep home volume
just test-nuke         # stop+rm container AND delete home volume
```

Scripts live in `scripts/` (`test-up/shell/run/down/nuke.sh`) and are thin
wrappers over `docker run/exec/volume`; `justfile` only forwards to them.

## License

MIT. Fork away.
