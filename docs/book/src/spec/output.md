# Output

The output is a visual language.
Phase titles mark progress.
Route trees show planned files.
Route trees also show landed files.

Count tables show plan totals.
Count tables also show close totals.
Stderr holds all text views.
Stdout holds JSON only.

[Schemas](schemas.md) holds JSON shapes.
[CLI](cli.md) lists flags that select each case.

## Symbols

Each mark has one meaning.

| Mark | Meaning |
| --- | --- |
| `+` | Addition |
| `~` | Change |
| `-` | Removal |
| `!` | Drift |
| `?` | Incomparable |
| `✓` | Landed |
| `✗` | Failed |
| `·` | Unattempted |
| `>` | Hook line |

## Colors

Color appears only on a terminal.
Piped output doesn't use color.
A set `NO_COLOR` value doesn't show color.
An empty `NO_COLOR` value keeps color on.

| Mark | Color |
| --- | --- |
| `+` | Green |
| `~` | Yellow |
| `-` | Red |
| `?` | Red |
| `!` | Magenta |
| `✓` | Green |
| `✗` | Bold red |
| `·` | Dim |
| `>` | Dim |
| Title | Bold blue underline |
| `Documents` | Bold |
| `[base]` | Bold cyan |
| `Log` | Dim with bold path |
| Binary | Bold green |
| Task | Dim |

## Phases

Phases appear in a fixed order.
The preview holds `Evaluation` through `Summary`.
The close holds `Persisting` onward.

Restore doesn't show `Evaluation`.
Bundle ends with `Packaging`.
Check ends after the preview.

| Phase | Rule |
| --- | --- |
| `Evaluation` | Shows for bundle check apply |
| `Comparison` | Always shows |
| `Documents` | Always shows |
| `Hooks` | Shows when hooks change |
| `Summary` | Always shows |
| `Persisting` | Shows for apply restore |
| `Packaging` | Shows for bundle |
| `Applying Changes` | Shows for apply restore |
| `Results` | Shows when counts exist |
| `Details` | Shows for check with yes |

A spinner marks the active fetch task on a terminal.
A spinner also marks the active pool task on a terminal.
A spinner also marks the active hook task on a terminal.
Piped output keeps finished lines with dim text.

## Trees

Based branches nest relative paths under folders.
Literal branches list full paths as a flat list.
Each leaf shows a sigil.
Each leaf also shows a name.
Each leaf also shows a kind.

Drift adds `!` after the sigil. A drifted change shows `~ ! .bashrc: rc` and a
drifted delete shows `- ! .bashrc: rc`.
An incomparable destination shows `?` with a reason.
A collapsed group shows one leaf with bracket counts. A tree removal with one
pending member shows `[-: 95, ·: 1]`.
`Details` reuses the same leaf shape under a header. The Details view shows the
plain route, and the `!` marker stays only in the preview.

The `Applying Changes` tree is rooted at the parent path.
Each landed change shows a check mark.
Each landed change also shows its plan sigil.

| Base | Holds |
| --- | --- |
| `[cache]` | Cache files |
| `[config]` | Config files |
| `[data]` | Data files |
| `[home]` | Home files |
| `[literal]` | Full paths |

| Reason | Case |
| --- | --- |
| `unreadable` | Cannot read path |
| `directory blocks the managed path` | Dir at path |
| `ancestor blocks the path` | Blocked parent |
| `special file blocks the path` | Special at path |

```text
Documents
└── [home]
    └── + .bashrc: text
```

```text
Applying Changes
└── /home/solrac
    └── ✓ + .bashrc: text
```

## Tables

The `Summary` table shows plan counts.
It holds a `Documents` row.
It also holds a `Hooks` row when hooks change.

The `Results` table adds a failed column.
It holds the same rows with close totals.
A failed hook shows compact lines.
A blocked hook also shows compact lines.
Compact lines show only nonzero categories.

| Column | Counts |
| --- | --- |
| `+` | Additions |
| `~` | Changes |
| `-` | Removals |
| `✗` | Failures |
| `·` | Pending |

```text
Summary
            +  ~  -
 Documents  1  0  1
```

## Lines

