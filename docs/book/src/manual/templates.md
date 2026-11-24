# Templates

Now that we install whole folders with one tree document, we will render config
files from a template. There are cases where we have a config file we do not
touch often, but we want to modify a single field. In bat we usually want to
just change the theme. A static file repeats the whole file per theme. Since it
is not a structured format we cannot load it as a Lua table to modify it, but
fortunately for these cases we have templates.

```text
resources/bat.conf.j2
--theme="{{ theme }}"
--style="numbers,changes,header"
```

That is the whole template. One variable for the theme, the rest is plain bat
syntax.

```lua
local TEMPLATE = "resources/bat.conf.j2"

local open = require("confit.open")
local render = require("confit.render")

local template = assert(assert(open(TEMPLATE)):text())
local text = assert(render(template, { theme = "catppuccin" }))
```

We open the template, read its bytes, and render it with our theme. One call,
one value.

We wrap the rendered bytes in a text document so the project can write them. Put
it in `tools/batconfig.lua`:

```lua
local sugar = require("plugins._.sugar")
local open = require("confit.open")
local render = require("confit.render")

local TEMPLATE = "resources/bat.conf.j2"

local template = assert(assert(open(TEMPLATE)):text())
local text = assert(render(template, { theme = "catppuccin" }))

return {
   name = id("batconfig"),
   documents = {
      { path = sugar.config("bat/config"), text = text },
   },
}
```

We add the config to the profile, after starship:

```lua
configs = { installer, shell, bat, eza, zoxide, ripgrep, starship, fonts, batconfig },
```

Apply writes those exact bytes to `~/.config/bat/config`.

The check lists the rendered line:

```text
+ config('bat/config'): text
  + --theme="catppuccin"
```

You read the theme before you write it.
