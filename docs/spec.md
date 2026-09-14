# ConfIt spec (current)

Spec-Version: 0.4.0

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

`confit plan` loads a Lua entrypoint, evaluates it to documents plus
patches plus configs, runs patch callbacks in pipeline order, hashes
the data, loads previous state, and diffs desired vs previous. Output:
a JSON plan file plus a terminal summary, with zero writes to home
paths.

### Status and drift

`diff` compares each document's `data_hash` against the previous state in
memory: absent means create, different means update, equal means
unchanged, previous-only keys mean delete. Plan files carry data payloads
alone, with `data_hash` computed at runtime.

`plan` also snapshots each document path off disk (`~` expands via the
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
  Contains document data plus `created_at`
  metadata (excluded from the SHA).
- State: JSON file (`--state`; omitted means empty previous, the run
  skips disk reads). `{ document_id -> { data_hash, output_hash, data? } }`.

### Terminal summary and color

The summary lists every entry per document; winners surface in the
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

### Documents, patches, configs, profiles, plugins

A **document** is the unit that touches disk once `apply` exists.
Every document has a `path`. Plan diffs and hashes happen at document
level alone. Four kinds cover everything: structured plus plain text
plus rc plus link.

A **patch** modifies documents through callbacks. `confit.patch.rc`
carries one callback for the rc document.
`confit.patch.structured` carries a format plus a path plus one
callback. The callback receives a live wrapper with `set` plus
`append`. Paths hold dotted keys plus single indices, one path per
call (`a.b[0]`). Each patch rides one of five priority levels, default
`NORMAL`. The engine sorts patches by priority desc plus owner asc and
runs them in that order. Op order inside one callback stays verbatim.

A **config** is a named bag holding documents plus patches for fonts,
tool settings, shell entries. The name serves as uid per plan plus
owner stamp on every patch; repeats are plan errors. One path holds
one document; repeated declarations fail as plan errors naming the
path. A patch to an undeclared document creates it. Patches to one
path agree on one format; mismatches fail as plan errors.

A **profile** is the composition root per machine or role. Its return
value is the entire resource graph; the return value serves as the
registration:

```lua
return {
  shells = { "bash" },
  documents = { rc },
  configs = { bat },
}
```

Profiles declare machine-owned bases, configs declare tool-owned
documents or just contribute.

A **plugin** is Lua framework code under
`confit.plugin.{username}.{plugin_name}`, embedded defaults plus a
`--plugins` folder. Plugins compose primitives: installers, helpers,
dialects. Data alone crosses the engine boundary, in both directions.

### Documents plan produces

`structured` plus `text` plus `rc` plus `link` assembled by the engine
from profile documents plus config documents plus patch output.
Structured documents merge through live patch callbacks.
Plain text plus link read declaration only.

| Kind | How it is built | Merge rule |
| --- | --- | --- |
| `structured` | `confit.document.structured(format, { path, data })` declarations; `confit.patch.structured(format, path, fn)` tweaks | callbacks run in pipeline order, first writer wins per slot |
| `text` | `confit.document.text(path, content)` declarations | one path holds one document; repeats fail as plan errors |
| `link` | `confit.document.link(path, target)` declarations | same rule as text |
| `rc` | `confit.document.rc.new({ profile, config, final })` base plus bare rc entries; `confit.patch.rc(fn)` tweaks; one file per declared shell (`bash` writes `~/.bashrc`) | sections plus slots, first writer wins, see Shell rc |

Formats cover `json`, `toml`, `yaml`. Template rendering lives in the
`solrachq.template` Lua plugin over `load_text` plus `text.render`
plus a plain text document; the fixtures exercise `toml` plus
templates.

### Shell rc

The rc document holds three sections. Sections mark position
plus guard alone: `profile` renders before the guard, `config`
renders after it, `final` renders last. Any entry kind renders
in any section. Every key stays optional. `confit.document.rc.new` builds the
machine-owned base; bare rc entry tables attach through
`config:add_document`; `confit.patch.rc` tweaks entries through `set`
plus `append` over the three section names.

Entry builders take `when` through opts. `when` holds a
condition table or a builder function over `confit.shell`, evaluated
by each new shell session. Unknown opts fields fail as plan errors.
Init strings render a `{{shell}}` slot with
the target shell name, so one entry addresses every shell.

```lua
local rc = confit.document.rc
return rc.new({
  profile = { rc.path_entry(confit.path.home(".local/bin")) },
  config = { rc.alias("ll", "ls -l") },
  final = { rc.eval({ "starship", "init", confit.shell.SHELL }) },
})
```

A write to a slot another patch wrote drops, plus one collision line
in the log. Named entries collide globally on name: one slot per name
across every section, first writer wins. Exec entries accumulate with
no collision.

Rc layout per file holds profile lines, then the guard, then config
lines, then final lines. The guard renders while config or final holds
entries. Within one section, entries render in declaration order.
Patches run in pipeline order, so appended entries follow the order
their patches ran.

```sh
case $- in
*i*) ;;
*) return ;;
esac
```

Setup-only output skips the guard. Blocks join with one blank line. Plain entries render as bare lines.
Guarded entries render inline, evaluated by each new shell session.

```sh
if command -v bat >/dev/null 2>&1; then
  alias cat=bat
fi
```

### Hashing

Tables hold fixed order (`BTreeMap`), ordered lists (`env`, `profile`,
`init`) keep declaration order. Each document hashes with SHA-256 over
its rendered bytes.

## DSL guide

`confit.config(name)` returns a handle: userdata with two
methods, `add_document` plus `add_patch`. Everything nice lives in
plugins composing these primitives:

```lua
local mise = confit.plugin.solrachq.mise

local bat = mise.package("bat", function(rc)
  rc:alias("cat", "bat --colors=always")
  rc:alias("c", "bat")
end)
bat:add_document(mise.activate())
return bat
```

One call generates the config under the package name, declares the
install, and sets `when` on every callback entry against the binary.
`activate()` returns the eval entry for mise activation.
Raw bags stay available:

```lua
local c = confit.config("bat")
c:add_document(confit.document.rc.alias("cat", "bat", {
  when = confit.shell.in_path("bat"),
}))
c:add_document(confit.document.structured("toml", {
  path = path,
  data = data,
}))
c:add_patch(confit.patch.structured("toml", path, function(data)
  data:set("user.theme", "catppuccin")
end):priority(confit.priority.HIGH))
```

- `confit.document.rc.alias/env/profile/profile_path/path_entry/eval/cmd/source`
  build rc entry tables, each taking `when` through opts.
  `confit.document.structured/text/link` plus
  `confit.document.rc.new` build document tables for
  `config:add_document`.
- `confit.patch.rc(fn)` plus `confit.patch.structured(format, path, fn)`
  build patch handles for `config:add_patch`. The chainable `:priority`
  method sets one of five levels (`MINOR`, `LOW`, `NORMAL`, `HIGH`,
  `MAJOR`), default `NORMAL`.
- `confit.shell.env_eq/env_set/in_path/exists/all/any/nop` build
  conditions as data. `when` takes a shape directly or a builder
  function over `confit.shell`, run during evaluation.
- `confit.plugin.helpers.error(msg)` raises plan errors with plugin
  attribution.
- Tables in document data hold JSON-shaped values alone; `plan`
  rejects functions (and userdata) with an error naming config and
  field.

Config files may return a parametrizing function; the profile calls it
with user values:

```lua
return function(user_config)
  local path = confit.path.config("starship.toml")
  local base = confit.resources.load_toml("resources/starship.toml")
  starship:add_document(confit.document.structured("toml", {
    path = path,
    data = base,
  }))
  starship:add_patch(confit.patch.structured("toml", path, function(data)
    for key, value in pairs(user_config) do
      data:set(key, value)
    end
  end))
  return starship
end
```

- `confit.resources.load_toml/load_json/load_yaml(path)` read a
  root-relative file into a Lua table. Absolute paths and escapes above
  root are plan errors.
- `confit.resources.load_text(path)` reads a root-relative file into a
  Lua string. Absolute paths and escapes above root are plan errors.
- `confit.text.render(template, vars)` renders minijinja slots with a
  vars table. Syntax failures are plan errors.
- `confit.plugin.solrachq.merge(base, overlay, opts?)` deep-merges:
  tables recurse, everything else (arrays included) last-wins.
  `shallow = true` merges top-level keys only; `list_append = true`
  concatenates arrays keeping duplicates. Unknown option keys are plan
  errors.
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
