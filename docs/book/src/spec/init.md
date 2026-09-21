# Scaffold a project

The init command fills a target folder with one profile and
editor stubs. DIR defaults to the current folder. One leading
`~` expands against the home folder before parsing.

## Stages

### Parse

Parse reads DIR with `.` as the default and expands the tilde
first. The profile path joins as `{DIR}/profile.lua`. Every
stub path joins under the same DIR.

### Clash check

The check stats the `stubs` folder first, then the profile
path and every stub path in write order. A present path
aborts the run before any write lands. The exact message and
the full path list live under Clash matrix.

### Write profile

The run writes the embedded starter profile to
`{DIR}/profile.lua`. The starter evaluates to one rc document
through the Lua framework.

### Write stubs

The run copies each embedded stub to its target-relative path
under DIR. Namespace stubs land under `stubs/`, plugin stubs
sit under `plugins/` next to their plugin names. The full
path list lives under Clash matrix.

### Report

The run reports the profile path and the written file count.
The count covers the profile and every stub.

## Clash matrix

Each present path aborts the run before any write lands. The
message names the first clash:

```sh
init: '{path}' already exists, remove it or pick another target
```

The check covers these paths:

```text
{DIR}/profile.lua
{DIR}/stubs
{DIR}/stubs/confit.d.lua
{DIR}/stubs/namespaces/config.d.lua
{DIR}/stubs/namespaces/document.d.lua
{DIR}/stubs/namespaces/hook.d.lua
{DIR}/stubs/namespaces/patch.d.lua
{DIR}/stubs/namespaces/paths.d.lua
{DIR}/stubs/namespaces/plugin.d.lua
{DIR}/stubs/namespaces/resources.d.lua
{DIR}/stubs/namespaces/runtime.d.lua
{DIR}/stubs/namespaces/utils.d.lua
{DIR}/plugins/solrachq/mise/plugin.d.lua
{DIR}/plugins/solrachq/nerd_fonts/plugin.d.lua
{DIR}/plugins/solrachq/merge/plugin.d.lua
{DIR}/plugins/solrachq/template/plugin.d.lua
```

The `stubs` folder check runs first, so a present folder
reports the folder path. Remaining paths report in write
order with the profile first. The check completes before the
profile write. The abort
exits 1 under the plan-error prefix, so stderr reads
`confit: plan error: ` and the message.

## Stdout

Stdout answers with one line holding the profile path and the
file count:

```sh
init: {profile} ({n} files)
```

The current scaffold writes 15 files: the profile and 14
stubs. The run completes in one pass: parse, check, write,
report. Stderr stays quiet.
