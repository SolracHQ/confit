# ConfIt architecture

Four crates form the app, each holding one main
responsibility.

The engine converts a Lua profile into manifest documents
and blob bytes. The store holds write-backed capability
roots for one run. The core manages state and diffs across bundles, manifests, 
the pool, and rotation.
The cli orchestrates both, calling engine and core where
needed, and provides user experience across prompts, progress,
previews, listings, scaffolding, and arg shapes.

The call flow is `main` to `actions`, with `main` rendering
reports through `presentation`. Effects hide behind traits
(`Filesystem`, `Fetch`, progress sink). `main` and `cli`
name nothing effectful. Test seams keep every crate hermetic:
memory filesystem, memory fetcher, silent progress.

## Dependencies

```text
confit-cli --> confit-engine --> confit-store --> confit-core
confit-cli --> confit-store
confit-cli --> confit-core
```

These five edges are the whole graph.

## confit-core

Pure data and render. Every function here runs as a unit test on data alone.

- `document` owns `ManifestDocument` and `ManifestData`:
  structured, text, link, rc, opaque, tree. One path holds
  one document. Paths expand tildes. Opaque and tree
  payloads persist as blob refs in manifests, raw bytes
  in the bundle blob map.
- `ids` owns `DocPath` and `ReadOutcome` (present, absent, 
  unreadable).
- `plan` owns versioned `Bundle` (`manifest`, `blobs`),
  and on-demand counts against previous bundles.
  The state slot holds the applied manifest.
- `drift` owns `Drift` entries comparing recorded manifests
  against disk, and their display lines.
- `render` owns shell-agnostic document bytes and text.
- `error` owns the plan-or-io failure shape.

## confit-engine

Lua profiles evaluate into manifest documents and blobs
through `evaluate(profile, EvalOpts)`. `EvalOpts` carries root, plugins, re-fetch, cache override,
fetcher override, plus the progress sink. Overrides keep
tests off the network and the OS cache.

- `surface` owns one namespace module per kind. Config,
  document, patch, shell, paths, resources, utils,
  plugin. Resources jail reads to the project root and the
  fetch cache. `require` jails module loads the same way.
- `model` owns Config and Patch handles, internal to the
  crate. `level` owns priority sorting. `exec` owns the live
  wrappers across first-writer-wins slots, collision logging,
  and per-shell materialization.
- `fetch` owns the `Fetch` trait with HTTP and memory
  sources. Sidecar shas guard the OS cache. Re-download
  fires on missing files, mismatched bytes, or re-fetch.
- `progress` owns slow-run facts across fetch, unpack, patch,
  hash, read, and write.
- Embedded plugins ship beside the loader under
  `solrachq` (mise, merge, template). External plugin
  folders attach beside them. Plugin Lua composes surface
  primitives only.

## confit-store

Write-backed capability roots for one run.

- `StoreRoots` owns the config base, the cache base, and
  the temp base. Roots arrive explicit at construction.
- `Stores` owns the roots for every capability. `Stores::host`
  serves CLI wiring, `Stores::memory` serves tests. Later
  sections add the capabilities behind these roots.

## confit-cli

Terminal surface over evaluation and bundles.

- `main` owns command dispatch and report printing.
- `cli` owns arg shapes for plan, apply, export, delete,
  init. Tildes expand across every path arg after parsing.
- `actions` owns the flows (see the plan, apply, and init
  spec pages). Plan
  evaluates, diffs, and stores payloads; apply previews,
  prompts, writes per kind, removes recorded orphans, writes
  state, and rotates history; export packs slots; delete
  drops named slots; init scaffolds profiles and stubs
  from embedded text.
- `fs` owns the `OsFs` host effect implementing the core `Filesystem` seam.
  Memory fakes live in core and keep tests hermetic.
- `presentation` owns summaries, drift lines, and report
  text. Summaries cover moving documents and counts.
- Logging rides `fern` into one file per run. The global
  `--log-level` gates verbosity, warn by default.

## Data flow

The profile evaluates to manifest documents and blobs. The build diffs desired
documents against the previous manifest and disk snapshots, 
hashing rendered bytes. The payload writes as pretty JSON,
metadata always. Binary bytes gzip once into the shared
pool under content hashes. Apply writes documents per kind, removes
state-recorded paths absent from desired documents, records
the fixed state slot, and rotates manifest history. Apply
re-applies past slots through the same write path.
