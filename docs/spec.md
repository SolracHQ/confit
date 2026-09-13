# ConfIt spec (current)

Spec-Version: 0.3.0

Living description of what confit does today. If you want to know why
it looks like this, the intent behind each version lives in `../design/`.
History of this file lives in git tags (`just show-spec`).

## What is

ConfIt (configure it!) succeeds a dotfiles setup:
one static binary to bootstrap machines and maintain user-space state.
Two-phase workflow: `plan` previews and diffs before `apply` touches
anything.

Working today: `plan` and `status` over Lua configs, JSON plans on disk.
`apply` follows in a later version.

## Features

### Plan before apply

`confit plan` loads a Lua entrypoint, evaluates it to a set of
configs, folds their artifacts, hashes the data, loads previous state,
and diffs desired vs previous. Output: a JSON
plan file plus a terminal summary, with zero writes to home paths.

### Status and drift

`diff` compares each artifact's `data_hash` against the previous state in
memory: absent means create, different means update, equal means
unchanged, previous-only keys mean delete. Plan files carry data payloads
alone, with `data_hash` computed at runtime.

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
  Contains artifact data plus `created_at`
  metadata (excluded from the SHA).
- State v1: JSON file (`--state`; omitted means empty previous, the run
  skips disk reads). `{ artifact_id -> { data_hash, output_hash, data? } }`.

### Terminal summary and color

The summary lists every entry per artifact; winners surface in the
collision log. Same-slot
collisions record one log-file line naming winner and loser:

```sh
~/.bashrc: rc
  + alias cat = bat
  + init[0] = eval "$(mise activate bash)"
~/.config/mise/config.toml: toml
  + tools.bat = latest
Plan: 2 to add, 0 to change, 0 to destroy.
log: /tmp/confit-123.log
```

```sh
collision on alias "cat": "eza" overwritten, "bat" wins
```

Color: terminal runs paint updates yellow, additions green, removals
red, headers bold. Piped output stays plain text, and `NO_COLOR`
disables color. The summary goes to stderr. Drift notes precede the
plan half when disk reads show changes.

## Concepts

### Configs, artifacts, profiles, plugins

A **config** is a named bag for artifacts: fonts, tool settings,
anything with files or shell entries. The name serves as uid per plan;
repeats are plan errors. Priority lives on submitted artifacts alone,
defaulting to 0.

An **artifact** is the unit that will touch disk once `apply` exists.
Every artifact has a `path`. Plan diffs and hashes happen only at
artifact level. Rc entries (aliases, env, profile, init) are artifacts
too, merged by `(name, when)` slots like everything else.

A **profile** is the composition root per machine or role. Its return
value is the entire resource graph; the return value serves as the
registration:

```lua
return {
  shells = { "bash" },
  configs = { bat },
}
```

A **plugin** is Lua framework code under
`confit.plugin.{username}.{plugin_name}`, embedded defaults plus a
`--plugins` folder. Plugins compose primitives: installers, helpers,
dialects. Data alone crosses the engine boundary, in both directions.

### Artifacts plan produces

`toml` (mise, via the plugin) and `rc` assembled by the engine, plus any
artifacts configs submit. Structured kinds merge by `(kind, path)`,
rc entries by `(name, when)`.

| Kind | How it is built | Merge rule |
| --- | --- | --- |
| `toml` | plugin specs group into `~/.config/mise/config.toml`; `confit.artifact.toml(path, table)` values submitted by configs | nested tables, deep-merge, priority plus name order |
| `rc` | one per declared shell (`bash` to `~/.bashrc`) from submitted entries | per-shell data object, see Shell rc |

Confit supports more kinds (`json`, `yaml`, `template`, `file`,
`link`) with constructors ready; the fixtures exercise `toml` plus
templates.

### Shell rc

Core accumulates one data object per declared shell from submitted
entries:

- `profile`: ordered list, every entry prepends (`profile_path(dir)` is
  prepend sugar for `PATH`). Destined for the always-loaded file per shell.
