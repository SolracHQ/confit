# Lua Framework Reference

The framework exposes one `confit` global. Each namespace
holds constructors. Constructors return tables or handles.
Configs collect documents plus patches plus hooks. The
profile returns shells plus configs plus documents.

```lua
local c = confit.config("bat")
c:add_document(confit.document.text(path, content))
c:add_patch(confit.patch.structured("toml", path, function(data)
  data:set("user.theme", "catppuccin")
end):priority(confit.priority.HIGH))
c:add_hook(confit.hook.run({ "mise", "install" }, {
  path = { confit.path.home(".local/bin") },
  requires = confit.runtime.in_path("mise"),
  when = confit.runtime.changed("~/.config/mise/config.toml"),
  checks = { confit.runtime.in_path("bat") },
  timeout = "10m",
}))
c:require("plugin:solrachq/mise:install", "Add mise.init() to configs.")
```

### Document

Builders return document tables. Tables carry a kind
marker. `config:add_document` accepts document tables
only.

#### `confit.document.structured`

```lua
confit.document.structured(format, { path = path, data = data })
```

The format names `json`, `toml`, or `yaml` in any letter
case. The args table holds `path` plus `data` only. Data
holds string keys plus JSON shaped values.

Preconditions hold the format known plus args keys known.
Plan errors name unknown formats plus unknown args
fields plus non-string args keys.

#### `confit.document.text`

```lua
confit.document.text(path, content, opts?)
```

The call takes a path plus content plus optional opts.
Opts holds `mode` plus `unmanaged`. The mode reads octal
or symbolic. `unmanaged` marks existence-only documents.

Preconditions hold path plus content strings. Plan
errors name unknown opts fields plus bad modes.

#### `confit.document.link`

```lua
confit.document.link(path, target)
```

The call takes a link path plus a target string.

Preconditions hold both args strings. Plan errors name
non-string fields.

#### `confit.document.opaque`

```lua
confit.document.opaque(path, content, opts?)
```

The call takes a path plus raw bytes plus optional opts.
Lua strings carry the bytes without text conversion. Opts
holds `mode` plus `unmanaged`. `unmanaged` marks
existence-only documents.

Preconditions hold path plus content strings. Plan errors
name unknown opts fields plus bad modes.

#### `confit.document.compressed`

```lua
confit.document.compressed(path, function(name, info, content)
  if name == "mise/bin/mise" then
    return confit.document.opaque(dest, content, { mode = "755" })
  end
end)
```

The call takes an archive path plus a callback. The
callback takes member path plus info plus raw bytes. Info
holds `size` plus `executable`. The callback returns one
document per kept member. It returns nil per skip.

Preconditions hold a readable archive plus a function
callback. Plan errors name empty paths plus jail escapes
plus unreadable archives plus non-document returns.

#### `confit.document.tree`

```lua
confit.document.tree(archive, dest, function(name, info, content)
  if not name:match("%.ttf$") then
    return nil
  end
  return name:match("([^/]+)$")
end)
```

The call takes an archive path plus a destination folder
plus a picker. The picker takes the callback shape. It
returns a destination-relative path per kept member. It
returns nil per skip.

Preconditions hold a readable archive plus a non-empty
dest plus a function picker. Plan errors name empty
paths plus jail escapes plus non-string returns plus
empty returns plus absolute returns plus dot-dot returns
plus repeated keeps plus empty picks.

#### `confit.document.rc.new`

```lua
confit.document.rc.new({
  profile = { ... },
  config = { ... },
  final = { ... },
})
```

The call takes section buckets. Keys name `profile`,
`config`, or `final`. Each key stays optional. Values
hold lists of rc entry tables.

Preconditions hold string section names from the known
three. Plan errors name non-string names plus unknown
sections.

#### `confit.document.rc.alias`

```lua
confit.document.rc.alias(name, value, opts?)
```

The call takes an alias name plus an expansion plus
optional opts. Opts holds `when` only.

