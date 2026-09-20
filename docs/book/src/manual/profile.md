# Profile

You stand at `~/confit-demo` with one shell and one alias. This chapter adds a second shell plus a shared tool file.

A profile composes a shared pool of configs into one user. Three fields compose a profile. `shells` lists the startup files to render. `documents` holds user-owned base files. `configs` holds the tool contributions.

The demo profile starts with `bash` alone. Add `zsh` beside it. Add one shared tool file under the root and require it. The demo layout reads:

```text
~/confit-demo/
  profile.lua
  tools/
    tools.lua
```

`profile.lua` now returns two shells plus two configs:

```lua
local tools = require("tools.tools")

return {
  shells = { "bash", "zsh" },
  documents = { base },
  configs = { shell, tools },
}
```

`bash` renders `~/.bashrc`, `zsh` renders `~/.zshrc`, every other name renders `~/.<name>rc`. `require` resolves files under the root. The root defaults to the profile folder. `--root` moves it for shared layouts.

`tools/tools.lua` holds the first extracted tool config. It returns one config built the same way the profile shell config reads. The profile stays small while the pool grows beside it.

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

`@name` saves a named slot for replay. Reference lists the saved shapes. `apply @name` replays it. Empty names plus separators fail. Switching profiles runs on named slots. Render each profile into its own name and replay by name:

```sh
confit plan laptop.lua -o @laptop
confit plan server.lua -o @server
confit apply @laptop
```

One shared slot keeps drift honest. Every switch diffs against the same applied result.

For the exact contract see [spec plan](../spec/plan.md).

The demo now covers two shells with room for more tools. Next, [Config](config.md) adds kitty as its second config.
