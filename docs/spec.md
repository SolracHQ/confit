# ConfIt spec (current)

Spec-Version: 0.5.0

Living description of what confit does today. If you want to know why
it looks like this, the intent behind each version lives in `../design/`.
History of this file lives in git tags (`just show-spec`).

## What is

ConfIt (Configure It) is a CaC tool: configuration as code for
one machine. It manages user-space files, never systems,
never fleets. Profiles declare the desired files in Lua, plans
preview the diff, apply writes it. Same profile always yields
the same documents.
Two-phase workflow: `plan` previews and diffs before `apply` touches
anything.

Working today: `plan` over Lua configs, JSON plans on disk,
`apply` with preview plus prompt, `recover` over stored plans, `init`
scaffolding. Apply removes state-recorded paths absent from desired
documents.

## Features

### Plan before apply

`confit plan` loads a Lua entrypoint, evaluates it to documents plus
patches plus configs, runs patch callbacks in pipeline order, hashes
the data, loads previous state, and diffs desired vs previous. Output:
a JSON plan file plus a terminal summary, with zero writes to home
paths.

### Plan and drift

Two comparisons drive `plan`.

Comparison 1 is state versus disk. Each recorded document renders
then snapshots its path (`~` expands via the home folder). Absent
paths read as manually deleted. Unreadable paths report path plus
reason. Structured documents parse disk bytes by format then diff
dotted leaves. Text documents diff with unified hunks from recorded
to disk. Link documents compare target strings. Opaque documents
compare raw bytes, changed bytes surfacing as hash plus size labels.
Text plus opaque documents carrying a recorded mode compare it
against the disk mode, mismatches surfacing as `mode` key lines.

Comparison 2 is plan versus state. Desired hashes diff against
recorded hashes. Absent means create. Different means update. Equal
means unchanged. Recorded only documents mean delete. Structured
plus link plus opaque updates show old to new values per key, opaque
under the `content` key.

Drift notes lead the summary. Key edits read
`~ {path}: {key} = {old} -> {new}` with `null` for absent sides.
Link edits use `target` as the key. Hunks land verbatim as unified
diffs from recorded to disk. Missing lines read
`{path}: manually deleted. changed outside config: add to config
or the next apply loses them`. Unreadable lines read
`cannot read '{path}': {reason}. changed outside config: add to
config or the next apply loses them`.

Invariant: drift rides along with a successful run. Exit stays 0
while drift exists.

### Plans on disk

- Plan: JSON pretty-printed (`-o ./plan.json`, omitted stores under tmp
  and prints the path), diffable, git-storable.
  Contains document data plus `created_at`
  metadata (excluded from the SHA).
- State: JSON file (`--state`; omitted means the fixed slot under the
  OS config folder, missing files read empty). Holds full recorded
  documents in path order, plan shaped, so a previous `-o` output feeds
  `--state` directly. Hashes persist in the file and read trusted, so
  loads skip rendering. Opaque bytes persist base64. Version mismatches
  fail as unsupported before parsing.

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
disables color. Result lines (summary, counts, plan path) go to
stdout. Prompts, previews, listings, and the `log:` line go to
stderr, so stdout stays pipeable while prompting. Drift notes precede
the plan half when disk reads show changes.

## Concepts

### Documents, patches, configs, profiles, plugins

A **document** is the unit that touches disk once `apply` exists.
Every document has a `path`. Plan diffs and hashes happen at document
level alone. Five kinds cover everything: structured plus plain text
plus rc plus link plus opaque.

A **patch** modifies documents through callbacks. `confit.patch.rc`
carries one callback for the rc document.
`confit.patch.structured` carries a format plus a path plus one
callback. The rc callback receives a live wrapper with `add(section,
entry)` over the three section names. The structured callback receives a live wrapper
with `set` plus `append`. Paths hold dotted keys plus single indices, one path per
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
value is the entire resource graph: a table with `shells` plus
`configs` plus optional `documents`, or a zero-arg function returning
that table. The return value serves as the registration:

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

