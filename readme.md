# ConfIt

Configure It. CaC (Configuration as Code) scoped in the user
space. One static binary to bootstrap machines and maintain
user-space files. Same job as a dotfiles setup, declarative,
with a preview before anything gets touched.

## Objectives

- Idempotent changes: applying twice gives the same result.
- Readable diffs before anything lands.
- Configs shaped per machine from Lua.
- Profiles sharing one config pool without redoing it.
- Editor-checked key names, configs landing where loaders read them.
- A preview loop: desired vs previous vs actual
  files, with hand edits reported before they get overwritten.

## State

One Rust binary, Lua 5.4 vendored inside, plan before apply.
Four commands work today. `plan` previews the change and
writes a JSON plan of the desired state. `apply` previews,
prompts, and writes the files. `recover` re-applies a stored
state. `init` scaffolds a project.

Profiles declare documents plus configs. Configs hold documents plus
patches. Documents cover shell entries plus structured configs plus
literal files plus binaries plus symlinks. Patches tweak documents through callbacks
in pipeline order. The mise plugin ships package configs plus shell
activation entries.

## Scope

User-space files. Everything runs without privilege steps.
External tools handle system software.

## Show

A profile declares configs, configs collect entries:

```lua
-- examples/0-basic_tool/tools/bat.lua
local mise = confit.plugin.solrachq.mise

local bat = mise.package("bat", function(rc)
	rc:alias("cat", "bat")
end)
bat:add_patch(mise.activate())
return bat
```

A plan run previews the change:

```sh
$ confit plan examples/0-basic_tool/profile.lua --root examples/0-basic_tool
Plan: 2 to add, 0 to change, 0 to destroy.
```

> Note: plans carry a `created_at` timestamp, so two runs differ in
> that field alone. Everything else is byte-identical.

| Fixture | Proves |
|---|---|
| `0-basic_tool` | mise package plus alias plus init |
| `1-structured_resource` | starship config declared plus patched |
| `2-templated_resource` | starship config rendered from a template with profile vars |
| `3-dotfiles-tools` | multi-config profile plus fetch plus unpack plus rc patch |

| Command | Does |
|---|---|
| `plan PROFILE` | writes the JSON plan, warnings on stderr |
| `apply PROFILE` | previews, prompts on literal `yes`, writes files |
| `apply --plan FILE` | writes a reviewed plan, skips preview |
| `recover [INDEX]` | lists stored states, re-applies the picked one |
| `init [DIR]` | scaffolds a profile plus stubs, default `.` |

`--root` defaults to the profile file parent directory.
`--state` selects the state file, defaulting to the shared slot.
`just plan-example` smokes the basic fixture and writes only to
`./target`.

## Build

```sh
cargo run -- --help
just check   # fmt + clippy (-D warnings) + test
```

Spec: `docs/spec.md` (living record of what is implemented).
Manual: `docs/manual/` (user guide). Intent per version: `docs/design/`. Changes: `docs/changelog.md`.

Old setup for reference: [SolracHQ/dotfiles](https://github.com/SolracHQ/dotfiles).

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

MIT.
