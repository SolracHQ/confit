# ConfIt architecture

Three crates form the app, each holding one main
responsibility.

The engine converts a Lua profile into manifest documents
plus blob bytes. The core manages state plus diffs: bundles
from manifests, manifest store plus load, bundle writes,
orphan removal, rotation.
The cli orchestrates both, calling engine plus core where
needed, and provides user experience: prompts, progress,
previews, listings, scaffolding, arg shapes.

The call flow is `main` to `actions`, with `main` rendering
reports through `presentation`. Effects hide behind traits
(`Filesystem`, `Fetch`, progress sink). `main` plus `cli`
name nothing effectful. Test seams keep every crate hermetic:
memory filesystem, memory fetcher, silent progress.

## Dependencies

```text
confit-cli --> confit-engine --> confit-core
confit-cli --> confit-core
```

These three edges are the whole graph.

## confit-core

Pure data plus render. Every function here runs as a unit test on data alone.

- `document` owns `ManifestDocument` plus `ManifestData`:
  structured, text, link, rc, opaque, tree. One path holds
  one document. Paths expand tildes. Opaque plus tree
  payloads persist as blob refs in manifests, raw bytes
  in the bundle blob map.
- `ids` owns `DocPath` plus `ReadOutcome` (present, absent,
  unreadable).
- `plan` owns versioned `Bundle` (`manifest` plus `blobs`)
  plus on-demand counts against previous bundles.
  The state slot holds the applied manifest.
- `drift` owns `Drift` entries comparing recorded manifests
  against disk, plus their display lines.
- `render` owns shell-agnostic document bytes plus text.
- `error` owns the plan-or-io failure shape.

## confit-engine

Lua profiles evaluate into manifest documents plus blobs
through `evaluate(profile, EvalOpts)`. `EvalOpts` carries root, plugins, re-fetch, cache override,
fetcher override, plus the progress sink. Overrides keep
tests off the network plus the OS cache.

- `surface` owns one namespace module each: config,
  document, patch, shell, paths, resources, utils,
  plugin. Resources jail reads to the project root plus the
  fetch cache. `require` jails module loads the same way.
- `model` owns Config plus Patch handles, internal to the
  crate. `level` owns priority sorting. `exec` owns the live
  wrappers: first-writer-wins slots, collision logging,
  per-shell materialization.
- `fetch` owns the `Fetch` trait with HTTP plus memory
  sources. Sidecar shas guard the OS cache. Re-download
  fires on missing files, mismatched bytes, or re-fetch.
- `progress` owns facts for slow runs: fetch, unpack, patch,
  hash, read, write.
- Embedded plugins ship beside the loader under
  `solrachq` (mise, merge, template). External plugin
  folders attach beside them. Plugin Lua composes surface
  primitives only.

## confit-cli

Terminal surface over evaluation plus bundles.

- `main` owns command dispatch plus report printing.
- `cli` owns arg shapes for plan, apply, export, delete,
  init. Tildes expand across every path arg after parsing.
- `actions` owns the flows (see `docs/flows`): plan
  evaluates, diffs, and stores payloads; apply previews,
  prompts, writes per kind, removes recorded orphans, writes
  state, and rotates history; export packs slots; delete
  drops named slots; init scaffolds profiles plus stubs
  from embedded text.
- `fs` owns the `Filesystem` seam with OS plus memory
  backends. Memory fakes keep tests hermetic.
- `presentation` owns summaries, drift lines, plus report
  text. Summaries cover moving documents plus counts.
- Logging rides `fern` into one file per run. The global
  `--log-level` gates verbosity, warn by default.

## Data flow

The profile evaluates to manifest documents plus blobs. The build diffs desired
documents against the previous manifest plus disk snapshots,
hashing rendered bytes. The payload writes as pretty JSON,
metadata always. Binary bytes gzip once into the shared
pool under content hashes. Apply writes documents per kind, removes
state-recorded paths absent from desired documents, records
the fixed state slot, and rotates manifest history. Apply
re-applies past slots through the same write path.
