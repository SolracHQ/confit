# Safety

Now that the project works, we will explain previews, the rescue state, and undo.

Every write previews first. You read the preview. You answer one question.
`check` prints the same preview and stops.

## Read the preview

A first run compares the manifest with the disk. The preview lists each new
document under `Resources`, each hook under `Hooks`, and the counts under
`Summary`. This excerpt keeps one document and one hook:

```text
Resources
+ config('starship.toml'): toml
  + add_newline = false
  + palette = catppuccin
Hooks
+ mise install
  + requires (in_path(mise))
Summary
Documents: 8 to add, 0 to change, 0 to destroy.
Hooks: 2 to add, 0 to change, 0 to destroy.
```

The mark starts each line. `+` creates, `~` updates, and `-` removes. The
terminal paints added lines green, changed lines yellow, and removed lines red.

## Answer the prompt

```text
Apply these changes? Type 'yes' to continue:
```

The prompt goes to stderr. The answer comes from stdin. A trimmed answer equal
to `yes` starts the write. Any other answer prints one line, writes nothing, and
exits 0:

```text
no confirmation, the run stops
applied: 0 files, 0 removed
```

```sh
confit apply ~/dotfiles --force
```

`--force` skips the first prompt. The preview still prints. A changed disk
difference asks again, with or without `--force`.

## Repair a hand edit

You edit a managed file by hand. The next preview shows the difference:

```text
Changes outside Confit will be overwritten on next apply
~ config('starship.toml'): add_newline = false -> true
```

To repair the edit, apply again. The run writes the desired bytes over the edit.
A deleted file returns the same way. The next apply creates it again.

## Repair a failed hook

A failed hook stops the run with exit 1. The documents and the state stay on
disk. The hook log records the command output. Fix the command or its guard,
then apply again.

## Undo a wrong apply

A run that changes files prints one hint when a previous entry exists:

```text
restore: confit restore %1
```

`%1` is the previous run. `confit restore %1` undoes one apply. `%2` is the run
before that. History keeps five previous entries. A number past the oldest entry
fails.

To repair a hand edit, apply again instead. The run rewrites the desired bytes
over the edit. To undo a run, restore the previous entry.

## The first apply is safe

A first apply has no history to undo with. That is the one dangerous moment. The
run overwrites files it never managed before, and `%1` does not exist yet.

confit saves those files first. Before the writes, the run copies every
destination that exists on disk and that no manifest records into a rescue state.
The report tells you when it happens:

```text
saved: 3 files, confit restore @!
```

`confit restore '@!'` writes those bytes back. Quote it in an interactive shell,
where a bare `!` triggers history expansion.

The state holds one rescue. Each capture replaces the last, so `@!` is always the
latest one. Files over 16 MiB stay out with one warning line. Hooks stay outside
the rescue. It restores files, not installed tools.

## Where to read next

| Page | Covers |
| --- | --- |
| [CLI](../spec/cli.md) | commands, flags, streams, and exit codes |
| [Project](../spec/project.md) | the folder, `project.toml`, require, and the sandbox |
| [Profile](../spec/profile.md) | the profile table and config order |
| [Documents](../spec/documents.md) | routes and payload kinds |
| [Patches](../spec/patches.md) | targets, filters, priorities, and merge rules |
| [Conditions](../spec/conditions.md) | env, path, existence, and changed tests |
| [Hooks](../spec/hooks.md) | commands, guards, timeouts, and logs |
| [Lua surface](../spec/lua.md) | builtin modules, handles, facts, and validation |
| [Plugins](../spec/plugins.md) | the shipped plugin tree and its helpers |
| [State](../spec/state.md) | the manifest, history, named states, and pool |
| [Difference](../spec/difference.md) | the comparison against state and disk |
| [Apply](../spec/apply.md) | the write sequence and its prompts |
| [Restore](../spec/restore.md) | bundles and stored states |
| [Output](../spec/output.md) | the terminal lines and the log file |
| [Init](../spec/init.md) | the scaffold and the refresh targets |
