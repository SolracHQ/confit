# Plugin

You stand at `~/confit-demo` with shared files patched from many configs. This chapter installs demo tools with reusable code.

A plugin is reusable code with a stable shape. Your own Lua library also works. The plugin shape adds two things:

- A stable structure.
- The `username/name` split, so each plugin grows into its own git project required from the profile:

```lua
local mise = confit.plugin.solrachq.mise
```

## Loading

Plugins load lazily. First access runs the plugin file, the result caches in the registry, later accesses reuse it. External plugins live under `{root}/plugins` as `{user}/{name}/plugin.lua`. Embedded defaults ship inside the binary. Both present means the embedded one wins, with a note on stderr.

The demo keeps this layout for its own plugins:

```text
confit-demo/
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

Four plugins ship embedded.

`solrachq.mise` installs tools and wires their shell lines. The demo uses it to install `bat` and a rust toolchain. `mise.package` takes a table. `name` stays required, omitted `version` writes `latest`, omitted `bin` proves the shim under the package name, `aliases` maps alias names to expansions, sorted by name, each guarded on the binary, `rc_builder` optional, `options` carries backend tool options (strings, numbers, booleans, or arrays of those) folding into the shared TOML beside the version. Each package folds its version into the shared TOML, declares the shared `mise install` hook, and requires the installer config. `mise.init` returns that installer. The mise binary composed from fetch, unpack, an opaque document, the activation patch with the PATH prepend and the init eval. An explicit version wins, omitted resolves the latest tag. Profiles list the installer once beside the packages. The demo profile ends with this shape:

```lua
local bat = mise.package({
  name = "bat",
  version = "2024.1.0",
  aliases = { cat = "bat" },
  rc_builder = function(rc)
    rc:env("BAT_THEME", "ansi")
  end,
})
```

A rust toolchain carries its components the same way:

```lua
local rust = mise.package({
  name = "rust",
  version = "1.83.0",
  bin = "rustc",
  options = {
    components = { "clippy", "rustfmt", "rust-src", "llvm-tools" },
  },
})
```

`solrachq.merge` deep-merges tables. Tables recurse, everything else last-wins. `shallow` merges top-level keys only. `list_append` concatenates arrays keeping duplicates:

```lua
local merged = confit.plugin.solrachq.merge(base, overlay)
local flat = confit.plugin.solrachq.merge(base, overlay, { shallow = true })
```

`solrachq.template` renders a minijinja file into a text document. `src` names the template, `vars` feeds the slots:

```lua
local page = confit.plugin.solrachq.template(path, {
  src = "resources/page.txt",
  vars = { name = "ada" },
})
tool:add_document(page)
```

`solrachq.nerd_fonts` installs nerd fonts and refreshes the font cache. `font` takes the font name and an optional version, omitted resolves the latest release. Each font builds one tree document flattened under its own `fonts/{name}` folder, then declares the `fc-cache -f` hook scoped to that folder. Each font carries its own hook argv, so every font refresh runs on its own. The hook carries no checks, a cache rebuild holds no stable disk proof, so it fires every apply while `fc-cache` resolves:

```lua
local nerd_fonts = confit.plugin.solrachq.nerd_fonts
local fonts = nerd_fonts.font("JetBrainsMono", "3.5.1")
```

## Naming internal configs

Plugin authors name internal configs `plugin:{user}/{name}:{capability}`. The shape reads as plugin scope, author and plugin, capability. Internal configs stay clear of user configs, and require errors point at a name the author owns:

```lua
-- inside mise.package, before returning the config
config:require("plugin:solrachq/mise:install", "Add mise.init() to the profile configs.")
```

User code keeps clear of require. The plugin injects it, and a profile missing the installer fails the plan with the hint.

For the exact contract see [spec lua](../spec/lua.md).

The demo now installs versioned tools through plugins. Next, [Machines](machines.md) splits the demo across laptop and server.