Preconditions hold name plus value strings. Plan errors
name unknown opts fields plus bad guards.

#### `confit.document.rc.env`

```lua
confit.document.rc.env(name, value, opts?)
```

The call takes a variable name plus a value plus optional
opts. Opts holds `when` only.

Preconditions hold name plus value strings. Plan errors
name unknown opts fields plus bad guards.

#### `confit.document.rc.prepend`

```lua
confit.document.rc.prepend(dir, opts?)
confit.document.rc.prepend(var, dir, opts?)
```

The call takes a dir alone for `PATH` or a var plus a
dir. Opts holds `when` only.

Preconditions hold string dirs plus string vars. Plan
errors name bad arity plus unknown opts fields plus bad
guards.

#### `confit.document.rc.eval`

```lua
confit.document.rc.eval(argv, opts?)
```

The call takes an argv array plus optional opts. Opts
holds `when` only. Argv entries render the `{{shell}}`
slot per shell.

Preconditions hold a dense argv string array. Plan errors
name sparse arrays plus non-string entries plus unknown
opts fields plus bad guards.

#### `confit.document.rc.cmd`

```lua
confit.document.rc.cmd(argv, opts?)
```

The call takes an argv array plus optional opts. Opts
holds `when` only.

Preconditions hold a dense argv string array. Plan errors
name sparse arrays plus non-string entries plus unknown
opts fields plus bad guards.

#### `confit.document.rc.source`

```lua
confit.document.rc.source(path, opts?)
```

The call takes a file path plus optional opts. Opts holds
`when` only.

Preconditions hold a string path. Plan errors name
unknown opts fields plus bad guards.

### Patch

Constructors return patch handles. Handles target `rc`
or one document path. `config:add_patch` accepts patch
handles only. The `:priority` method sets the merge
level and returns the handle.

| Level | Rank |
| --- | --- |
| `MINOR` | 0, runs last |
| `LOW` | 1 |
| `NORMAL` | 2, the default |
| `HIGH` | 3 |
| `MAJOR` | 4, runs first |

#### `confit.patch.rc`

```lua
confit.patch.rc(function(data)
  data:add("config", confit.document.rc.alias("cat", "bat"))
end)
```

The call takes one callback. The callback takes the live
rc wrapper. The wrapper exposes `add` only. `add` takes
a section plus an entry table.

Preconditions hold a function callback plus entry tables
from the rc builders. Plan errors name non-functions
plus bad arity plus unknown sections plus non-entry
values.

#### `confit.patch.structured`

```lua
confit.patch.structured(format, path, function(data)
  data:set("tools.bat", "latest")
  data:append("user.plugins", "tabnine")
end)
```

The call takes a format plus a path plus a callback. The
callback takes the live structured wrapper. The wrapper
exposes `set` plus `append` only. `set` writes one dotted
path. `append` extends one list. Paths hold dotted keys
plus single indices. List positions count from 1, so
`servers[1].host` names the first server. `[0]` fails the
plan naming the path. One path rides each call.

Preconditions hold a known format plus a string path
plus a function callback. Plan errors name unknown
formats plus empty paths plus bad indices plus blocked
shapes plus out of bounds indices plus non-list appends
plus format mismatches against the base.

#### `:priority`

```lua
confit.patch.structured("toml", path, fn):priority(confit.priority.HIGH)
```

The method takes one level name. Names read any letter
case. The default reads `NORMAL`.

Preconditions hold one level name from MINOR, LOW, NORMAL,
HIGH, MAJOR. Plan errors name unknown levels.

### Runtime

Constructors return condition tables. Conditions ride
`when` plus `requires` plus `checks`. Shell sessions
evaluate rc guards. Apply evaluates hook gates against
the changed set.

#### `confit.runtime.env_eq`

```lua
confit.runtime.env_eq({ key = "SHELL", value = "bash" })
```

The call takes an opts table with `key` plus `value`.
The gate reads true while the variable equals the value.

