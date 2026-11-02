# Project

A project folder contains a profile and the folders that the project declares.
`project.toml` is optional. The folder bounds the project files that a profile
may load.

```text
site/
  project.toml
  main.lua
  plugins/_/sugar/init.lua
  stubs/confit.d.lua
```

A missing file uses the defaults. The profile is required for `bundle`, `check`,
and `apply`. The folder names in the file decide where `init --stubs`,
`init --plugins`, and `init --lsp` write.

## project.toml

```toml
version = "0.9.0"

[plugins]
folder = "plugins"
allow_external_folders = false

[stubs]
folder = "stubs"
```

| Field | Type | Default | Meaning |
| --- | --- | --- | --- |
| `version` | string | binary version | scaffold version |
| `plugins.folder` | path | `plugins` | plugin tree |
| `plugins.allow_external_folders` | bool | `false` | external plugins |
| `stubs.folder` | path | `stubs` | editor stubs |

A folder is project relative. An absolute path is kept. A missing field takes
its default, so this file is valid:

```toml
[plugins]
folder = "third_party/plugins"
```

The parser refuses an unknown field at any level. It also refuses malformed TOML
and text that is not UTF-8. A refused file stops the run before profile
evaluation.

The `version` field records the scaffold that wrote the file. Evaluation does
not compare it with the binary.

The plugin folder maps to the `plugins` require name. The folder
must resolve inside the project folder. The comparison uses canonical paths, so
a symlink counts at its target. A folder outside the project fails evaluation
unless `allow_external_folders = true`. The flag does not apply to the stubs
folder.

The stubs folder is editor metadata. Profile evaluation does not read it.
`init --stubs` writes it, and `init --lsp` lists it in the editor config.

## Profile selection

The default profile is `main.lua` inside the project folder.

```sh
confit check site
confit check site --profile server.lua
confit check site --profile ~/profiles/laptop.lua
```

`--profile FILE` resolves against the project folder. An absolute path is used
as written. A leading `~` expands to the home folder before resolution. An
explicit profile may sit outside the project folder. Its requires still resolve
against the project root.

A missing default profile fails as a missing project folder. A missing explicit
profile fails as an unreadable profile. One run evaluates one profile.

## Require

Each file runs with its own `require`. The name selects a builtin module or a
Lua file:

```lua
local sugar = require("plugins._.sugar")
local shell = require("tools.shell")
local render = require("confit.render")
```

A name that starts with `confit.` loads a builtin module. The
[Lua surface](lua.md) page lists the modules.

Any other name resolves under the project root, no matter which folder the
caller is in. A dot maps to a path separator. Empty components collapse, so
`a..b` and
`a//b` name `a/b`. A backslash, a drive prefix, and a leading slash fail as
invalid names.

A name tries a file and then an entry file:

- `tools.shell` tries `tools/shell.lua`, then `tools/shell/init.lua`.
- `plugins._.sugar` tries `{plugins.folder}/_/sugar.lua`, then
  `{plugins.folder}/_/sugar/init.lua`.

A name whose first component is `plugins` resolves under the plugin folder. Any
other name resolves under the project root. A name that ends in `.lua` maps the
suffix to a path component, so `require("helper.lua")` looks for
`PROJECT/helper/lua.lua`.

Each target resolves to a canonical path. The path must sit inside its scope:

- A project caller may load any file inside the project root and any file inside
  the plugin folder.
- A plugin caller may load only files inside the plugin folder.
- A builtin module is available to both callers.

A target outside the scope fails, including a target that reaches outside
through a symlink. A missing file fails with an open-module error. When
`allow_external_folders = true`, a plugin target may resolve outside the project
root, and the plugin-caller rule above no longer applies. A project target still
stays inside the root.

A require cycle fails with the chain from the first name back to itself. A
module value caches for the rest of the run. A module that returns nil is not
cached and loads again on the next require. Module tables from a builtin refuse
a write.

## Sandbox

Every profile and module runs in a table environment with these globals:

| Kind | Names |
| --- | --- |
| functions | `assert`, `error`, `ipairs`, `next`, `pairs` |
| functions | `pcall`, `select`, `tonumber`, `tostring`, `type` |
| libraries | `string`, `table`, `math`, `utf8` |
| app values | `require`, `id`, `route` |

The sandbox has no `os`, `io`, `package`, `debug`, or `coroutine` library. An
unknown global is nil. A file cannot read the
environment, start a process, read a clock, or write a file through a Lua
library. The builtin modules provide the supported effects; the
[Lua surface](lua.md) page states them.

## Editor files

`init --stubs` writes one stub per builtin module into the stubs folder:

```text
confit.d.lua
confit.fact.d.lua
confit.fetch.d.lua
confit.input.d.lua
confit.open.d.lua
confit.render.d.lua
confit.validate.d.lua
```

The stubs come from the current API of the binary. A refresh replaces them,
so they match the installed version.

`init --lsp` writes `.luarc.json` at the project root. The file also carries the
LuaLS settings schema in `$schema`. These values map the plugin folder and the
stubs folder from `project.toml`:

```json
{
  "runtime": {
    "version": "Lua 5.4",
    "path": ["?.lua", "?/init.lua"]
  },
  "diagnostics": {
    "globals": ["require"]
  },
  "workspace": {
    "library": ["plugins", "stubs"]
  }
}
```

The plain scaffold does not write `.luarc.json`. The [init](init.md) page states
the generated files and the refresh rules.
