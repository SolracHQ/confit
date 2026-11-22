# Init

`init` scaffolds a project folder from resources in the binary and from the
current API shapes. A plain command writes the profile, the project file, the
stubs, the core plugins, and the editor config. A refresh flag rewrites one
generated target and overwrites it. Use `--lsp` again after changing a folder
name in `project.toml`. The editor config then maps the new folders.

```sh
confit init [DIR]
confit init [DIR] --stubs
confit init [DIR] --plugins
confit init [DIR] --lsp
```

`DIR` defaults to the current folder. The command reads `project.toml` under
that folder before it writes. A missing file uses the default folders. The
[project](project.md) page states the fields, and the [CLI](cli.md) page states
the flags.

## Stages

One command follows these stages:

1. Resolve `DIR`, or the current folder.
2. Read `project.toml`. A malformed file fails before any write.
3. Choose the plain scaffold or the refresh path. A refresh flag skips the plain
scaffold.
4. For the plain scaffold, check every destination for a clash. A clash fails
before any write.
5. Write the target files and create parent folders.
6. Leave stdout empty. The command prints only the `Log:` line.

`init` uses resource copies and API shapes as its inputs. A broken `main.lua`
still leaves `init --stubs` and `init --lsp` working. A failed write stops the
command. Files that the command wrote before the failure stay.

## Scaffold

The plain command writes five targets:

| Target | Path | Files |
| --- | --- | --- |
| profile | `{DIR}/main.lua` | 1 |
| project | `{DIR}/project.toml` | 1 |
| stubs | `{DIR}/{stubs.folder}/` | 7 |
| plugins | `{DIR}/{plugins.folder}/` | 8 |
| lsp | `{DIR}/.luarc.json` | 1 |

With the default folders the command writes 18 files. The project file:

```toml
version = "0.9.0"

[plugins]
folder = "plugins"
allow_external_folders = false

[stubs]
folder = "stubs"
```

A present `project.toml` refuses the plain command. The scaffold writes the
default file only. Write a custom file after the scaffold.

The generated profile requires the `sugar` plugin, declares a config named
`shell` with one patch, and returns one rc document at `~/.bashrc`:

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

One stub file per builtin module:

```text
confit.d.lua
confit.fact.d.lua
confit.fetch.d.lua
confit.input.d.lua
confit.open.d.lua
confit.render.d.lua
confit.validate.d.lua
```

Eight plugin files:

```text
_/is/init.lua
_/mise/init.lua
_/mise/rc.lua
_/mise/release.lua
_/mise/spec.lua
_/nerd_fonts/init.lua
_/sugar/init.lua
_/tab/init.lua
```

These plugin files are scaffold code. Edit them in the project. A refresh
replaces them with the copy from the installed binary.

## Refresh flags

| Flag | Target | Files |
| --- | --- | --- |
| `--stubs` | `{DIR}/{stubs.folder}` | 7 |
| `--plugins` | `{DIR}/{plugins.folder}` | 8 |
| `--lsp` | `{DIR}/.luarc.json` | 1 |

A refresh writes every file of its target and overwrites a present file. It
creates a missing folder and writes only its target files.

The flags combine. The command handles each flagged target in this order: stubs,
plugins, lsp.

`--stubs` generates the files from the current API of the binary, so the stubs
match the installed version. `--plugins` copies the core plugin tree. `--lsp`
writes the editor config at the project root. The file carries the LuaLS
settings schema in `$schema` with these values:

```json
{
  "runtime": {
    "version": "Lua 5.4",
    "path": ["?.lua", "?/init.lua"]
  },
  "diagnostics": {
    "globals": ["require"]
  },
  "workspace": {
    "library": ["plugins", "stubs"]
  }
}
```

The library list is the plugin folder and the stubs folder from `project.toml`.
The writer escapes a backslash and a double quote in a folder name.

## Clash

The plain scaffold refuses a present destination with this message:

```text
Error: Cannot overwrite existing '{path}'
```

An existing stubs folder refuses the command even when it is empty. After the
stubs folder, the command checks the profile, the project file, every stub file,
every plugin file, and the editor config.

This check happens before the first write. A clash doesn't write a file. The
exit code is 1, and stderr shows the log line and the error line.

A plugin folder may exist when every plugin file is absent. The writer creates
missing parent folders.

A refresh flag skips the clash check. It overwrites the files of its target and
keeps other files in the folder as is.

## Output

The command doesn't produce a result line and keeps stdout empty. Stderr
carries only the `Log:` line on success.

The file counts are fixed per target: 18 for the scaffold, 7 for the stubs, 8
for the plugins, and 1 for the editor config. You get 18 files from a plain
command: the profile, the project file, 7 stubs, 8 plugins, and the editor
config.

A refresh overwrites the files of its target and exits 0. A second command with
the same flags writes the same files again.

A failed command exits 1. The command prints `Log:` and `Error:` lines to
stderr. A usage error exits 2 before the logs folder is resolved.
