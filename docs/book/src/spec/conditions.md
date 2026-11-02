# Conditions

A condition is a table that answers one question about the host or the run.
Conditions guard rc entries and hook decisions.

One condition table carries exactly one key. A table with two keys fails.
`nop` wraps exactly one condition, and a list under `nop` fails.

| Place | Field | Evaluation |
| --- | --- | --- |
| Hook | `requires` | the engine, before the run |
| Hook | `when` | the engine, before the run |
| Hook | `check` | the engine, before and after the run |
| Rc entry | `when` | the shell, when it sources the file |

Documents and patches carry no conditions.

## Leaves

A leaf tests one fact.

| Leaf | Shape |
| --- | --- |
| `env_eq` | `{ env_eq = { key = "SHELL", value = "bash" } }` |
| `env_set` | `{ env_set = { key = "SSH_CONNECTION" } }` |
| `in_path` | `{ in_path = { name = "bat" } }` |
| `exists` | `{ exists = { route = route("home", ".secrets") } }` |
| `changed` | `{ changed = { route = route("config", "starship.toml") } }` |

`key`, `value`, and `name` are strings. `route` is a route.

- `env_eq` is true when the variable exists and its value is exactly the
  expected text. An unset variable fails.
- `env_set` is true when the variable exists and its value is non-empty. An
  unset or empty variable fails.
- `in_path` is true when a search folder contains an executable file with the
  given name. For a hook, the search visits the hook PATH entries, then the
  process PATH.
- `exists` is true when the route expands and the path exists. Any file type
  counts. A missing base folder is false.
- `changed` is true when the route is in the changed set. See
  [Changed](#changed).

The `exists` route may use the `literal` base to test an absolute host path.

## Combinators

```lua
{ all = { cond_a, cond_b } }
{ any = { cond_a, cond_b } }
{ nop = cond }
```

- `all` is true when every member is true. An empty `all` is true. The default
  condition is the empty `all`.
- `any` is true when one member is true. An empty `any` fails.
- `nop` is true when its member fails.

A nested list keeps its own operator. `all` inside `any` stays a conjunction,
and `any` inside `all` stays a disjunction.

## Changed

The changed set is the routes that the current apply changes.

On the first run, the set is every built document path. A `changed`
condition on a built route is therefore true on the first apply. Later runs
cover every built path that is not unchanged and every built path with a
difference against the recorded state. A tree member change maps to the tree
destination route.

A changed route must be a built document. A hook condition that takes
another route fails the plan.

`changed` rides only hooks. An rc guard with a `changed` leaf fails the
plan before rendering. The shell has no form for the leaf, so no rc file can
carry it.

## Shell rendering

An rc `when` renders as a Bourne test at the start of the guard.

| Condition | Shell text |
| --- | --- |
| `env_eq` | `[ "${KEY}" = value ]` |
| `env_set` | `[ -n "${KEY}" ]` |
| `in_path` | `command -v name >/dev/null 2>&1` |
| `exists` | `[ -e path ]` |
| empty `all` | `true` |
| empty `any` | `false` |

- `all` joins its members with ` && `.
- `any` joins its members with ` || `.
- A nested list whose operator differs from its parent wraps in `( ... )`.
- `nop` over a leaf renders `! test`.
- `nop` over a list renders `! ( ... )`.
- `exists` expands its route at render time.
- Values and paths use Bourne quoting. An unsafe byte brings single quotes,
  and an empty value renders `''`.

A guarded entry renders three lines. The test line ends with `; then`, the
entry line takes two spaces, and `fi` closes the block.

```lua
{
   alias = { name = "ll", expansion = "ls -l" },
   when = {
      all = {
         { env_eq = { key = "SHELL", value = "bash" } },
         { in_path = { name = "starship" } },
      },
   },
}
```

```sh
if [ "${SHELL}" = bash ] && command -v starship >/dev/null 2>&1; then
  alias ll='ls -l'
fi
```

A `changed` leaf has no shell form. An rc entry that carries one fails the
plan, so the renderer never writes it.
