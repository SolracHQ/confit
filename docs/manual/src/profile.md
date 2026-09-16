# Profile

A profile composes a shared pool of configs into one machine.
I keep few computers, each with its own specs and purpose. I
reinstall from scratch each 6 months to work on clean machines.
One pool plus one profile per machine makes that neat.

## Parts

Three fields compose a profile. `shells` lists the startup
files to render. `documents` holds machine-owned base files.
`configs` holds the tool contributions.

```lua
local tools = require("tools.tools")
return {
  shells = { "bash", "zsh" },
  documents = { fonts },
  configs = { tools, kitty },
}
```

`bash` renders `~/.bashrc`, `zsh` renders `~/.zshrc`, every
other name renders `~/.<name>rc`. `require` resolves files
under the root. The root defaults to the profile folder.
`--root` moves it.

## Commands

Every command takes the profile first:

```sh
confit plan laptop.lua --root .            # preview
confit plan laptop.lua -o plan.json        # preview into a file
confit apply laptop.lua                    # preview, prompt, write
confit apply --plan plan.json --force      # reviewed plan, no prompt
confit apply laptop.lua --state laptop.json
confit recover                             # list stored states
confit recover 0                           # re-apply one
confit init myproject                      # scaffold, default .
```

`--state` selects the state file. Omitted means the shared
slot. One state per profile keeps drift honest across
machines:

```sh
confit plan laptop.lua --state laptop.json
confit apply server.lua --state server.json
```

## Experiments

I try new things constantly. The tool stays idempotent: the
same profile always yields the same documents, and the full
desired state stays reachable from plan alone. So I copy my
profile into a temporal one, add the neat new tool, test it.
I like it, it becomes my profile. I dislike it, I delete the
file, apply the old profile or recover, done. Simpler
impossible.
