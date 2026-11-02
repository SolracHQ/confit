# Machines

Now that the laptop setup is finished, we will add a smaller server profile.

Your laptop setup works. Your server needs less. The server needs no prompt
theme, no fonts, and no bat config. One profile installs tools that the server
never uses.

Add a second profile beside `main.lua`:

```lua
local sugar = require("plugins._.sugar")

local installer = require("tools.mise")
local shell = require("tools.shell")
local ripgrep = require("tools.ripgrep")

return {
   configs = { installer, shell, ripgrep },
   documents = {
      sugar.rc(sugar.home(".bashrc")),
   },
}
```

`server.lua` returns three configs and one shell document. It skips the prompt,
the fonts, and the theme. One project describes two machines.

Select the profile with a flag:

```sh
confit check ~/dotfiles --profile server.lua
```

```text
Summary
Documents: 3 to add, 0 to change, 0 to destroy.
Hooks: 1 to add, 0 to change, 0 to destroy.
```

`--profile FILE` resolves against the project folder. The default profile stays
`main.lua`. One run evaluates one profile.

```sh
confit apply ~/dotfiles --profile server.lua
```

```text
applied: 3 files, 0 removed
restore: confit restore %1
```

The server gets the shell and two tools. The laptop keeps the full setup. You
change one folder for both machines.

## A minimal laptop

The server profile also works on your laptop. Bundle it once as a named state,
and restore it when you need the fancy setup gone:

```sh
confit bundle ~/dotfiles --profile server.lua -o @minimal
```

```text
bundle: @minimal
```

```sh
confit restore @minimal
```

The laptop loses the prompt, the fonts, and the theme. Your shell keeps working
because the profile still manages `.bashrc`.

Name the normal profile too, and rollback is one command:

```sh
confit bundle ~/dotfiles -o @laptop
```

```text
bundle: @laptop
```

Try the minimal setup, break things, experiment. `confit restore @laptop` brings
the full setup back.

> [!NOTE]
> Named states share one pool, so `@laptop` and `@minimal` store common
> bytes once. Blobs are compressed, with a compression ratio around 50%.

Run the bundle after an apply of the main profile, so the shared bytes are
already stored.