`structured` plus `text` plus `rc` plus `link` plus `opaque`
assembled by the engine from profile documents plus config documents
plus patch output. Structured documents merge through live patch
callbacks. Plain text plus link plus opaque read declaration only.

| Kind | How it is built | Merge rule |
| --- | --- | --- |
| `structured` | `confit.document.structured(format, { path, data })` declarations; `confit.patch.structured(format, path, fn)` tweaks | callbacks run in pipeline order, first writer wins per slot |
| `text` | `confit.document.text(path, content, opts?)` declarations | one path holds one document; repeats fail as plan errors |
| `link` | `confit.document.link(path, target)` declarations | same rule as text |
| `opaque` | `confit.document.opaque(path, content, opts?)` declarations holding raw bytes | same rule as text |
| `rc` | `confit.document.rc.new({ profile, config, final })` base; `confit.patch.rc(fn)` tweaks; one file per declared shell | sections plus slots, first writer wins, see Shell rc |

Formats cover `json`, `toml`, `yaml`. Template rendering lives in the
`solrachq.template` Lua plugin over `load_text` plus `utils.render`
plus a plain text document; the fixtures exercise `toml` plus
templates.

### Shell rc

The rc document holds three sections. Sections mark position
plus guard alone: `profile` renders before the guard, `config`
renders after it, `final` renders last. Any entry kind renders
in any section. Every key stays optional. One plan holds one rc base:
the profile or one config declares it through
`confit.document.rc.new`; repeats fail naming both owners. Entry
tables live in `rc.new` section buckets alone; `config:add_document`
rejects bare entries as plan errors. `confit.patch.rc` tweaks entries
through `add(section, entry)` over the three section names; unknown
sections fail as plan errors.

Entry builders take `when` alone through opts. `when` holds a
condition table or a builder function over `confit.shell`, evaluated
by each new shell session. Unknown opts fields fail as plan errors.
`rc.prepend` takes a dir alone for `PATH` or var plus dir.
Init strings render a `{{shell}}` slot with
the target shell name, so one entry addresses every shell.

```lua
local rc = confit.document.rc
return rc.new({
  profile = { rc.prepend(confit.path.home(".local/bin")) },
  config = { rc.alias("ll", "ls -l") },
  final = { rc.eval({ "starship", "init", confit.shell.SHELL }) },
})
```

A write to a slot another patch wrote drops, plus one collision line
in the log. Named entries share one slot per name plus guard across
every section: one name under another guard holds its own slot, first
writer wins per slot. Exec entries (`eval`, `cmd`, `source`)
accumulate with no collision.

Rc layout per file holds profile lines, then the guard, then config
lines, then final lines. The guard renders while config or final holds
entries. Within one section, entries render in declaration order.
Patches run in pipeline order, so added entries follow the order
their patches ran. One file renders per declared shell: `bash` writes
`~/.bashrc`, `zsh` writes `~/.zshrc`, other names write `~/.{name}rc`.

```sh
case $- in
*i*) ;;
*) return ;;
esac
```

Setup-only output skips the guard. Blocks join with one blank line. Plain entries render as bare lines.
Guarded entries render as if/then/fi blocks, evaluated by each new shell session.

```sh
if command -v bat >/dev/null 2>&1; then
  alias cat=bat
fi
```

### Archives plus opaque

`confit.document.compressed(path, fn)` unpacks one archive into kept
documents in callback order. Gzip bodies gunzip first, then parse as
tar with one single-file fallback unless the name wants tar (`.tar.gz`,
`.tgz`, `.tar`). Zip bodies parse as zip. Other bodies parse as tar.
Directory members skip. The callback takes `(name, info, content)`:
member path, `{ size, executable }`, raw bytes. It returns one document
per kept member, nil per skip; other returns fail as plan errors. Kept
documents ride the profile `documents` array or `config:add_document`.

