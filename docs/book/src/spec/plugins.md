# Plugins

A plugin is Lua code that returns a value a profile can use: a config, a
document, a patch, a hook, or a function that builds one. The shipped plugins
are scaffold copies under `plugins/_`. `init` writes them into the project, and
the project owns them from then on. They are ordinary Lua modules that load
from the project folder.

See [Profile](profile.md) for the profile table and [Project](project.md) for
the plugin folder setting.

## Loading

`require` loads builtin modules, project modules, and plugins. A dotted name
maps to a path with slashes, then tries `<name>.lua` before `<name>/init.lua`.

```lua
local is = require("plugins._.is")         -- plugins/_/is/init.lua
local theme = require("plugins.solrac.theme") -- plugins/solrac/theme/init.lua
local shell = require("tools.shell")       -- tools/shell.lua
```

- `confit.<name>` loads a builtin module.
- A name whose first component is `plugins` resolves under the plugin folder.
  The prefix stays `plugins` when the folder has another name.
- Another name resolves under the project root.
- A name with a backslash, a drive prefix, or a leading slash fails.
- A project caller loads any file inside the project root.
- A plugin caller loads only files inside the plugin folder.
- A module cycle fails and shows the chain.
- Module values cache for one run.

A plugin caller that needs a sibling module uses the full name:

```lua
local is = require("plugins._.is")
local tab = require("plugins._.tab")
```

The plugin folder comes from `[plugins] folder`. A folder outside the project
root requires `allow_external_folders = true`.

## sugar

`require("plugins._.sugar")` returns route constructors, an rc document, a
patch, and entry constructors.

```lua
local sugar = require("plugins._.sugar")

sugar.home(".bashrc")       -- route("home", ".bashrc")
sugar.config("bat/config")  -- route("config", "bat/config")
sugar.data("fonts")         -- route("data", "fonts")
sugar.cache("tool")         -- route("cache", "tool")
sugar.literal("/opt/tool")  -- route("literal", "/opt/tool")
```

`sugar.rc(path)` returns a flat rc document:

```lua
sugar.rc(sugar.home(".zshrc"))
-- { path = home('.zshrc'), profile = {}, config = {}, final = {} }
```

`sugar.patch(target, apply)` returns a target patch:

```lua
sugar.patch(sugar.home(".bashrc"), function(document)
   table.insert(document.config, sugar.alias("ll", "ls -l"))
end)
```

A filter patch uses the plain table, because `sugar.patch` sets only `target`:

```lua
{
   filter = function(route)
      return route.base == "home" and route.relative:match("^%.%a+rc$")
   end,
   apply = function(document)
      table.insert(document.config, sugar.env("EDITOR", "nvim"))
   end,
}
```

Each constructor returns one entry table:

```text
sugar.alias(name, value)   { alias = { name = name, expansion = value } }
sugar.env(name, value)     { env = { name = name, value = value } }
sugar.prepend(dir)         { path = { name = "PATH", dir = dir } }
sugar.prepend_to(var, dir) { path = { name = var, dir = dir } }
sugar.eval(argv)           { eval = { argv = argv } }
sugar.cmd(argv)            { cmd = { argv = argv } }
sugar.source(path)         { source = { path = path } }
```

A constructor adds no `when` field. Add the condition inside the entry variant:
`entry.alias.when = condition`, `entry.env.when = condition`, or
`entry.eval.when = condition`.

## is

`require("plugins._.is")` returns value checks and three helpers.

| Call | True for |
| --- | --- |
| `is.string(value)` | A string |
| `is.nonempty(value)` | A string with at least one character |
| `is.num(value)` | A number |
| `is.bool(value)` | A boolean |
| `is.tab(value)` | A table |
| `is.fun(value)` | A function |

`is.assert(pred, value, message)` returns `value` when `pred(value)` is true. It
raises `message` otherwise.

`is.keys(prefix, checked, allowed, name)` raises when `checked` has a key
absent from `allowed`. The message is
`{prefix}: field '{name}' unknown field '{key}'`.

`is.must(prefix, value, reason)` returns `value` when it is not nil. It raises
`prefix .. reason` otherwise. The prefix carries its own trailing separator.

## tab

`require("plugins._.tab")` returns one utility.

`tab.clone(source)` returns a shallow copy of one table. A non-table raises
`tab: source must be a table`.

## mise

`require("plugins._.mise")` returns `init` and `package`.

`mise.init(version?)` builds the installer config once per evaluation. The
version is a release tag, or nil for the latest release from the releases feed.
The config reads `fact("os")` and `fact("arch")`, fetches the matching tarball,
and takes `mise/bin/mise`:

```text
base   https://github.com/jdx/mise/releases/download/v{version}/
asset  mise-v{version}-{platform}.tar.gz
```