Preconditions hold string leaves plus known keys only.
Plan errors name unknown opts fields plus non-string
leaves.

#### `confit.runtime.env_set`

```lua
confit.runtime.env_set({ key = "SHELL" })
```

The call takes an opts table with `key`. The gate reads
true while the variable holds a non-empty value.

Preconditions hold a string key plus known keys only.
Plan errors name unknown opts fields plus non-string
leaves.

#### `confit.runtime.in_path`

```lua
confit.runtime.in_path("mise")
```

The call takes one binary name. The gate reads true
while the binary resolves executable across the hook
path dirs plus the process PATH.

Preconditions hold a string name. Plan errors name
non-string names.

#### `confit.runtime.exists`

```lua
confit.runtime.exists("/opt/probe")
```

The call takes one path. The gate reads true while the
path stats. A leading tilde expands through the home
folder.

Preconditions hold a string path. Plan errors name
non-string paths.

#### `confit.runtime.changed`

```lua
confit.runtime.changed("~/.config/mise/config.toml")
```

The call takes one document path. The gate reads true
while the path sits in the changed set. Hooks only. Rc
guards holding it fail.

Preconditions hold a string path naming a built
document. Plan errors name unknown documents.

#### `confit.runtime.all`

```lua
confit.runtime.all({ cond_a, cond_b })
```

The call takes a dense condition array. The gate reads
true while every member holds. An empty list reads true.

Preconditions hold dense condition tables from index 1.
Plan errors name sparse lists plus bad nested shapes.

#### `confit.runtime.any`

```lua
confit.runtime.any({ cond_a, cond_b })
```

The call takes a dense condition array. The gate reads
true while some member holds. An empty list reads false.

Preconditions hold dense condition tables from index 1.
Plan errors name sparse lists plus bad nested shapes.

#### `confit.runtime.nop`

```lua
confit.runtime.nop(cond)
```

The call takes one nested condition. The gate reads true
while the nested gate reads false.

Preconditions hold one condition table. Plan errors name
bad nested shapes.

#### `confit.runtime.SHELL`

```lua
confit.document.rc.eval({ "mise", "activate", confit.runtime.SHELL })
```

The constant holds the literal `{{shell}}`. The plan
fold renders it per shell with minijinja. Entries
without template syntax pass through byte-identical.

### Resources

Loaders read project files. Fetchers read remote URLs
through a shared sidecar cache. Jail rules shape every
path. Root-relative paths resolve under the project
root. Cache-absolute paths resolve under the fetch
cache.

#### `confit.resources.load_toml`

```lua
local base = confit.resources.load_toml("resources/starship.toml")
```

The call takes one root-relative path. It returns a Lua
table.

Preconditions hold a readable path inside the jail plus
parsable TOML. Plan errors name absolute paths plus root
escapes plus unreadable files plus parse failures.

#### `confit.resources.load_json`

```lua
local base = confit.resources.load_json("resources/data.json")
```

The call takes one root-relative path. It returns a Lua
table.

Preconditions hold a readable path inside the jail plus
parsable JSON. Plan errors name absolute paths plus root
escapes plus unreadable files plus parse failures.

#### `confit.resources.load_yaml`

```lua
local base = confit.resources.load_yaml("resources/data.yaml")
```

The call takes one root-relative path. It returns a Lua
table.

Preconditions hold a readable path inside the jail plus
parsable YAML. Plan errors name absolute paths plus root
escapes plus unreadable files plus parse failures.

#### `confit.resources.load_text`

```lua
local text = confit.resources.load_text("resources/greeting.txt")
```

The call takes one root-relative path. It returns a Lua
string.

Preconditions hold a readable path inside the jail. Plan
errors name absolute paths plus root escapes plus
unreadable files.

#### `confit.resources.load_bytes`

```lua
local raw = confit.resources.load_bytes("resources/logo.bin")
```

