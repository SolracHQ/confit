# Restore

```sh
confit restore SOURCE [--force]
```

This command restores a stored state to the machine. The manifest is already
built. The artifacts come from the pool or the bundle. `SOURCE` takes one of
three shapes. A path with extension `cb` in any letter case reads a bundle.
`@name` and `@!` read stored states. `%N` reads a history entry. Any other
source fails:

```text
Error: Cannot restore the state
```

State pickers stay literal.

> [!NOTE]
> `@~/name` stays a state name. The picker never expands `~`.

Restore resolves the already-built manifest first, prepares it against the
applied state, follows the shared commit sequence from [Apply](apply.md),
and stores a new history entry as an apply does. A restore from an old entry
can rewrite or remove later files. You review the preview before you
answer `yes`.

## Bundle source

```sh
confit restore ~/packages/laptop.cb
```

A bundle is a gzipped tar with `manifest.json` first. Content entries follow
the manifest. The command reads the manifest and the content from the
file. A bundle restore draws content from the file. An absent or unreadable
file fails before any write:

```text
Error: Cannot read the bundle at '/home/solrac/packages/laptop.cb'
```

A member that the manifest lists and the bundle lacks fails the command. The
failure happens before any write. The bundle layout belongs to
[State](state.md).

## Named source

```sh
confit restore @laptop
```

`@name` reads `states/{name}.json` under the state folder. The name has
positive length with slash, backslash, and NUL outside the allowed set. `.`
and `..` fail as names. A malformed state picker fails before any write:

```text
Error: Cannot restore the state '{state}', {reason}
```

The reasons are `name is empty` and `name holds forbidden characters`. Other
reasons are `history state expects a number` and `state expects @name or %n` and
are `history state is out of range` and `history holds fewer entries`. A name
that doesn't have a stored manifest fails before any write. `confit bundle
PROJECT -o @name`
writes that manifest. `confit export @name` packs it into a bundle.

## History source

```sh
confit restore %1
```

`%N` picks a previous entry newest first from 1. The store keeps five previous
entries and the current one. Picker numbering starts at the previous entry.
`%1` is the previous entry. `%0` fails as an out-of-range state. A number past
the oldest entry fails before any write. A non-number fails as a bad state
picker. The reason reads `history state expects a number`.

The restore hint after an apply names `confit restore %1`. That command restores
the previous entry when that entry exists. To repair a hand edit you apply
again. The apply rewrites the desired content over the edit.

## Rescue source

```sh
confit restore '@!'
```

`@!` is the rescue state. It holds the files that were on disk before Restore
overwrote them. Restore saves managed-absent destinations there before the
writes and replaces the state on each capture. `@!` is the latest rescue. You
use the quoted form in an interactive shell. A bare `!` triggers history
expansion there:

```sh
confit restore '@!'
```

## Shared sequence

The preview starts at `Comparison` and holds `Comparison` and `Documents`
and `Hooks` and `Summary` in order. `Documents` is the same route tree as in
apply. The branches are `[home]` and `[config]` and `[data]` and
`[literal]`. Drift shows as `!` after the sigil. `Summary` is the same `+`
and `~` and `-` table.

You answer one confirmation question. The question reads:

```text
Do you want to restore these changes? Type 'yes' to continue:
```

`--force` skips this question. The command asks the question once. Only a
trimmed `yes` continues the command. Any other answer declines the command.
After a decline the command prints `Cancelled.`. A declined command exits 0
and limits further output to the log path.

Restore saves unmanaged destinations into the rescue state, writes documents,
removes dropped paths, stores history, prunes the pool, and starts the hooks.
[Apply](apply.md) covers the prompt and the write order and the hooks and the
failure behavior.

`Applying Changes` shows the expanded parent path as root. Each landed change
shows a check mark. Each change shows its plan sigil after the mark. `Results`
shows the extra failed column when counts exist, and the output limits lines
to the tree changes and the table.

The exact disk hint prints while the rescue capture holds files:

```text
To restore previous exact disk state:
> confit restore '@!'
```

The undo hint prints while a previous entry exists:

```text
To restore previous state:
> confit restore %1
```

A drifted disk changes the undo title:

```text
To restore previous state (dropping disk changes):
> confit restore %1
```

The undo hint prints with zero changes. A command that saved unmanaged
destinations still prints only these hints. Every command prints the log path to
stderr:

```text
Log: /tmp/confit-4242/confit.log
```

[Output](output.md) covers the preview sections and the streams.
[Output](output.md) covers the exit codes.
