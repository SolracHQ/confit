# Applying safely

`apply` previews first, then asks. Only the literal `yes`
writes. Every other answer leaves the disk untouched.

```sh
confit apply profile.lua
Plan: 2 to add, 0 to change, 0 to destroy.
Apply these changes? Type 'yes' to continue: yes
```

The run writes every document to its path: files land with
their rendered content, symlinks point at their targets,
missing parent folders appear. Shell lines land in the
startup files from the profile shells. Post-config steps run
after the files land. Each hook prints `hook n of m: {argv}`
beside a spinner, and its output streams into the run log
file. Passing checks skip the hook with a `skipped:` line,
closed gates warn and excuse it, failures abort the rest.

`require` resolves under the root, so profiles plus tools
plus resources travel as one folder. The root defaults to
the profile folder. `--root` moves it for shared layouts.

When a file changed on disk since the last apply, confit says
so and asks again. The world moved, the preview is stale.

Every apply stores the previous state, five deep. `recover`
lists them and re-applies the picked one:

```sh
confit recover
0 @ 2026-09-16T10:00:00+00:00
```

Hand-edit a file and run plan: the drift shows. The summary
names every change.

## One state per user

Plan and state share the exact shape on purpose. A state
file holds the plan that already applied. One user holds one
applied result, and one state slot mirrors it, so every
apply diffs against the same recorded result. Reach for
saved plans when switching profiles. A rendered plan replays
with `apply --plan`, evaluation plus fetching costs stay
paid, and the switch stays trivial and fast. Experiments
point `--plan` at a rendered file.
