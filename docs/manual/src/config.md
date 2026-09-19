# Config

You stand at `~/confit-demo` with two shells and one shared tool file. This chapter adds kitty as a second config in the same folder.

A config packages related documents plus the steps building them. One file per tool is the habit. Several configs in one file also reads fine. Save the kitty config as `~/confit-demo/tools/kitty.lua` and require it from the demo profile beside the tool config.

The simplest config is a shell config. One rc base plus one alias patch:

```lua
local shell = confit.config("shell")

local base = confit.document.rc.new({
  profile = {
    confit.document.rc.prepend(confit.path.home(".local/bin")),
  },
  config = {},
  final = {},
})
shell:add_document(base)

shell:add_patch(confit.patch.rc(function(doc)
  doc:add("config", confit.document.rc.alias("ll", "ls -l"))
end))

return shell
```

This shape already runs the demo. The kitty config follows the same shape and grows it. A config grows with the tool. Kitty shows the reach. A `kitty.conf` text document, symlinks for the binaries:

```lua
local kitty = confit.config("kitty")

kitty:add_document(confit.document.text(
  confit.path.config("kitty/kitty.conf"),
  "font_size 12.0\n"
))

kitty:add_document(confit.document.link(
  confit.path.home(".local/bin/kitty"),
  confit.path.home(".local/kitty.app/bin/kitty")
))
kitty:add_document(confit.document.link(
  confit.path.home(".local/bin/kitten"),
  confit.path.home(".local/kitty.app/bin/kitten")
))

return kitty
```

## Tarball install

Kitty installs from a tarball, and every step maps to a confit primitive. The version pointer is a text fetch. The tarball URL follows the release pattern. The archive unpacks into `kitty.app` through `compressed`:

```lua
local version = confit.resources.fetch_text(
  "https://sw.kovidgoyal.net/kitty/current-version.txt"
):match("^%s*(.-)%s*$")
local tarball = confit.resources.fetch_file(
  "https://github.com/kovidgoyal/kitty/releases/download/v" .. version
    .. "/kitty-" .. version .. "-x86_64.txz"
)
local app = confit.document.compressed(tarball, function(path, _, content)
  return confit.document.opaque(
    confit.path.home(".local/kitty.app/" .. path), content)
end)
kitty:add_document(app)
```

`fetch_file` caches the tarball beside its sha sidecar, so re-plans cost zero network. `compressed` returns one document per member. The callback forwards the archive exec bit, so the kitty binary lands with `+x`:

```lua
local app = confit.document.compressed(tarball, function(path, info, content)
  return confit.document.opaque(
    confit.path.home(".local/kitty.app/" .. path),
    content,
    { mode = info.executable and "755" or "644" }
  )
end)
```

Links plus desktop files from the previous section integrate the tree. The launch command waits for hooks. See Applying safely plus Reference hooks for the run model. Everything else in the guide works now.

Desktop integration follows the same shape. The guide links the binaries plus copies desktop files. confit owns the files it writes, so the desktop entries become text documents and the terminal list becomes a managed file:

```lua
kitty:add_document(confit.document.link(
  confit.path.home(".local/share/applications/kitty.desktop"),
  confit.path.home(".local/kitty.app/share/applications/kitty.desktop")
))

kitty:add_document(confit.document.text(
  confit.path.config("xdg-terminals.list"),
  "kitty.desktop\n"
))
```

Links track the kitty tree. The managed list declares kitty the terminal. `~/.config/xdg-terminals.list` stays a file like every other.

The demo now manages a full tool with install steps plus integration files. Next, [Document](document.md) names each file shape the demo uses.