The call takes one root-relative path or one
cache-absolute path. It returns a Lua string holding raw
bytes.

Preconditions hold a readable path inside the jail or
the cache. Plan errors name outside absolutes plus root
escapes plus unreadable files.

#### `confit.resources.fetch_text`

```lua
local body = confit.resources.fetch_text(url, { sha256 = digest })
```

The call takes a URL plus optional opts. Opts holds
`sha256` only. The digest holds 64 hex chars. The call
returns the body string. Bodies outside UTF-8 fail.

Preconditions hold a non-empty URL plus a well shaped
digest while present. The first download writes bytes
plus a digest sidecar. Later runs reuse passing bytes.
`--re-fetch` forces fresh downloads. Plan errors name
empty URLs plus unknown opts fields plus bad digests
plus sha mismatches plus fetch failures plus invalid
UTF-8.

#### `confit.resources.fetch_file`

```lua
local archive = confit.resources.fetch_file(url, { sha256 = digest })
```

The call takes a URL plus optional opts. Opts holds
`sha256` only. The call streams the body into the OS
cache and returns its absolute path. The cache path
feeds `load_bytes` plus `compressed` plus `tree`
directly.

Preconditions hold a non-empty URL plus a well shaped
digest while present. Cache rules match `fetch_text`.
Plan errors name empty URLs plus unknown opts fields
plus bad digests plus sha mismatches plus fetch
failures plus cache write failures.

### Path

Helpers join segments onto base folders. Each helper
takes string segments and returns a string path.

| Helper | Base |
| --- | --- |
| `home` | `$HOME` |
| `config` | OS config folder |
| `data` | OS data folder, serves local installs |
| `confroot` | project root, serves shipped symlinks |

```lua
confit.path.home(".local/bin")
confit.path.config("starship.toml")
confit.path.data("mise/shims/bat")
confit.path.confroot("resources/logo.bin")
```

Preconditions hold string segments. Plan errors name
non-string segments. A missing home folder fails with a
plan error naming the `HOME` variable.

### Utils

#### `confit.utils.render`

```lua
local out = confit.utils.render("hello {{ name }}", { name = "u" })
```

The call takes a template string plus a vars table. Vars
holds string keys plus JSON shaped values. Rendering
runs through minijinja.

Preconditions hold a string template plus an object
vars table. Plan errors name non-object vars plus
template syntax failures.

#### `confit.utils.holds_cycle`

```lua
if confit.utils.holds_cycle(value) then
  return
end
```

The call takes any Lua value. It returns true while one
table reaches itself through keys or values. Shared
tables without a cycle read false.

#### `confit.utils.is_array`

```lua
if confit.utils.is_array(value) then
  return
end
```

The call takes any Lua value. It returns true for dense
integer keys from 1. Objects plus empties plus sparse
lists read false.

### Hook

#### `confit.hook.run`

```lua
confit.hook.run({ "mise", "install" }, {
  path = { confit.path.home(".local/bin") },
  requires = confit.runtime.in_path("mise"),
  when = confit.runtime.changed("~/.config/mise/config.toml"),
  checks = { confit.runtime.exists(bin) },
  timeout = "10m",
})
```

The call takes an argv array plus optional opts. Argv
holds a dense non-empty string array. Argv executes
directly with no shell in between. Opts holds `path`
plus `requires` plus `when` plus `checks` plus `timeout`.

`path` extends PATH for the hook subprocess alone. Hook
path dirs search first. Runtime dirs follow. The preview
prints the resolved absolute binary.

`requires` gates the run on capability. A closed gate
warns and excuses the hook. `when` gates the run on
need. A closed gate skips it as unneeded. `requires`
plus `when` each take a condition table or a builder
function over the runtime namespace.

`checks` prove the run with condition tables. Passing
checks skip the hook. Failing checks run it. Checks
still failing after the run fail the apply and abort the
rest.

`timeout` caps the run in seconds. Values read
Lua-shaped durations like `1h10m10s`. Bare digits read
as seconds. The default reads `10m`.

