# ConfIt spec (current)

Spec-Version: 0.1.0

Living description of what confit does today. If you want to know why
it looks like this, the intent behind each version lives in `../design/`.
History of this file lives in git tags (`just show-spec`).

## What is

ConfIt (configure it!) succeeds a dotfiles setup:
one static binary to bootstrap machines and maintain user-space state.
Two-phase workflow: `plan` previews and diffs before `apply` touches
anything.

Working today: `plan` and `status` over Lua tools, JSON plans on disk.
`apply` follows in a later version.

## Features

### Plan before apply

`confit plan` loads a Lua entrypoint, evaluates it to a set of
contributions, groups them into artifacts, canonicalizes and hashes the
data, loads previous state, and diffs desired vs previous. Output: a JSON
plan file plus a terminal summary. Home stays untouched.

### Status and drift

`diff` compares each artifact's `data_hash` against the previous state in
memory: absent means create, different means update, equal means
unchanged, previous-only keys mean delete. Plan files carry data payloads
alone (`data_hash` computes at runtime and stays outside serialization).

`plan` also snapshots each artifact path off disk (`~` expands via the
home folder). Absent paths read as absence. Unreadable paths warn
on stderr naming path and reason, read as absent, exit stays 0. Disk
differs from recorded means manual modification: warned, counted as
update.

Invariant: warnings ride along with a successful run. Exit stays 0 while
warnings exist. Warning lines read `<path>: exists but no record: will
be overwritten` for untracked paths, `<path>: differs from recorded:
manual modification will be overwritten` for manual edits, and
`cannot read '<path>': <reason>` for failing reads.

### Plans on disk

- Plan: JSON pretty-printed (`-o ./plan.json`, omitted prints to stdout),
  diffable, git-storable.
  Contains canonical artifact data plus `created_at`
  metadata (excluded from the SHA).
- State v1: JSON file (`--state`; omitted means empty previous, touching
  zero disk paths). `{ artifact_id -> { data_hash, output_hash, data? } }`.

### Terminal summary and color

The summary lists every entry with its winner tool; `--conflicts` names
the loser on changed lines (`(starship wins over bat)`):

```sh
~/.bashrc: rc ← bat
  + alias cat = bat (bat)
  + init[0] = eval "$(mise activate bash)" (bat)
~/.config/mise/config.toml: toml ← bat
  + tools.bat = latest (bat)
Plan: 2 to add, 0 to change, 0 to destroy.
```

Color: terminal runs paint updates yellow, additions green, removals
red, headers bold. Piped output stays plain text, and `NO_COLOR`
disables color. The summary goes to stderr. Drift notes precede the
plan half when disk reads show changes.

## Concepts

### Tools, artifacts, profiles

A **tool** is a bundle declaration: something installable and configurable
(zoxide, starship, bat). It ships data contributions: a package entry to the mise artifact, vars/aliases/init
to shell artifacts.

An **artifact** is the unit that will touch disk once `apply` exists.
Every artifact has a `path`. Plan diffs and hashes happen only at
artifact level; tools are invisible to state.

A **profile** is the composition root per machine or role. Its return
value is the entire resource graph; the return value serves as the
registration:

```lua
return {
  shells = { "bash" },
  tools = { bat },
}
```

### Artifacts plan produces

`toml` (mise) and `rc` assembled by core, plus any artifacts tools
append. All merge by `(kind, path)` with tool attribution.

| Kind | How it is built | Merge rule |
| --- | --- | --- |
| `toml` | `mise.package(...)` specs group into `~/.config/mise/config.toml`; `confit.artifact.toml(path, table)` values appended by tools | nested tables, deep-merge, last-writer-wins |
| `rc` | one per declared shell (`bash` to `~/.bashrc`) from handle methods | per-shell data object, see Shell rc |

Confit supports more kinds (`json`, `yaml`, `template`, `file`,
`link`) with constructors ready; the fixture exercises `toml` so
far.

### Shell rc

Core accumulates one data object per declared shell from handle methods:

- `profile`: ordered list, every entry prepends (`profile_path(dir)` is
  prepend sugar for `PATH`). Destined for the always-loaded file per shell.
- `env`: ordered list `{ name, value }`, unconditional only. Same name
  twice = conflict: last wins, loser goes to `_shadowed`.
- `aliases`: plain map, key-by-key merge, last-writer-wins.
- `init`: ordered list. `eval` means `eval "$(argv...)"`; `cmd` means the
  argv as a plain command line; `source` means `source path` for a single
  file path. Structurally identical entries
  collapse to one, each collapse recorded in `_shadowed`.

Contribution order = profile tool order, then declaration order within
each tool. Require order is meaningful: last wins.

Rc layout per file: env block, aliases block, init block; each
tool's section wrapped in `# >>> confit:<tool>` / `# <<< confit`
markers.

The model represents conditional entries (`when`) already. The DSL
produces unconditional ones today. That wiring is future work.

### Canonicalization and hashing

Before hashing, each merged artifact is canonicalized:

1. Recursively sort all map keys. Lua map iteration order varies;
   sorting keeps one profile hashing one way on every run.
2. Ordered lists (`env`, `profile`, `init`) keep declaration order.
3. Keys starting with `_` stay outside the hash. `_shadowed` (losing
   values, each with a `reason`) ships in the plan JSON for
   inspection while staying outside the SHA and the diff. Changing
   losers alone leaves the plan unchanged.

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
bat:init({ source = "~/.cargo/env" })                 -- source path

return bat
```

- `opts.install` takes the `confit.mise.package({ name, version? })`
  table. A tool holds at most one installer spec. A mise install
  ships activation with the install as the first init entry
  of every shell: `eval "$(mise activate <shell>)"`.
- Tables in tool data hold JSON-shaped values alone; `plan` rejects functions
  (and userdata) with an error naming tool and field.

Tool files may return a parametrizing function; the profile calls it
with user values:

```lua
return function(user_config)
  local resource = confit.resources.load_toml("resources/starship.toml")
  local config = confit.resources.merge(resource, user_config)
  local artifact = confit.artifact.toml(
    confit.path.config("starship.toml"),
    config
  )
  starship:append_artifact(artifact)
  return starship
end
```

- `confit.resources.load_toml/load_json/load_yaml(path)` read a
  root-relative file into a Lua table. Absolute paths and escapes above
  root are plan errors.
- `confit.resources.merge(base, overlay, opts?)` deep-merges: tables
  recurse, everything else (arrays included) last-wins. `shallow = true`
  merges top-level keys only; `list_append = true` concatenates arrays
  keeping duplicates. Unknown option keys are plan errors.
- `confit.artifact.toml/json/yaml/file/template/link` build artifact
  values; `tool:append_artifact` attaches them, merged by `(kind, path)`.
- `confit.path.home/config/data/confroot` join `$HOME`, the OS config
  folder, the OS data folder, and the project root with the segments.
  `data` serves local installs like fonts. `confroot` serves symlinks
  to shipped resources.

## CLI guide

```sh
confit plan --profile profiles/desktop.lua [-o ./plan.json] [--root .] [--state ./state.json] [--conflicts]
confit status --profile profiles/desktop.lua [--root .] [--state ./state.json] [--conflicts]
```

- `-o`/`--output` is the explicit output path; omitted prints the plan to
  stdout. The summary always goes to stderr, so stdout carries the plan
  payload alone. Home stays untouched during plan.
- `--root` (require resolution base) defaults to the profile file's parent.
- `--state` points at the previous-state file; omitted means empty previous.
- `--conflicts` names losing tools on changed summary lines.
