# Documents

A document holds one destination plus one payload. The plan
builds one document per path. Hashes cover rendered bytes.
Apply writes each document to its expanded path.

| Kind | Payload | Path rule |
| --- | --- | --- |
| `structured` | data table plus format | one path holds one document |
| `text` | exact file text | one path holds one document |
| `rc` | three section buckets | one base fans out per shell |
| `link` | link target | one path holds one document |
| `opaque` | raw bytes by blob ref | one path holds one document |
| `tree` | member list under one folder | one path holds one file set |

### Structured

#### Declaration shape

```lua
confit.document.structured("toml", { path = path, data = data })
```

The format names one of `json`, `toml`, or `yaml` in any
letter case. The args table holds `path` plus `data` only.
The data table holds string keys plus JSON shaped values.

#### Merge rule

Patches run in pipeline order. Priority sorts descending.
Declaration order breaks ties. The base seeds leaf owners
under the declaring owner. Each `set` writes one dotted
path. Each `append` extends one list. A later write to a
slot another owner holds drops. One collision line names
the format plus the path plus the winner.

```lua
confit.patch.structured("toml", path, function(data)
  data:set("user.theme", "catppuccin")
  data:append("user.plugins", "tabnine")
end):priority(confit.priority.HIGH)
```

Paths hold dotted keys plus single indices. List positions
count from 1, so `servers[1]` names the first entry. One
path rides each call. `[0]` fails the plan naming the
path. A `set` past the list tail fails. An `append`
onto a non-list leaf fails. A write through a scalar leaf
fails as blocked. A patch naming another format than the
base fails as a format mismatch. Patches with no base
create the document. Patches that agree on nothing fail
as a format mismatch.

#### Render rule

Each format serializes through its own renderer. TOML
renders through the TOML encoder. JSON renders pretty.
YAML renders through the YAML encoder.

#### Hash rule

The hash covers rendered bytes with SHA-256. The digest
reads as lowercase hex.

#### Drift shape

Recorded plus disk tables diff leaf by leaf. Dotted keys
name leaves. Indices ride brackets counting from 1. Changed leaves carry
old plus new values. Added leaves carry new values only.
Removed leaves carry old values only. An explicit null
stays distinct from an absent key. Disk bytes outside the
format fall back to a hunk.

#### Apply behavior

Apply writes rendered bytes to the expanded path. Recorded
paths absent from the manifest delete. Kind changes outside
opaque read as one create plus one delete.

### Text

#### Declaration shape

```lua
confit.document.text(path, content, { mode = "644" })
```

The call takes a path plus content plus optional opts. Opts
holds `mode` plus `unmanaged`. The mode reads octal like
`755` or symbolic like `rwxr-xr-x`. A missing mode leaves
the file mode to the process umask. `unmanaged` marks
existence-only documents, sharing the opaque rule.

#### Merge rule

Text reads declaration only. No patch targets text. One
path holds one document. A repeat fails naming both
owners.

#### Render rule

Content passes through byte for byte. Present unmanaged
documents skip the write while their declaration matches
the recorded manifest, rewritten ones land once.

#### Hash rule

The hash covers content bytes with SHA-256. The digest
reads as lowercase hex.

#### Drift shape

Changed content renders as one unified hunk. The hunk
carries content lines only. The renderer strips file
markers. A recorded mode plus a differing disk mode adds
a `mode` key entry.

#### Apply behavior

Apply writes content bytes to the expanded path. Apply
sets the recorded mode after the bytes land. A missing
recorded mode skips the mode step.

### Rc

#### Declaration shape

```lua
confit.document.rc.new({
  profile = { rc.prepend(home .. "/.local/bin") },
  config = { rc.alias("ll", "ls -l") },
  final = { rc.eval({ "starship", "init", confit.runtime.SHELL }) },
})
```

The base holds section buckets only. Keys name `profile`,
`config`, or `final`. Each key stays optional. Values hold
lists of rc entry tables. Bare entries outside `rc.new`
fail. One manifest holds one rc base. A repeat fails naming
both owners.

#### Merge rule

Patches run in pipeline order. Each `add` takes a section
plus an entry. Unknown sections fail. Named slots keep
the first writer. Exec entries accumulate. Added entries
follow patch order.

```lua
confit.patch.rc(function(data)
  data:add("config", confit.document.rc.alias("cat", "bat"))
end)
```

#### Render rule