- `env`: ordered list `{ name, value, when, priority }`. Same name plus
  structurally equal `when` is one slot; different conditions coexist.
- `aliases`: entry list `{ name, value, when, priority }`, same slot rule as env.
- `init`: ordered list. `eval` means `eval "$(argv...)"`; `cmd` means the
  argv as a plain command line; `source` means `source path` for a single
file path. Identity is `(spec, when)`; priority decides collisions
separately. Identical pairs collapse, different conditions coexist.

Same-slot collisions resolve by highest carried priority, ties by
lexicographically smaller config name. Total order, independent of
registration order. Every cross-config collision records one log-file
line; exit stays 0.

Rc layout per file: plain entries in env, aliases, init blocks separated
by blank lines. Conditional entries render guarded inline with
`if <test>; then ... fi`, evaluated by each new shell session.

### Hashing

Tables hold fixed order (`BTreeMap`), ordered lists (`env`, `profile`,
`init`) keep declaration order. Each artifact hashes with SHA-256 over
its rendered bytes.

## DSL guide

`confit.config(name)` returns a handle: userdata with one
method, `add_artifact`. Everything nice lives in plugins composing
this primitive:

```lua
local mise_package = confit.plugin.solrachq.mise_package

local bat = mise_package("bat", function(rc)
  rc:alias("cat", "bat --colors=always")
  rc:alias("c", "bat")
end)
return bat
```

One call generates the config under the package name, declares the
install, and sets `when` on every callback entry against the binary.
Raw bags stay available:

```lua
local c = confit.config("bat")
c:add_artifact(confit.artifact.rc.alias("cat", "bat", {
  when = confit.shell.in_path("bat"),
  priority = 1,
}))
c:add_artifact(confit.artifact.toml(path, data):with_priority(1))
```

- `confit.artifact.rc.alias/env/profile/profile_path/init` build rc
  entry handles, each taking `when` plus `priority` through opts or
  through the chainable `:when` and `:with_priority` methods. File kinds
  (`toml/json/yaml/file/template/link`) take `:with_priority` the same way.
- `confit.shell.env_eq/env_set/in_path/exists/all/any/nop` build
  conditions as data. `when` takes a shape directly or a builder
  function over `confit.shell`, run during evaluation.
- `confit.plugin.helpers.error(msg)` raises plan errors with plugin
  attribution.
- Tables in artifact data hold JSON-shaped values alone; `plan`
  rejects functions (and userdata) with an error naming config and
  field.

Config files may return a parametrizing function; the profile calls it
with user values:

```lua
return function(user_config)
  local resource = confit.resources.load_toml("resources/starship.toml")
  local config = confit.resources.merge(resource, user_config)
  local artifact = confit.artifact.toml(
    confit.path.config("starship.toml"),
    config
  )
  starship:add_artifact(artifact)
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
- `confit.path.home/config/data/confroot` join `$HOME`, the OS config
  folder, the OS data folder, and the project root with the segments.
  `data` serves local installs like fonts. `confroot` serves symlinks
  to shipped resources.

## CLI guide

```sh
confit plan --profile profiles/desktop.lua [-o ./plan.json] [--root .] [--state ./state.json] [--plugins ./plugins] [--log-file ./confit.log]
confit status --profile profiles/desktop.lua [--root .] [--state ./state.json] [--plugins ./plugins] [--log-file ./confit.log]
```

- `-o`/`--output` is the explicit output path; omitted prints the plan to
  stdout. The summary always goes to stderr, so stdout carries the plan
  payload alone, with zero writes to home paths.
- `--root` (require resolution base) defaults to the profile file's parent.
- `--state` points at the previous-state file; omitted means empty previous.
- `--plugins` points at the plugin folder (`{user}/{name}/plugin.lua`);
  embedded defaults always load.
- `--log-file` sets the collision log path; empty resolves to a
  per-process file under the system temp folder. The run prints
  `log: <path>` on stderr after the summary.
