# Changelog

## [Unreleased]

### Changed

- `mise.init` without a version resolves the latest release
  from the releases feed. The tags feed heads with `vfox-*`
  registry tags holding no release assets, so latest
  resolution 404'd on a ghost version.
- `nerd_fonts.font` lands each font under its own
  `fonts/{name}` folder with its own `fc-cache -f` hook
  scoped to that folder. The shared installer config plus
  `nerd_fonts.init` disappear with it.
- CI runs on pull requests alone, so tag pushes run only
  the CD workflow.
- `write_documents` uses `?` over a manual `Ok`/`Err` match
  for the rendered bytes. Newer clippy demands it.
- Test `pin_home` pins the XDG vars under the fake home.
  Runners exporting `XDG_CONFIG_HOME` outside HOME broke
  the fixed-slot assertion.

## [0.6.1] - 2026-09-17

### Added

- MIT license in `LICENSE`, copyright SolracHQ 2026.
- CI workflow running format, lints, plus tests on push and
  pull requests. CD workflow building the release binary and
  publishing it as `confit-linux-x64` on version tags after
  proving the tag matches every crate version.

### Changed

- Doc examples are real doctests. Every `text` fence holding
  Rust now reads `rust` and runs under `cargo test`. The pass
  fixed drifted snippets, mostly the `mode` field on document
  data.

## [0.6] - 2026-09-17

Design spec: `docs/design/v0.6.md`.

### Added

- Hooks: configs carry post-config steps through
  `confit.hook.run(argv, opts)` plus `config:add_hook`. Argv
  lists execute directly with no shell in between. Opts hold
  `path` (subprocess PATH dirs alone), `when` (run gate),
  `checks` (prove the run before plus after), `timeout`
  (Lua-shaped durations, default `10m`). Plan previews each
  hook as a `! run:` line with the resolved absolute binary.
  Apply runs hooks after documents land with `hook n of m`
  plus a spinner, output streaming into the run log file.
  Passing checks skip, closed gates warn and excuse,
  failures abort the rest with exit 1.
- Hook merge: identical argv plus path collapse into one run
  in first-declaration order. Gates join with OR, checks
  concatenate, timeout takes the max. Each check binds to its
  own gate on the merged run.
- Condition evaluator in Rust over path, file, plus
  environment shapes: `in_path`, `exists`, `env_eq`,
  `env_set`, `all`, `any`, `nop`.
- Plan format version 3 carrying `hooks`. Hooks persist into
  state as pure data and re-evaluate each plan: failed checks
  read as drift, passing checks read as applied.
- Named plans: `-o @work` stores under the user config
  folder as `plans/work.json`, `--plan @work` replays it.
  Empty names plus separators fail as plan errors.
- `config:require(name, hint?)`: missing siblings fail the
  plan naming both configs, hint on its own line. Existence
  alone, cycles resolve fine. The
  `plugin:{user}/{name}:{capability}` shape stays pure
  convention.
- `mise.init(version?)` returns the installer config: the
  mise binary composed from fetch plus unpack plus an opaque
  document, plus the activation patch. Explicit version wins,
  omitted resolves the
  latest tag. No `activate` call lives on the public contract;
  profiles list the installer once and gain activation with
  it.
- Tree documents: `confit.document.tree(archive, dest, fn)`
  builds one document holding many files under one folder.
  The callback keeps the `(name, info, content)` shape and
  returns a relative path per kept member, nil per skip.
  Manifests sort by relative path, modes inherit the archive
  executable bit, empty picks fail naming the filter. Plans
  read one line either way: `tree (n files)` adds,
  `tree (changed of total files changed)` updates. Drift
  walks members, apply rewrites changed members plus removes
  dropped ones while hand-placed files stay untouched.
- `solrachq.nerd_fonts` embedded plugin:
  `font(name, version?)` builds one font config holding a
  tree document flattened under the managed fonts folder,
  the shared `fc-cache -f` hook, plus an implicit require
  on the installer. The hook carries no checks and fires
  every apply while `fc-cache` resolves.
  `init()` returns that installer holding the shared hook.
  Omitted versions resolve the latest tag.

### Changed

- Plan format version 4 carrying the tree kind. State files
  at version 3 read as unsupported.
- Crates version 0.6.0 across core, engine, plus cli.
- `mise.package` takes a table: `name` required, `version`
  defaulting to `latest`, `bin` defaulting to the name for the
  shim proof, `aliases` mapping alias names to expansions with
  the binary guard, `rc_builder` optional. Each package
  folds its version into the shared TOML, declares the shared
  `mise install` hook, and requires the installer config.
  The old positional shape breaks outright.
