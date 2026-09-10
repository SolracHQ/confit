# Changelog

Follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Behavior changes land here under `Unreleased` in the same task as the
code; sealing moves them under the version.

## [Unreleased]

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
