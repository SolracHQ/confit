# ConfIt technical reference

```text
Spec-Version: 0.7.0
```

Spec-Version 0.7.0 tracks the workspace crates at version 0.7.0.
`confit --version` reports the same version through the clap
`version` attribute on the CLI parser.
This page stays a living document on the main branch.
Sealed releases stay readable under git tags through one recipe:

```sh
just show-spec VERSION=0.7
```

This reference addresses developers plus advanced users.
End users learn ConfIt from the manual part of this book.

## Scope

This reference records exact behavior contracts for the tool.
Design intent for each release lives at `docs/design/` in repo
root form.
Usage teaching lives in the manual part of this book.

## Conventions

`{name}` marks a variable value in a path or command shape.
A leading `~` marks a path under the OS home folder.
Version strings name exact contract versions, such as bundle
format version 7.
Fenced blocks hold literal text: commands, paths, plus messages.
The verb reads introduces exact output text from the tool.

## System overview

The Lua framework evaluates a profile into documents plus hooks.
Plan previews the evaluation into a bundle.
Apply writes the bundle through the fixed slot.

| Slot | Path under the OS config folder |
| ---- | ------------------------------- |
| Live state | `confit/state.json` |
| History | `confit/previous/{stamp}.json` |
| Named slots | `confit/plans/{name}.json` |
| Blob pool | `confit/blobs/{hash}` |

Same profile plus same cache yields the same bundle.

## Map

- cli: The cli page records flags, sources, plus slot pickers
  for each subcommand.
- plan: The plan page records profile evaluation, diffing, plus
  bundle output.
- drift: The drift page records state-versus-disk notes plus
  first-run behavior.
- apply: The apply page records guarded writes plus hook runs.
- storage: The storage page records bundles, slots, plus the blob
  pool.
- documents: The documents page records documents, patches, trees,
  plus hashing.
- lua: The lua page records Lua primitives plus resources.
- output: The output page records the summary, color, plus
  streams.
- init: The init page records profile plus stub scaffolding in a
  target folder.
