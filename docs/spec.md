# ConfIt spec (current)

Spec-Version: 0.6.0

Living description of what confit does today. If you want to know why
it looks like this, the intent behind each version lives in `../design/`.
History of this file lives in git tags (`just show-spec`).

## What is

ConfIt (Configure It) keeps configuration as code for
one user. It manages that user's files. Profiles declare the desired files in Lua, plans
preview the diff, apply writes it. Same profile always yields
the same documents.
Work flows in two phases. `plan` previews and diffs before `apply` touches
anything.

Working today means `plan` over Lua configs, portable bundle files
(plus named slots under `@`), `apply` with preview plus
prompt plus post-config hooks, past slots through `apply`,
`init` scaffolding. Apply removes state-recorded paths absent
from desired documents.

## Features

### Plan before apply

`confit plan` loads a Lua entrypoint, evaluates it to documents plus
patches plus hooks plus configs, runs patch callbacks in pipeline order, merges
hooks sharing argv plus path, hashes
the data, loads previous state, and diffs desired vs previous. Output:
a JSON bundle file carrying documents plus hooks plus a terminal summary, with zero writes to home
paths. The preview lists each hook as a `! run:` line with the
resolved absolute binary, and the literal `yes` covers files
plus hooks together.

### Plan and drift

Two comparisons drive `plan` once the state slot exists.

Comparison 1 is state versus disk. Each recorded document renders
then snapshots its path (`~` expands via the home folder). Absent
paths read as manually deleted. Unreadable paths report path plus
reason. Structured documents parse disk bytes by format then diff
dotted leaves. Text plus rc documents diff with unified hunks from
recorded to disk. Link documents compare target strings. Opaque
documents compare raw bytes, changed bytes surfacing as hash plus
size labels.
Text plus opaque documents carrying a recorded mode compare it
against the disk mode, mismatches surfacing as `mode` key lines.

Comparison 2 is plan versus state. Desired hashes diff against
recorded hashes. Absent means create. Different means update. Equal
means unchanged. Recorded only documents mean delete. Structured
plus link plus opaque updates show old to new values per key, opaque
under the `content` key. Text plus rc updates render content
hunks against recorded documents.

Drift notes lead the summary. Changed keys read
`~ {path}: {key} = {old} -> {new}`. Added keys read
`+ {path}: {key} = {new}`, removed keys read
`- {path}: {key} = {old}`. An explicit null value reads as
`null`, distinct from an absent key.
Link edits use `target` as the key. Hunks render content lines
from recorded to disk under the document header. File markers
never render. Removals read red, additions read green, context
stays plain. Missing lines read
`{path}: manually deleted. changed outside config: add to config
or the next apply loses them`. Unreadable lines read
`cannot read '{path}': {reason}. changed outside config: add to
config or the next apply loses them`.

Drift rides along with a successful run. Exit stays 0
while drift exists.

A missing state slot means the first run. The plan diffs
desired documents against disk bytes instead, rendering one
lifecycle block per document holding drift entries. Whole
disk-absent documents read as creates. Remaining groups read
as updates with disk values first. Structured plus link plus
opaque leaves read `~ {key} = {disk} -> {desired}`, text
plus rc hunks render content lines disk-first under the
document header, trees
collapse to `~ tree ({changed} of {total} files changed)`. Documents
holding no entries read no lines and leave the add count.
The counts line reads
`Bundle: {add} to add, {in_place} already in place.`

### Bundles on disk

- Bundle. One portable bundle per run (`-o ./plan.cb`, omitted
  stores a bundle under tmp and prints the path), git-storable.
  `-o` appends `.cb` when the path lacks the extension.
  `-o @work` stores a named slot under the user config folder
  as `plans/work.json`. Contains document metadata plus hook
  declarations plus `created_at` metadata (excluded from the SHA). Bundle
  format version 6. Text, structured, rc, plus link payloads
  stay inline. Opaque files plus tree members read as `blob`
  hash refs. Planning writes bundles holding their own blobs
  and the pool fills on apply alone.
- State. JSON manifest at the fixed slot under the
  OS config folder, missing files read empty. Holds recorded
  documents in path order, plan shaped, so a previous `-o` output feeds
  back directly. Hashes persist in the file and read trusted, so
  loads skip rendering. Binary bytes live gzipped once in the
  shared pool under content hashes (`blobs/<sha>`). Writes store
  missing blobs and skip present ones, so slots share stored
  bytes. Loads hydrate lazily. Disk bytes matching a hash skip
  pool reads, and missing blobs fail naming the hash. Version
  mismatches fail as unsupported before parsing. Hooks persist
  as pure data (argv, gates, checks) and re-evaluate each plan.