A key line is one route, one key, and two values:

| Line | Form |
| --- | --- |
| change | `~ {path}: {key} = {old} -> {new}` |
| addition | `+ {path}: {key} = {value}` |
| removal | `- {path}: {key} = {value}` |

A key is a dotted leaf key. An array position starts at 1, as in `tools[1]`.
A link uses `target`. An opaque document uses `content`. A tree member uses its
relative path. A mode uses `mode` or `{member}:mode`.

A text value renders bare. A number, a boolean, and null render as literals.
An array and an object render as compact JSON.

A text diff keeps a context line with one leading space. An addition uses `+`
and a removal uses `-`. The preview adds two spaces of indentation. Each line
is painted by its sign.

A document mode change yields `~ {path}: mode = {wanted} -> {seen}`. A tree
member mode change yields `~ {path}: {member}:mode = {wanted} -> {seen}`. Modes
render in octal starting with the first significant digit, as in `755` and
`644`. A text content change may carry a mode line after it. An opaque content
change reports only the content line. A symlink destination reports only the
target line.

A recorded hash without pooled content produces a warning:

```text
Cannot state the difference for '{route}': {cause}
```

The command continues. The line shows the bare hash.

| Command | JSON shape |
| --- | --- |
| `bundle` | [Bundle result](schemas.md#bundle-result) |
| `check` | [Check result](schemas.md#check-result) |
| `apply` | [Apply result](schemas.md#apply-result) |
| `restore` | [Restore result](schemas.md#restore-result) |
| `delete` | [Delete result](schemas.md#delete-result) |
| `init` | [Init result](schemas.md#init-result) |
| `export -m` | [Manifest](schemas.md#manifest) |

## Approval

Approval sits between `Summary` and `Persisting`.
The preview still prints when force skips the question.
Only a trimmed `yes` approves.
Any other answer declines.
A decline prints `Cancelled.`.
A decline exits with `0`.

| Backend | Approval | Details |
| --- | --- | --- |
| `plain` | Typed line | Typed line |
| `inline` | yes/no/details | yes/no |
| `none` | Force only | Closes directly |

The `plain` backend accepts a typed line.
The `inline` backend shows a picker on a terminal.
The `none` backend closes details with the stored view.
The `none` backend approves only with force.

## Hints

Hints follow `Results` as `>` lines.
The rescue hint points at `@!` while capture holds files.
The undo hint points at the prior entry while one exists.

The drift hint names dropping disk changes while drift exists.
Zero changes don't show a `Results` table.
Zero changes still show hints.

| Case | Hint target |
| --- | --- |
| Rescue holds files | `@!` |
| Undo entry exists | `%1` |
| Drift exists | Drop note |

## Logs

The logs folder defaults to the temp folder with the pid.
`CONFIT_LOG_DIR` sets the folder.
The `--logs-dir` flag wins over the environment.
The flags come before the subcommand.

The log file is `confit.log`.
Each record is a pretty JSON object.
Hook output lands under `hooks` with a slug hash name.
[Hooks](hooks.md) covers hook log names.

```sh
confit --logs-dir ~/logs --log-level debug apply ~/dotfiles
```

| Level | Use |
| --- | --- |
| `off` | Doesn't write |
| `error` | Error only |
| `warn` | Default |
| `info` | Informative |
| `debug` | Verbose |
| `trace` | Most verbose |

## Errors

A failure closes with a summary line.
A failure also closes with a closing line.
The `Log` line comes last.

A pool trim failure shows only the summary line.
A usage failure doesn't write a log file.

| Case | Shows |
| --- | --- |
| Write fail | Error with log |
| Pool trim fail | Summary only |
| Usage fail | Parser only |

```text
Error: Cannot load the project
For more information please see the log
Log: /tmp/confit-4242/confit.log
```

## Exits

Exit `0` marks success.
A declined approval also exits with `0`.
Exit `1` marks a failed command.
Exit `2` marks a usage error.

`--help` exits with `0`.
`--version` also exits with `0`.

| Exit | Meaning |
| --- | --- |
| `0` | Success with decline |
| `1` | Failed command |
| `2` | Usage error |
