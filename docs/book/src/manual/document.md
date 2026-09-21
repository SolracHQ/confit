# Document

You stand at `~/confit-demo` with shell and kitty configs in place. This chapter names each file shape those configs use.

A document maps 1 to 1 to a file on the filesystem. It is the basis of the project. Everything confit materializes is a document. A second declaration of one path fails the plan and names both owners. Shared files change through patches.

## Structured

The demo uses this shape for tool data files like the mise TOML in a later chapter. Tool data files read toml, json, yaml. Data holds tables, lists, strings, numbers, booleans. Functions, userdata, and cycles fail the plan with a data-only error. Lua values cross into files, so only file-shaped values cross:

```lua
confit.document.structured("toml", { path = path, data = data })
```

Drift compares key by key. One changed value reports its dotted key with old and new. A disk file that no longer parses falls back to a text hunk.

## Text

The demo kitty config uses this shape for `kitty.conf` and the desktop list. Raw content, byte for byte. Templates render first, then the result lands verbatim:

```lua
confit.document.text(path, content)
```

Drift compares full bytes and reports a text hunk around the changed lines.

## Opaque

The demo kitty install uses this shape for fonts and binaries. Source files on disk, named by path. Text kinds keep clear of them. Plans carry hashes and sizes, apply writes raw bytes:

```lua
confit.document.opaque(path, source)
confit.document.opaque(path, source, { mode = "755" })
confit.document.opaque(path, source, { unmanaged = true })
```

Text documents take the same opts. The mode reads octal (`"755"`) or symbolic (`"rwxr-xr-x"`) shape. Omitted means the process umask. Structured documents keep clear of modes. `unmanaged` marks existence-only files like self-updating tools: present bytes read as already in place, missing ones land from the source file, rewritten declarations land once.

Drift compares bytes and reports sha and size. Content stays out of the output. A recorded mode compares against the disk mode and reports a `mode` key line on mismatch.

## Rc

The demo shell config uses this shape for its startup lines. Shell lines live in three sections:

- `profile` renders first and always runs.
- `config` and `final` render behind the interactive guard.

Builders cover every line: `alias`, `env`, `prepend`, `eval`, `cmd`, `source`. Each builder takes `{ when = guard }` last:

```lua
confit.document.rc.new({
  profile = { confit.document.rc.prepend("~/.local/bin") },
  config = { confit.document.rc.alias("ll", "ls -l") },
  final = { confit.document.rc.eval({ "zoxide", "init", "bash" }) },
})
```

Drift compares the rendered file as full bytes with a text hunk, same as text documents.

## Link

The demo kitty config uses this shape for its binary and desktop links. A symlink from path to target:

```lua
confit.document.link(path, target)
```

Drift compares the target string. A moved link reports old target and new target.

## Compressed

The demo kitty install uses this shape to unpack its tarball. A document source, not a file. It unpacks an archive and the callback returns one document per member. Returning nil skips the member:

```lua
local fonts = confit.document.compressed(archive, function(path, _, member)
  if path:match("%.ttf$") then
    return confit.document.opaque(dest, member)
  end
end)
```

## Tree

The demo font setup in a later chapter uses this shape to unpack many files under one folder. A document source holding many files, not a file. It unpacks an archive into one document under one destination folder. The callback keeps the compressed shape and returns a relative path per kept member instead of a document. Returning nil skips the member:

```lua
local fonts = confit.document.tree(archive, confit.path.data("fonts"), function(path, _, member)
  if not path:match("%.ttf$") then
    return nil
  end
  return path:match("([^/]+)$")
end)
```

The manifest reads as one line either way. One add with the file count, silence on repeat runs, one update with the changed count. Member modes inherit the archive executable bit. An empty pick fails naming the filter.

For the exact contract see [spec documents](../spec/documents.md).

The demo file shapes now have names. Next, [Shell lines](shell-rc.md) covers how the demo renders startup files.
