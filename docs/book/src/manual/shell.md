# Shell

Now that we completed the project setup with one rc file, we will declare both
shells and patch them once.

Some OS use bash and others use zsh, so usually you want to have both rc
available but sharing common configuration like aliases, path routes or
initialization statements of some tools.

Fortunately confit allows us to declare both files and patch them once:

```lua
local sugar = require("plugins._.sugar")
local shell = require("tools.shell")

return {
   configs = { shell },
   documents = {
      sugar.rc(sugar.home(".bashrc")),
      sugar.rc(sugar.home(".zshrc")),
   },
}
```

One config now covers both shells. An alias added in one place lands in both
files.

## One patch for both files

Move the shell config into `tools/shell.lua`:

```lua
local sugar = require("plugins._.sugar")

local function is_shell(route)
   return route.base == "home" and (route.relative == ".bashrc" or route.relative == ".zshrc")
end

return {
   name = id("shell"),
   patches = {
      {
         filter = is_shell,
         apply = function(document)
            table.insert(document.config, sugar.alias("..", "cd .."))
            table.insert(document.config, sugar.alias("g", "git"))
            table.insert(document.config, sugar.env("EDITOR", "nvim"))
         end,
      },
   },
}
```

The filter takes a route and returns a boolean, so both documents share one
callback. A third shell adds one clause to the predicate, and that is the whole
change.

> [!WARNING]
> You cannot modify the document path. The plan fails when the
> callback moves `path`.

## Aliases

Now add one of your own. Open `tools/shell.lua` and append an alias you type
every day:

```lua
table.insert(document.config, sugar.alias("k", "kubectl"))
```

Run `check` and your alias shows up in both files:

```text
+ home('.bashrc'): rc
  + alias k = kubectl
+ home('.zshrc'): rc
  + alias k = kubectl
```

Entries with a space take quotes. `sugar.alias("..", "cd ..")` renders as
`alias ..='cd ..'`, keeping the space inside one argument.

## Check and apply

```sh
confit check ~/dotfiles
```

```text
Resources
+ home('.bashrc'): rc
  + alias .. = cd ..
  + alias g = git
  + profile EDITOR = nvim
+ home('.zshrc'): rc
  + alias .. = cd ..
  + alias g = git
  + profile EDITOR = nvim
Summary
Documents: 2 to add, 0 to change, 0 to destroy.
```

Both files, same entries, one preview. Apply them:

```sh
confit apply ~/dotfiles
```

```text
Apply these changes? Type 'yes' to continue: yes
applied: 2 files, 0 removed
restore: confit restore %1
```

The rendered `~/.bashrc`:

```sh
case $- in
*i*) ;;
*) return ;;
esac

alias ..='cd ..'
alias g=git
export EDITOR=nvim
```

`~/.zshrc` renders the same entries. The shell-specific lines arrive with the
zoxide and starship configs. A second check finds the files in place and reports
zero changes:

```text
Summary
Documents: 0 to add, 0 to change, 0 to destroy.
```
