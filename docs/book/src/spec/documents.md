# Documents

A document pairs one destination route with one payload. The payload is what
apply writes at the destination. The profile declares documents, and patches
mutate them before the write.

```lua
{
   path = route("home", ".bashrc"),
   profile = {},
   config = {
      { alias = { name = "ll", expansion = "ls -l" } },
   },
   final = {},
}
```

The `path` field is required and takes a route. The remaining fields name one
payload kind.

| Kind | Fields |
| --- | --- |
| Structured | `format`, `data` |
| Text | `text`, `mode` |
| Link | `target` |
| Rc | `profile`, `config`, `final` |
| Opaque | `handle`, `mode` |
| Tree | `members` |

Every kind takes an optional `unmanaged` flag beside `path`.

A table that carries fields from two kinds fails. An unknown field fails. A
table with only `path` is an rc document with three empty sections.

## Routes

A route is a destination before the host path exists. The `route` function takes
a base and a relative path.

```lua
route("home", ".bashrc")
route("config", "starship.toml")
route("data", "fonts/JetBrainsMono")
route("cache", "tool")
route("literal", "/opt/tool")
```

| Base | Expansion |
| --- | --- |
| `home` | the user home folder |
| `config` | the platform config folder |
| `data` | the platform data folder |
| `cache` | the platform cache folder |
| `literal` | the relative path, unchanged |

A route carries two fields. `base` is the base name. `relative` is the path
below the base.

```lua
local path = route("home", ".bashrc")

path.base      -- "home"
path.relative  -- ".bashrc"
path:display() -- "home('.bashrc')"
```

`:display()` returns call text for a named base and the bare path for `literal`.
The display text shows the route in output lines and log lines. You see the same
display in trees and in Details. The preview groups documents under `[base]`
branches with nested relative paths.

A route field takes a value from `route(...)`. A string or a table fails. An
empty relative path fails. A named base that the host lacks fails when the route
expands.

Expansion happens at write time, when a shell guard renders a path, and when a
hook resolves an argument. The manifest stores the base and the relative path,
so a recorded document expands again on the next apply.

## Common rules

- One document per path. A repeated path fails the plan.
- Documents sort by route. Tree members sort by relative path.
- A mode is a string. Octal forms are `755` and `0755`. A symbolic form is
`rwxr-xr-x`. Any other text fails.
- Apply writes `644` for a text or opaque document that doesn't have a
mode. A ref that doesn't have a mode takes `644`.
- Structured, link, and rc documents don't carry a mode. A `mode` field on
one of them fails.
- `unmanaged` marks an existence-only document. Every kind carries the
flag. A tree checks each member for existence.
- The manifest records one content hash for each document.

An unmanaged document writes on the first apply. A later apply leaves an
existing destination untouched while the declaration matches the recorded
state. A changed declaration writes once, then leaves the destination
untouched again. An unmanaged tree recreates only missing members. Disk
changes to present members stay.

| Kind | Hash input |
| --- | --- |
| Structured | the rendered content |
| Text | the text content |
| Link | the target route text |
| Rc | the rendered shell text |
| Opaque | the content hash of the handle |
| Tree | one line per member with its mode, path, and content hash |

## Structured

A structured document is a table in one serialization format.

```lua
{
   path = route("config", "starship.toml"),
   format = "toml",
   data = {
      add_newline = false,
      format = "$all",
   },
}
```

`format` takes `json`, `toml`, or `yaml` in any letter case. `data` takes a
table with string keys. Values are booleans, numbers, strings, arrays, and
nested tables. An integer key fails.

Confit starts the serializer for the format. JSON output is pretty printed. The
rendered text is the file body. Apply creates the file with the process file
mode, usually `644`.

