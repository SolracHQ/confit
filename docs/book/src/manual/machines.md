# Machines

You stand at `~/confit-demo` with tools installed through plugins. This chapter splits the demo across two machines. This is the payoff for the whole book.

One shared pool of configs, one profile per machine. The laptop profile declares laptop tools, the server profile declares server tools, both load the same shared files. A fresh machine needs only the binary and the project folder.

## One profile per machine

Keep the demo folder with one profile per machine and shared tool files beside them:

```text
~/confit-demo/
  laptop.lua
  server.lua
  tools/
    shared.lua
```

Each profile returns shells, documents, and configs. Shared tools load through `require` under the root. The root defaults to the profile folder, so sibling files resolve out of the box. The demo laptop profile reads:

```lua
local shared = require("tools.shared")

return {
  shells = { "bash" },
  documents = {},
  configs = { shared },
}
```

The server profile mirrors it with server tools in place of laptop tools.

## Switching with named slots

Render each profile into its own named slot:

```sh
confit plan laptop.lua -o @laptop
confit plan server.lua -o @server
```

Replay a slot by name:

```sh
confit apply @laptop
```

`apply @laptop` previews the stored result, prompts, writes. Switch back any time:

```sh
confit apply @server
```

Every switch diffs against the same applied result, so drift stays honest. Empty names and separators fail, so keep slot names to simple words.

## Reinstall from scratch

A clean machine needs the binary and the project folder. Bring both, then apply the profile for that machine:

```sh
confit apply laptop.lua
```

Type `yes` at the prompt. Every managed file lands as declared. The machine matches the profile.

## Moving with export

Pack the applied result into one portable file:

```sh
confit export -o backup.cb
```

No picker means the applied slot. Copy `backup.cb` to the new machine, then replay it:

```sh
confit apply backup.cb
```

The bundle carries the full desired state. The new machine converges from that file alone. Install the binary, copy the file, apply it. Fresh machine, same setup.

For the exact contract see [spec storage](../spec/storage.md).

The demo now runs per machine from one pool. Next, [Applying safely](applying.md) covers prompts, drift, and history.
