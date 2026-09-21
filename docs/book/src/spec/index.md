# ConfIt technical reference

```text
Spec-Version: 0.8.0
```

Spec-Version 0.8.0 tracks the workspace crates at version 0.8.0.
`confit --version` reports the same version through the clap
`version` attribute on the CLI parser.
This page stays a living document on the main branch.
Sealed releases stay readable under git tags through one recipe:

```sh
just show-spec VERSION=0.7
```

This reference addresses developers and advanced users.
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
Fenced blocks hold literal text: commands, paths, and messages.
The verb reads introduces exact output text from the tool.

## System overview

The Lua framework evaluates a profile into documents and hooks.
Plan previews the evaluation into a bundle.
Apply writes the bundle through the fixed slot.

| Slot | Path under the OS config folder |
| ---- | ------------------------------- |
| Live state | `confit/state.json` |
| History | `confit/previous/{stamp}.json` |
| Named slots | `confit/plans/{name}.json` |
| Blob pool | `confit/blobs/{hash}` |

Same profile and same cache yields the same bundle.

## Map

- cli: The cli page records flags, sources, and slot pickers
  for each subcommand.
- plan: The plan page records profile evaluation, diffing, and
  bundle output.
- drift: The drift page records state-versus-disk notes and
  first-run behavior.
- apply: The apply page records guarded writes and hook runs.
- storage: The storage page records bundles, slots, and the blob
  pool.
- documents: The documents page records documents, patches, trees,
  and hashing.
- lua: The lua page records Lua primitives and resources.
- output: The output page records the summary, color, and
  streams.
- init: The init page records profile and stub scaffolding in a
  target folder.
