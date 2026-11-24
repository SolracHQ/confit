# State

Every successful `apply` and `restore` stores one manifest. The manifest answers
the next difference check. It supplies history picks and named states, stores
the content pool, and backs bundles. Durable state lives under the OS config
folder.

```text
{config}  the operating system config folder and confit
```

## Layout

| Path | Contents |
| --- | --- |
| `{config}/history/{stamp}.json` | one rotated manifest |
| `{config}/states/{name}.json` | one named manifest |
| `{config}/states/!.json` | one rescue capture |
| `{config}/blobs/{sha}` | one gzipped body |

- `{name}` is a state name.
- `{stamp}` is a clock reading in nanoseconds.
- `{sha}` is the SHA-256 hex of the uncompressed content.

The `states` folder holds every `@name`. The config folder is durable state.
The pool and the manifest stay together. A named state resolves while its
content remains.

## Manifest

A manifest is one JSON object. It holds `version` and `documents` and `hooks`.
`version` is the binary triplet. Version 0.9.0 stores `[0, 9, 0]`. `documents`
holds every document in path order. `hooks` holds every hook in profile order.

A document holds `path` with `base` and `relative`, and holds `unmanaged`. A
text document holds `content` and `mode`. An opaque document carries the hash
of the content and holds `mode`. A tree member carries the hash of its member
body and holds `mode`. A tree document hashes its member list. Each member
line holds mode and relative path and member hash. `apply` fills the pool
with every opaque body and every tree member body before the preview. A stored
manifest therefore resolves its handles only from the pool.

```json
{
  "version": [0, 9, 0],
  "documents": [
    {
      "path": { "base": "home", "relative": ".bashrc" },
      "unmanaged": false,
      "content": "export EDITOR=hx\n",
      "mode": 420
    }
  ],
  "hooks": []
}
```

The example shows one text document. An opaque document replaces `content`
with a `handle`. The handle is the hex hash of the body.

## History

Each apply and each restore rotates the fresh manifest into history. The
fresh file is `history/{stamp}.json` as the newest entry. The stamp is the
current clock in nanoseconds, and an existing name gains one until it is free.

The folder keeps six files: the current manifest and five previous ones. `%1`
through `%5` name the previous ones, newest first. `%1` is the previous
command. The oldest file drops when a seventh lands. The history keeps only
apply and restore manifests. The hint `confit restore %1` undoes the command.
It prints only when a previous entry exists.

## Named states

`bundle PROJECT --profile FILE -o @name` builds one named manifest. The file
is `states/{name}.json` and uses the manifest shape from this page. The
command produces one named manifest and doesn't produce a bundle file. The
command persists the referenced content into the pool first, then stores the
manifest. A later write with the same name replaces the manifest.

`restore @name` reads the manifest and writes the documents. A handle resolves
from the pool. A hash that doesn't have stored content fails the restore. The
named round trip keeps the manifest and its content. The pool holds the opaque
blobs and holds the tree member blobs. Each apply and each delete prunes
unreferenced blobs. A blob stays while any stored manifest references
its hash.

## Rescue state

Before the writes, the command captures destinations that a stored manifest
doesn't record. The capture lands in `states/!.json` and mirrors the desired
shapes. Single files land as opaque documents. Tree members land inside a tree
at the same route. A later capture replaces the file. The state holds one
rescue.

| Command | Effect |
| --- | --- |
| `restore @!` | writes the capture back |
| `export @!` | packs the capture |
| `delete @!` | drops the capture |

The rescue round trip keeps the before content in the pool. Saving the rescue
prints a packing line: `packing @! ({n} members)`.

The closing hint blocks carry the restore commands:

| Hint | Command |
| --- | --- |
| `To restore previous exact disk state:` | `restore '@!'` |
| `To restore previous state:` | `restore %1` |
| `To restore previous state (dropping disk changes):` | `restore %1` |

## Pool

`blobs/{sha}` is one body, compressed with gzip at level 6. The file name is
the SHA-256 hex of the uncompressed content. A read decompresses through a
hash check, and a tampered entry fails.

`apply` writes the content of every opaque handle and every tree member before
the preview. The pool fill prints `compressing pool entries ({done} of {total}
files)`. After a state write, the command prunes unreferenced blobs across
applied, history, named, and rescue manifests, and prints `pruning pool
entries ({done} of {total} files)`.

`delete @name` removes the named manifest then prunes, and stays silent on
success. A blob stays while any stored manifest references its hash.

## Version

`version` is the binary triplet `[major, minor, fix]`. Version 0.9.0 stores
`[0, 9, 0]`. The field is required.

| Manifest | Rule |
| --- | --- |
| version before 1.0 | the major and the minor must match; a different fix loads |
| version from 1.0 | the major must match; a different minor and fix load |
| a different major | fails |
| a missing field | fails as a manifest without a version |
| a non-triplet value | fails and shows the raw value |

The rule covers history entries, named states, and bundle manifests. A manifest
that fails the rule stops the command before any document decodes.
