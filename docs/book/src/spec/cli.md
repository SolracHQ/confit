# CLI

## Commands

### plan

```sh
confit plan PROFILE [-o PATH | -o @name] [--root DIR] [--plugins DIR] [--re-fetch]
```

`confit plan` evaluates one profile file into documents and
hooks and diffs drift against the fixed slot. The run previews
by default. The positional holds the profile path.

`-o PATH` writes a bundle file, adding `.cb` to names missing
it. The match reads case-insensitively, so `plan.CB` keeps its
spelling. `-o @name` writes a slot manifest under the plans
folder and adds no suffix. Omitted output writes nothing;
the run previews only.

Stdout carries the summary every run. Stderr carries a `log: {path}` line. Failures exit 1.
Plan-shaped failures print `confit: plan error: {message}` on
stderr; remaining failures print `confit: {message}`. The
prefix wraps command messages from every subcommand. Drift
leaves the exit at 0.

### apply

```sh
confit apply SOURCE [--force] [--root DIR] [--plugins DIR] [--re-fetch]
```

SOURCE is a required positional. Values starting with `@` or
`%` load a slot manifest with preview and prompts. Values ending
in `.cb` in any letter case load a bundle file and run from
that file alone; the preview stays skipped. Remaining values
evaluate as a profile path in any extension, extensionless
included. A missing profile path fails as a plan error:

```sh
apply reads no profile '{path}'
```

Slot failures carry the command name as an `apply: ` prefix.
The preview stream and the prompt flow live on the apply
page. With `--force`, the first confirmation prompt drops out.
A fresh drift check ahead of writes still asks again.

Stdout carries `applied: {written} files, {removed} removed`,
and `previous: {history path}`. The `previous:` line names
the fresh history entry. Stderr carries a `log: {path}` line
on success and failure runs. Failures exit 1 under the same
`confit: ` shapes as plan. Drift leaves the exit at 0.

### export

```sh
confit export [PICKER] [-o PATH] [-m | --manifest]
```

The picker selects the slot. Omitted pickers read the applied
slot. `@name` reads a named slot. `%N` reads history
newest-first from 1. `-o PATH` writes a bundle file at the
literal path, adding `.cb` to names missing it, in any letter
case. Without `-o`, the name follows the slot: `applied.cb`,
`{name}.cb`, or `prev-{N}.cb`. `-m` renders the manifest as
pretty JSON on stdout instead and writes no bundle file. The
JSON carries blob hashes and sizes only; document bytes stay in the
pool.

A file export answers with `export: {path}` on stdout.
Manifest runs print the JSON alone to stdout. Export refuses `-o` and `--manifest`
together:

```sh
export: '-o' plus '--manifest' refuse together, pick one
```

Picker failures carry the command name as an `export: `
prefix. The exact shapes live under Pickers and slot names.

### delete

```sh
confit delete @name
```

The positional names one slot with a leading `@`. Delete
removes the named manifest first. A pool prune follows and
drops blobs orphaned by the removal. The prune keeps blobs
still referenced by remaining slots.

Stdout prints the slot name and the pruned count:

```sh
delete: @{name} ({m} blobs pruned)
```

Shapes outside `@name` fail with the want named:

```sh
delete: '{value}' reads unsupported, want '@name'; history and the current slot never delete
```

Absent slots fail as:

```sh
delete: '@{name}' reads absent
```

Names behind the sigil follow the slot-name rules below. `@`
alone fails as `slot name reads empty`.

### init

```sh
confit init [DIR]
```

`confit init` fills DIR with a fresh profile and editor
stubs. Omitted DIR means the current folder. The stage flow
and the clash matrix live on the init page.

## Global flags

Three flags tune the run:

- `--root DIR` sets the require base for plan and apply runs. Omitted roots fall back to the profile parent, then to the current folder.
- `--plugins DIR` sets the plugin folder shaped `{user}/{name}/plugin.lua`. Omitted plugin folders fall back to `{root}/plugins`.
- `--re-fetch` refetches remotes past the sidecar cache. Omitted runs trust the sidecar cache.

The three flags ride plan and apply only.

`--log-file PATH` sets the log path for the run. Omitted log
paths resolve to a per-process file named `confit-{pid}.log`
under the OS temp folder. Collision lines land in the log
file; the run prints the path after the summary. `--log-level
LEVEL` gates verbosity. Levels span `off`, `error`, `warn`,
`info`, `debug`, `trace`, matched in any letter case. Omitted
levels mean `warn`. Version output answers `-V` and
`--version` with `confit {version}`.

Every path arg expands one leading `~` against the home
folder. Bare `~` and `~/`-led values resolve; all other
values pass through untouched. The expansion covers the plan
profile, `--root`, `--plugins`, plan `-o`, the apply source,
the init dir, export `-o`, and `--log-file`. Slot pickers
stay literal.

## Pickers and slot names

A picker names one stored manifest. Omitted pickers read the
applied slot. An absent applied slot fails as:

```sh
the applied slot reads absent, apply first
```

`@name` reads a named slot. An absent name fails as
`'@{name}' reads absent`. `%N` reads history newest-first
from 1. A non-number fails as:

```sh
'{raw}' reads unsupported, want '%N' holding a number from 1
```

A pick past the stored count fails as:

```sh
'{raw}' reads out of range, holding {total} stored manifests
```

Bare values fail as:

```sh
'{raw}' reads unsupported, want '%N', '@name', or nothing
```

Apply prefixes each shape with `apply: `, export with
`export: `. Names hold one normal path component with no
separators. Refusals read:

```sh
slot name reads empty
slot name '{name}' holds separators
slot name '{name}' reads unsupported
```

Empty names hit the first shape, separator carriers the
second, dot segments, NUL, and other non-normal
components the third.
