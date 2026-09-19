# Reference

You stand at a working demo with fixes at hand. This chapter lists every command shape.

## Commands

```sh
confit plan PROFILE [-o FILE|@NAME]         # preview
confit apply SOURCE [--force]
confit export [PICKER] [-o FILE] [-m]        # pack a slot, default applied
confit delete @NAME                          # drop a named slot
confit init [DIR]                           # scaffold, default .
```

Plan `-o` names the bundle file and gains `.cb` unless
present. Omitted plan output stores a bundle under tmp and
prints the path. `@NAME` stores or loads a named slot under
the user config folder. Named slots hold saved plans.
Portable bundles hold `.cb` files.
Empty names plus separators fail. A source reads `.lua` plus
extensionless paths as a profile, `.cb` as a bundle file,
`@NAME` as a named slot, `%N` as history newest-first from
one.

The export picker reads `%N` for history newest-first
from one, `@NAME` for a named slot, nothing for the applied
slot. `-o` names the bundle file and gains `.cb` unless
present. Omitted export output derives the name from the
slot as `applied.cb`, `personal.cb`, or `prev-2.cb`. `-m`
prints the manifest, not the file contents. `-o` plus
`--manifest` refuse together.

## Documents

```lua
confit.document.structured("toml"|"json"|"yaml", { path, data })
confit.document.text(path, content)
confit.document.link(path, target)
confit.document.opaque(path, content)
confit.document.compressed(path, fn)          -- path reads root or cache relative alone, URLs travel through fetch_file; fn returns Document or nil
confit.document.tree(archive, dest, fn)       -- fn returns relative path or nil
confit.document.rc.new({ profile = {}, config = {}, final = {} })
```

## rc entries

```lua
confit.document.rc.alias(name, expansion)
confit.document.rc.env(name, value)
confit.document.rc.prepend(dir)               -- PATH prepend
confit.document.rc.prepend(var, dir)
confit.document.rc.eval({ "cmd", "args" })
confit.document.rc.cmd({ "cmd", "args" })
confit.document.rc.source(path)
-- every builder takes { when = guard } as last arg
```

## Patches

```lua
confit.patch.rc(function(doc)                 -- doc:add(section, entry)
end)
confit.patch.structured("toml", path, function(data)
  data:set("a.b", value)                      -- dotted write
  data:append("list", value)                  -- list extension
end)
confit.patch.structured(...):priority("HIGH") -- MINOR LOW NORMAL HIGH MAJOR
```

## Resources and paths

```lua
confit.resources.load_toml(path)  load_json  load_yaml  load_text  load_bytes
confit.resources.fetch_text(url, { sha256 = "..." }?)
confit.resources.fetch_file(url, { sha256 = "..." }?)
confit.path.home(...)  confit.path.config(...)  confit.path.data(...)  confit.path.confroot(...)
```

## Guards

```lua
confit.runtime.env_eq({ key = "X", value = "y" })
confit.runtime.env_set({ key = "X" })
confit.runtime.in_path("bat")
confit.runtime.exists("~/.secrets")
confit.runtime.all({ ... })  any({ ... })
```

## Hooks

```lua
confit.hook.run({ "mise", "install" }, {
  path = { "/home/ada/.local/bin" },          -- extra PATH dirs for the run alone
  when = confit.runtime.in_path("mise"),      -- closed gate warns and excuses
  checks = { confit.runtime.in_path("bat") }, -- pass skips, fail runs, still-fail aborts
  timeout = "10m",                             -- h m s shapes, default 10m
})
config:add_hook(hook)
config:require("plugin:solrachq/mise:install", "Add mise.init() to the profile configs.")
```

## Utils and plugins

```lua
confit.utils.render(template, vars)
confit.utils.holds_cycle(value)  confit.utils.is_array(value)
confit.plugin.solrachq.mise.package({ name, version?, bin?, aliases?, options?, rc_builder? })  .init(version?)
confit.plugin.solrachq.nerd_fonts.font(name, version?)
confit.plugin.solrachq.merge(base, overlay, { shallow, list_append }?)
confit.plugin.solrachq.template(path, { src, vars })
```

Every shape now sits in one place. Next, [Philosophy](philosophy.md) tells why the tool reads this way.