- History plus named slots. `previous/<stamp>.json` entries plus
  `plans/<name>.json` slots hold manifests against the same pool.
  Pruning drops pool entries referenced by no slot, history
  entry, or named slot. Apply prunes after archiving, so a
  rotated-out entry releases its bytes at once.
- Bundle: one portable `.cb` file holding `manifest.json` plus
  the referenced blobs alone, so any stored state travels by file.

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
Bundle: 2 to add, 0 to change, 0 to destroy.
log: /tmp/confit-123.log
```

```sh
collision on alias "cat": "eza" overwritten, "bat" wins
```

Hook lines ride beside the summary. Runnable hooks print
`! run: {absolute} {args}` in the preview. Passing checks
print `skipped: {argv} (checks pass)`. Closed gates print
`warn: {argv} cannot run ({gate})`. Apply runs hooks after
files land, printing `hook n of m: {argv}` beside a spinner,
with hook output streaming into the run log file.

Terminal runs paint updates yellow, additions green, removals
red, headers bold. Piped output stays plain text, and `NO_COLOR`
disables color. Result lines (summary, counts, plan path) go to
stdout. Prompts, previews, listings, and the `log:` line go to
stderr, so stdout stays pipeable while prompting. Drift notes precede
the plan half when disk reads show changes.

## Concepts

### Documents, patches, configs, profiles, plugins

A **document** is the unit that touches disk once `apply` exists.
Every document has a `path`. Bundle diffs and hashes happen at document
level alone. Six kinds cover everything. Structured plus plain text
plus rc plus link plus opaque plus tree.

A **patch** modifies documents through callbacks. `confit.patch.rc`
carries one callback for the rc document.
`confit.patch.structured` carries a format plus a path plus one
callback. The rc callback receives a live wrapper with `add(section,
entry)` over the three section names. The structured callback receives a live wrapper
with `set` plus `append`. Paths hold dotted keys plus single indices, one path per
call (`a.b[0]`). Each patch rides one of five priority levels, default
`NORMAL`. The engine sorts patches by priority desc plus config
declaration order and runs them in that order. Op order inside one callback stays verbatim.

A **config** is a named bag holding documents plus patches plus hooks for fonts,
tool settings, shell entries. The name serves as uid per plan plus
owner stamp on every patch; repeats are plan errors. One path holds
one document; repeated declarations fail as plan errors naming the
path. A patch to an undeclared document creates it. Patches to one
path agree on one format; mismatches fail as plan errors.
A config requires siblings through `require(name, hint?)`;
a missing target fails the plan naming both configs, with the
hint on its own line while present. Requires check existence
alone and compose nothing.

A **profile** is the composition root per user or role. Its return
value is the entire resource graph. A table holds `shells` plus
`configs` plus optional `documents`, or a zero-arg function returning
that table. The return value serves as the registration:

```lua
return {
  shells = { "bash" },
  documents = { rc },
  configs = { bat },
}
```

Profiles declare user-owned bases, configs declare tool-owned
documents or just contribute.

A **plugin** is Lua framework code under
`confit.plugin.{username}.{plugin_name}`, embedded defaults plus a
`--plugins` folder. Plugins compose primitives: installers, helpers,
dialects. Data alone crosses the engine boundary, in both directions.

A **hook** is a post-config step riding a config beside documents
plus patches. It holds an argv list plus `path` dirs plus a `when`
gate plus `checks` plus a timeout. Apply resolves `argv[0]` against
the hook path dirs plus the engine process PATH and prints the
absolute in the preview. Apply runs hooks after documents
materialize. A closed gate warns and excuses the hook. Passing
checks skip it, failing checks run it, still-failing checks fail
the apply and abort the rest. Hooks sharing argv plus path merge
into one run. Gates join with OR, checks concatenate, timeout
takes the max. Timeouts read Lua-shaped durations (`1h10m10s`,
bare digits as seconds), default `10m`.

### Documents plan produces

`structured` plus `text` plus `rc` plus `link` plus `opaque`
plus `tree` assembled by the engine from profile documents plus config documents
plus patch output. Structured documents merge through live patch
callbacks. Plain text plus link plus opaque read declaration only.
Tree documents read one archive plus one destination plus one picker.

| Kind | How it is built | Merge rule |
| --- | --- | --- |
| `structured` | `confit.document.structured(format, { path, data })` declarations; `confit.patch.structured(format, path, fn)` tweaks | callbacks run in pipeline order, first writer wins per slot |
| `text` | `confit.document.text(path, content, opts?)` declarations | one path holds one document; repeats fail as plan errors |
| `link` | `confit.document.link(path, target)` declarations | same rule as text |
| `opaque` | `confit.document.opaque(path, content, opts?)` declarations holding raw bytes | same rule as text |
| `tree` | `confit.document.tree(archive, dest, fn)` declarations holding one managed file set | same rule as text |
| `rc` | `confit.document.rc.new({ profile, config, final })` base; `confit.patch.rc(fn)` tweaks; one file per declared shell | sections plus slots, first writer wins, see Shell rc |

Formats cover `json`, `toml`, `yaml`. Template rendering lives in the
`solrachq.template` Lua plugin over `load_text` plus `utils.render`
plus a plain text document; the fixtures exercise `toml` plus
templates.

### Shell rc

The rc document holds three sections. Sections mark position
plus guard alone. `profile` renders before the guard, `config`
renders after it, `final` renders last. Any entry kind renders
in any section. Every key stays optional. One plan holds one rc base:
the profile or one config declares it through
`confit.document.rc.new`; repeats fail naming both owners. Entry
tables live in `rc.new` section buckets alone; `config:add_document`
rejects bare entries as plan errors. `confit.patch.rc` tweaks entries
through `add(section, entry)` over the three section names; unknown
sections fail as plan errors.

Entry builders take `when` alone through opts. `when` holds a
condition table or a builder function over `confit.runtime`, evaluated
by each new shell session. Unknown opts fields fail as plan errors.
`rc.prepend` takes a dir alone for `PATH` or var plus dir.
Init strings render a `{{shell}}` slot with
the target shell name, so one entry addresses every shell.

```lua
local rc = confit.document.rc
return rc.new({
  profile = { rc.prepend(confit.path.home(".local/bin")) },
  config = { rc.alias("ll", "ls -l") },
  final = { rc.eval({ "starship", "init", confit.runtime.SHELL }) },
})
```

A write to a slot another patch wrote drops, plus one collision line
in the log. Named entries share one slot per name plus guard across
every section. One name under another guard holds its own slot, first
writer wins per slot. Exec entries (`eval`, `cmd`, `source`)
accumulate with no collision.

Rc layout per file holds profile lines, then the guard, then config
lines, then final lines. The guard renders while config or final holds
entries. Within one section, entries render in declaration order.
Patches run in pipeline order, so added entries follow the order
their patches ran. One file renders per declared shell. `bash` writes
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

Opaque documents carry raw bytes end to end. `load_bytes` plus
compressed callbacks plus `fetch_file` bodies supply the bytes,
`confit.document.opaque(path, content, opts?)` declares them, bundle files hold
blob refs into the shared pool, hashes cover raw bytes, apply writes raw bytes. The summary
lists creates as `opaque (n bytes)` bodies and updates as `~ content`
hash lines; kind changes to or from opaque count as updates with a
`~ kind` line.

Text plus opaque declarations accept `{ mode = ... }` carrying
`"755"` octal or `"rwxr-xr-x"` symbolic shape. Omitted means the
process umask. Apply sets the recorded mode after writing bytes.
Structured documents never carry modes.

### Trees

`confit.document.tree(archive, dest, fn)` builds one document
holding many files under one destination folder. The callback
takes the same `(name, info, content)` shape as `compressed`
and returns a destination-relative path per kept member, nil
per skip. Relative paths stay under the folder. Empty plus
absolute plus dot-dot carriers fail as plan errors, repeats
fail as plan errors, empty picks fail as plan errors naming
the filter. The manifest sorts by relative path. Member modes
inherit the archive executable bit (`755` where set, `644`
otherwise), so trees never depend on the umask.

Tree documents carry member bytes end to end. Bundle files hold
blob refs per member, hashes cover the canonical manifest (octal
mode plus relative path plus member sha per line). The summary
lists creates as `tree (n files)` bodies and updates as
`~ tree (changed of total files changed)` lines; deletes name
the kind with its count. Drift walks the destination member
by member. Missing members report missing under their joined
path, changed bytes report hash plus size labels under the
member key, changed modes report under the member mode key.
Disk extras stay quiet. Apply writes members to their joined
paths with per-member modes, then removes recorded members
absent from the desired manifest. Hand-placed files stay
untouched, destinations never delete.

### Hashing

Tables hold fixed order (`BTreeMap`), ordered lists (`env`, `profile`,
`init`) keep declaration order. Each document hashes with SHA-256 over
its rendered bytes. Tree documents hash the canonical manifest
instead. One `mode relative blob` line per member in relative path order.

## DSL guide

`confit.config(name)` returns a handle: userdata with four
methods, `add_document` plus `add_patch` plus `add_hook` plus
`require`. Everything nice lives in
plugins composing these primitives:

```lua
local mise = confit.plugin.solrachq.mise

