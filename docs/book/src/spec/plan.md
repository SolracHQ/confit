# Plan

`confit plan PROFILE` turns one profile file into desired state,
compares that state against the recorded slot and disk, writes a
portable payload, and prints a summary. The profile path rides
positionally. Shared flags tune root, plugins, and refetch. The
flow moves through eight stages in order:

- Load profile resolves paths and flags.
- Evaluate runs the framework over the profile.
- Patches execute callbacks in priority order.
- Merge hooks folds shared hooks into single runs.
- Hash fills content hashes and sorts documents.
- Load previous reads the recorded slot.
- Diff compares desired against recorded and disk.
- Write bundle stores the payload.

## Load profile

### Flag parsing

Flags parse before sibling work starts. Tildes expand inside the
profile path and the output path. The root flag defaults to the
profile parent folder. The plugins flag defaults to `plugins`
under root. Refetch defaults to reuse of cached sources.

### Profile handoff

The engine reads the profile file and runs it under Lua with a
narrow standard library. A scoped require serves profile
relatives. The returned table coerces to a profile and passes
validation before documents assemble.

## Evaluate

### Document assembly

Configs contribute documents in declaration order. Structured
documents assemble first. Text and link documents follow. Rc
documents close the assembly. Blob refs collect beside the
manifest under content hashes through the run.

### Patch handles

Patch handles gather in config declaration order with running
indexes. Totals emit one progress fact while a sender rides
along. Assembly consumes the handles per target group after
collection.

## Patches

### Execution order

Patches sort by priority with highest first. Declaration order
breaks ties inside equal priority. Each callback runs against
its live document table in that order.

### Collision wins

Each colliding slot keeps its first writer. Later writers yield
the slot and log one collision warning to the log file. Patch
progress reports done against total through the run.

## Merge Hooks

### Shared runs

Declared hooks merge by shared argv and path. First seen order
wins each slot. Requires gates join with AND. When gates join
with OR. Checks concatenate in order. Timeout keeps the max.
Single hooks pass through with bare shape kept.

## Hash

### Content hashes

The build fills one data hash per document from rendered bytes.
Opaque hashes copy the blob reference. Tree hashes cover
canonical manifest bytes over blob references. Documents sort
by path after hashing. Blob refs attach beside the manifest
after the build.

## Load Previous

### Slot read

The slot lives at `{config}/confit/state.json`. The run loads
this slot before hashing, so fresh hashes meet recorded hashes
right away. A missing file reads as an empty manifest and marks
the first run.

### Version probe

A present file parses in two steps:

- Version probe runs first. Version 7 passes. Other versions fail as unsupported. A missing version fails as a plan error naming the file.
- Manifest cast runs second. Unknown fields fail the load.

Hashes persist
in the file and read trusted, so loads skip rendering.

## Diff

### Disk comparison

First runs compare desired documents against disk with disk
values leading. Steady runs compare recorded documents against
disk with recorded values leading. Drift entries arrive in
recorded path order.

### Lifecycle counts

Counts compare desired hashes against recorded hashes. New keys
read create. Changed hashes read update. Recorded-only keys
read delete. Mode edits read update while hashes agree, since
hashes cover bytes alone. Opaque kind changes read update in
both directions. Other kind changes read create and delete.

### Hook lines

Hook lifecycle lines derive beside the summary in plan order
with removals trailing. Finished documents log one debug line
each carrying path, kind, and lifecycle status.

## Write Bundle

### File output

An explicit output path resolves first. A missing suffix gains
`.cb`, with present suffixes kept in any letter case. The run
writes a compressed tar archive there. The archive holds
`manifest.json` first, then one `blobs/<sha>` gzip entry per
referenced blob in sorted order with duplicates excluded.
Referenced blobs alone ship inside.

### Named slot output

`@name` strips the sigil and resolves under
`{config}/confit/plans/{name}.json`. The run writes the
manifest form there with pool blobs stored first, so the slot
stays resolvable after the write. Slot outputs keep their own
name with zero suffix imposed. Empty names, separator
carriers, and dot segments fail as plan errors.

## Outputs

The two destinations compare as follows:

- File output writes `{output}.cb` as a portable tar archive.
- Named slot output writes `{config}/confit/plans/{name}.json`
  as manifest JSON.

Omitted output runs preview-only. Stdout carries the summary.
Stderr carries the spinner and the `log:` path.

## Zero home writes

Snapshots read managed destinations through expanded paths.
Plan reads destinations and writes the payload and the log
file alone. Documents land on disk through apply alone.
