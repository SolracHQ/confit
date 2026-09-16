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
startup files from the profile shells.

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
