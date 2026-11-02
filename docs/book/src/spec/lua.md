# Lua surface

A profile is one Lua chunk that returns a table with `configs` and `documents`.
See [Profile](profile.md). Every chunk runs in a sandbox with two global
constructors and six builtin modules.

```lua
local open = require("confit.open")
local fetch = require("confit.fetch")
local render = require("confit.render")
local fact = require("confit.fact")
local input = require("confit.input")
local validate = require("confit.validate")
```

`require("confit.open")` returns the open function. `require("confit.fetch")`
returns the fetch function. `require("confit.render")` returns the render
function, `require("confit.fact")` returns the fact function, and
`require("confit.input")` returns the input function.
`require("confit.validate")` returns a table with seven checks.

The sandbox exposes these names:

```text
globals    assert, error, ipairs, next, pairs, pcall, select, tonumber,
           tostring, type
libraries  string, table, math, utf8
confit     require, id, route
```

The sandbox omits `os`, `io`, `package`, `debug`, and `coroutine`.
An unknown global is nil. A write to a builtin module table fails.

The `require` function is described in [Plugins](plugins.md).

## id

`id(name)` returns one opaque identity. The identity is the text and the
declaration site. Two identities with the same text are equal, so one name
identifies one config and one hook.

```lua
{ name = id("shell") }
{ id = id("plugin:_/mise:install") }
```

`name` is a string. A non-string fails the call. The text appears in a duplicate
failure and in the stored manifest. A repeated config name fails the plan.

## route

`route(base, relative)` returns one destination route. The base is `home`,
`config`, `data`, `cache`, or `literal`. The relative path is a string. An empty
relative path fails the call. The base folder expands when a run writes.

```lua
route("home", ".bashrc")          -- home('.bashrc')
route("config", "starship.toml")  -- config('starship.toml')
route("data", "fonts/JetBrainsMono")
route("cache", "tool")
route("literal", "/opt/tool")     -- /opt/tool
```

| Field | Value |
| --- | --- |
| `base` | The base name. |
| `relative` | The path below the base. |
| `:display()` | The route as text. |

`:display()` shows `home('.bashrc')` for a standard base. A literal route shows
only its path. Error messages and manifest hash keys use the display text.

## confit.open

`open(path)` returns one handle and an optional reason. The path is project
relative. The file must exist and stay inside the project root.

```lua
local handle, reason = open("resources/starship.toml")
if not handle then
   error("starship: " .. reason)
end
```

| Path | Result |
| --- | --- |
| `"resources/starship.toml"` | A handle for the file |
| `"/etc/passwd"` | Nil and a reason |
| `"../outside"` | Nil and a reason |
| `"sub\\note"` | Nil and a reason |
| `"sub//note"` | Nil and a reason |
| `"absent.toml"` | Nil and a reason |

A drive prefix fails like an absolute path. A symlink that points outside the
project fails. A non-string path fails the call. The reason text is the
failing rule or the missing file. A project handle reads the file again on each
call.

## confit.fetch

`fetch(url, sha256?)` returns one handle and an optional reason. The body lands
in the fetch cache. The optional `sha256` pins the content hash.

```lua
local handle, reason = fetch(
   "https://example.com/tool.tar.gz",
   "9f2c4e8a1d6b3f0c5e7a9b2d4f6a8c0e1b3d5f7a9c2e4b6d8f0a1c3e5b7d9f2c"
)
```

- `url` is a non-empty string. An empty string fails the call.
- `sha256` is 64 hexadecimal characters. Malformed text fails the call.
- A cache hit serves the stored body with no request.
- A cache miss downloads the body and stores it under its content hash.
- A failed download, a failed cache write, or a digest mismatch returns nil and
  a reason.

One fetch returns a handle for text or binary bytes alike. A cache hit serves
the stored body even when the remote body changed. Remove the cached body to
fetch again. A pin checks the served body on a hit and on a download.

Each fetch reports `collecting artifacts: {url}` when it starts. The end line
shows the transferred byte count, or `cached` for a hit.

## Handles

A handle comes from `open`, `fetch`, or `archive:member`.

| Method | Returns |
| --- | --- |
| `handle:text()` | The body as UTF-8 text, else nil and a reason |
| `handle:load()` | The body as one table, else nil and a reason |
| `handle:archive()` | An archive handle, else nil and a reason |

`text` fails on bytes that are not UTF-8. A project handle reads the live file,
so a changed or deleted file shows on the next call.

`load` sniffs the body format and parses JSON, TOML, YAML, or INI into one
table. The file extension has no role. The parsers run in that order. The root
must be an object, a table, a mapping, or an INI key set. A binary body fails
with the detected format kind. A text body with no object fails with a
reason.

```lua
local handle = assert(open("resources/mise.toml"))
local data = assert(handle:load())
print(data.tools.bat)
```

`archive` accepts a zip, a tar, or a gzipped tar. A bare gzip stream yields a
handle, and the first member call fails with a not-archive reason.

```lua
local archive = assert(handle:archive())
local names = assert(archive:members())
local binary = assert(archive:member(names[1]))
```

| Method | Returns |
| --- | --- |
| `archive:members()` | The member names |
| `archive:member(name)` | One member handle, else nil and a reason |

`members` returns archive-relative names with forward slashes, sorted in
ascending order. Folder entries are omitted. `member` matches the name exactly;
an absent name returns nil and a reason. A member body lands in the cache under
its content hash.

## confit.render

`render(template, vars)` returns the rendered text and an optional reason. The
template uses Jinja syntax. `vars` is a JSON-shaped Lua value.

```lua
local text = assert(render(
   "theme = \"{{ theme }}\"\n",
   { theme = "catppuccin" }
))
```

- `template` is a string. A non-string fails the call.
- A missing name renders as an empty string.
- A syntax error or an unrenderable value returns nil and a reason.

## confit.fact

`fact(name)` returns one host fact. The name matches in any letter case.

| Name | Value |
| --- | --- |
| `version` | A three-item table `{major, minor, fix}` |
| `os` | The operating system name |
| `arch` | The processor architecture name |

```lua
local version = fact("version")
if version[1] == 0 and version[2] < 9 then
   error("Confit 0.9 or newer is required")
end

local os = fact("os")     -- "linux"
local arch = fact("arch") -- "x86_64"
```

`version` returns numbers, so `{0, 9, 0}` describes this release. `os` and
`arch` return strings. An unknown name fails the call.

## confit.input

`input(prompt)` writes one prompt and returns one line. The prompt goes to
standard error. The answer loses its trailing newline.

```lua
local name = input("Theme name: ")
```

When the profile calls it, every evaluation asks, so `check`, `apply`, and
`bundle` all prompt. An empty answer returns an empty string. A closed prompt
channel fails the call.

## confit.validate

`validate` has seven checks. Each returns the decoded value and an optional
reason. A bad shape returns nil and a reason. The checks do not raise.

```lua
local config, reason = validate.config({ name = id("theme") })
if not config then
   error("theme: " .. reason)
end
```

| Verb | Checks |
| --- | --- |
| `document` | One document table of any kind |
| `config` | One config table with a required `name` |
| `patch` | One patch table in any of the three shapes |
| `hook` | One hook table |
| `condition` | One condition table |
| `entry` | One rc entry table |
| `route` | One route handle |

The decoded value is the model shape, so a caller returns it in place of the
literal table. A non-table argument returns nil and a reason. See
[Documents](documents.md), [Patches](patches.md), [Conditions](conditions.md),
and [Hooks](hooks.md) for the accepted shapes.
