# Output

ConfIt writes three kinds of text: the preview and prompts on stderr, the result
lines on stdout, and the log file on disk. A command that fails prints one
summary line and a closing line on stderr.

## Sections

The preview is a list of titled sections. An empty section skips its title. A
repeat run shows four sections in this order:

1. `Changes outside Confit will be overwritten on next apply` lists the
   differences between the applied state and disk.
1. `Resources` lists the document blocks with a status sigil.
1. `Hooks` lists the hook lifecycle lines.
1. `Summary` lists the document counts and the hook counts.

A first run has no applied state, so it skips the difference section.
`Resources`, `Hooks`, and `Summary` follow.

A difference line reports one destination:

```text
~ {path}: {key} = {old} -> {new}
'{path}': directory blocks the managed path. remove the directory or apply fails
{path}: deleted by hand. Add it to the config or the next apply overwrites it
```

An unreadable destination prints `cannot read '{path}': {reason}` followed by
`Add it to the config or the next apply overwrites it`.

A text difference prints a header with the document and its kind, then the
changed lines. Recorded lines start with `-`, disk lines with `+`, and unchanged
lines with two spaces:

```text
~ home('.bashrc'): rc
  *) return ;;
  esac

  -alias ll='ls -l'
  +alias ll='ls --color'

  eval "$(zoxide init bash)"
```

A resource block starts with the document path and a label. A structured
document shows its format, and every other kind shows its kind name:

```text
~ config('starship.toml'): toml
  + command_timeout = 500
  ~ scan_timeout = 10 -> 20
+ home('.config/eza/theme.yml'): text
  + colour: always
- home('.config/bat/config'): text
```

A create prints one `+` line per body entry. A structured body prints
`{key} = {value}` for each leaf, with dots between nested keys and array
positions counted from 1. An rc body prints one entry per line. A text body
prints each content line. A tree body prints `tree ({n} files)`, a link body
prints the target route, and an opaque body prints `opaque {label}`.

An update prints only the changed values:

- `structured` prints a leaf change per line, with `+`, `~`, or `-`.
- `rc` prints a unified diff from recorded to desired.
- `text` prints each desired content line with `~`.
- `link` prints `~ target = {old} -> {new}`.
- `opaque` prints `~ content = {old} -> {new}`.
- `tree` prints `~ tree ({changed} of {total} files changed)`.

A change to or from an opaque document prints `~ kind = {old} -> {new}` and the
desired body. A removed document prints only a header:

```text
- home('.config/bat/config'): text
```

A removed tree header adds its member count as `({n} files)`.

The `Hooks` section prints the lifecycle of each hook:

```text
+ mise install
  + requires (in_path(mise))
  + when (changed(config('mise/config.toml')))
~ probe refresh
  ~ timeout (30s -> 600s)
- absent-probe run
```

An added hook prints `+ {argv}` and one line per condition that is not true. A
changed hook prints `~ {argv}` and one line per changed condition or timeout. A
removed hook prints `- {argv}`. An unchanged hook prints nothing.

The `Summary` section always prints the document counts:

```text
Documents: 1 to add, 2 to change, 1 to destroy.
```

A hook line follows while at least one hook moves:

```text
Hooks: 0 to add, 1 to change, 0 to destroy.
```

On a first run the document counts come from the differences against disk, so a
present document with a different body counts as a change.

## Sigils and color

- `+` marks an added line.
- `~` marks a changed line.
- `-` marks a removed line.
- A section title carries no sigil, and a context line starts with two spaces.

The terminal paints an added line green, a changed line yellow, a removed line
red, and a section title bold. Color appears only when stderr is a terminal and
`NO_COLOR` is unset. A value on `NO_COLOR`, including an empty value, turns
color off. Redirecting stderr also turns color off. The color decision reads
stderr for `check` as well, whose preview prints on stdout.

## Streams

- `bundle` prints `bundle: {path}` or `bundle: @name` on stdout, and `log:` on
  failure.
- `check` prints the preview sections on stdout and `log:` on stderr.
- `apply` prints the counts and the restore hint on stdout, and the preview, the
  prompt, and `log:` on stderr.
- `restore` prints the counts and the restore hint on stdout, and the preview,
  the prompt, and `log:` on stderr.
- `export` prints `export: {path}` or the manifest on stdout, and `log:` on
  failure.
- `delete` prints `delete: @{name} ({n} entries pruned)` on stdout, and `log:`
  on failure.
- `init` prints the `init:`, `stubs:`, `plugins:`, or `lsp:` lines on stdout,
  and `log:` on failure.

For `apply` and `restore`, stderr also carries the warnings, the
`no confirmation, the run stops` line, and the hook run lines. The restore hint
prints while the run wrote or removed at least one document. The log path prints
on success and on failure for `check`, `apply`, and `restore`. The other
commands print it on failure.

The spinner also uses stderr. On a terminal it updates in place and shows the
current apply or hook. Off a terminal the spinner hides, and only the
printed lines remain.

## Logs

The default logs folder is the OS temp folder and `confit-{pid}`. The
`CONFIT_LOG_DIR` environment variable sets the folder, and `--logs-dir` wins
over it. A leading `~` expands to the home folder. The global flags come before
the subcommand:

```sh
confit --logs-dir ~/logs --log-level debug apply ~/dotfiles
```

The log file is `confit.log`. Each record is a pretty JSON object with a
`timestamp`, a `level`, and a `message`. An error record adds `error`, a
`reason`, and a `trace` list when frames exist.

The default level is `warn`. `--log-level` takes `off`, `error`, `warn`, `info`,
`debug`, or `trace`.

Hook output goes to `hooks/{slug}-{hash}.log` under the logs folder. The slug is
the hook id lowercased with each non-alphanumeric run replaced by a hyphen and
cut to 40 characters. The hash is the first 12 hex characters of the id hash.
The file opens with `hook {position} of {total}: {argv}` followed by the
process output. [Hooks](hooks.md) covers the run rules.

## Errors

A failed run prints a summary line and a closing line on stderr:

```text
error: {summary}
For more information please see the log
```

Once the log opens, the run prints `log: {path}` before these lines, so the log
is easy to find:

```text
log: /tmp/confit-4242/confit.log
error: Cannot load the project, the folder '/home/ada/dotfiles' is absent
For more information please see the log
```

A pool prune failure prints only its summary and omits the closing line.

## Exits

- 0: the command finished. A declined `apply` or `restore` also exits 0.
- 1: a run failure.
- 2: a command line usage error.

A usage error prints the parser message and the command usage, then exits 2:

```text
error: unexpected argument 'extra' found

Usage: confit check [OPTIONS] <PROJECT>

For more information, try '--help'.
```

[CLI](cli.md) covers the arguments. [Apply](apply.md) and [Restore](restore.md)
cover the commands that write documents.
