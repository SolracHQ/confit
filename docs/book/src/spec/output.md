# Output

The summary renders titled sections in fixed order. Drift
notes lead. Resources follow. Hooks trail resources. Summary
plus counts close the text. Sections holding zero lines skip
their title. Titles read as follows:

```
Changes outside Confit will be overwritten on next apply
Resources
Hooks
Summary
```

## Drift section

Steady drift groups hunk content under one header per path
with key lines inline beside it:

```
Changes outside Confit will be overwritten on next apply
~ note: text
  -old line
  +new line
~ app.toml: tools.bat = recorded -> disk
```

Headers carry the update sigil. Hunk lines carry per symbol
paint through shared hunk rendering. Key lines reuse drift
grammar from the drift page.

## Resources section

Create blocks open with `+` plus list full bodies under the
add sigil. Bodies vary by kind as follows:

```
+ note: text
  + hello
+ app.toml: toml
  + tools.bat = latest
+ shortcut: link
  + /opt/target
+ blob.bin: opaque
  + opaque (2 bytes)
+ fonts: tree
  + tree (2 files)
+ ~/.bashrc: rc
  + alias cat = bat
  + init[1] = eval "$(mise activate bash)"
```

Update blocks open with `~` plus render per kind shapes from
the drift page. Delete blocks close with `-` plus carry zero
bodies:

```
- gone: text
- fonts: tree (1 files)
```

## Hooks section

Plan hooks render as lifecycle data. Added hooks print `+`
with full gates. Modified hooks print `~` with gate diffs.
Removed hooks print `-` with zero detail. Unchanged hooks
render zero lines:

```
+ mise install
  + requires (in_path(mise))
~ fc-cache -f
  ~ when (before) -> (after)
- old hook
```

Detail lines nest under two spaces, so top level sigils
alone drive hook counts.

## Sigils plus color

`+` marks additions with green paint. `~` marks changes with
yellow paint. `-` marks removals with red paint. Headers plus
counts carry bold paint. Paint applies while stderr runs as a
terminal with `NO_COLOR` unset. The check runs once per
summary. Piped output stays plain text.

## Stdout against stderr

Result lines stay pipeable on stdout. Transient plus
diagnostic lines ride stderr. Routing reads as follows:

| Line                     | Stream |
| ------------------------ | ------ |
| Summary text plus counts | stdout |
| Spinner plus progress    | stderr |
| Prompts                  | stderr |
| `log: {path}`            | stderr |

Prompts park widgets plus write to stderr, so stdout keeps
carrying pipeable lines through answers.

## Log line

`log: {path}` prints to stderr on every plan plus apply run
through success plus failure paths alike. The default path reads
`{temp}/confit-{pid}.log`. The log file flag overrides the
path with tilde expansion. Collision warnings plus debug
facts land in that file through the log channel.

## Collision lines

Colliding slots keep the first writer with later writers
yielding. Each yield logs one warning line in this shape:

```
collision on {label} "{name}": "{owner}" overwritten, "{winner}" wins
```

Rc plus structured collisions share the shape with kind
fitting labels. Winners surface in the log file alone with
zero summary lines spent on losers.

## Counts grammar

The documents line always renders under the Summary title.
The hooks line joins while hooks move:

```
Documents: {adds} to add, {changes} to change, {deletes} to destroy.
Hooks: {added} to add, {changed} to change, {destroyed} to destroy.
```

Hook counts read top level sigils alone with nested detail
skipping counts. First run counts derive from drift groups
with destroy pinned at zero.
