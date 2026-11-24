# Patches

A patch reads a working document or hook, mutates the table, and returns
nothing. The engine merges the result over the assembled base. A patch declares
one destination and one callback.

```lua
{
   target = route("config", "mise/config.toml"),
   format = "toml",
   apply = function(document)
      document.data.tools.bat = "0.24.0"
   end,
}
```

The callback edits fields directly. A returned value fails the plan. The
working table is the whole write surface.

A patch table takes one of three shapes.

| Shape | Marker | Callback argument |
| --- | --- | --- |
| Structured | `format` | `{ path, format, data }` |
| Rc | route with default `format` | `{ path, profile, config, final }` |
| Hook | a string `target` | the working hook table |

## Destination

A patch declares exactly one of `target` and `filter`. A table with both fields
fails. A table with neither fails.

`target` is one destination. A route points at a document. A string points at a
hook id. The string target makes a hook patch.

| Target | With `format` | With default `format` |
| --- | --- | --- |
| route | structured patch | rc patch |
| string | fails | hook patch |

`filter` takes a function over a route and returns a boolean.

```lua
{
   filter = function(route)
      return route.base == "home" and route.relative:match("%.sh$") ~= nil
   end,
   apply = function(document)
      table.insert(document.config, {
         alias = { name = "cat", expansion = "bat" },
      })
   end,
}
```

A structured filter sees only structured documents. An rc filter sees only rc
documents. A false result skips the document. A filter with zero matches keeps
the document set intact. A non-boolean result fails the plan.

A target must be a document of the patch kind or a hook id. A missing target
fails the plan with a nearby-name suggestion. A target of another kind fails the
plan.

A structured patch must take the same format as its base. A different format
fails the plan.

## Priority

Structured and rc patches accept a `priority` field. Confit sorts higher levels
first.

| Level | Rank |
| --- | --- |
| `minor` | 0 |
| `low` | 1 |
| `normal` | 2 |
| `high` | 3 |
| `major` | 4 |

The level reads in any letter case. The default is `normal`. An unknown name
fails the plan.

Declaration order breaks a tie and is profile config order, then patch order
inside one config.

Hook patches use declaration order as the sort key.

## Structured patches

A structured patch receives the working document with its format and data.

```lua
{
   target = route("config", "starship.toml"),
   format = "toml",
   apply = function(document)
      document.data.add_newline = false
      document.data.palette = "catppuccin"
   end,
}
```

The callback writes through `document.data`. It keeps `document.path` and
`document.format` fixed. Either change fails the plan.

The base owns every leaf. A patch inserts a new leaf and owns it. A later write
of a leaf that another writer owns is dropped, and one collision line records
the loss.

```text
collision on config('starship.toml') "add_newline": "theme" loses, "base" wins
```

These rules decide the merge:

- The first writer owns a leaf. The base is the first writer.
- Lists are leaves. A write of a different list loses whole.
- A table over a scalar leaf loses. The scalar stays.
- An equal value leaves the document unchanged.
- A key that the patch omits stays in the document.
- A base leaf persists through patch writes. A removal is logged and the
leaf stays.

A patch preserves base leaves when keys leave the returned table. The engine
folds new values over the working table and preserves every working leaf.

## Rc patches

An rc patch receives the working rc document with its three sections.

```lua
{
   target = route("home", ".bashrc"),
   apply = function(document)
      table.insert(document.config, {
         alias = { name = "cat", expansion = "bat" },
      })
      table.insert(document.final, {
         eval = { argv = { "zoxide", "init", "bash" } },
      })
   end,
}
```

The callback reads and mutates `document.profile`, `document.config`, and
`document.final`. It keeps `document.path` fixed.

A named entry collides on its name within one section. The names come from
`env`, `path`, and `alias`. An entry named `PATH` in `config` collides with any
other `config` entry named `PATH`, whatever its kind. Collision checks stay
within one section.

The first writer owns a named entry. A later entry with the same name and a
different body is dropped, and one collision line records the loss.

```text
collision on home('.bashrc') config "ll": "theme" loses, "base" wins
```

Name collision checks cover `env`, `path`, and `alias` entries. `eval`,
`cmd`, and `source` accumulate in the returned order. An entry equal to
a working entry replaces that entry instead of adding a duplicate.

Entries keep the order of the returned section. A patch that leaves a base entry
untouched keeps it in place.

## Hook patches

A hook patch takes a hook id as a plain string. The callback receives the
working hook table.

```lua
{
   target = "plugin:_/mise:install",
   apply = function(hook)
      hook.timeout = "20m"
      hook.check = {
         all = {
            hook.check,
            { exists = { route = route("data", "mise/shims/bat") } },
         },
      }
   end,
}
```

The callback mutates fields such as `argv`, `path`, `requires`, `when`, `check`,
and `timeout`. The result is decoded again. A bad shape fails the plan. A target
with no matching hook id fails the plan with a suggestion.

Hooks keep separate identity. One shared hook is one hook that other configs
patch. See [hooks](hooks.md#identity).

## Order

The engine splits patches by kind and groups them by destination.

1. Confit handles every structured destination in first-seen order.
2. Confit handles every rc destination in first-seen order.
3. The assembled documents join the unpatched documents and sort by
route.

Within one destination, patches sort by priority with the highest first. Equal
priorities keep declaration order. First-seen order follows the profile config
order and the patch order inside each config.

Hook patches visit the collected hooks in declaration order. This happens after
the profile builds the hook list.