Preconditions hold dense argv plus known opts keys plus
well shaped gates. Plan errors name empty argv plus
sparse argv plus non-string keys plus unknown opts
fields plus bad gates plus bad timeouts.

Hooks sharing argv plus path merge into one run.
First-seen order wins. Requires join with AND. Whens
join with OR. An ungated side keeps the merged hook
ungated on that slot. Checks concatenate. Timeout takes
the max.

### Config

#### `confit.config`

```lua
local c = confit.config("bat")
```

The call takes one name and returns a handle. The handle
carries `add_document` plus `add_patch` plus `add_hook`
plus `require`. Names serve as plan owners plus patch
owners. Repeats fail.

Preconditions hold a fresh name. Plan errors name
already defined configs.

#### `config:add_document`

```lua
c:add_document(confit.document.text(path, content))
```

The call takes one document table. Rc entry tables fail.
Rc bases ride `rc.new` tables only. One config declares
the rc base at most once.

Preconditions hold a document table with a kind marker.
Plan errors name non-documents plus rc entries plus
repeated rc bases plus bad document fields.

#### `config:add_patch`

```lua
c:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.alias("cat", "bat"))
end))
```

The call takes one patch handle. Handles keep
declaration order for tie breaks.

Preconditions hold a patch handle. Plan errors name
non-patch values.

#### `config:add_hook`

```lua
c:add_hook(confit.hook.run({ "fc-cache", "-f", dest }, {
  requires = confit.runtime.in_path("fc-cache"),
  when = confit.runtime.changed(dest),
}))
```

The call takes one hook table. Hooks keep declaration
order for merge order.

Preconditions hold a hook table with the hook marker.
Plan errors name non-hook values plus bad hook fields.

#### `config:require`

```lua
c:require("plugin:solrachq/mise:install", "Add mise.init() to configs.")
```

The call takes a sibling name plus an optional hint. The
hint renders on its own line while present. Requires
check existence alone and compose nothing. A missing
target fails the plan naming both configs.

Preconditions hold a non-empty string name plus a string
hint while present. Plan errors name empty names plus
non-string hints plus bad arity plus missing targets.

### Framework plugins

Embedded plugins ship with the engine. External folders
hold overrides by `user/name`. Embedded defaults win
while both exist, with one warning line. Loaded plugins
cache by `user/name`. Scoped `require` inside plugins
stays jailed to the plugin folder.

#### `mise.package`

```lua
local bat = mise.package({
  name = "bat",
  version = "latest",
  bin = "bat",
  aliases = { cat = "bat" },
  options = { extra_args = { "--style", "plain" } },
  rc_builder = function(rc)
    rc:alias("cat", "bat")
  end,
})
```

The call takes one opts table. `name` holds a non-empty
string. `version` defaults to `latest`. `bin` defaults
to `name` and guards rc entries through `in_path`.
`aliases` maps alias names to expansions and renders
sorted. `options` holds scalar or array values under
string keys. `rc_builder` takes a function over the rc
collector. Unknown fields fail.

The option fold shapes the shared mise TOML. A nil
`options` folds the version string under `tools.{name}`.
A present `options` folds a table holding `version` plus
each option under `tools.{name}`.

Each package requires the installer config. Each package
declares one shared `mise install` hook. The hook
carries path `~/.local/bin` plus `requires` on `mise`
plus `when` on the mise config path plus `checks` on the
shim for `bin`. Hooks sharing argv plus path merge into
one run across packages.

The rc collector stashes one patch per callback run.
Each collected entry carries the `in_path` guard for
`bin`. Each builder method takes a `section` key inside
its opts table, defaulting to `config`. The section key
never reaches the entry constructor.

Preconditions hold a table plus valid leaves. Plan
errors name non-table opts plus empty names plus empty
versions plus empty bins plus non-function builders
plus non-table aliases plus bad option leaves plus
unknown fields.

