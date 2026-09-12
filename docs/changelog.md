# Changelog

## [Unreleased]

## [0.2] - 2026-09-12

### Added

- `render` module: pure data-to-bytes rendering for every artifact kind.
  Rc files render env, aliases, and init blocks in per-tool
  `# >>> confit:<tool>` sections; tables serialize fixed-order;
  templates render via minijinja with root-relative `src` reads;
  rendered bytes hash with SHA-256.
- `confit.resources` (`load_toml`/`load_json`/`load_yaml` root-relative
  reads, `merge` with `shallow`/`list_append` opts) and
  `confit.artifact` constructors plus `tool:append_artifact`, folded
  into the plan by `(kind, path)`; proven by
  `examples/1-structured_resource` (structured loads plus merge) and
  `examples/2-templated_resource` (minijinja template plus variables).
- `confit.path` lib (`home`, `config`, `data`, `confroot`): joins over
  the OS folders plus the project root for `confroot`.
- Filesystem-aware `plan`/`status`: each artifact path snapshots behind
  the `SnapshotStore` trait; absent paths read as absence, unreadable paths
  warn on stderr naming path and reason, exit stays 0.
- Three-way diff (desired vs previous vs disk): create, update,
  unchanged, delete counts plus `OverwriteUntracked`,
  `ManualModification`, `Unreadable` warnings.
- Disk diffs: key-value lines for structured kinds, unified diffs for
  scripts and raw bytes, wrapped in the drift note with
  `~ artifact` stanzas and a `Plan: N to add, M to change, D to destroy.`
  final line.
- A mise install contributes `eval "$(mise activate <shell>)"` as the
  first init entry of every shell.
- `tool:init({ source = "path" })` third init shape, rendering
  `source path` next to `eval` and `cmd` entries.
- New direct dependencies: `anstream` (color honoring `NO_COLOR` plus
  terminal detection; pipes read plain text), `diffy` (unified hunks for
  byte kinds behind headers plus line vocabulary), `directories` (home
  folder expansion plus OS folders for the path lib), `noyalib`
  (YAML resource loads plus YAML rendering).

### Changed

- State and plan IO sit behind repository seams, so tests run on
  memory fakes and no test touches real config or state files.
- Rendering, merging, digests, plan building, and both diffs gather in
  services, so a wrong plan output traces to one layer.
- The Lua side splits into a framework holding the `resources`,
  `artifact`, and `path` namespaces plus a read-only evaluator, so the
  stdlib replacement and profile loading change independently. One
  `install_confit` call wires every namespace; the evaluator keeps no
  install sequence.
- Plan assembly moves into actions with one fold per contribution
  kind. `main` parses, binds the live filesystem backend, and prints
  alone.
- Shared shapes centralize in a model split between state entities and
  transfer shapes, with one plan-version constant replacing the magic
  number.
- The CLI holds argument shapes alone. Every user-facing string moves
  to presentation, which also owns terminal color that stays off on
  pipes.
- One shared hashing utility in `security` serves snapshots, digests,
  and plan ids, replacing the scattered copies.
- Modules take the `name.rs` plus `name/` children form with zero
  `mod.rs` files.
- YAML backend moves from deprecated `serde_yml` to `noyalib`.
  Structured renderers emit each serializer native output with no
  trailing-newline normalization; output stays deterministic per
  serializer.
- Repository collapses to a single `Filesystem` seam, services own
  expansion plus parsing plus hashing plus shaping.
- Root escapes fail closed even while the target stays absent;
  missing escape targets previously read inline silently.

### Removed

- `Plan.hooks`: the old implementation was half-broken and useless
  before `apply` replaces files, so a formal design waits until that
  part works. Plan still has pending work of its own, alias guards and
  install checks among it, and hooks are not needed in the near
  horizon.
- `--format` plus the TOML plan export: pretty JSON already covers
  readability, and the export failed on legal plan data holding nulls.
  Plans serialize as JSON alone.

## [0.1.0] - 2026-09-09

First working version: `plan` and `status` over Lua tools.
Design spec: `docs/design/v0.1.md`.

### Added

- Lua 5.4 DSL (`mlua`, vendored): `confit.tool` handles (`:env`, `:alias`,
  `:init`, `:profile`, `:profile_path`), `confit.mise.package`.
- `plan`: Lua entrypoint to contributions to artifacts; merge key
  `(kind, path)`; canonical bytes + SHA; JSON pretty-printed to
  `-o`/`--output` (omitted prints to stdout); `--format toml` export.
  `created_at` metadata excluded from the hash. Summary goes to stderr.
- `status`: diff desired vs previous state in memory; summary to stderr.
- Structural-equality env shadowing with `_shadowed` losers (excluded from
  hash and diff); rich summary lists every entry with its winner tool;
  `--conflicts` names losers (`starship wins over bat`); `--root` defaults
  to the profile file's parent.
- Docker test harness: Fedora-minimal `confit-test` container, `tester`
  user, `confit-test-home` volume, `:ro` target mount, `just test-*`
  recipes plus `plan-example` smoke.
- Lint policy: `unwrap`/`expect` denied in production code, allowed in
  tests and doctests only.