Each shell gains one file. Profile lines render first. The
guard follows while config or final holds entries. Config
lines render next. Final lines render last. Blocks join
with one blank line. The file ends with a trailing
newline. Plain entries render as bare lines. Guarded
entries render as `if` blocks.

#### Hash rule

Each per-shell file hashes separately. The hash covers
rendered shell text with SHA-256. The digest reads as
lowercase hex.

#### Drift shape

Changed rc files render as one unified hunk, like text.
Modes never attach to rc documents.

#### Apply behavior

Apply writes each per-shell file to its shell path. The
`{{shell}}` slot materializes per shell before the write.
Recorded shell files absent from the manifest delete.

### Link

#### Declaration shape

```lua
confit.document.link(path, target)
```

The call takes a link path plus a target string.

#### Merge rule

Links read declaration only. One path holds one document.
A repeat fails naming both owners.

#### Render rule

The target string forms the link body.

#### Hash rule

The hash covers target bytes with SHA-256. The digest
reads as lowercase hex.

#### Drift shape

A changed target reports under the `target` key with old
plus new strings.

#### Apply behavior

Apply places a symlink at the expanded path. A recorded
mode never attaches to links.

### Opaque

#### Declaration shape

```lua
confit.document.opaque(path, source, { mode = "755" })
```

The call takes a path plus a source path plus optional
opts. Sources name project files root-relative, fetch
cache files absolute, or extract member files absolute.
Opts holds `mode` plus `unmanaged`.
`unmanaged` marks existence-only documents, present bytes
read as already in place whatever their content. The flag
rides outside the data hash, so toggling it with identical
bytes shows no plan line. Assembly streams the source file
into the plan and records its hash plus size.

#### Merge rule

Opaque reads declaration only. One path holds one
document. A repeat fails naming both owners.

#### Render rule

Raw blob bytes land on disk unchanged. Present unmanaged
documents skip the write while their declaration matches
the recorded manifest, rewritten ones land once.

#### Hash rule

The hash copies the blob reference. The blob reference
holds the SHA-256 over raw bytes. The digest reads as
lowercase hex.

#### Drift shape

Changed bytes report under the `content` key. Values carry
hash plus size labels. Equal bytes stay quiet. A recorded
mode plus a differing disk mode adds a `mode` key entry.

#### Apply behavior

Apply writes raw blob bytes to the expanded path. Apply
sets the recorded mode after the bytes land. A kind change
to or from opaque reads as an update. All other kind
changes read as one create plus one delete.

### Tree

#### Declaration shape

```lua
confit.document.tree(archive, dest, function(name, info, member)
  if not name:match("%.ttf$") then
    return nil
  end
  return name:match("([^/]+)$")
end)
```

The call takes an archive path plus a destination folder
plus a picker. The picker takes member path plus info
plus member file path. Info holds `size` plus `executable`. The
picker returns a destination-relative path per kept
member. It returns nil per skip. A non-string return
fails. An empty return fails. An absolute return fails.
A dot-dot return fails. A repeated return fails. A pick
that keeps nothing fails naming the filter.

#### Merge rule

Trees read declaration only. One path holds one document.
A repeat fails naming both owners. Members sort by
relative path. Member modes follow the archive executable
bit. Set bits read `755`. Clear bits read `644`. Member
modes come from the archive alone.

#### Render rule

Members write as separate files under the destination.
The destination folder itself stays a folder.
Parents build on demand.

#### Hash rule

The hash covers the canonical manifest with SHA-256. The
manifest sorts by relative path. Each line holds octal
mode plus relative path plus member blob hash. The digest
reads as lowercase hex.

```text
644 JetBrainsMono.ttf {hex}
755 setup.sh {hex}
```

#### Drift shape

Drift walks the destination member by member. Missing
members report missing under the joined path. Changed
bytes report hash plus size labels under the member key.
Changed modes report under the `member:mode` key. Drift
covers recorded members alone.

#### Apply behavior

Apply writes members to joined paths with per-member
modes. Apply removes recorded members absent from the
desired manifest. Hand-placed files plus the destination
folder stay.

### Shell rc

The rc document carries three sections. `profile` renders
before the guard. `config` renders after the guard.
`final` renders last. Any entry kind renders in any
section. Entries keep declaration order within one
section.

| Section | Position | Guard |
| --- | --- | --- |
| `profile` | file head | no guard |
| `config` | past the guard | interactive only |
| `final` | file tail | interactive only |

