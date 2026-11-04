# Restore

```sh
confit restore SOURCE [--force]
```

`restore` writes a stored manifest to the machine. `SOURCE` takes one of three
shapes:

- `PATH.cb` is a bundle file.
- `@name` is a stored state.
- `%N` is a previous entry, newest first from 1.

A path whose extension is `cb`, in any letter case, reads a bundle. A leading
`@` or `%` reads stored state. Any other source fails:

```text
error: Cannot restore the state
```

A leading `~` expands to the home folder. State pickers stay literal, so
`@~/name` stays a state name.

The run resolves the manifest, prepares it against the applied state, then runs
the shared commit sequence. The restore stores a new history entry, as an apply
does. A restore from an old entry can rewrite or
remove documents that later applies added, so review the preview before you
answer `yes`.

## Bundle source

```sh
confit restore ~/packages/laptop.cb
```

A bundle is a gzipped tar with `manifest.json` first and the content entries
after it. The command reads the manifest and the content bytes from the file, so
a bundle restore needs no pool entry. An absent or unreadable file fails before
any write:

```text
error: Cannot read the bundle at '/home/ada/packages/laptop.cb'
```

A member that the manifest lists and the bundle lacks fails the run before any
write. The bundle layout belongs to [State](state.md).

## Named source

```sh
confit restore @laptop
```

`@name` reads `states/{name}.json` under the state folder. The name is non-empty
with no slash, backslash, or NUL byte. `.` and `..` fail as names. A
malformed state picker fails before any write:

```text
Cannot restore the state '{state}', invalid stored state: {reason}
```

The reasons are `name is empty`, `name holds forbidden characters`,
`history state expects a number`, `state expects @name or %n`,
`history state is out of range`, and `history holds fewer entries`.

A name with no stored manifest fails before any write.
`confit bundle PROJECT -o @name` writes that manifest, and `confit export @name`
packs it into a bundle.

## History source

```sh
confit restore %1
```

`%N` picks a previous entry, newest first, counted from 1. The store keeps five
previous entries and the current one, which has no picker. `%1` is the previous
run. `%0` fails as an out-of-range state. A number past the
oldest entry fails before any write. A non-number fails as a bad state picker, with
`history state expects a number` as the reason.

The restore hint after an apply prints `restore: confit restore %1` when a
previous entry exists. That command undoes the run. To repair a hand edit, apply
again instead: the run rewrites the desired bytes over the edit.

## Rescue source

```sh
confit restore @!
```

`@!` is the rescue state: the files that were on disk before the run
overwrote them. The run saves managed-absent destinations there before the
writes and replaces the state on each capture, so `@!` is the latest rescue.
Use the quoted form in an interactive shell, where a bare `!` triggers
history expansion:

```sh
confit restore '@!'
```

## Shared sequence

`restore` runs the same steps as `apply`:

1. Compare the stored manifest against the applied state for create, update, and
   delete.
1. Compare the applied state against disk for changes outside ConfIt.
1. Print the preview to stderr.
1. Ask the confirmation question from [Apply](apply.md) unless `--force` is set.
1. Diff the state against disk again and ask a second time when the differences
   changed.
1. Save unmanaged destinations into the rescue state.
1. Write documents, remove dropped paths, store history, prune the pool, and
   run hooks.
1. Print the counts and the restore hint to stdout and the log path to stderr.

[Apply](apply.md) covers the prompt, the write order, the hooks, and the failure
behavior. The count line reads:

```text
restored: 4 files, 1 removed
```

The restore hint prints while the run wrote or removed at least one document:

```text
restore: confit restore %1
```

A run that saved unmanaged destinations prints the saved line too:

```text
saved: 3 files, confit restore @!
```

[Output](output.md) covers the preview sections, the streams, and the exit
codes.
