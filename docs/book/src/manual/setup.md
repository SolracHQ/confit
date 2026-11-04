# Setup

We start from an empty machine. In this chapter we will install confit,
initialize the project, and run the first check and apply.

## Install confit

Before init anything we need to well... install it, otherwise the rest of the
book can be very hard to follow. The following command installs it for linux
x64, if your system is not supported please leave an issue in the repo
https://github.com/SolracHQ/confit to add it to CI, in the meanwhile you can go
to the [build from source](https://github.com/SolracHQ/confit#build) section.

```sh
mkdir -p ~/.local/bin
curl -LO \
  https://github.com/SolracHQ/confit/releases/download/v0.9.0/confit-linux-x64
install -m 755 confit-linux-x64 ~/.local/bin/confit
confit --version
```

```text
confit 0.9.0
```

## Initialize the project

Before you write configuration, initialize the project. confit provides a simple
way to start, the init command.

```sh
mkdir -p ~/dotfiles
confit init ~/dotfiles
```

```text
init: /home/you/dotfiles/main.lua (18 files)
```

And that is it. You get a full project setup with default utility plugins,
stubs, and default configuration. It is ready to open in your code editor:

```sh
nvim ~/dotfiles
```

```sh
code ~/dotfiles
```

## Lets read the profile

`main.lua` is the default profile. Here it is in full, and it is the only file
you need to understand right now:

```lua
local sugar = require("plugins._.sugar")

local bashrc = sugar.home(".bashrc")

local shell = {
   name = id("shell"),
   patches = {
      sugar.patch(bashrc, function(document)
         table.insert(document.config, sugar.alias("ll", "ls -l"))
      end),
   },
}

return {
   documents = {
      sugar.rc(bashrc),
   },
   configs = { shell },
}
```

One rc document for `~/.bashrc`, one config that patches it with a single alias.
That is all the scaffold does, so your first preview stays readable.

The moving parts are two. `documents` describe the files you want, and `configs`
describe the changes to apply to them. The `sugar` plugin is a helper for the
common shapes, so you write `sugar.alias` instead of the raw entry table.

> [!NOTE]
> Both fields are optional. A profile with only `documents` writes
> static files with no patches.

## First check

Let us see what the project does before it touches anything. `check` prints the
preview and stops:

```sh
confit check ~/dotfiles
```

```text
Resources
+ home('.bashrc'): rc
  + alias ll = ls -l
Summary
Documents: 1 to add, 0 to change, 0 to destroy.
```

One file to add, nothing else. Each `+` line is one rendered line waiting to
land. `check` writes nothing, so run it as often as you like.

## First apply

> [!NOTE]
> The first apply overwrites `~/.bashrc`. When the file already exists,
> the run saves it into the rescue state first, and the report prints
> `saved: 1 files, confit restore @!`. That command brings it back. The state
> holds one rescue, so a second capture replaces the first.

```sh
confit apply ~/dotfiles
```

```text
Apply these changes? Type 'yes' to continue: yes
applied: 1 files, 0 removed
restore: confit restore %1
```

Type `yes` and the file lands. The interesting bit is the last line. `%1` is the
previous run, so `confit restore %1` undoes this apply. You will use that hint a
lot while you experiment. A second `check` reports zero changes.

The written `~/.bashrc`:

```sh
case $- in
*i*) ;;
*) return ;;
esac

alias ll='ls -l'
```

## Cheatsheet

From now on we will play a lot with confit, but you can get lost with so many
subcommands, so here you have a small cheatsheet for the most common commands:

| Command | Does | | --- | --- | | `check ~/dotfiles` | previews the differences
| | `apply ~/dotfiles` | previews, prompts, writes | |
`bundle ~/dotfiles -o laptop.cb` | packs one `.cb` archive | |
`restore laptop.cb` | writes a `.cb` back | | `restore @name` | writes a named
state back | | `restore %N` | writes a stored state back | | `restore '@!'` |
writes the rescue back |

## Appendix: what init wrote

Curious what the other 17 files do? Start with the stubs folder. It contains the
definitions of the builtin framework. They are not needed for normal
functionality, but they add autocompletion. That is pretty useful in a heavy
data oriented framework like confit.

```text
stubs/
  confit.d.lua
  confit.fact.d.lua
  confit.fetch.d.lua
  confit.input.d.lua
  confit.open.d.lua
  confit.render.d.lua
  confit.validate.d.lua
```

Next are the default utility plugins, eight files under `plugins/_/`:

```text
plugins/
  _/
    is/init.lua
    mise/init.lua
    mise/rc.lua
    mise/release.lua
    mise/spec.lua
    nerd_fonts/init.lua
    sugar/init.lua
    tab/init.lua
```

These are editable copies. Your profiles require them like any other Lua module,
and you can read or change them. `init --plugins` rewrites them from the binary,
so the flag refreshes the whole folder after an update.

Then the project configuration, `project.toml`:

```toml
version = "0.9.0"

[plugins]
folder = "plugins"
allow_external_folders = false

[stubs]
folder = "stubs"
```

It records the scaffold version and the two folder names.

Last is the editor configuration, `.luarc.json`. It maps the `plugins` and
`stubs` folders for LuaLS, so your editor completes `require` paths and checks
your configs against the stubs.

> [!NOTE]
> Do not worry if you do not like it. You can change it in the future.
> Only the profile file is required to run.
