# ConfIt

User space configuration manager. Following the configuration as code principle,
allows defining your configuration as Lua code, previewing the changes and
applying them idempotently.

## Demo

![Confit demo](assets/confit-demo.gif)

## Index

- [Install](#install)
- [Quick start](#quick-start)
- [Configs](#configs)
- [Preview and rescue](#preview-and-rescue)
- [Scope](#scope)
- [Examples](#examples)
- [Commands](#commands)
- [Book](#book)
- [Build from source](#build-from-source)
- [License](#license)

## Install

For one-shot configuration you can just run this.

```sh
curl -L https://github.com/SolracHQ/confit/releases/download/v0.9.0/confit-linux-x64 -o /tmp/confit
chmod u+x /tmp/confit
/tmp/confit {YOUR_PROFILE}
```

For develop and test profiles, repeated usage.

```sh
mkdir -p ~/.local/bin
curl -LO https://github.com/SolracHQ/confit/releases/download/v0.9.0/confit-linux-x64
install -m 755 confit-linux-x64 ~/.local/bin/confit
confit --version
```

```text
confit 0.9.0
```

## Quick start

To start creating profiles run `confit init /your/dotfiles/path` to init a new
dotfiles project. You can modify the default main.lua profile and guide based on
the generated stubs and plugin folder. For a step by step guide please see the
guide part of the book at <https://solrachq.github.io/confit/>

To apply an existing profile just run `confit apply /path/to/profile`. Ensure to
see what the profile will create and which hooks will execute. In case you want
to recover your files execute `confit restore @!`.

## Configs

Configs are the logical units. A config correlates created files with others via
patches. If you add a new CLI tool you not only add the binary but the aliases
in the shell files with its configs in `.config` and a trigger for the install
hook. It is a core and powerful concept.

```lua
-- tools/bat.lua
local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

return mise.package({
  name = "bat",
  rc = {
    sugar.alias("cat", "bat"),
  },
})
```

This config adds the binary entry with the shell alias and the install hook
trigger in one place. Disabling the config disables every part.

## Preview and rescue

Check shows what a profile produces. It uses the desired view by default. The
desired view compares the profile against stored state. It gives the ideal
result on a clean machine. It fits profile development and portable bundles. Run
it as often as needed while editing. The view can be changed with `--view`.

Apply uses the effective view by default. It compares the profile against disk
and marks drift with `!`. It fits the machine in front of you.

The first apply has no history to undo. It copies replaced files into the rescue
state first and reports it.

```sh
confit restore '@!'
```

> [!NOTE] Quote `'@!'` in some shells. In some of them a bare `!` triggers
> history expansion.

A later apply prints the history hint.

```sh
confit restore %1
```

`%1` is the previous run and `%2` is the run before that. History keeps five
previous entries.

## Scope

Confit does not support and will never support scaling mechanisms nor native
support for system-wide package managers. It was designed to be user space only.

## Examples

| Example | Shows |
|---|---|
| `0-basic_tool` | mise package, alias, init |
| `1-structured_resource` | declared and patched config |
| `2-templated_resource` | rendered config with profile vars |
| `3-fonts` | font tree, fetch, `fc-cache` hook |
| `4-dotfiles` | multi config profile, rc patch |
| `5-interactivity` | prompted input shaping config |
| `book-example` | didactic profiles from the book |

The book example is didactic, not the cleanest setup. The starship chapter
downloads the binary by hand to teach binary fetch. Real setups prefer the mise
plugin. Read the book narrative first, then open `book-example`.

## Commands

| Command | Does |
|---|---|
| `bundle PROJECT -o PATH\|@name` | writes a `.cb` bundle or named slot |
| `check PROJECT` | shows the desired preview fast for profile development |
| `apply PROJECT` | previews effective, prompts `yes`, writes files |
| `restore SOURCE` | previews and writes one `.cb`, `@name`, `@!`, or `%N` |
| `export [PICKER]` | packs one stored slot into a portable bundle |
| `delete @name` | drops one named slot and its orphaned entries |
| `init [DIR]` | scaffolds a profile with stubs and plugins, default `.` |

Full flags with streams and exit codes live in `docs/book/src/spec/cli.md`. JSON
shapes live in `docs/book/src/spec/schemas.md`.

## Book

The manual and the spec live as one book at `docs/book/`. It is published at
<https://solrachq.github.io/confit/>

Intent per version lives under `docs/design/`. Changes live in
`docs/changelog.md`.

## Build from source

The binary is the recommended use. Build from source only for users who build
their own software from source.

```sh
cargo run -- --help
just check
```

## License

MIT.