local bat = mise.package({
  name = "bat",
  rc_builder = function(rc)
    rc:alias("cat", "bat --colors=always")
    rc:alias("c", "bat")
  end,
})
return bat
```

One call generates the config under the package name, declares the
install hook, requires the installer config, and sets `when` on every callback entry against the binary.
`mise.init(version?)` returns the installer config holding the
mise binary plus the activation patch; an explicit version wins, omitted resolves the
latest release.

```lua
local nerd_fonts = confit.plugin.solrachq.nerd_fonts

local fonts = nerd_fonts.font("JetBrainsMono", "3.5.1")
```

One call generates the config under the font name, builds the
tree document under the managed `fonts/{name}` folder, and
declares the `fc-cache -f` hook scoped to that folder. Each
font carries its own hook argv, so every font refresh runs on
its own with no shared installer config. Omitted
versions resolve the latest release. Raw bags stay available:

```lua
local c = confit.config("bat")
c:add_document(confit.document.rc.alias("cat", "bat", {
  when = confit.runtime.in_path("bat"),
}))
c:add_document(confit.document.structured("toml", {
  path = path,
  data = data,
}))
c:add_patch(confit.patch.structured("toml", path, function(data)
  data:set("user.theme", "catppuccin")
end):priority(confit.priority.HIGH))
c:add_hook(confit.hook.run({ "mise", "install" }, {
  path = { confit.path.home(".local/bin") },
  when = confit.runtime.in_path("mise"),
  checks = { confit.runtime.in_path("bat") },
  timeout = "10m",
}))
c:require("plugin:solrachq/mise:install", "Add mise.init() to the profile configs.")
```

- `confit.document.rc.alias/env/prepend/eval/cmd/source`
  build rc entry tables, each taking `when` through opts.
  `confit.document.structured/text/link/opaque` plus
  `confit.document.rc.new` build document tables for
  `config:add_document`. `confit.document.compressed` unpacks
  one archive into kept documents, `confit.document.tree`
  unpacks one archive into one managed file set.
- `confit.patch.rc(fn)` plus `confit.patch.structured(format, path, fn)`
  build patch handles for `config:add_patch`. The chainable `:priority`
  method sets one of five levels (`MINOR`, `LOW`, `NORMAL`, `HIGH`,
  `MAJOR`), default `NORMAL`.
- `confit.runtime.env_eq/env_set/in_path/exists/all/any/nop` build
  conditions as data. `when` takes a shape directly or a builder
  function over `confit.runtime`, run during evaluation.
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
  sidecar cache. The first download writes bytes plus a `.sha` digest,
  later runs reuse passing bytes. `--re-fetch` forces fresh downloads.
  Cache paths feed `load_bytes` plus `compressed` plus `tree` directly.
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
confit [--log-file ./confit.log] [--log-level debug] plan profiles/desktop.lua [-o ./plan.cb|@name] [--root .] [--plugins ./plugins] [--re-fetch]
confit [--log-file ./confit.log] [--log-level debug] apply [SOURCE] [--force] [--root .] [--plugins ./plugins] [--re-fetch]
confit export [PICKER] [-o ./out.cb] [-m]
confit delete @name
confit init [DIR]
```

