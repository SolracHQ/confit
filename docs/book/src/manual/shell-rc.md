# Shell lines

You stand at `~/confit-demo` with named file shapes behind you. This chapter shows how the demo renders startup files.

Shell setup means lines in a startup file. `bash` renders `~/.bashrc`, `zsh` renders `~/.zshrc`, every other name renders `~/.<name>rc`. The demo profile lists both shells:

```lua
return { shells = { "bash", "zsh" }, configs = { tools } }
```

One rc document renders once per shell. `{{shell}}` inside `eval` and `source` entries resolves per shell.

## Sections

Entries live in three sections:

- `profile` lines render first and always run.
- `config` plus `final` lines render after an interactive guard, so scripts stay quiet:

```sh
case $- in
*i*) ;;
*) return ;;
esac
```

Six builders cover every line. The demo shell config draws its alias plus its PATH line from this set:

```lua
confit.document.rc.alias("ll", "ls -l")          -- alias ll=...
confit.document.rc.env("EDITOR", "hx")           -- export EDITOR=hx
confit.document.rc.prepend("~/.local/bin")       -- PATH prepend
confit.document.rc.eval({ "zoxide", "init", "bash" })  -- eval "$(...)"
confit.document.rc.cmd({ "mise", "activate", "bash" }) -- bare command
confit.document.rc.source("~/.secrets")          -- source ...
```

## Guards

Every builder takes `{ when = guard }` as last argument. The entry renders inside an `if`, and the shell evaluates the condition at startup. The demo uses guards to keep optional lines quiet until their tool arrives:

```lua
confit.document.rc.alias("ll", "ls -l", {
  when = confit.runtime.in_path("eza"),
})
confit.document.rc.env("EDITOR", "hx", {
  when = confit.runtime.all({
    confit.runtime.env_set({ key = "SSH_CONNECTION" }),
    confit.runtime.exists("~/.config/hx"),
  }),
})
```

Guards compose. `env_eq` matches a variable, `env_set` tests presence, `in_path` tests a binary, `exists` tests a file, `all` plus `any` combine them.

The demo shells now render per shell with guards where it counts. Next, [Patch](patch.md) shows how demo configs share one file.
