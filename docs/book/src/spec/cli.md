# CLI

Every run accepts one command and the two global flags. The global flags come
before the command name.

```sh
confit [--logs-dir DIR] [--log-level LEVEL] COMMAND ...
```

| Command | Purpose |
| --- | --- |
| `bundle` | evaluate a profile into a bundle or named state |
| `check` | preview the profile against the machine |
| `apply` | write the project onto the machine |
| `restore` | write one bundle or stored state onto the machine |
| `export` | pack one state into a bundle or print its manifest |
| `delete` | delete one named state and prune unreferenced content |
| `init` | scaffold a project or refresh one generated target |

The complete synopsis:

```sh
confit bundle PROJECT [--profile FILE] -o PATH|@name
confit check PROJECT [--profile FILE]
confit apply PROJECT [--profile FILE] [--force]
confit restore SOURCE [--force]
confit export [PICKER] [-o PATH] [-m|--manifest]
confit delete @name
confit init [DIR] [--stubs] [--plugins] [--lsp]
```

`PROJECT` is a folder. The default profile is `main.lua` inside that folder. A
missing default profile fails with the message
`Cannot load the project, the folder '{PROJECT}' is absent`. An explicit profile
that is missing fails as an unreadable profile.

`--profile FILE` resolves relative to the project folder. An absolute path is
kept. A leading `~` expands before resolution. The [project](project.md) page
states profile selection in full.

Every path argument expands a leading `~` before use:

- the project folder and the profile of `bundle`, `check`, and `apply`
- the source of `restore`
- the output of `bundle` and `export`
- the folder of `init`
- the value of `--logs-dir`

A state picker stays literal. `@name` and `%N` are never paths.

The run creates the logs folder and opens `confit.log` before it dispatches a
command. A folder or file failure stops the run with exit 1.

## bundle

```sh
confit bundle PROJECT [--profile FILE] -o PATH|@name
```

| Flag | Value | Rule |
| --- | --- | --- |
| `-p`, `--profile` | file | defaults to `main.lua` |
| `-o`, `--output` | path or `@name` | required |

`bundle` evaluates the profile and builds the manifest. It does not preview the
difference, prompt, or write a home document. Hooks are recorded in the manifest
and do not run.

A path output writes a bundle file. A path without a `.cb` extension gains it.
An existing `.cb` or `.CB` extension is kept. The command prints the path:

```text
bundle: target/dotfiles.cb
```

An `@name` output writes a named state at `states/{name}.json` and prints the
picker:

```text
bundle: @laptop
```

A named output stores only the manifest. It writes no content bytes. A restore
reads content from the pool or the cache, so those bytes must already be there.
A file output embeds the content that the manifest references.

A name fails when it is empty, contains `/` or `\`, is `.` or `..`, or contains
a NUL byte. A bare output that starts with `@` is a state. Any other output is a
file.

## check

```sh
confit check PROJECT [--profile FILE]
```

`check` evaluates the profile, builds the manifest, and compares it with the
applied state and with the disk. It writes the summary to stdout and the log
path to stderr.

```text
Resources
+ home('.bashrc'): rc
  + alias ll = ls -l