A structured document is the base for a structured patch. The patch takes the
same format. See [patches](patches.md#structured-patches).

## Text

A text document is exact file text.

```lua
{
   path = route("config", "bat/config"),
   text = "--theme=\"catppuccin\"\n",
   mode = "644",
   unmanaged = false,
}
```

The `text` field lands unchanged. `mode` sets the permission bits after the
write. Apply replaces a symlink at the path with a regular file.

A text document doesn't have a patch kind. A structured or rc patch cannot
target it.

## Link

A link document places a symbolic link.

```lua
{
   path = route("home", ".config-link"),
   target = route("config", "real"),
}
```

`target` takes a route. A string fails. Apply writes a symlink at the
destination. The link body is the target route text from `:display()`.

A link doesn't carry a mode. A changed target reports under the `target`
key, and the next apply rewrites the link. A link doesn't have a patch
kind.

## Rc

An rc document renders shell text. Declare each shell document explicitly. A
path such as `.bashrc` doesn't select a shell on its own.

```lua
{
   path = route("home", ".bashrc"),
   profile = {
      { path = { name = "PATH", dir = route("home", ".local/bin") } },
   },
   config = {
      { alias = { name = "ll", expansion = "ls -l" } },
   },
   final = {
      { eval = { argv = { "starship", "init", "bash" } } },
   },
}
```

The three sections are optional and default to empty.

| Section | Position |
| --- | --- |
| `profile` | before the guard, in every shell |
| `config` | after the guard |
| `final` | after `config` |

The guard appears when `config` or `final` has an entry.

```sh
case $- in
*i*) ;;
*) return ;;
esac
```

The guard returns from a non-interactive shell and lets an interactive shell
continue.

An entry table carries one entry kind and an optional `when` condition.

| Entry | Fields | Rendered line |
| --- | --- | --- |
| `env` | `name`, `value` | `export NAME=value` |
| `path` | `name`, `dir` | `export NAME=dir:"${NAME}"` |
| `alias` | `name`, `expansion` | `alias NAME=expansion` |
| `eval` | `argv` | `eval "$(argv)"` |
| `cmd` | `argv` | `argv` |
| `source` | `path` | `source path` |

A `path` entry puts its directory before the current value, so it prepends.
`dir` and `source.path` take routes that expand to host paths. `argv` takes a
list of text or routes. Values use Bourne quoting.

A `when` condition wraps the line in a guard.

```lua
{
   alias = { name = "ll", expansion = "ls -l" },
   when = { env_set = { key = "SSH_CONNECTION" } },
}
```

```sh
if [ -n "${SSH_CONNECTION}" ]; then
  alias ll='ls -l'
fi
```

Entries render in declaration order inside each section. The blocks join with
one blank line, and the file ends with one newline. A document with three empty
sections renders empty content. An rc document doesn't have a mode, and apply
creates the file with the process file mode, usually `644`.

## Opaque

An opaque document writes content from a handle.

```lua
{
   path = route("home", ".local/bin/starship"),
   handle = handle:ref(),
   mode = "755",
}
```

A handle comes from `confit.open`, `confit.fetch`, or `archive:member`. Call
`ref` first: documents take the ref form. See the [Lua surface](lua.md#handles).

Apply streams the handle content to the path, then sets the mode. A missing
mode takes `644`. A handle hash is the content, so the manifest copies the
hash and doesn't read the content again.

An opaque document doesn't have a rendered form. A tree or opaque document
cannot be rendered as text.

## Tree

A tree document writes a set of files under one folder.

```lua
{
   path = route("data", "fonts/JetBrainsMono"),
   members = {
      ["JetBrainsMono-Regular.ttf"] = regular:ref(),
      ["JetBrainsMono-Bold.ttf"] = { handle = bold:ref(), mode = "755" },
   },
}
```

A member value is a ref or a table with `handle` and `mode`. A ref that
doesn't have a mode takes `644`.

A member key is a relative path with forward slashes. Profile decode rejects
these keys:

- an empty key
- an absolute key
- a `.` segment
- a `..` segment
- an empty segment
- a backslash

Apply writes each member under the destination folder and creates parent folders
on demand. Files that you place under the folder by hand stay. A member that a
later declaration drops is removed on apply.

The tree is one document at one path. Its hash covers the member list, so a
member change reads as a document update.

## Kind changes

The recorded manifest identifies one entry by display path. [Difference](difference.md) states how the kinds compare.

Apply writes the new payload at the path. The recorded entry takes the new kind.
