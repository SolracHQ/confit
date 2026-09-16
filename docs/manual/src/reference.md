# Reference

## Commands

```sh
confit plan PROFILE [--state F] [-o FILE]     # preview
confit apply PROFILE [--state F] [--plan F] [--force]
confit apply --plan FILE
confit recover [--state F] [INDEX]
confit init [DIR]                             # scaffold, default .
```

## Documents

```lua
confit.document.structured("toml"|"json"|"yaml", { path, data })
confit.document.text(path, content)
confit.document.link(path, target)
confit.document.opaque(path, content)
confit.document.compressed(path_or_url, fn)   -- fn returns Document or nil
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
confit.shell.env_eq({ key = "X", value = "y" })
confit.shell.env_set({ key = "X" })
confit.shell.in_path("bat")
confit.shell.exists("~/.secrets")
confit.shell.all({ ... })  any({ ... })
```

## Utils and plugins

```lua
confit.utils.render(template, vars)
confit.utils.holds_cycle(value)  confit.utils.is_array(value)
confit.plugin.solrachq.mise.package(name, fn?)  .activate()
confit.plugin.solrachq.merge(base, overlay, { shallow, list_append }?)
confit.plugin.solrachq.template(path, { src, vars })
```
