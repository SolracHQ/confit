# Difference

A preview compares two pairs of manifests:

| Comparison | Sides | Shows |
| --- | --- | --- |
| State against disk | Recorded manifest and disk | Manual edits |
| Desired against state | Desired and recorded manifest | Add, change, destroy |

The first run has no recorded manifest and uses one comparison against the disk.
See [Apply](apply.md) for the run sequence.

## State against disk

- The comparison covers every document in the recorded manifest, including a
  document that the profile no longer declares.
- Entries follow the recorded path order.
- A destination with no file yields a missing line.
- A directory at the destination yields a blocked line.
- An unreadable destination yields an unreadable line with its reason.
- A present file compares by kind, as described below.
- A non-link destination follows a symlink to its target. A link destination
  reads the link target text.
- The section starts with the line
  `Changes outside Confit will be overwritten on next apply`.

## Per kind

| Kind | Comparison |
| --- | --- |
| Structured | The recorded data against the disk parsed in the same format |
| Text | The rendered bytes against the disk bytes |
| Rc | The rendered rc text against the disk bytes |
| Link | The recorded target text against the disk link target |
| Opaque | The recorded content hash against the disk hash |
| Tree | Each declared member by hash, then by mode |

- A structured disk that parses yields one line per changed leaf. A disk that
  does not parse yields one text diff. Formatting only yields no line.
- A text document yields one text diff for any byte change.
- An rc document renders the guard and every entry first, then yields one text
  diff for any byte change.
- A link yields one `target` line for a changed target.
- An opaque document yields one `content` line for changed bytes. The value
  reads `sha256:{hex} ({n} bytes)`.
- A tree yields entries for only its declared members. An extra file under the
  tree folder yields no line.
- A text document marked `unmanaged` reports no content difference. An unmanaged
  opaque document still compares its content hash. A missing or blocked
  destination still reports.

## Modes

- A text document and an opaque document carry a mode. A tree member carries a
  mode.
- A structured document, a link, and an rc document carry no mode.
- A document mode change yields `~ {path}: mode = {wanted} -> {seen}`.
- A tree member mode change yields
  `~ {path}: {member}:mode = {wanted} -> {seen}`.
- Modes render in octal with no leading zero, as in `755` and `644`.
- A text or structured content change may carry a mode line after it. An opaque
  content change reports no mode line.
- A symlink destination reports no mode.

## Desired against state

| Status | Condition |
| --- | --- |
| Create | No recorded document with the same kind and path |
| Update | The same kind and path with a different hash or mode |
| Unchanged | The same kind, path, hash, and mode |

- A change between two kinds reads as a create and a delete. A change to or from
  opaque reads as an update.
- A recorded document absent from the desired manifest is a delete, unless an
  opaque sibling at the same path replaces it.
- A create shows as `+ {path}: {kind}` with one `+` line per entry body.
- An update shows as `~ {path}: {kind}` with its changed lines.
- A delete shows as `- {path}: {kind}`, with a member count for a tree.
- The summary reads
  `Documents: {add} to add, {change} to change, {destroy} to destroy.`
- A second summary line `Hooks: ...` appears when a hook adds, changes, or is
  removed.

## First run

The first run starts with no history.

- The comparison uses the desired manifest as its reference, with the disk side
  first.
- A destination with no file is a create. The preview lists each entry body with
  `+`.
- A destination that exists with different content is a change. The line shows
  the disk value first, then the desired value.
- A destination that matches the disk yields no line.
- The summary reads `Documents: {add} to add, {change} to change, 0 to destroy.`
- The preview shows only the Resources, Hooks, and Summary sections.

## Lines

A key line is one route, one key, and the two values:

```text
~ {path}: {key} = {old} -> {new}
+ {path}: {key} = {value}
- {path}: {key} = {value}
```

A destination line is one route and the disk state:

```text
{path}: deleted by hand. Add it to the config or the next apply overwrites it
'{path}': directory blocks the managed path. remove the directory or apply fails
cannot read '{path}': {reason}
```

An unreadable line appends
`. Add it to the config or the next apply overwrites it` to the reason.

A text diff is a header and the changed lines:

```text
~ home('.bashrc'): rc
  -export EDITOR=vi
  +export EDITOR=hx
  unchanged
```

- `{path}` is the route display text.
- `{key}` is a dotted leaf key. An array position starts at 1, as in `tools[1]`.
  A link uses `target`. An opaque document uses `content`. A tree member uses
  its relative path. A mode uses `mode` or `{member}:mode`.
- A text value renders bare. A number, a boolean, and null render as their
  literals. An array and an object render as compact JSON.
- A text diff keeps a context line with one leading space, an addition with `+`,
  and a removal with `-`. The preview adds two spaces of indentation and paints
  each line by its sign.
- A recorded hash with no pooled bytes produces
  `Cannot state the difference for '{route}': {cause}`. The run continues and
  the line shows the bare hash.

The summary counts the desired manifest against the recorded manifest. The
difference lines above it describe the disk, so a hand edit and a profile change
can both concern one path.