| `fact("os")` | `fact("arch")` | `{platform}` |
| --- | --- | --- |
| `linux` | `x86_64` | `linux-x64` |
| `linux` | `aarch64` | `linux-arm64` |
| `macos` | `x86_64` | `macos-x64` |
| `macos` | `aarch64` | `macos-arm64` |

Any other pair fails the plan with
`mise: unsupported platform '{os}/{arch}'`.

The installer config:

- An opaque document at `~/.local/bin/mise` with mode `755`.
- A TOML document at `~/.config/mise/config.toml` with an empty `[tools]` table.
- A filter patch on every home dot-rc document (`~/.{shell}rc`). The patch
  prepends `~/.local/bin` to `PATH` and adds `eval "$(mise activate {shell})"`
  in the profile section, before the interactive guard.
- One install hook with id `plugin:_/mise:install`, argv
  `{ "mise", "install" }`, the `~/.local/bin` PATH entry, a `requires` check for
  `mise` on PATH, a `when` check on the config document hash, and a ten minute
  timeout.

Add the returned config to the profile `configs` list.

`mise.package(spec)` declares one tool and returns its config.

```lua
mise.package({
   name = "bat",
   version = "latest",
   bin = "bat",
   options = { backend = "aqua" },
   rc = {
      profile = { sugar.env("BAT_THEME", "catppuccin") },
      config = { sugar.alias("cat", "bat") },
      final = {},
   },
})
```

| Field | Meaning |
| --- | --- |
| `name` | The package name and the config name. Required. |
| `version` | The tool version. Defaults to `"latest"`. |
| `bin` | The binary name. Defaults to `name`. |
| `options` | Backend tool options. Optional. |
| `rc` | Rc entries. Optional. |

The package config has no documents, only patches and one requirement:

- A structured patch adds `tools.{name}` to the shared
  `~/.config/mise/config.toml`. The value is the version string, or a table of
  `options` and `version`.
- A hook patch adds one check to the shared install hook: `exists` on
  `data/mise/shims/{bin}`. Repeated packages join the checks with `all`.
- A filter patch adds the `rc` entries to every home dot-rc document. Each entry
  gains the guard `in_path(bin)`. An entry with its own `when` gains
  `{ all = { guard, when } }`.
- `requirements` records `plugin:_/mise:install` with the hint
  `add mise.init() to the profile configs`. A profile without the installer
  fails the plan with a missing-config error. The hint stays in the requirement
  and is not printed.

A positional `rc` list lands in `config`. A named `profile`, `config`, or
`final` list lands in that section. `options` has non-empty string keys with
string, number, or boolean values, or dense arrays of those values. An unknown
spec field fails.

Two packages with one `name` produce one config name twice, and the plan fails.

## nerd_fonts

`require("plugins._.nerd_fonts")` returns `font`.

```lua
nerd_fonts.font("JetBrainsMono", "3.5.1")
```

`font(name, version?)` resolves the version, or the latest release from the
releases feed, and fetches:

```text
https://github.com/ryanoasis/nerd-fonts/releases/download/v{version}/{name}.zip
```

The config:

- One tree document at `data('fonts/{name}')`.
- Every archive member whose name ends in `.ttf`, keyed by its archive path. A
  ref without a mode takes 644. An archive without a `.ttf` member
  fails.
- One hook with id `plugin:_/nerd_fonts:{name}`, argv
  `{ "fc-cache", "-f", dest }`, a `requires` check for `fc-cache` on PATH, and a
  `when` check on the tree document. The default timeout applies.

The plugin declares its own refresh hook. It does not share one with other
fonts.

## Authoring

Write a plugin under the plugin folder. The entry file is `<name>.lua` or
`<name>/init.lua`. The file returns a value.

```lua
local sugar = require("plugins._.sugar")
local open = require("confit.open")
local render = require("confit.render")
local validate = require("confit.validate")

return function(theme)
   local template = assert(assert(open("resources/bat.conf.j2")):text())
   local text = assert(render(template, { theme = theme }))
   local config = {
      name = id("theme"),
      documents = {
         { path = sugar.config("bat/config"), text = text },
      },
   }
   local checked, reason = validate.config(config)
   if not checked then
      error("theme: " .. tostring(reason))
   end
   return checked
end
```

- A config table and a function that returns one are the common shapes.
- `open` paths are project relative, so a plugin reads a project resource by its
  project path.
- A plugin caller loads a sibling module by its `plugins.` name.
- Return the validated table. An unknown field then fails at the plugin boundary
  with the plugin's own message.
- Config names and hook ids are unique across the profile. A repeated name fails
  the plan.

The module annotation sits in the plugin file as
`---@module "plugins.user.name"`. `.luarc.json` at the project root maps the
plugin folder and the stubs folder on the library path. `confit init --lsp`
writes the file. `confit init --stubs` writes the stub files that describe
`id`, `route`, the six modules, and every document shape. Run both after a
version change. See [init](init.md).
