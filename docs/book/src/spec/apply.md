# Apply

```sh
confit apply PROJECT [--profile FILE] [--force]
```

This command writes one project to the machine. By default Apply reads
`main.lua` inside `PROJECT`. A relative `--profile` name resolves against the
project folder. An absolute `--profile` name stays as is. A project folder
that doesn't include the default profile fails as a missing project.

Apply follows these steps in order:

1. Evaluates the profile into documents and hooks.
1. Builds the desired manifest in path order.
1. Persists opaque and tree content into the pool.
1. Loads the applied state and compares it with disk.
1. Prints the preview.
1. Asks for confirmation.
1. Writes documents.
1. Removes dropped paths.
1. Stores the applied state.
1. Adds the state to the history.
1. Prunes the pool.
1. Starts the hooks.

[State](state.md) covers the stored manifest. [Difference](difference.md) covers
the comparisons. [Restore](restore.md) uses the same commit sequence from a
stored source. Content from `open` and `fetch` handles lands in the pool before
the preview. After a decline the pool stays filled. The command limits writes
to the pool.

## Preview

The preview appears on stderr with five sections in this order:
`Evaluation` first, `Comparison` next, `Documents` next, `Hooks` next, and
`Summary` last. Each section with content shows its title, and `Comparison`
shows its title in all cases. [Output](output.md) covers the order and the
colors.

The preview limits titles to the five named sections, marks drift as `!`
after the change sigil, shows `~ ! .bashrc: rc` for a drifted update, and
shows `- ! .bashrc: rc` for a drifted delete.

`Documents` is a route tree. The tree holds one branch per base. The
branches are `[home]` and `[config]` and `[data]` and `[literal]`. A based
branch nests relative paths under folders. A `[literal]` branch shows full
paths as a flat list. Each leaf shows its sigil, shows its name, and shows
its kind. A tree group shows one leaf with bracket counts. A first apply
shows additions against disk. A later apply shows wanted content against
disk with drift marks.

```text
Documents
├── [config]
│   └── mise
│       └── + config.toml: toml
└── [home]
    ├── + .bashrc: rc
    └── .config
        └── + bat: text
```

`Hooks` shows the hook lifecycle lines. `Summary` shows one table only. The
table has `+` and `~` and `-` columns. It has one `Documents` row and has
one `Hooks` row while hooks change. The table appears with zeros when counts
are zero.

```text
Summary
            +  ~  -
 Documents  1  0  1
 Hooks      1  0  0
```

`check` prints the same preview to stdout, limits writes to the log file,
and ends after the preview. You use it to review a profile in a single
pass.

## Prompt

One question appears on stderr. The question reads:

```text
Do you want to apply these changes? Type 'yes' to continue:
```

The question ends with one space. You answer on stdin. Only a trimmed `yes`
continues the command. Any other answer declines the command. A typed `details`
declines the command. After a decline the command prints this line:

```text
Cancelled.
```

A declined command prints only `Cancelled.` and the log path and exits 0.
`--force` skips this question. The preview still prints with `--force`. The
command asks the question once.

## Write order

Apply commits the effective view changes in view order. An addition or change
writes its destination. A removal removes its destination.

After the changes land, the command stores the applied state, adds the state to
the history, and prunes pool entries outside the referenced set. Then the
command starts the hooks.

Parents build on demand. The command creates parent folders for destinations
needing them. A pre-existing symlink at a destination is unlinked first. The
old link target stays at its path.

Text and opaque documents carry a mode, and the mode lands after the content.
Each tree member carries its own mode, and each member mode lands after the
member content.

An unmanaged text document writes on declaration change and at first creation,
and retains disk content under a matching recorded manifest. An unmanaged tree
recreates only missing members. A dropped tree member from the previous
manifest has its disk copy removed. Files placed by hand under the tree
destination stay.

## Hooks

The command starts hooks after the documents and the state and the history land
and starts them in profile config order. Every hook decides against the host
facts after the writes. An unmet `requires` condition stops the command before
that hook. An unmet `when` condition skips the hook and continues. A met `check`
condition skips the hook and continues. An unmet `check` condition starts the
hook. An unmet `check` condition after the hook blocks later hooks.

A finished hook prints its command line:

```text
> mise install
```

Hook output streams into `hooks/{slug}-{hash}.log`. The slug is the id
lowercased with each group outside letters and digits replaced by hyphen and
is cut to 40 characters. The hash is the first 12 hex characters of the id
hash. A nonzero exit fails the hook. A signal death reads as exit code 1. A
timeout kills the process and fails the hook. [Hooks](hooks.md) covers the
declarations and the conditions.

## Report

`Results` follows the write tree when counts exist. The table adds one
failed column. The failed column uses `✗`.

```text
Results

            +  ~  -  ✗
 Documents  1  0  0  0
 Hooks      1  0  0  0
```

With zero changes and zero hook moves the output consists of restore hints
after `Applying Changes`. With table content the hints follow the table. The
exact disk hint prints while the rescue capture holds files:

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

The undo hint prints with zero changes and names the previous entry. Every
command prints the log path to stderr:

```text
Log: /tmp/confit-4242/confit.log
```

A declined command prints `Cancelled.` and the log path and exits 0.

## Failures

A failed write stops the command at that document. Remaining documents and
removals stay unapplied. The command skips state and history and prune and
hooks. Documents that already landed stay on disk. A failed removal stops the
command before the state lands. State and history land only after every write
and removal succeeds. A failed hook starts after state and history land. The
files and the state stay. The command stops before later hooks and doesn't
print the hint table on failure except the rescue hint.

Every failure prints one summary line and one closing line on stderr. The
command exits 1:

```text
Error: Cannot write the destination at '/home/solrac/.bashrc'
For more information please see the log
```

[Output](output.md) covers the closing lines and the exit codes.
