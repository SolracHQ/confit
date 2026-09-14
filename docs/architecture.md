# ConfIt architecture

Three crates form the app. One rule places each function: a
function goes to core if you can unit-test it without Lua, files,
or a terminal. Lua evaluation lives in engine. Files plus terminal
live in cli.

`confit-core` holds pure data plus render. It owns Document, Plan,
State, Snapshot, Diff, ids, hashing, Error.

`confit-engine` evaluates Lua profiles into documents. It owns
Patch, Config, Level, sorting, live wrappers, jailed reads, plugins,
shell expansion.

`confit-cli` owns the terminal surface. It owns args, run_plan,
run_status, presentation, fs seam, logging, main.

## Dependencies

```text
confit-cli --> confit-engine --> confit-core
confit-cli --> confit-core
```

No other edges exist. Engine exposes `evaluate(profile) ->
Result<Vec<Document>>`. Core render stays shell agnostic. Shell
expansion stays engine internal.

## Data flow

The profile evaluates to a vector of documents. The plan builds
desired state against previous state plus disk snapshots, hashing
rendered bytes. The plan writes as JSON. The terminal prints drift
warnings plus the summary plus the log path.
