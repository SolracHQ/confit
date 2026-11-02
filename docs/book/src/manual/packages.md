# Packages

Now that we have our initial shell configuration, we will install cli tools with
the mise plugin.

> [!NOTE]
> I chose mise for a few reasons: it stores its configuration as a
> structured file, and it has a broad list of tools and languages to install.
> But if you prefer any other installer, feel free to create a plugin for it.

> [!NOTE]
> The mise plugin takes ownership of `mise/config.toml`. That means
> manually installed things will be removed on apply. There are several possible
> workarounds, like loading the original and patching it (currently not
> supported by confit due to jailing rules), or having a separate config and
> making the installation hook-based.

## The installer

`tools/mise.lua` is one call:

```lua
local mise = require("plugins._.mise")

return mise.init("2026.10.8")
```

`mise.init` receives the desired mise version. Put it if you want to use a fixed
version, otherwise just omit it. Without a version, mise follows the latest
release tag.

Your profile lists the installer before the packages:

```lua
local sugar = require("plugins._.sugar")
local installer = require("tools.mise")
local shell = require("tools.shell")

return {
   configs = { installer, shell },
   documents = {
      sugar.rc(sugar.home(".bashrc")),
      sugar.rc(sugar.home(".zshrc")),
   },
}
```

The installer writes three things. The mise binary at `~/.local/bin/mise`, an
empty `[tools]` table at `~/.config/mise/config.toml`, and the PATH entry and
the activation line in each rc file. The table starts empty and every package
adds its own key.

## One package

`tools/bat.lua` declares the first package in full:

```lua
local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

return mise.package({
   name = "bat",
   rc = {
      sugar.alias("cat", "bat"),
   },
})
```

One call does three jobs. It folds `tools.bat` into the shared TOML, so the
table stays in one file no matter how many packages you add. It appends a shim
check to the shared install hook, so the hook knows what to verify. And it adds
the alias inside a binary check, so your shell skips the line while the shim is
missing. That guard is what keeps a fresh machine working before the install
runs.

The profile gains one entry:

```lua
configs = { installer, shell, bat },
```

`confit check` shows what that entry adds:

```text
Resources
+ config('mise/config.toml'): toml
  + tools.bat = latest
+ home('.bashrc'): rc
  + alias cat = bat
+ home('.local/bin/mise'): opaque
Hooks
+ mise install
  + check (exists(data('mise/shims/bat')))
Summary
Documents: 4 to add, 0 to change, 0 to destroy.
Hooks: 1 to add, 0 to change, 0 to destroy.
```

The hook is shared, not per package. Every package patches the same
`mise install` hook with its own shim check, so ten tools still run one install.
On the first run the shim is absent, the check fails, and the hook runs. On
later runs with an unchanged TOML the hook skips.

## More packages

eza follows the same shape with two aliases:

```lua
-- tools/eza.lua
local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

return mise.package({
   name = "eza",
   rc = {
      sugar.alias("ls", "eza --icons=auto --color=auto"),
      sugar.alias("ll", "eza --long --icons=auto --color=auto"),
   },
})
```

ripgrep sets its binary, since the package and the binary differ:

```lua
-- tools/ripgrep.lua
local mise = require("plugins._.mise")

return mise.package({ name = "ripgrep", bin = "rg" })
```

The hook checks `shims/rg` for this one, not `shims/ripgrep`.

zoxide installs like the rest:

```lua
-- tools/zoxide.lua
local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

return mise.package({
   name = "zoxide",
   rc = {
      sugar.alias("cd", "z"),
   },
})
```

`cd` now calls `z`, but `z` does nothing yet. The shell needs the init line
first, and that is the next chapter.

The profile lists all four packages in order:

```lua
configs = { installer, shell, bat, eza, zoxide, ripgrep },
```

`confit apply` writes 4 documents and runs the hook once:

```text
hook 1 of 1: mise install
applied: 4 files, 0 removed
restore: confit restore %1
```

The 4 documents are the shared TOML, the mise binary, and the two rc files.
`confit restore %1` undoes this run.
