# Shell init

Now that we installed zoxide and aliased `cd` to `z`, we will add the init line
that makes it work. That line goes in your rc files, with your shell name, after
everything else.

## The patch

Open `tools/zoxide.lua`. The package lines stay as they are. Above them, add the
shell pattern:

```lua
local SHELL_PATTERN = "^%.(%a+)rc$"
--- more code here
```

Then append the patch. The filter keeps every home dot-rc document:

```lua
--- more code here
table.insert(config.patches, {
   filter = function(route)
      return route.base == "home" and route.relative:match(SHELL_PATTERN) ~= nil
   end,
--- more code here
})
```

The callback reads the shell name from the same pattern and adds both lines:

```lua
--- more code here
   apply = function(document)
      local shell = document.path.relative:match(SHELL_PATTERN)
      if not shell then
         error("zoxide: route '" .. document.path.relative .. "' holds no shell name")
      end
      table.insert(document.config, sugar.alias("cd", "z"))
      table.insert(document.final, sugar.eval({ "zoxide", "init", shell }))
   end,
})
```

`mise.package` adds `tools.zoxide` to the shared TOML, the shim check to the
install hook, and the alias from the last chapter. The patch goes onto the same
config with `table.insert`.

The alias lands in `config`. The init line lands in `final`, which renders last.
Both sections render after the guard.

All three pieces together read like this. The file in full:

```lua
local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

local SHELL_PATTERN = "^%.(%a+)rc$"

local config = mise.package({
   name = "zoxide",
   rc = {
      sugar.alias("cd", "z"),
   },
})

table.insert(config.patches, {
   filter = function(route)
      return route.base == "home" and route.relative:match(SHELL_PATTERN) ~= nil
   end,
   apply = function(document)
      local shell = document.path.relative:match(SHELL_PATTERN)
      if not shell then
         error("zoxide: route '" .. document.path.relative .. "' holds no shell name")
      end
      table.insert(document.config, sugar.alias("cd", "z"))
      table.insert(document.final, sugar.eval({ "zoxide", "init", shell }))
   end,
})

return config
```

## One shell name per file

The engine matches documents by route. A route is a base and a relative path.
The relative path starts with the shell name. `.bashrc` starts with `bash`, and
`.zshrc` starts with `zsh`. A bash document gets `init bash`. A zsh document
gets `init zsh`.

The profile gains one entry:

```lua
configs = { installer, shell, bat, eza, zoxide, ripgrep },
```

`confit check` shows the new rc lines:

```text
+ home('.bashrc'): rc
  + alias cd = z
  + init[0] = eval "$(zoxide init bash)"
+ home('.zshrc'): rc
  + alias cd = z
  + init[0] = eval "$(zoxide init zsh)"
```

Apply installs the new tool through the same hook. Run check again and the
preview is empty.
