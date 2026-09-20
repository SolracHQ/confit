# ConfIt

Configure It. CaC (Configuration as Code) scoped in the user
space. One static binary to bootstrap user setups and maintain
user-space files. Same job as a dotfiles setup, declarative,
with a preview before anything gets touched.

## Demo

<video src="assets/confit-demo.mp4" controls width="100%"></video>

## Objectives

- Idempotent changes. Applying twice gives the same result.
- Readable diffs before anything lands.
- Configs shaped per user from Lua.
- Profiles sharing one config pool across runs.
- Editor-checked key names, configs landing where loaders read them.
- A preview loop comparing desired plus previous plus actual
  files, with hand edits reported before they get overwritten.

## State

One Rust binary, Lua 5.4 vendored inside, plan before apply.
Five commands work today. `plan` previews the change and
writes a portable `.cb` bundle holding `manifest.json` plus blobs.
`apply` previews, prompts, and writes the files. `export` packs
a slot into a portable bundle. `delete` drops a named slot.
`init` scaffolds a project.

Slots hold manifests, one shared pool holds blobs under content hashes.
First runs compare desired documents against disk bytes and report
creates plus overwrites. One spinner carries progress counters through
hash plus compression plus fetch phases. Serde derives serve as the schema.

Profiles declare documents plus configs. Configs hold documents plus
patches plus hooks. Documents cover shell entries plus structured configs plus
literal files plus binaries plus symlinks plus managed file sets. Patches tweak documents through callbacks
in pipeline order. Hooks hand tools their post-config steps after
the files land. The mise plugin ships package configs plus shell
activation entries plus a generic `options` table folding tool
options into the shared TOML. The nerd fonts plugin ships one
`fonts/{name}` folder per font plus one `fc-cache -f` hook scoped
to that folder.

## Scope

User-space files. Everything runs in user space.
External tools handle system software.

## Show

A profile declares configs, configs collect entries:

```lua
-- examples/0-basic_tool/tools/bat.lua
local mise = confit.plugin.solrachq.mise

local bat = mise.package({
 name = "bat",
 aliases = { cat = "bat" },
})
return bat
```

A plan run previews the change:

```sh
$ confit plan examples/0-basic_tool/profile.lua --root examples/0-basic_tool
Hooks
+ mise install
  + requires (in_path(mise))
  + when (changed(~/.config/mise/config.toml))
  + checks (exists(/home/tester/.local/share/mise/shims/bat))
Summary
Documents: 1 to add, 1 to change, 0 to destroy.
Hooks: 1 to add, 0 to change, 0 to destroy.
```

Beyond packages, tools finish their own setup. One font call
lands 96 files as one manifest line and refreshes the font cache:

```lua
-- examples/3-dotfiles-tools/tools/fonts.lua
local nerd_fonts = confit.plugin.solrachq.nerd_fonts

return nerd_fonts.font("JetBrainsMono", "3.5.1")
```

```sh
$ confit plan examples/3-dotfiles-tools/profile.lua --root examples/3-dotfiles-tools
Resources
+ /home/tester/.local/share/fonts/JetBrainsMono: tree
  + tree (96 files)
Hooks
+ fc-cache -f /home/tester/.local/share/fonts/JetBrainsMono
  + requires (in_path(fc-cache))
Summary
Documents: 3 to add, 1 to change, 0 to destroy.
Hooks: 2 to add, 0 to change, 0 to destroy.
```

| Fixture | Proves |
|---|---|
| `0-basic_tool` | mise package plus alias plus init |
| `1-structured_resource` | starship config declared plus patched |
| `2-templated_resource` | starship config rendered from a template with profile vars |
| `3-dotfiles-tools` | multi-config profile plus fetch plus unpack plus rc patch plus hooks plus tree |

| Command | Does |
|---|---|
| `plan PROFILE` | writes a portable `.cb` bundle, warnings on stderr |
| `apply SOURCE` | previews, prompts on literal `yes`, writes files |
| `export [PICKER]` | packs one slot into a portable bundle, prints the path |
| `delete @name` | drops one named slot plus its orphaned blobs |
| `init [DIR]` | scaffolds a profile plus stubs, default `.` |

`--root` defaults to the profile file parent directory.
`apply` reads SOURCE by shape. `.lua` plus extensionless paths run
a profile, `.cb` runs a bundle file, `@name` runs a named slot,
`%N` runs history newest-first from 1.
`-o @name` stores a slot manifest, `apply @name` replays it.
Slots hold manifests, one pool holds blobs, history reads newest-first.
`export` packs `%N` plus `@name` plus the applied slot into `.cb`,
`-m` prints the manifest. First runs diff desired state against disk
and close with create plus overwrite counts.
`just plan-example` smokes the basic fixture and writes only to
`./target`.

## Build

```sh
cargo run -- --help
just check   # fmt + clippy (-D warnings) + test
```

The manual plus the spec live as one book at `docs/book/`, published at
[solrachq.github.io/confit](https://solrachq.github.io/confit/). Intent per version
lives under `docs/design/`. Changes live in `docs/changelog.md`.

Old setup lives at [SolracHQ/dotfiles](https://github.com/SolracHQ/dotfiles).

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