Opaque documents carry raw bytes end to end: `load_bytes` plus
compressed callbacks plus `fetch_file` bodies supply the bytes,
`confit.document.opaque(path, content, opts?)` declares them, plan JSON holds
base64, hashes cover raw bytes, apply writes raw bytes. The summary
lists creates as `opaque (n bytes)` bodies and updates as `~ content`
hash lines; kind changes to or from opaque count as updates with a
`~ kind` line.

Text plus opaque declarations accept `{ mode = ... }` carrying
`"755"` octal or `"rwxr-xr-x"` symbolic shape. Omitted means the
process umask. Apply sets the recorded mode after writing bytes.
Structured documents never carry modes.

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

- `confit.document.rc.alias/env/prepend/eval/cmd/source`
  build rc entry tables, each taking `when` through opts.
  `confit.document.structured/text/link/opaque` plus
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
- `confit.resources.load_bytes(path)` reads a root-relative file into a
  raw-bytes string for opaque use. Absolute cache paths from
  `fetch_file` also read.
- `confit.resources.fetch_text(url, opts?)` fetches one URL body into a
  string; bodies outside UTF-8 fail as plan errors.
  `confit.resources.fetch_file(url, opts?)` streams one URL into the OS
  cache and returns its absolute path. `opts` holds `sha256` alone, 64
  hex chars; mismatches fail naming want plus got. Both calls share one
  sidecar cache: the first download writes bytes plus a `.sha` digest,
  later runs reuse passing bytes. `--re-fetch` forces fresh downloads.
  Cache paths feed `load_bytes` plus `compressed` directly.
- `confit.utils.render(template, vars)` renders minijinja slots with a
  vars table. Syntax failures are plan errors. `confit.utils.holds_cycle`
  plus `confit.utils.is_array` check table shapes.
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
confit [--log-file ./confit.log] [--log-level debug] plan profiles/desktop.lua [-o ./plan.json] [--root .] [--state ./state.json] [--plugins ./plugins] [--re-fetch]
confit [--log-file ./confit.log] [--log-level debug] apply [PROFILE] [--plan ./plan.json] [--force] [--root .] [--state ./state.json] [--plugins ./plugins] [--re-fetch]
confit recover [INDEX] [--force] [--state ./state.json]
confit init [DIR]
```

- `-o`/`--output` is the explicit output path; omitted stores the payload
  under tmp and prints the path. The summary goes to stdout, with zero
  writes to home paths.
- `--root` (require resolution base) defaults to the profile file's parent.
- `--state` points at the previous-state file; omitted means the fixed slot
  under the OS config folder, missing files read empty.
- `apply --plan FILE` runs on the file alone with no profile flag; PROFILE
  stays required otherwise. Previous reads the same resolved state file in
  both shapes, and the new plan writes back to it.
- `recover` with no index lists stored plans as `index @ timestamp` lines;
  with an index it re-applies the picked plan through preview plus prompts.
  `--force` skips the first prompt while drift still re-prompts. `--state`
  names the file gaining the re-applied plan.
- `init [DIR]` writes `profile.lua` plus editor stubs under DIR, omitted
  means the current folder; present files abort the run with zero writes.
- `--re-fetch` forces remote downloads past the sidecar cache; omitted
  reuses passing cached bytes.
- `--plugins` points at the plugin folder (`{user}/{name}/plugin.lua`);
  omitted means `{root}/plugins`. Embedded defaults always load. Each
  plugin ships its own stubs beside its entrypoint
  (`{user}/{name}/plugin.d.lua`) for language servers; the loader
  reads `plugin.lua` only.
- `--log-file` is a global flag setting the collision log path; empty resolves to a
  per-process file under the system temp folder. The run prints
  `log: <path>` on stderr after the summary.
- `--log-level` is a global flag gating verbosity; omitted means warn.
