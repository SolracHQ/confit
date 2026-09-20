# Storage

Confit keeps slots plus history plus the blob pool under one
base folder. Bundles travel as files. Every path below names the
exact file the framework reads plus writes.

### On-disk layout

The base folder joins the OS config folder with `confit`. Each
entry below lives under that base:

| Entry | Name | Holds |
|---|---|---|
| Base folder | `confit` | slots, history, pool |
| Fixed slot | `state.json` | last applied manifest |
| Named slots | `plans/{name}.json` | stored manifests |
| History | `previous/{stamp}.json` | rotated manifests |
| Blob pool | `blobs/{sha}` | gzipped blob bytes |

Slot names hold one file stem. Empty names fail, separator
carriers fail, plus dot segments fail, each as a plan error:

```
slot name reads empty
slot name '{name}' holds separators
slot name '{name}' reads unsupported
```

### Folder roles

The OS config folder holds slots plus history plus the pool.
Fetch bodies live under the OS cache folder joined with
`confit`. Each body names the URL hash. A `.sha` sidecar beside
it holds the hex digest. Lookups verify the digest before
serving bytes, so tampered bodies read as misses. Profile path
joins read the OS data folder through `confit.path.data`. Tmp
bundles plus the fallback run log live under the OS temp
folder.

### Rotation

History holds the newest five entries. Stamps hold wall-clock
nanos. A colliding stamp bumps up by one until the name reads
fresh. Past five, the oldest drops first. Picks start at one
with newest-first order, so `%1` names the just-previous entry.
Every apply records a fresh entry, so history keeps moving
forward.

### Blob pool

Manifest writes store missing pool blobs first, so stored files
stay resolvable. Present hashes skip the write, so slots share
stored bytes. Blob bytes land gzipped with level 6 under content
hashes. Compression runs in parallel with per-blob progress.
Loads hydrate lazily. Disk bytes matching a hash win before any
pool read, so steady plans skip pool bytes. Pool hits verify
plus cache per run. Missing pool entries fail naming the hash:

```
read state '{file}': missing blob '{sha}' for '{path}'
```

`plan -o @name` fills the pool through the manifest write path,
so the pool fills on plan plus apply.

### Prune refs

Three slot groups keep their blobs. The applied slot plus every
history entry plus every named slot contributes refs. Collection
reads blob refs straight from raw manifests, so stale plus
unreadable manifests count for nothing. Apply prunes once the
archive lands, so a rotated-out entry frees its bytes at once.
Delete prunes once the named file leaves, so shared blobs stay
kept.

### Bundle files

One portable `.cb` file holds `manifest.json` first plus the
referenced blobs alone. Blob entries sort by hash with no
duplicates. Blob bytes compress with gzip level 6. The outer
tar wraps with gzip level 0. Explicit plan outputs gain `.cb`
while the suffix reads absent. Other inputs read as manifests
first, then retry as bundles, so renamed bundles still load.

### Versioning

`BUNDLE_VERSION` reads 7. A slot load probes the version first
through the raw value. Mismatches fail before parsing:

```
state version {v} reads unsupported, want 7
```

Missing versions fail naming the file:

```
read state '{file}': missing manifest version
```

Bundle reads apply the same gate:

```
bundle version {v} reads unsupported, want 7
```

The manifest shape refuses unknown fields. Document, member,
plus hook shapes carry the same strictness. Stale versions pass
quietly through history listing plus prune. The workspace crates
read 0.7.0, and `confit --version` prints the same through
clap.
