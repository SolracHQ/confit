# Applying safely

You stand at a demo that runs on two machines. This chapter covers safe applies.

`apply` previews first, then asks. Only the literal `yes`
writes. Every other answer leaves the disk untouched.

```sh
confit apply profile.lua
```

A first run compares planned files against disk bytes and
prints one line naming adds plus files already in place:

```text
Bundle: 2 to add, 1 already in place.
```

A repeat run prints one `Bundle:` line naming adds, changes,
and removals.
The run writes every document to its path. Files land with
their rendered content, symlinks point at their targets,
missing parent folders appear. Shell lines land in the
startup files from the profile shells. Post-config steps run
after the files land. Each hook prints one line naming its
place in the order, and its output lands in the run log.
The log path prints after the run. Passing checks skip the
hook, closed gates skip it, failures stop the rest.

`require` resolves under the root, so profiles plus tools
plus resources travel as one folder. The root defaults to
the profile folder. `--root` moves it for shared layouts.

When a file changed on disk since the last apply, confit says
so and asks again:

```text
vanished: manually deleted. changed outside config: add to config or the next apply loses them
```

Answer `yes` to overwrite with the planned content. Any
other answer writes nothing. The world moved, the preview
is stale.

Every apply stores the previous state, five deep. `apply %N`
re-applies one newest-first from one:

```sh
confit apply %1
```

Hand-edit a file and run plan. The drift shows. The summary
names every change.

## One state per user

Manifest and state share the exact shape on purpose. A state
file holds the manifest that already applied. One user holds one
applied result, and one state slot mirrors it, so every
apply diffs against the same recorded result. Reach for
saved manifests when switching profiles. A rendered manifest replays
through the positional, evaluation plus fetching costs stay
paid, and the switch stays trivial and fast. Experiments
apply a rendered file.

For the exact contract see [spec apply](../spec/apply.md) and [spec drift](../spec/drift.md).

Safe applies now read clearly. Next, [Troubleshooting](troubleshooting.md) fixes common stops.