Summary
Documents: 1 to add, 0 to change, 0 to destroy.
log: /tmp/confit-4812/confit.log
```

A summary can include a difference section titled
`Changes outside Confit will be overwritten on next apply`, then `Resources`,
`Hooks`, and `Summary`. The `Documents:` line is always present. The `Hooks:`
line appears when a hook changes. A first run compares the manifest with the
disk. A later run compares it with the applied state.

`check` never writes a home document, a state file, or a hook log. It writes the
run log only. It exits 0 when the summary shows changes. The
[output](output.md) page states the line formats.

## apply

```sh
confit apply PROJECT [--profile FILE] [--force]
```

`apply` evaluates the profile, stores content in the pool, prints the preview,
asks for confirmation, then writes.

The preview is the same summary that `check` prints, and it goes to stderr. The
prompt also goes to stderr:

```text
Apply these changes? Type 'yes' to continue:
```

The answer comes from stdin. A trimmed answer equal to `yes` proceeds. Any other
answer, a dismissed prompt, or an empty reply at end of input prints
`no confirmation, the run stops` to stderr, writes nothing, and exits 0 with
these counts:

```text
applied: 0 files, 0 removed
log: /tmp/confit-4812/confit.log
```

`--force` skips this first prompt. Before the write, the run checks the
difference against the disk again. A changed difference prints its lines and
asks again. `--force` does not skip this second prompt.

After the confirmation, the run writes documents, removes dropped documents and
tree members, stores the applied state, rotates history, prunes unreferenced
pool entries, and runs the hooks. Hook lines go to stderr. Counts go to stdout:

```text
hook 1 of 2: mise install
applied: 4 files, 1 removed
restore: confit restore %1
log: /tmp/confit-4812/confit.log
```

The `restore:` line appears when the written count or the removed count is above
zero. It is the previous history entry, so the command undoes the run.

When the run saves unmanaged files, it prints a saved line before the hint:

```text
saved: 3 files, confit restore @!
```

The line is the rescue state. It prints only when the capture holds at
least one file. [State](state.md) covers the state.

A failure during evaluation, writing, state storage, or a hook stops the run
with exit 1. The [apply](apply.md) page states the write sequence.

## restore

```sh
confit restore SOURCE [--force]
```

`SOURCE` takes one of three forms:

| Form | Resolution |
| --- | --- |
| path ending in `.cb` | bundle file, case-insensitive extension |
| `@name` | named state |
| `@!` | rescue state, the pre-run capture |
| `%N` | history entry, previous runs, newest first from 1 |

A source without `.cb` and without an `@` or `%` prefix fails as an invalid
restore source. `restore` reads no project and accepts no profile. The bundle or
state carries the manifest and its hooks.

`restore` uses the apply sequence: preview, prompt, fresh difference check,
write, store the applied state, rotate history, prune the pool, and run hooks.
`--force` has the same meaning as for `apply`. Counts and the restore hint go to
stdout:

```text
restored: 3 files, 0 removed
restore: confit restore %1
log: /tmp/confit-4812/confit.log
```

After a restore, the restored manifest is the applied state. The
[restore](restore.md) page states bundle content rules.

## export

```sh
confit export [PICKER] [-o PATH] [-m|--manifest]
```

| Argument | Rule |
| --- | --- |
| `PICKER` | `@name` or `%N`; default is applied |
| `-o`, `--output` | bundle path; a missing `.cb` is added |
| `-m`, `--manifest` | print the manifest as pretty JSON |

`-o` and `--manifest` conflict. Both flags in one run are a usage error with
exit 2.

A file export reads the state and writes a bundle. The automatic output name
is in the current folder:

| Picker | Name |
| --- | --- |
| applied | `applied.cb` |
| `@name` | `{name}.cb` |
| `%N` | `prev-{N}.cb` |

The command prints the written path:

```text
export: applied.cb
```

`--manifest` prints the manifest as pretty JSON to stdout and prints no path
line. Content for a bundle comes from the pool or the cache. A missing entry
fails the run. A picker that names no stored state fails.

## delete

```sh
confit delete @name
```

`delete` accepts only an `@name`. A bare name and `%N` fail. The command
removes `states/{name}.json`, then removes every pool entry that no remaining
state references. The applied state, the history entries, and the other named
states keep their entries.

```text
delete: @laptop (12 entries pruned)
```

`delete` writes no home document and asks no question. A missing name fails
because the state does not exist.

## init

```sh
confit init [DIR] [--stubs] [--plugins] [--lsp]
```

`DIR` defaults to the current folder. A plain run scaffolds the profile, the
project file, the stubs, the core plugins, and the editor config. The run
refuses a present destination.

```text
init: site/main.lua (18 files)
```

`--stubs`, `--plugins`, and `--lsp` refresh one generated target and overwrite
the present files. The plain scaffold does not run when a refresh flag is
present. Flags combine, and each flagged target prints one line:

```text
stubs: site/stubs (7 files)
plugins: site/plugins (8 files)
lsp: site/.luarc.json (1 files)
```

The [init](init.md) page states the scaffold contents and the clash rules.

## Global flags

```sh
confit --logs-dir DIR --log-level LEVEL COMMAND ...
```

`--logs-dir DIR` sets the folder for `confit.log` and the `hooks` subfolder. A
leading `~` expands. The flag wins over `CONFIT_LOG_DIR`. The environment
variable applies when the flag is absent. The default is the OS temp folder and
`confit-{pid}`.

`--log-level LEVEL` accepts `off`, `error`, `warn`, `info`, `debug`, and
`trace`. The default is `warn`. The level filters the messages in `confit.log`.
The command output on stdout and stderr does not change.

## Pickers and state names

A picker selects one stored manifest:

| Picker | File |
| --- | --- |
| applied | newest file under `history` |
| `@name` | `states/{name}.json` |
| `%N` | the Nth previous file under `history`, newest first |

A name is one path component. It is non-empty, with no `/` or `\`, and is
not `.` or `..`. A NUL byte fails. The store adds `.json` to the name.

`%N` accepts a decimal number from 1 through the count of previous entries.
The store keeps five previous entries. `%0` fails as an out-of-range state. A
number past the oldest entry fails.

`bundle -o` accepts only `@name` among the pickers. `delete` accepts only
`@name`.
`export` accepts the default applied state, `@name`, and `%N`. `restore`
accepts `@name`, `%N`, and a bundle path. A picker is literal text; it does not
expand a leading `~`.

## Streams and exits

| Command | stdout | stderr |
| --- | --- | --- |
| `bundle` | `bundle: {path}` or `bundle: @name` | `log:` on failure |
| `check` | the summary | `log: {path}` |
| `apply` | counts, restore hint | preview, hook lines, prompt, `log:` |
| `restore` | counts, restore hint | preview, hook lines, prompt, `log:` |
| `export` | `export: {path}` or the manifest | `log:` on failure |
| `delete` | `delete: @{name} ({n} pruned)` | `log:` on failure |
| `init` | one line per target | `log:` on failure |

The prompt goes to stderr and the answer comes from stdin. A result line goes to
stdout.

Exit 0 means success. A declined `apply` or `restore` exits 0 and writes
nothing. `check` exits 0 when the summary shows changes.

Exit 1 means a failed run. The run prints `log: {path}` and an `error:` line to
stderr, then an optional closing hint.

Exit 2 means a usage error from the argument parser. The message and the usage
line go to stderr. No log file is written. `--help` and `--version` exit 0.
