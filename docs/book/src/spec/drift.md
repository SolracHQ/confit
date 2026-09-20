# Drift

Two comparisons drive plan. State against disk reports manual
edits behind recorded paths. Plan against state reports
lifecycle moves between recorded plus desired documents. A
missing state slot switches the run to first run mode, where
desired documents meet disk bytes directly.

## State against disk

The run renders each recorded document, then snapshots its
path from disk. Paths expand before disk reads. Entries arrive
in recorded path order. Absent paths report manual deletion.
Unreadable paths report the path plus the failure detail.

### Structured

Disk bytes parse by format, with json, toml, plus yaml
supported. Tables flatten to dotted leaves, with arrays
gaining `[index]` segments. Changed keys carry old plus new
values. Added keys carry new alone. Removed keys carry old
alone. An explicit null renders as `null`, distinct from a
missing key. Disk bytes past parsing fall back to a text hunk
in current side order.

### Text

Byte inequality renders one unified hunk between recorded plus
disk bytes. Equal bytes render zero entries. File markers
strip before display on every path.

### Rc

Rendered bytes compare like text. Mismatches render hunks in
current side order. Equal renders stay quiet like text.

### Link

Disk bytes read as a UTF-8 target string. The recorded target
compares against that string. Edits use `target` as the key
with old plus new values.

### Opaque

Raw bytes compare directly. Edits surface under the `content`
key. Both values read hash plus size labels in this shape:

```
sha256:{hex} ({n} bytes)
```

### Tree

The destination folder walks member by member in manifest
order. Missing members report deletion under the joined
destination path. Changed bytes report hash plus size labels
under the member key at the destination path. Changed modes
report under `{member}:mode`. Drift covers recorded members
alone.

### Modes

Modes compare while the recorded document carries one.
Recorded None skips the check, keeping default permission
files quiet. Mismatches surface as `mode` key lines with
octal digits from old to new. Tree member modes surface as
`{member}:mode` lines under the destination path.

## Plan against state

Desired hashes diff against recorded hashes. New keys read
create. Changed hashes read update. Recorded-only keys read
delete. Hashes cover bytes alone with modes compared apart,
so mode edits read update while hashes agree. Opaque kind
changes read update in both directions with the superseded
key skipping delete. Other kind changes read create plus
delete.

### Structured leaves

Changed leaves render old to new per key. Added leaves render
new alone. Removed leaves render old alone:

```
  ~ tools.bat = old -> new
  + tools.fresh = new
  - tools.gone = old
```

### Link target

Target edits render old to new under one line:

```
  ~ target = old-dest -> new-dest
```

### Opaque content

Content edits render label to label under one line:

```
  ~ content = sha256:{old} ({n} bytes) -> sha256:{new} ({m} bytes)
```

### Tree count

Tree updates collapse to one counted line. Changed counts
added plus removed plus modified members with order ignored:

```
  ~ tree ({changed} of {total} files changed)
```

### Text bodies

Text updates render desired lines under the update sigil.
Hunks stay out of this comparison. Recorded bytes serve the
hash alone.

### Rc hunks

Rc updates render one recorded to desired hunk with per
symbol paint. Markers strip before display like drift hunks.

### Headers

Changed documents open with `~`, added documents open with
`+`, removed documents close with `-`:

```
+ note: text
~ app.toml: toml
- fonts: tree (1 files)
```

Structured headers carry the format name. Other kinds carry
the kind name. Unchanged documents render zero lines.

## First run

A missing state slot marks the first run. Desired documents
meet disk bytes directly with disk values leading every
entry. Whole disk-absent documents read create with full
bodies under `+`. Remaining groups read update with disk
first:

```
~ app.toml: toml
  ~ name = disk -> desired
```

Text hunks render disk first under the document header.
Structured plus link plus opaque leaves read
`~ {key} = {disk} -> {desired}`. Trees collapse to one
counted line:

```
~ fonts: tree
  ~ tree (2 of 2 files changed)
```

Documents holding zero entries render zero lines plus leave
the counts. First runs skip the drift section, with drift
entries folding into resource blocks instead. Counts read
as follows, with destroy pinned at zero:

```
Documents: {adds} to add, {changes} to change, 0 to destroy.
```

The Hooks line joins while hooks move.

## Line shapes

Steady drift lines follow this grammar:

```
~ {path}: {key} = {old} -> {new}
+ {path}: {key} = {new}
- {path}: {key} = {old}
{path}: manually deleted. changed outside config: add to config
or the next apply loses them
cannot read '{path}': {reason}. changed outside config: add to
config or the next apply loses them
```

Hunk content lines drop `---`, `+++`, plus `@@` markers
before display. Removals print first with the red style,
additions follow with the green style, context stays plain
with the marker space dropped so code aligns. Scalars render
with strings bare, numbers plus bools as JSON, null as
`null`, arrays plus objects as compact JSON.

Drift rides along with a successful run. Exit stays 0 while
drift entries exist.
