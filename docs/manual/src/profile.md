# Profile

A profile composes a shared pool of configs into one user.
I keep few computers, each with its own specs and purpose. I
reinstall from scratch each 6 months to work on clean
systems. One pool plus one profile per user makes that neat.

## Parts

Three fields compose a profile. `shells` lists the startup
files to render. `documents` holds user-owned base files.
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

Apply reads profiles, bundles, and slots through one positional:

```sh
confit plan laptop.lua --root .            # preview
confit plan laptop.lua -o @laptop          # preview into a named slot
confit apply laptop.lua                    # preview, prompt, write
confit apply @laptop --force        # reviewed plan, no prompt
confit apply %1                            # re-apply just-previous
confit init myproject                      # scaffold, default .
```

`@name` stores the rendered bundle under the user config
folder as `plans/{name}.json`, pretty printed like any plan.
`apply @name` replays it. Empty names plus separators fail.
Switching profiles runs on named slots. Render each profile
into its own name and replay by name:

```sh
confit plan laptop.lua -o @laptop
confit plan server.lua -o @server
confit apply @laptop
```

One shared slot keeps drift honest. Every switch diffs
against the same applied result.

## Experiments

I try new things constantly. The tool stays idempotent. The
same profile always yields the same documents, and the full
desired state stays reachable from plan alone. So I copy my
profile into a temporal one, add the neat new tool, test it.
I like it, it becomes my profile. I dislike it, I delete the
file, apply the old profile or past slot, done. Simpler
impossible.
