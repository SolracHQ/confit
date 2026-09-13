# ConfIt

Successor to a dotfiles setup that installs languages and tools, fonts,
a prompt, and a few configs. Same job, declarative, with a preview
before anything gets touched.

## Objectives

- Idempotent changes: applying twice gives the same result.
- Readable diffs before anything lands.
- Configs shaped per machine from Lua.
- Profiles sharing one config without redoing it.
- Editor-checked key names, configs landing where loaders read them.
- A preview loop borrowed from Terraform: desired vs previous vs actual
  files, with hand edits reported before they get overwritten.

## State

One Rust binary, Lua 5.4 vendored inside, plan before apply. Two commands
work today. `plan` writes a JSON plan of the desired state. `status`
diffs desired vs previous vs the actual files on disk, in memory. Absent
paths read as absence. Failing reads warn and continue with exit 0.
Hand edits get named warnings. `apply` and `explain` do not exist yet.

Profiles declare configs. Configs collect aliases, env, profile entries,
and init lines in `eval`, `cmd`, and `source` shapes. Configs also carry
file artifacts: structured configs, minijinja templates, literal files,
and symlinks. The mise plugin ships its activation as the first init
entry of every shell.

## Scope

User-space files plus the post commands triggered after they land.
Escalating permissions is not planned.

## Show

A profile declares configs, configs collect entries:

```lua
-- examples/0-basic_tool/tools/bat.lua
local mise_package = confit.plugin.solrachq.mise_package

local bat = mise_package("bat", function(rc)
  rc:alias("cat", "bat")
end)
return bat
```

A plan run previews the change without touching anything:

```sh
$ confit plan --profile examples/0-basic_tool/profile.lua --root examples/0-basic_tool
Plan: 2 to add, 0 to change, 0 to destroy.
```

> Note: plans carry a `created_at` timestamp, so two runs differ in
> that field alone. Everything else is byte-identical.

| Fixture | Proves |
|---|---|
| `0-basic_tool` | mise package plus alias plus init |
| `1-structured_resource` | starship config loaded through `confit.resources` and merged |
| `2-templated_resource` | starship config rendered from a template with profile vars |

| Command | Does |
|---|---|
| `plan` | writes the JSON plan, warnings on stderr |
| `status` | same diff, in memory, nothing written |

`--root` defaults to the profile file parent directory.
`just plan-example` smokes the basic fixture and writes only to
`./target`.

## Build

```sh
cargo run -- --help
just check   # fmt + clippy (-D warnings) + test
```

Spec: `docs/spec.md` (living record of what is implemented).
Intent per version: `docs/design/`. Changes: `docs/changelog.md`.

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