The guard marks interactive shells. It renders while
config or final holds entries. Setup-only output skips
the guard.

```sh
case $- in
*i*) ;;
*) return ;;
esac
```

Slots decide collisions. Named entries share one slot per
name plus guard across every section. One name under
another guard holds its own slot. The first writer wins
per slot. A later write drops plus one collision line in
the log.

```text
collision on alias "ll": "starship" overwritten, "bat" wins
```

Exec entries carry no slot. `eval` plus `cmd` plus
`source` accumulate with no collision.

| Entry kind | Builder | Slot |
| --- | --- | --- |
| `env` | `rc.env(name, value, opts?)` | name plus guard |
| `path` | `rc.prepend(dir, opts?)` | name plus guard |
| `alias` | `rc.alias(name, value, opts?)` | name plus guard |
| `eval` | `rc.eval(argv, opts?)` | none, accumulates |
| `cmd` | `rc.cmd(argv, opts?)` | none, accumulates |
| `source` | `rc.source(path, opts?)` | none, accumulates |

Entry builders take `when` alone through opts. Unknown
opts fields fail. `prepend` takes a dir alone for `PATH`
or a var plus a dir. Init strings render the `{{shell}}`
slot with the target shell name. One entry addresses
every shell.

`when` scopes per shell session. Each new shell evaluates
the guard and skips entries lacking their binary or
state. `when` holds a condition table or a builder
function over the runtime namespace. The engine calls the
builder during evaluation. `Changed` rides hooks alone.
Rc guards holding it fail with a hooks-only plan error.

One file renders per declared shell. `bash` writes
`~/.bashrc`. `zsh` writes `~/.zshrc`. Other names write
`~/.{name}rc`.

Layout order holds profile lines, then the guard, then
config lines, then final lines:

```sh
export PATH=/home/u/.local/bin:"${PATH}"
export EDITOR=hx

case $- in
*i*) ;;
*) return ;;
esac

alias ll='ls -l'
if command -v bat >/dev/null 2>&1; then
  alias cat=bat
fi

eval "$(mise activate bash)"
eval "$(starship init bash)"
```

### Archives

`compressed` unpacks one archive into kept documents. The
callback takes member path plus info plus member file
path. Info holds `size` plus `executable`. The callback
returns one document per kept member. It returns nil per
skip. Other returns fail naming the constructor. Kept
documents ride the profile `documents` array or
`config:add_document`. Results follow callback order.
Transforms read the member file through `load_bytes` or
`load_text`, and `opaque` takes the member path as its
source.

```lua
local kept = confit.document.compressed(archive, function(name, info, member)
  if name == "mise/bin/mise" then
    return confit.document.opaque(bin .. "/mise", member, { mode = "755" })
  end
end)
```

Byte routing follows magic plus name. Gzip bodies gunzip
first, then parse as tar with one single-file fallback.
Names wanting tar skip the fallback. Zip bodies parse as
zip. Other bodies parse as tar. Directory members skip.
Empty member names skip.

| Body | Name | Result |
| --- | --- | --- |
| gzip | `.tar.gz`, `.tgz`, `.tar` | tar members, tar failure fails |
| gzip | other names | tar members, else one single file |
| zip magic | any | zip members |
| other bytes | any | tar members |

Path reads stay jailed. Root-relative paths resolve under
the project root. Cache-absolute paths resolve under the
fetch cache. Extract-absolute paths resolve under the
extract root. Absolute paths outside both fail. Empty
paths fail. Escapes above the root fail. Remote bytes
ride `fetch_file` first. The cache path feeds
`compressed` plus `tree` plus `load_bytes` directly.
Member paths feed `opaque` plus `load_bytes` plus
`load_text` directly.

### Hashing

Each document hashes with SHA-256 over its rendered
bytes. Digests read as lowercase hex. Opaque hashes copy
the blob reference. Tree hashes cover the canonical
manifest instead. Tables hold fixed key order through the
sorted map. Ordered lists keep declaration order.

Blobs gzip at level 6 for pool plus bundle entries. The
outer bundle tar gzips at level 0. Manifest files hold
pretty JSON with blob refs into the shared pool. Raw
bytes travel in the pool beside the manifests.

Hashes persist in the file and load trusted. Loads skip
rendering. Hydration stays lazy. Disk bytes matching a
blob hash win before any pool read. Pool hits verify the
hash and cache per run. Missing pool blobs fail the load
naming the hash.
