# Hooks

A hook is a command that runs after the documents, the state, and the history
land. Three conditions decide the run. `requires` asks whether the host can
run the command. `when` asks whether the run is needed. `check` asks whether
the work is already done.

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
| `timeout` | duration string | run limit, default 600 seconds |

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

A hook patch takes the id text as a plain string and receives the
working hook table. See [patches](patches.md#hook-patches).

Hooks do not merge by identity. Two configs that need a shared command declare
one hook and patch it. A second declaration with the same id fails.

The id is also the hook log file. See [Run](#run).

## Decision order

The engine decides each hook in order.

1. `requires` passes? A failure aborts the apply and stops the remaining hooks.
2. `when` passes? A failure skips the hook and writes an info log line.
3. `check` passes? A pass skips the hook and writes an info log line. The empty
   `check` is the exception: it marks a hook with nothing to confirm, so the
   hook runs.
4. The binary resolves? A missing binary fails the hook.
5. The command runs.
6. The exit code is zero? A nonzero code fails the hook.
7. `check` passes after the run? A failure fails the hook.

A hook with the default `when` is considered on every apply, even when no
document changed. Add a `when` condition to make the run depend on a route.

A `check` that fails before the run starts the command. The same `check` must
pass after a zero exit, or the hook fails. A hook that repairs its own subject
passes this way.

## Run

Hooks run after the documents, the state, and the history land. The order is
profile config order, then declaration order. A hook patch keeps the position
of the patched hook.

Each run prints one line to stderr.

```text
hook 2 of 5: mise install
```

The position counts every hook, including skipped hooks. The total counts
every hook in the profile.

The engine snapshots the host once per apply. The snapshot is the
environment variables, the PATH folders, and the changed set. `exists` and
`in_path` then read the filesystem as they run.

`path` entries come first in PATH for three uses: the hook conditions, the
binary lookup, and the subprocess. The rest of the run keeps the process PATH.

The first `argv` item is the binary. The engine searches the hook PATH
folders, then the process PATH, for that name. The remaining items pass as
arguments. A route item expands to its host path.

The command runs without a shell. Quotes, globs, pipes, and variable
expansion are literal text.

The process writes stdout and stderr to its own log file under the run hooks
folder. The tool output does not reach the terminal. The log name is
`{slug}-{hash}.log`. The slug comes from the id: runs of letters and digits,
lowercased and joined with hyphens, at most 40 characters long. The hash is a
short hash of the id.

The log opens with the same position line, then the streamed output. The
log path appears in the info log. See [output](output.md).

## Timeout

`timeout` takes a duration string.

```lua
timeout = "30s"
timeout = "10m"
timeout = "1h10m10s"
timeout = "600"
```

The units are `h`, `m`, and `s`, in that order. Each unit appears at most
once. A bare number has no unit and counts seconds. Any other order or a
repeated unit fails the plan.

The default is 600 seconds, the same as `10m`.

When the limit passes, the engine kills the process, waits for it, and fails
the hook. The log keeps the output written before the kill.

## Failures

| Failure | Result |
| --- | --- |
| `requires` unmet | the apply fails, later hooks stop |
| `when` unmet | the hook skips and logs |
| `check` passes before the run | the hook skips and logs |
| binary missing | the hook fails |
| spawn failure | the hook fails |
| nonzero exit | the hook fails |
| signal death | the hook fails with exit code 1 |
| timeout | the hook fails |
| `check` unmet after the run | the hook fails |

A failed hook stops the remaining hooks. The apply exits with failure. The
documents, the state, and the history already written stay in place.
