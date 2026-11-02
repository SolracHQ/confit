# Plugins

Now that the theme name is hardcoded in the bat config, we will extract it into
a local plugin.

Your theme name repeats across configs. The bat file needs it. The shell export
needs it. You pass the same string in two places. A rename misses one.

Extract the theme into a local plugin:

```text
~/dotfiles/
  plugins/
    solrac/theme/init.lua
  resources/
    bat.conf.j2
```

```lua
local theme = require("plugins.solrac.theme")("catppuccin")
```

The profile calls the plugin once. The call passes the name to the template and
to the shell export. One call sets both. Delete `tools/batconfig.lua` and swap
it for the theme in the profile:

```lua
configs = { installer, shell, bat, eza, zoxide, ripgrep, starship, fonts, theme },
```

The document count stays flat. The theme takes over the bat config document that
`batconfig` owned.

A local plugin is your code under the plugin folder. A `_` plugin is a shipped
copy that `init` wrote into your project.

## The plugin file

```lua
---@module "plugins.solrac.theme"

local sugar = require("plugins._.sugar")
local open = require("confit.open")
local render = require("confit.render")
local validate = require("confit.validate")

--- Bat config template.
local TEMPLATE = "resources/bat.conf.j2"

--- Home dot-rc routes carry shell documents.
local SHELL_PATTERN = "^%.(%a+)rc$"

---@type Route
local BAT_CONFIG = sugar.config("bat/config")

--- Builds the bat theme config from the template.
---@param name string Bat theme name.
---@return Config Theme config for the profile configs array.
return function(name)
   local template = assert(assert(open(TEMPLATE)):text())
   local text = assert(render(template, { theme = name }))
   local config = {
      name = id("theme"),
      documents = {
         { path = BAT_CONFIG, text = text },
      },
      patches = {
         {
            filter = function(route)
               return route.base == "home"
                  and route.relative:match(SHELL_PATTERN) ~= nil
            end,
            ---@param document Document_Rc Working shell document under patching.
            apply = function(document)
               table.insert(document.config, sugar.env("BAT_THEME", name))
            end,
         },
      },
   }
   local checked, reason = validate.config(config)
   if not checked then
      error("theme: invalid config: " .. tostring(reason))
   end
   return checked
end
```

`plugins/solrac/theme` is the plugin folder. `init.lua` is its module entry. The
`---@module` line tells the editor the module. The module returns a function.
The profile calls it with one theme name.

`validate.config` checks one config table. The function returns the checked
table. A bad plugin stops the evaluation before the run builds a manifest.

The text document writes the rendered bytes. The filter matches the two shell
documents. The callback adds one export line inside the interactive guard.

## Require rules

A dotted name maps to a path. The run tries `<name>.lua` before
`<name>/init.lua`. A project caller loads any file inside the project root. A
plugin caller loads only files inside the plugin folder.

## Check and apply

The check lists every rendered line. This excerpt keeps the theme line:

```text
+ config('bat/config'): text
  + --theme="catppuccin"
```

The written rc documents gain one line each:

```text
  + profile BAT_THEME = catppuccin
```

```text
Summary
Documents: 8 to add, 0 to change, 0 to destroy.
Hooks: 2 to add, 0 to change, 0 to destroy.
```

```sh
confit apply ~/dotfiles
```

```text
applied: 8 files, 0 removed
restore: confit restore %1
```

The bat config reads `~/.config/bat/config`. The two shell documents export
`BAT_THEME=catppuccin`. The theme name sits in one profile call.
