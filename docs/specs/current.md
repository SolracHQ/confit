# ConfIt spec (current)

Spec-Version: 0.1.0

Living description of what confit does today. If you want to know why
it looks like this, the intent behind each version lives in `../design/`.
History of this file lives in git tags (`just show-spec`).

## What is

ConfIt (configure it!) is the spiritual successor of my old dotfiles:
one static binary to bootstrap machines and maintain user-space state.
Two-phase workflow borrowed from Terraform: `plan` (preview and diff,
writes nothing) before `apply`.

Working today: `plan` and `status` over Lua tools, JSON plans on disk
with TOML export. `apply` is not implemented.

## Features

### Plan before apply

`confit plan` loads a Lua entrypoint, evaluates it to a set of
contributions, groups them into artifacts, canonicalizes and hashes the
data, loads previous state, and diffs desired vs previous. Output: a JSON
plan file plus a terminal summary. No writes to home.

### Status and drift

`diff` compares each artifact's `data_hash` against the previous state in
memory: absent means create, different means update, equal means
unchanged. Plan files carry no hashes (`data_hash` is computed but not
serialized). Nothing reads the files on disk yet, so hand-edits go
unnoticed. That is v0.2 work.

### Plans on disk

- Plan: JSON pretty-printed (`-o ./plan.json`, omitted prints to stdout),
  diffable, git-storable.
  Contains canonical artifact data, hook list, plus `created_at`
  metadata (excluded from the SHA). `--format toml` exports the same plan
  as TOML for readability.
- State v1: JSON file (`--state`; omitted means empty previous, no disk
  touch). `{ artifact_id -> { data_hash, output_hash, data? } }`.

### Terminal summary

The summary lists every entry with its winner tool; `--conflicts` names
the loser on changed lines (`(starship wins over bat)`):

```sh
~/.bashrc: rc ← bat
  + alias cat = bat (bat)
~/.config/mise/config.toml: toml ← bat
  + tools.bat = latest (bat)
plan: 2 create, 0 update, 0 unchanged
```

## Concepts

### Tools, artifacts, profiles

A **tool** is a bundle declaration: something installable and configurable
(zoxide, starship, bat). It installs and writes nothing by itself. It
*contributes* data: a package entry to the mise artifact, vars/aliases/init
to shell artifacts.

An **artifact** is the unit that will touch disk once `apply` exists.
Every artifact has a `path`. Plan diffs and hashes happen only at
artifact level; tools are invisible to state.

A **profile** is the composition root per machine or role. Its return
value is the entire resource graph; there is no separate registration
step:

```lua
return {
  shells = { "bash" },
  tools = { bat },
}
```

### Artifacts plan produces

Two kinds, both assembled by core (no Lua constructor):

| Kind | How it is built | Merge rule |
| --- | --- | --- |
| `toml` | `mise.package(...)` specs group into `~/.config/mise/config.toml` | nested tables, deep-merge, last-writer-wins |
| `rc` | one per declared shell (`bash` to `~/.bashrc`) from handle methods | per-shell data object, see Shell rc |

The model defines more kinds (`json`, `yaml`, `template`, `file`,
`link`, `fetched`) for later layers to grow into. Lua cannot construct
artifacts directly yet.

### Shell rc

Core accumulates one data object per declared shell from handle methods:

- `profile`: ordered list, every entry prepends (`profile_path(dir)` is
  prepend sugar for `PATH`). Destined for the always-loaded file per shell.
- `env`: ordered list `{ name, value }`, unconditional only. Same name
  twice = conflict: last wins, loser goes to `_shadowed`.
- `aliases`: plain map, key-by-key merge, last-writer-wins.
- `init`: ordered list. `eval` means `eval "$(argv...)"`; `cmd` means the
  argv as a plain command line. Structurally identical entries
  collapse to one (recorded in `_shadowed`, never silently dropped).

Contribution order = profile tool order, then declaration order within
each tool. Require order is meaningful: last wins.

Planned rc layout per file (byte rendering lands with materialization):
env block, aliases block, init block; each tool's section wrapped in
`# >>> confit:<tool>` / `# <<< confit` markers.

The model already represents conditional entries (`when`), but the DSL
only produces unconditional ones. That wiring is future work.

### Canonicalization and hashing

Before hashing, each merged artifact is canonicalized:

1. Recursively sort all map keys. Lua map iteration order is random;
   without this, the same profile hashes differently on every run.
2. Ordered lists (`env`, `profile`, `init`) keep declaration order.
3. Keys starting with `_` are skipped by the hasher. `_shadowed` (losing
   values, each with a `reason`) is stored in the plan JSON for
   inspection but never contributes to the SHA or the diff. Changing only
   losers is not a change.

Each artifact hashes with SHA-256 over its canonical JSON bytes.

## DSL guide

`confit.tool(name, opts)` returns a handle: userdata holding a Rust-owned
builder, mutated in place by each method call. Method syntax (`:env`,
`:alias`) marks Rust-owned mutable state; plain calls (`.`) mark
constructors and data (`confit.mise.package`).

```lua
local bat = confit.tool("bat", {
  install = confit.mise.package({ name = "bat" }),  -- version defaults to "latest"
})

bat:alias("cat", "bat")                             -- alias name = value
bat:env("_ZO_DATA_DIR", "/data/zoxide")             -- env name = value, unconditional
bat:profile("PATH", "~/.cargo/bin")                 -- prepends to PATH-like var
bat:profile_path("~/go/bin")                        -- prepend sugar for PATH
bat:init({ eval = { "zoxide", "init", "bash" } })    -- eval "$(argv...)"
bat:init({ cmd = { "task", "--completion", "bash" } }) -- plain command line

return bat
```

- `opts.install` takes the `confit.mise.package({ name, version? })`
  table. A tool has at most one installer spec.
- Tables in tool data must be function-free; `plan` rejects functions
  (and userdata) with an error naming tool and field. If it cannot
  survive a JSON round-trip, it is not data.

## CLI guide

```sh
confit plan --profile profiles/desktop.lua [-o ./plan.json] [--root .] [--format toml] [--state ./state.json] [--conflicts]
confit status --profile profiles/desktop.lua [--root .] [--state ./state.json] [--conflicts]
```

- `-o`/`--output` is the explicit output path; omitted prints the plan to
  stdout. The summary always goes to stderr, so stdout carries only the
  plan payload. Nothing else is written during plan.
- `--root` (require resolution base) defaults to the profile file's parent.
- `--format toml` exports the same plan as TOML for readability.
- `--state` points at the previous-state file; omitted means empty previous.
- `--conflicts` names losing tools on changed summary lines.
