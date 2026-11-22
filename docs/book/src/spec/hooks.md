# Hooks

A hook is a command. The command starts after the documents, the state, and the
history land. Three conditions decide the start. `requires` asks whether the
host supports the command. `when` asks whether the command is needed. `check`
asks whether the work is already done.

```lua
{
   id = id("plugin:_/mise:install"),
   argv = { "mise", "install" },
   path = { route("home", ".local/bin") },
   requires = { in_path = { name = "mise" } },
   when = { changed = { route = route("config", "mise/config.toml") } },
   check = { exists = { route = route("data", "mise/shims/bat") } },
   timeout = "10m",
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `argv` | non-empty list | the command |
| `path` | list | PATH entries for this hook |
| `id` | `id(name)` | hook identity |
| `requires` | condition | capability, a failure aborts |
| `when` | condition | need, a failure skips |
| `check` | condition | completion, a pass skips |
| `timeout` | duration string | time limit, default 600 seconds |

## Shape

- `argv` is required and has at least one item. An empty list fails.
- An argv item is text or a route. A route expands to a host path.
- `path` is optional and defaults to empty. An entry is text or a route.
- `id` is required and takes `id(name)`. A bare string fails.
- `requires`, `when`, and `check` are conditions and default to the empty
`all`, which is true.
- `check` is singular. A `checks` field fails.
- `timeout` is a string. An integer fails.
- An unknown field fails.

Hooks collect in profile config order, then in declaration order inside one
config. See [profile](profile.md).

## Identity

A hook id is unique across the profile. A repeated id fails the plan and shows
every config that declares it. The id is also the handle for hook patches.

A hook patch takes the id text as a plain string and receives the working hook
table. See [patches](patches.md#hook-patches).

Hooks keep separate identity. Two configs that need a shared command declare
one hook and patch it. A second declaration with the same id fails.

The id is also the hook log file name. See [Start](#start).

## Decision order

The engine decides each hook in order.

1. `requires` passes? A failure aborts the apply and stops the remaining hooks.
2. `when` passes? A failure skips the hook and writes an info log line.
3. `check` passes? A pass skips the hook and writes an info log line. The
empty `check` is the exception. It marks a hook with confirmation treated as
complete. Confit
starts the hook.
4. The binary resolves? A missing binary fails the hook.
5. The command starts.
6. The exit code is zero? A nonzero code fails the hook.
7. `check` passes after the command? A failure fails the hook.

A hook with the default `when` is considered on every apply. This holds
across applies with identical document state. Add a `when` condition to tie
the start to a route.

Confit starts the command after a failed `check` before the start. The same
`check` must pass after a zero exit, or the hook fails. A hook that repairs its
own subject passes this way.

## Start

The command starts hooks after the documents, the state, and the history land.
The order is profile config order, then declaration order. A hook patch keeps
the position of the patched hook.

A hook that starts prints one line to stderr.

```text
> /usr/bin/true
```

The line shows the resolved binary with a `>` prefix, then the arguments. The
position counts every hook, including skipped hooks. The total counts every hook
in the profile. You see this line on the terminal. The `hook {position} of
{total}: {argv}` line heads the per-hook log file instead.

The engine snapshots the host once per apply. The snapshot is the environment
variables, the PATH folders, and the changed set. `exists` and `in_path` then
read the filesystem at that point.

`path` entries come first in PATH for three uses: the hook conditions, the
binary lookup, and the subprocess. The command otherwise keeps the process PATH.

The first `argv` item is the binary. The engine searches the hook PATH folders,
then the process PATH, for that name. The remaining items pass as arguments. A
route item expands to its host path.

The command starts through direct invocation. Quotes, globs, pipes, and
variable expansion are literal text.

The process writes stdout and stderr to its own log file under the `hooks`
folder in the logs folder. The tool output stays in the log file. The log
name is `{slug}-{hash}.log`. The slug comes from the id. It uses groups of
letters and digits. Groups are lowercased and joined with hyphens. The slug is
at most 40 characters long. The hash is a short hash of the id.

The log opens with `hook {position} of {total}: {argv}`, then the streamed
output. The log path appears in the info log. See [output](output.md).

## Timeout

`timeout` takes a duration string.

```lua
timeout = "30s"
timeout = "10m"
timeout = "1h10m10s"
timeout = "600"
```

The units are `h`, `m`, and `s`, in that order. Each unit appears at most once.
A bare number counts seconds by default. Any other order or a repeated unit
fails the plan.

The default is 600 seconds, the same as `10m`.

When the limit passes, the engine kills the process, waits for it, and fails the
hook. The log keeps the output written before the kill.

## Failures

| Failure | Result |
| --- | --- |
| `requires` unmet | the apply fails, later hooks stop |
| `when` unmet | the hook skips and logs |
| `check` passes before the start | the hook skips and logs |
| binary missing | the hook fails |
| spawn failure | the hook fails |
| nonzero exit | the hook fails |
| signal death | the hook fails with exit code 1 |
| timeout | the hook fails |
| `check` unmet after the command | the hook fails |

A blocked hook exits 1 with:

```text
Error: Cannot run the hook 'true': hook blocked
```

A missing binary exits 1 with:

```text
Error: Cannot run the hook 'nosuchbin123 x', invalid command
```

A nonzero exit exits 1 with:

```text
Error: Cannot run the hook 'false': hook failed with code 1
```

A successful hook shows in Results as a table. A failed or blocked hook switches
Results to compact `[...]` lines.

A failed hook stops the remaining hooks. The apply exits with failure. The
documents, the state, and the history already written stay in place.
