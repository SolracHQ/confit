# Document

A document maps 1 to 1 to a file on the filesystem. It is the
basis of the project. Everything confit materializes is a
document. A second declaration of one path fails the plan and
names both owners. Shared files change through patches.

## Structured

Tool data files: toml, json, yaml. Data holds tables, lists,
strings, numbers, booleans. Functions, userdata, threads, and
cycles fail the plan with a data-only error. Lua values cross
into files, so only file-shaped values cross.

```lua
confit.document.structured("toml", { path = path, data = data })
```

Drift compares key by key. One changed value reports its
dotted key with old plus new. A disk file that no longer
parses falls back to a text hunk.

## Text

Raw content, byte for byte. Templates render first, then the
result lands verbatim.

```lua
confit.document.text(path, content)
```

Drift compares full bytes and reports a text hunk around the
changed lines.

## Opaque

Binary bytes. Text kinds never touch them. Plans carry base64,
apply writes raw bytes.

```lua
confit.document.opaque(path, content)
confit.document.opaque(path, content, { mode = "755" })
```

Text documents take the same opt. The mode reads octal
(`"755"`) or symbolic (`"rwxr-xr-x"`) shape. Omitted means the
process umask. Structured documents never carry modes.

Drift compares bytes and reports sha plus size. Content never
prints. A recorded mode compares against the disk mode and
reports a `mode` key line on mismatch.

## Rc

Shell lines in three sections. Builders cover every line:
`alias`, `env`, `prepend`, `eval`, `cmd`, `source`. Each
builder takes `{ when = guard }` last. Sections order the
render: `profile` first and always, `config` plus `final`
behind the interactive guard.

```lua
confit.document.rc.new({
  profile = { confit.document.rc.prepend("~/.local/bin") },
  config = { confit.document.rc.alias("ll", "ls -l") },
  final = { confit.document.rc.eval({ "zoxide", "init", "bash" }) },
})
```

Drift compares the rendered file as full bytes with a text
hunk, same as text documents.

## Link

A symlink from path to target.

```lua
confit.document.link(path, target)
```

Drift compares the target string. A moved link reports old
target plus new target.

## Compressed

A document source, not a file. It unpacks an archive and the
callback returns one document per member. Returning nil skips
the member.

```lua
local fonts = confit.document.compressed(archive, function(path, _, content)
  if path:match("%.ttf$") then
    return confit.document.opaque(dest, content)
  end
end)
```
