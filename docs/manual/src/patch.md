# Patch

A patch is the only way to modify a document. Documents declare
the base. Patches carry the dynamism. Different configs
contribute to shared documents through patches.

Two kinds exist today: rc patches and structured patches.

```lua
shell:add_patch(confit.patch.rc(function(doc)
  doc:add("config", confit.document.rc.alias("ll", "ls -l"))
end))

starship:add_patch(confit.patch.structured("toml", path, function(data)
  data:set("command_timeout", 10000)
end))
```

The rc callback receives a handle with `add(section, entry)`.
The structured callback receives a handle with `set(path,
value)` plus `append(path, value)`. Each handle exposes its
own verbs. The wrong verb means a missing method, not a
runtime surprise.

## Priority

Patches sort by priority, then by config name. Five levels
exist: `MINOR`, `LOW`, `NORMAL`, `HIGH`, `MAJOR`. Omitted
means `NORMAL`.

```lua
shell:add_patch(
  confit.patch.rc(function(doc)
    doc:add("config", confit.document.rc.alias("ll", "ls -l"))
  end):priority("HIGH")
)
```

The order is stable: profile order leaves the plan unchanged.
Only priority plus owner decide.

## Conflict

Same slot twice means first writer wins, with a warning naming
both owners. The log shows the decision:

```sh
collision on alias "ll": "eza" overwritten, "shell" wins
```

`"shell"` wrote first, so its expansion lands. `"eza"` keeps
its other entries. Rename one alias or raise one priority to
resolve it.

## Many changes, one patch

One callback holds many changes, and they run in call order.
Relative order survives, so tools needing subsequent steps
express them in one patch:

```lua
tool:add_patch(confit.patch.rc(function(doc)
  doc:add("profile", confit.document.rc.prepend("/opt/tool/bin"))
  doc:add("config", confit.document.rc.eval({ "tool", "init", "bash" }))
end))
```

The prepend lands before the init eval, every run.

## Key language

Structured writes address dotted keys. Dots walk tables, one
`[N]` per segment walks lists starting at 0:

```lua
data:set("server.host", "example.com")
data:set("tools.bat", "latest")
data:set("servers[0].host", "example.com")
data:append("plugins", "tail")
```

`set` writes the leaf, creating parent tables along the way.
`append` extends the list at the key, creating it on nil. A
non-list leaf under `append` fails the plan naming the key.