#### `mise.init`

```lua
local installer = mise.init("2026.9.10")
local installer = mise.init()
```

The call takes an optional version string. An explicit
version wins. An omitted version reads the latest
release tag from the releases feed and strips the
leading `v`. The module builds once. Repeated calls
return the first installer.

The installer holds the shared base plus the binary.
The base holds an empty `tools` table at
`~/.config/mise/config.toml`. The binary fetches the
release tarball and unpacks `mise/bin/mise`. It places
an opaque document under `~/.local/bin/mise` with mode
`755`. The tarball must hold the member exactly once.
Any other count fails naming the member. The installer
carries one rc patch with a PATH prepend plus an eval
entry holding the `{{shell}}` slot.

Preconditions hold a non-empty string version or nil.
Plan errors name empty versions plus unresolvable feeds
plus tags holding no version number plus installer
tarballs missing the member.

#### `nerd_fonts.font`

```lua
local font = nerd_fonts.font("JetBrainsMono")
local font = nerd_fonts.font("JetBrainsMono", "3.4.0")
```

The call takes a font name plus an optional version. An
explicit version wins. An omitted version reads the
latest release tag and strips the leading `v`. The call
fetches the release zip and builds one tree under
`data/fonts/{name}`. The picker keeps `.ttf` members
flattened to basenames.

Each font declares its own refresh hook. The argv reads
`fc-cache -f {dest}` for that font folder alone. The
hook carries `requires` on `fc-cache` plus `when` on the
destination plus no checks. It fires every apply while
the binary resolves.

Preconditions hold a non-empty name plus a non-empty
version or nil. Plan errors name empty names plus empty
versions plus unresolvable feeds plus empty picks.

#### `template`

```lua
local doc = template(path, { src = "resources/note.txt", vars = {} })
```

The call takes a destination path plus an opts table.
Opts holds `src` plus `vars`. `src` names a
root-relative file. `vars` holds a table defaulting to
empty. The call loads text plus renders through
minijinja plus returns a plain text document.

Preconditions hold a non-empty path plus a table opts
plus a non-empty src plus a table vars. Plan errors name
empty paths plus non-table opts plus unknown option
keys plus empty src plus non-table vars plus unreadable
sources plus render failures.

### Cross-cutting shapes

#### `when` table-or-builder

`when` takes a condition table directly or a builder
function over the runtime namespace. The engine calls
the builder during evaluation and validates the returned
table. Hook `requires` plus hook `when` plus rc `when`
share this shape.

```lua
{ when = confit.runtime.in_path("bat") }
{ when = function(rt) return rt.in_path("bat") end }
```

Preconditions hold a condition table or a function
returning one. Plan errors name non-tables plus bad
shapes plus builder failures.

#### `changed` first-run plus hooks-only

`changed` reads membership in the changed set. First
runs hold every built path in the set, so every gate
reads true. Later runs hold rewritten documents plus
drifted documents. Tree member drift maps to the parent
destination. Unknown paths fail the plan naming the
path.

`changed` rides hooks alone. Rc guards holding it fail
with a hooks-only plan error. The refusal names the
field.

```lua
c:add_hook(confit.hook.run({ "tool" }, {
  when = confit.runtime.changed("note"),
}))
```

#### Config as a function of user values

Config files return handles or functions. A function
takes user values and returns the handle. The profile
requires the file plus calls the function with user
values plus lists the returned config.

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

```lua
local starship = require("tools.starship")({ command_timeout = 10000 })
return {
  shells = { "bash" },
  configs = { installer, starship },
}
```

#### JSON-only data

Document data holds JSON shaped values alone. Strings
plus numbers plus booleans plus null plus arrays plus
objects pass. Functions fail. Userdata fails. Threads
fail. Recursive tables fail. Errors name the config
plus the field.

```lua
confit.patch.structured("toml", path, function(data)
  data:set("user.theme", "catppuccin")
end)
```
