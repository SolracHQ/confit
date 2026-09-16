# Plugin

A plugin is reusable code with a stable shape. Your own Lua
library also works. The plugin shape adds two things: a stable
structure, and the `username/name` split so each plugin grows
into its own git project required from the profile.

```lua
local mise = confit.plugin.solrachq.mise
```

## Loading

Plugins load lazily. First access runs the plugin file, the
result caches in the registry, later accesses reuse it.
External plugins live under `{root}/plugins` as
`{user}/{name}/plugin.lua`. Embedded defaults ship inside the
binary. Both present means the embedded one wins, with a note
on stderr.

```sh
myproject/
  profile.lua
  plugins/
    solrac/
      theme/
        plugin.lua
```

```lua
local theme = confit.plugin.solrac.theme
```

## Embedded plugins

Three plugins ship embedded.

`solrachq.mise` installs tools plus wires their shell lines.
`mise.package` names the tool and collects rc calls. `activate`
adds the PATH prepend plus the init eval.

```lua
local bat = mise.package("bat", function(rc)
  rc:alias("cat", "bat")
end)
bat:add_patch(mise.activate())
```

`solrachq.merge` deep-merges tables. Tables recurse,
everything else last-wins. `shallow` merges top-level keys
only. `list_append` concatenates arrays keeping duplicates.

```lua
local merged = confit.plugin.solrachq.merge(base, overlay)
local flat = confit.plugin.solrachq.merge(base, overlay, { shallow = true })
```

`solrachq.template` renders a minijinja file into a text
document. `src` names the template, `vars` feeds the slots.

```lua
local page = confit.plugin.solrachq.template(path, {
  src = "resources/page.txt",
  vars = { name = "ada" },
})
tool:add_document(page)
```