- `confit.shell` renames to `confit.runtime`. `{{shell}}`
  template slots stay.
- The built binary names `confit` again through a `[[bin]]`
  section, so the test harness mounts the real name.
- Patch collisions settle by config declaration order
  instead of owner name. Rc plus structured patches share the
  one sort.
- Spec at 0.6.0.

### Removed

- The `--state` flag. Every run reads plus writes the fixed
  slot. Experiments point `--plan` at a rendered file,
  backups copy a plan file, sharing sends a plan file.

## [0.5] - 2026-09-16

Design spec: `docs/design/v0.5.md`.

### Added

- `init` scaffolds a project: `confit init [DIR]` (default `.`)
  writes plugin plus namespace stubs and one profile holding one
  rc document with manual pointers. Stubs copy as files.
- `apply` over files with preview-then-prompt: literal `yes`
  proceeds, a plan file flag skips the preview, a force flag
  skips the prompt, drift re-prompts always.
- Previous-states rotation plus `recover`: one fixed live slot
  under the OS config folder, `--state` override per run, five
  kept plans, `recover` lists `index @ created_at` and re-applies
  the picked one.
- `fetch_text` plus `fetch_file` with optional sha guarantee.
  `fetch_file` streams into the OS cache folder beside a sha
  sidecar owned by the `Cache` type; re-download runs on missing
  file or sidecar, sidecar mismatch, or `--re-fetch`.
- `confit.document.compressed` unpacks gzip, tar, plus zip through
  a per-member callback returning a document, nil skipping the
  member. Filters read path plus meta plus data.
- Opaque document kind for binaries: base64 payloads in the plan,
  raw bytes on apply, hash plus size drift.
- `confit.utils` namespace: `render` (moved from `confit.text`),
  `holds_cycle`, `is_array`.
- Rc api: `rc.prepend` constructor plus the `add` verb on patch
  handles, with `RcPatch`/`StructuredPatch` split types behind
  the callbacks.

### Changed

- Engine cleanup: `values.rs` splits into `error`, `lua`
  (`ValueExt`/`TableExt` `req_*` extractors), and `path_expr`;
  surface impls thin out onto the extractors.
- Error prefix fix: core `Display` shapes stay bare, the CLI owns
  the `confit:` plus `plan error:` prefixes at the edge.
- Spec at 0.5.0.

### Removed

- Dead `PathOp::Append` variant.
- Purged stale tests around the old shapes.

## [0.4] - 2026-09-14

Design spec: `docs/design/v0.4.md`. v0.3 was planned and half
built, then superseded without sealing.

### Added

- Workspace with three crates: `confit-core` (types plus pure
  functions), `confit-engine` (every Lua touch), `confit-cli`
  (args, files, terminal, logging). `engine::evaluate` returns
  finished `Vec<Document>`; configs, patches, shells, and owners
  never cross the boundary.
- Documents, patches, configs, profiles as the only concepts.
  Profiles declare machine-owned bases, configs contribute,
  patches modify through live callbacks in pipeline order
  (priority desc plus owner asc, op order verbatim).
- Five priority levels on patches (`MINOR` to `MAJOR`, default
  `NORMAL`). The engine sorts and never interprets beyond order.
- Plugin namespaces (`confit.plugin.{user}.{name}`), embedded
  `solrachq` defaults (mise, merge, template) in external shape,
  lazy loading, note-and-skip collisions, reads jailed to the
  project root.
- Rc sections as position plus guard: any entry in any section,
  profile always runs, guard splits the rest, declaration order
  inside sections. One `RcOp`/`RcEntry` model replaces the four
  entry structs.
- Plan format version 2. Version 1 covered artifacts and reads
  incompatible.
- `Filesystem` trait plus `MemoryFs` fake in the cli crate. Tests
  run on memory and never touch home folders.

### Changed

- Everything rewritten from the v0.3 tree, nothing copied. Behavior
  colocates with data; no separate model layer, no free functions
  with long parameter lists.
- One path holds one document. Repeats fail as plan errors naming
  the path. Documents carry no owner.
- Collision slots span sections by name; exec entries accumulate
  with no collision. Collision lines keep their shape with op
  kind labels.
- Shells expand to per-shell rc documents inside the engine with
  `{{shell}}` substitution. Core render is shell-agnostic.

### Removed

- Artifacts and tools vocabulary, plan format version 1.
- `--strict` flag plus warn-keep-first conflicts.
- Lanes, document owners, `ProfileGraph`, layered architecture doc.
- Legacy `plugins/` folder; engine embeds from
  `crates/engine/plugins/`.

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
