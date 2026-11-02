# ConfIt specification

```text
Spec-Version: 0.9.0
```

This page states the behavior of ConfIt 0.9.0. A binary at this version follows
this page. A behavior change updates this record in the same change that ships
it.

## Scope

The specification covers the command line, the project folder, profile
evaluation, the Lua surface, documents, patches, conditions, hooks, the
manifest, bundles, and the state store.

It covers the bytes that ConfIt writes and the files that it reads. It does not
cover the behavior of a shell that reads an rc document, a program that a hook
starts, or a file that a managed document points at.

An operating system convention, such as the location of the config folder,
appears as the convention. A statement that this page leaves out is outside the
0.9 contract.

## Conventions

A path argument expands a leading `~` to the home folder. A relative path
resolves against the folder that the command accepts, unless the text says
otherwise. File paths in examples use forward slashes.

A condition is true or is false. The verb `fails` means that the run stops with
exit 1 and an `error:` line. The verb `refuses` means that the run stops before
the write.

Fenced blocks carry the language that they show:

| Fence | Content |
| --- | --- |
| `sh` | a command line |
| `toml` | a project file |
| `lua` | a profile or module |
| `text` | output or a path |

The text uses these names:

| Name | Meaning |
| --- | --- |
| `PROJECT` | project folder |
| `FILE` | a file path |
| `@name` | a named state |
| `%N` | a previous entry, newest first from 1 |

## System model

A run reads one project, evaluates one profile, assembles documents, applies
patches, and builds a manifest. The manifest is the desired state. A command
compares it with the applied state and with the disk before it writes.

| Term | Meaning |
| --- | --- |
| project | folder with the profile and `project.toml` |
| profile | Lua file that returns configs and documents |
| config | named group of documents, patches, hooks, requirements |
| document | one destination path with one payload |
| patch | callback that edits a document or a hook |
| hook | command that runs after the writes |
| state | manifest for the applied run, a history entry, or a name |
| bundle | gzipped tar with a manifest and content |
| pool | content-addressed store of document bytes |

The stages of one run are:

1. Read `project.toml` and evaluate the profile.
2. Assemble documents from the profile and its configs, then sort by path.
3. Apply patches in priority order to live document and hook tables.
4. Validate conditions and build one manifest.
5. Compare the manifest with the applied state and with the disk.
6. Write documents, remove dropped ones, store state, prune the pool, and run
   hooks for `apply` and `restore`.

The [project](project.md) and [profile](profile.md) pages define the inputs. The
[documents](documents.md), [patches](patches.md), [conditions](conditions.md),
and [hooks](hooks.md) pages define the manifest content. The [state](state.md)
page defines what a run records.

## State layout

Every path sits under the OS config folder and `confit`. On Linux the folder is
`${XDG_CONFIG_HOME:-~/.config}/confit`.

| Path | Contents |
| --- | --- |
| `history/{stamp}.json` | one rotated manifest |
| `states/{name}.json` | named manifests |
| `states/!.json` | rescue capture, replaced per run |
| `blobs/{sha}` | gzipped content at level 6 |

`{stamp}` is nanoseconds since the Unix epoch. A collision increments the value
until the name is free. `{sha}` is the 64 lowercase hex characters of the
SHA-256 digest of the uncompressed content.

Fetched bodies and archive members live under the OS temp folder and
`confit/cache`:

| Path | Contents |
| --- | --- |
| `cache/{sha}` | fetched body or archive member |
| `cache/{sha}.url` | URL index for a fetched body |

The default logs folder is the OS temp folder and `confit-{pid}`:

| Path | Contents |
| --- | --- |
| `confit.log` | the run log |
| `hooks/{slug}-{hash}.log` | output of one hook run |

`{slug}` is up to 40 lowercase alphanumeric characters from the hook id,
joined by hyphens. `{hash}` is the first 12 hex characters of the SHA-256 digest
of the hook id. The `--logs-dir` flag and `CONFIT_LOG_DIR` change the logs
folder. The [CLI](cli.md) page states the order.

## Version compatibility

The binary carries a version triplet. Version 0.9.0 stores `[0, 9, 0]`. The
`version` field of a manifest is the same three integers.

A manifest loads when its major number matches the binary and one more rule
applies:

- Before 1.0, the minor number matches as well. A different fix number loads.
- From 1.0, the minor number may differ. A different fix number loads.

A different major number fails. A manifest without a version fails. Both
failures stop the run.

The `version` field of `project.toml` records the scaffold that wrote the file.
Evaluation does not compare it with the binary.

## Map

| Page | Covers |
| --- | --- |
| [CLI](cli.md) | commands, flags, output streams, and exit codes |
| [Project](project.md) | the folder, `project.toml`, require, sandbox |
| [Profile](profile.md) | the profile table, the config table, and order |
| [Documents](documents.md) | routes, payload kinds, modes, and trees |
| [Patches](patches.md) | targets, filters, priorities, and merge rules |
| [Conditions](conditions.md) | env, path, existence, and changed tests |
| [Hooks](hooks.md) | commands, guards, timeouts, and logs |
| [Lua surface](lua.md) | builtin modules, handles, facts, and validation |
| [Plugins](plugins.md) | the shipped plugin tree and its helpers |
| [State](state.md) | the manifest, history, named states, and pool |
| [Difference](difference.md) | the comparison against state and disk |
| [Apply](apply.md) | the write sequence and its prompts |
| [Restore](restore.md) | bundles and stored states |
| [Output](output.md) | the terminal lines and the log file |
| [Init](init.md) | the scaffold and the refresh targets |
