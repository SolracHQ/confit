# State

Every successful `apply` and `restore` stores a manifest of the documents that
landed. The state answers the next difference check, supplies history picks and
named states, stores the content pool, and backs bundles. Two folders carry the
data:

```text
{config}  the operating system config folder and confit
{temp}    the operating system temp folder and confit
```

## Layout

| Path | Contents |
| --- | --- |
| `{config}/history/{stamp}.json` | One rotated manifest |
| `{config}/states/{name}.json` | One named manifest |
| `{config}/blobs/{sha}` | One gzipped content body |
| `{temp}/cache/{sha}` | One fetched or extracted body |
| `{temp}/cache/{hex}.url` | The URL index of one fetch |
| `{temp}/cache/{hex}.map` | The member index of one archive |

`{name}` is a state name. `{stamp}` is a clock reading in nanoseconds. `{sha}`
is the SHA-256 hex of the uncompressed bytes. `{hex}` is the hash of the key:
the URL for a fetch index, the archive hash for a member index.

The `{config}` folder is durable state. The `{temp}` folder is a cache
that a later run rebuilds. The pool and the manifest stay together, so a named
state resolves while its bytes remain.

## Manifest

A manifest is one JSON object:

```json
{
  "version": [0, 9, 0],
  "documents": [
    {
      "path": { "base": "home", "relative": ".bashrc" },
      "profile": [{ "env": { "name": "EDITOR", "value": "nvim", "when": null } }],
      "config": [],
      "final": []
    }
  ],
  "hooks": []
}
```

| Field | Content |
| --- | --- |
| `version` | The binary triplet |
| `documents` | Every document, in path order |
| `hooks` | Every hook, in profile order |

A reference serializes as its hex hash. An opaque document carries the
hash of the bytes, and a tree member carries the hash of its member body.
A tree document hashes its member list: mode, relative path, and member
hash per line.

`apply` fills the pool with every opaque and tree member body before the
preview. A stored manifest therefore resolves its handles only from the pool.

## History

- Each apply and each restore rotates the fresh manifest into
  `history/{stamp}.json` as the newest entry.
- The stamp is the current clock in nanoseconds. An existing name gains one
  until it is free.
- The folder keeps the current manifest and five previous ones. `%1` through
  `%5` name the previous ones, newest first. The oldest file drops when a
  seventh lands.
- `%N` picks a previous entry, newest first. `%1` is the previous run. `%0`
  fails as an out-of-range state. A number past the oldest entry fails.
- A named state never enters the history.

The hint `confit restore %1` undoes the run. It prints only when a previous
entry exists.

## Named states

`bundle PROJECT --profile FILE -o @name` writes the built manifest to
`states/{name}.json`. The file uses the manifest shape from this page. The run
writes no bundle file and stores no content bytes.

- The output must start with `@`. A bare name is a file output.
- An empty name fails. A name with `/`, `\`, or a component of `.` or `..`
  fails.
- A later write with the same name replaces the manifest.
- `restore @name` reads the manifest and writes the documents.
- A handle resolves from the cache, then from the pool. A hash with no stored
  bytes fails the restore.

The pool holds the opaque and tree member blobs. The cache holds the
fetched and extracted blobs. The named manifest holds the hashes that point
at them. A blob with no bytes on disk fails the restore. Each apply and
delete prunes the blobs that no stored manifest references, so manual
deletion only breaks restores.

## Rescue state

Before the writes, the run saves every destination that exists on disk and
that no manifest records into `states/!.json`. The capture mirrors the
desired shapes: single files land as opaque documents, tree members land
inside a tree at the same route. A later capture replaces the file, so the
state holds one rescue. `restore @!` writes it back, `export @!` packs it,
and `delete @!` drops it. Files over 16 MiB stay out with one warning
line.

## Pool

- `blobs/{sha}` is one body, compressed with gzip at level 6.
- The file name is the SHA-256 hex of the uncompressed bytes.
- A read decompresses through a hash check. A tampered entry fails.
- `apply` writes the bytes behind every opaque handle and every tree member into
  the pool before the preview.
- After a state write, the run prunes every blob that no applied, history,
  named, or rescue manifest references.
- `delete @name` removes the named manifest, then prunes.
- A blob stays while any stored manifest references its hash.

## Cache

The fetch cache and the archive cache share `{temp}/cache`.

- A fetch stores the body under its content hash and writes `{sha256(url)}.url`
  with that hash. A later fetch of the URL reads the index and serves the body
  with no request.
- A missing index, a corrupt index, or an absent body makes the next fetch
  download again.
- The first archive list extracts every member into the cache and writes
  `{archive sha}.map`. The index maps each member name to its content hash and
  mode.
- A later archive read uses the index. An absent member body extracts again.
- A cache body lands read-only with mode 444.

The cache has no manifest and no durable state. Remove it and the next run
downloads or extracts again. A restore that needs a pooled body promotes it back
into the cache.

## Bundles

A bundle is one gzipped tar file with the `.cb` extension.

```text
manifest.json          the manifest, first
{sha}                  one raw content body per distinct hash
```

- `manifest.json` is the first member. It is the manifest JSON.
- Content members follow. Each name is the bare hex hash of its body.
- Content members are sorted by hash name.
- A content member is the raw body bytes with mode 644.
- One member exists per distinct hash, so a repeated handle adds no member.
- A document without a handle adds no content member.

`bundle -o PATH` writes the file. A path without `.cb` gains the extension.
`-o @name` writes a named state instead, as described above.

`restore` reads a bundle, checks the manifest, and lands every content member in
the cache under its name hash. A body that does not match its name fails the
read. A missing `manifest.json`, a repeated `manifest.json`, a foreign member
name, or an absent content entry fails the read.

`export` packs a stored manifest into a bundle, or prints the manifest with
`-m`. See [CLI](cli.md).

## Version

- `version` is the binary triplet `[major, minor, fix]`. Version 0.9.0 stores
  `[0, 9, 0]`.
- The field is required. A missing field fails as a manifest without a version.
- The value must be three numbers. Another value fails and shows the raw value.
- Before 1.0, the major and the minor must match. A different fix loads.
- From 1.0, the major must match. A different minor and fix load.
- A different major fails.

The rule covers history entries, named states, and bundle
manifests. A manifest that fails the rule stops the run before any document
decodes.