- `-o`/`--output` is the explicit output path holding a bundle,
  or `@name` for a named slot under the user config folder
  (`plans/{name}.json`); omitted stores a bundle
  under tmp and prints the path. The summary goes to stdout, with zero
  writes to home paths.
- `--root` (require resolution base) defaults to the profile file's parent.
- `apply` reads its positional by shape: `.lua` plus extensionless
  paths evaluate a profile, `.cb` runs a bundle file, `@name`
  runs a named slot, `%N` runs history newest-first from one.
  Bundle files run on the file alone with no profile and no preview.
  Previous reads the fixed slot in every shape, and the new plan writes back to it. Experiments
  apply a bundle file, backups copy a bundle file, sharing sends a bundle file.
- `export` packs one slot into a portable bundle file and prints
  the path. The picker reads `%N` history newest-first from one,
  `@name` a named slot, nothing the applied slot; other values
  refuse as unsupported pickers. `-o` names the destination and
  gains `.cb` unless present, omitted derives the name from the
  slot (`applied.cb`, `personal.cb`, `prev-2.cb`). `-m`/`--manifest`
  prints pretty manifest JSON to stdout, holding blob references
  with zero inline bytes. `-o` plus `--manifest` together refuse.
- `delete` takes one `@name` slot. It drops the named manifest
  and prunes pool blobs orphaned by the removal, keeping bytes
  shared with remaining slots.
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
