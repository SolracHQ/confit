# Apply

```sh
confit apply PROJECT [--profile FILE] [--force]
```

`apply` evaluates a project and writes its documents to the machine. Without
`--profile`, the run reads `main.lua` inside `PROJECT`. A relative `--profile`
name resolves against the project folder, and an absolute name stays as is. A
project folder without the default profile fails as a missing project.

The run follows this sequence:

1. Evaluate the profile into documents and hooks.
1. Build the desired manifest with the documents in path order.
1. Persist opaque and tree content into the pool.
1. Load the applied state and compare it with the desired manifest and disk.
1. Print the preview and ask for confirmation.
1. Write documents, remove dropped paths, store state and history, prune the
   pool, and run hooks.

[State](state.md) covers the stored manifest. [Difference](difference.md) covers
the comparisons. [Restore](restore.md) runs the same commit sequence from a
stored source.

Content from `open` and `fetch` handles lands in the pool before the preview. A
declined run leaves the pool filled and writes no document.

## Preview

The preview prints to stderr. It shows up to four sections, and an empty section
skips its title. [Output](output.md) covers the section order, the sigils, and
the color rule.

A first run has no applied state, so the preview starts with `Resources`:

```text
Resources
+ config('starship.toml'): toml
  + add_newline = false
  + scan_timeout = 10
+ data('fonts/JetBrainsMono'): tree
  + tree (2 files)
+ home('.bashrc'): rc
  + profile EDITOR = hx
  + alias ll = ls -l
  + init[0] = eval "$(zoxide init bash)"
+ home('.config/bat/config'): text
  + --theme="catppuccin"
Hooks
+ mise install
Summary
Documents: 4 to add, 0 to change, 0 to destroy.
Hooks: 1 to add, 0 to change, 0 to destroy.
```

A later run starts with
`Changes outside Confit will be overwritten on next apply` when the disk differs
from the applied state, then lists the resource blocks. A document whose
declaration matches the state and the disk adds no block.

After the preview, each destination whose difference failed prints one warning,
and the run continues:

```text
Cannot state the difference for '{route}': {cause}
```

`check` prints the same preview to stdout and writes no document. Use it to
review a profile without a prompt.

## Prompt

The run asks one question on stderr:

```text
Apply these changes? Type 'yes' to continue:
```

The question ends with one space. A reply whose trimmed text is `yes` continues.
Any other reply prints this line and stops the run:

```text
no confirmation, the run stops
```

A stopped run writes no document, prints `applied: 0 files, 0 removed`, and
exits 0.

`--force` skips this question. The preview still prints.

The run compares the applied state against disk again before the first write. A
fresh difference set prints its lines and the run continues with the confirmed
plan. The rescue state holds the pre-write capture either way.

## Write order

After confirmation the run commits in this order:

1. Write the documents in manifest path order.
1. Remove recorded destinations that the manifest lacks.
1. Remove tree members that the manifest drops.
1. Store the applied state.
1. Add the state to the history.
1. Prune pool entries that no state references.
1. Run the hooks.

Path order is the display order of the route, so `cache('...')` and
`config('...')` documents land before `home('...')` documents.

- `text` lands the content byte for byte.
- `structured` lands the serializer output for `json`, `toml`, or `yaml`.
- `rc` lands the rendered sections and the interactive guard.
- `opaque` lands the raw handle bytes.
- `tree` lands one file per member under the destination.
- `link` lands a symlink at the destination.

Parents build on demand, so a missing parent folder is created. Text and opaque
documents carry a mode, and the mode lands after the bytes. Each tree member
carries its own mode, and each lands after the member bytes.

A destination that is a pre-existing symlink is unlinked before a regular
document lands there. The target of the old link stays untouched.

An unmanaged text or opaque document writes only when its declaration changed or
its destination is absent. A present unmanaged destination whose declaration
matches the recorded manifest skips the write.

A tree member in the previous manifest but missing from the desired manifest is
removed from the destination. Files placed by hand under the tree destination
stay.

Each written destination reports one line on stderr:

```text
Added home('.bashrc')
```

Each removed destination reports one line:

```text
Removed home('.old')
```

The run ends with a count line:

```text
2 of 3 files applied
```

## Hooks

Hooks run after the documents, the state, and the history land. They run in
profile config order. Every hook decides against the host facts after the
writes:

- An unmet `requires` condition stops the run before this hook and the remaining
  hooks.
- An unmet `when` condition skips the hook, and the run continues.
- A met `check` condition skips the hook, and the run continues.
- An unmet `check` condition runs the hook.
- An unmet `check` condition after the run stops the run before the remaining
  hooks.

The changed set is every built path on a first run. Later runs cover the
rewritten paths and the differenced paths. A changed tree member marks its
parent destination.

A hook that runs prints one line to stderr:

```text
hook 1 of 3: mise install
```

Hook output streams into `logs_dir/hooks/{slug}-{hash}.log`. The slug is the id
with each non-alphanumeric run replaced by a hyphen and the letters lowercased,
cut to 40 characters. The hash is the first 12 hex characters of the id hash. A
nonzero exit fails the hook, and a signal death reads as exit code 1. A timeout
kills the process and fails the hook.

[Hooks](hooks.md) covers the declarations and the conditions.

## Report

A completed run prints the count line to stdout:

```text
applied: 4 files, 1 removed
```

The written count adds one per document. A tree adds one for its destination
rather than one per member. The removed count adds one per removed document and
one per dropped tree member.

While the run wrote or removed at least one document, it prints the restore
hint:

```text
restore: confit restore %1
```

The hint is the previous history entry, so the command undoes the run. It
prints only when a previous entry exists. A run that changed
nothing prints only the count line.

Before the writes, the run saves unmanaged destinations into the rescue state.
When the capture holds at least one file, the report gains a saved line:

```text
saved: 3 files, confit restore @!
```

`confit restore @!` writes the saved bytes back. The [state](state.md) page
covers the state.

Every run prints the log path to stderr:

```text
log: /tmp/confit-4242/confit.log
```

A declined run prints `applied: 0 files, 0 removed`, no hint, and the log path,
then exits 0.

## Failures

A failed write stops the run at that document. The remaining documents,
removals, state, history, prune, and hooks do not run. Documents that already
landed stay on disk. A failed removal stops the run before the state lands.

The state and the history land only after every write and removal succeeds. A
failed hook runs after the state and history land, so the files and the state
stay. The run stops before the remaining hooks and prints no count line.

Every failure prints one summary line and a closing line to stderr, then exits
1:

```text
error: Cannot write the destination at '/home/ada/.bashrc'
For more information please see the log
```

[Output](output.md) covers the closing lines and the exit codes.
