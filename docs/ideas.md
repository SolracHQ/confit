# Ideas

Cards for future work. None sits near a plan document.
Shipped ideas graduate off this page.

---

Idea: Add plan time interactive variables
Importance: Mid/Low
Pain: Being able to have plan time facts could be good for dynamic behaviors as have exactly the same tools and language and fonts but maybe customize if use one starship theme or other, or a python version, small things, this is technically already cover by profiles but is a good to have so is not really a priority

---

Idea: Host facts (plan time conditions)
Importance: Mid
Pain: All my personal machines are linux, but sometimes I install windows in my gaming pc and I need to think in others that could want to use the tool, so the profiles should know the host facts to be able to use scoop or chocolatey instead of mise or even brew with simple lua ifs, is something simple to implement but that i did not do until now because was not yet necessary for my pcs, since all are x64 linux pcs, but the world is a jungle

What I have in my head

```lua
if confit.facts.os == "windows" then
  return confit.config(INSTALLER_NAME):require(SCOPE_INSTALLER_NAME)
end
```

On windows mise installs through scoop, so the installer top takes that road first and the rest stays as today.

Facts make the tool multiplatform, and that brings preoccupations worth solving early. Paths resolve at plan time and hardcode into the bundle today. If the path library returns a struct pointing at home instead of a bare string, and apply resolves it late, one bundle serves several users on one machine or several machines with several users, as long as they stay similar enough.

Hardness can emerge from use instead of declarations. Hook `__index` on the facts table, so touching a fact records it. If the profile ifs over that fact, it turns hard. Home counts too. A bundle that ifs over os applies only where os matches exactly.

---

Idea: Stop holding the whole run in memory
Importance: Critical
Status: Fully planned in v0.8. The memory section carries the whole pipeline.
Pain: My dotfiles plan peaks at 1.7GB RSS in 6.6s and example 3 alone hits 1.3GB, and with the ram prices that is a big pain. We are using memory for use it not because we need it, I have almost everything in memory all the time.

My suspects so far:

- On store I am storing whole gzips in vectors. Raw blobs get cloned beside the originals, the parallel compression holds all inputs plus all outputs at once, and the whole bundle gzip gets built in memory before a single byte hits disk.
- On compress we pass all the childs of the compressed files as literal lua strings putting pressure in the gc, while Rust keeps the same bytes alive. There is not one explicit GC call in the whole run.
- Built plus previous bundles stay alive together through presentation, so all the peaks overlap instead of sequencing.

What I have in my head

Stop holding everything in memory. Stream when possible, make things lazy as possible. I have no idea how to solve it yet.

---

Idea: Let hooks wait for other hooks
Importance: Mid
Pain: On windows scoop installs mise and mise install needs mise present, but scoop has no declarative file like mise.toml to hang a require on. Today every hook depends on configs alone and runs in declaration order. I need a hook that waits for another hook, or the windows installer story cannot sequence.

What I have in my head
```lua
config:add_hook(confit.hook.run({ "scoop", "install", "mise" }))
config:add_hook(confit.hook.run({ "mise", "install" }, {
  after = { { "scoop", "install", "mise" } },
}))
```
Hooks already answer to argv everywhere, so dependencies name argv too. Cycles fail the plan. Declaration order still decides the rest.

---

Idea: Take secret sources from commands at apply time, never store them
Importance: Lowest
Pain: I want my setup to run from my 1password ssh key to a full git config that clones my private repos after my confit call. That means bytes I refuse to store, only how to create them. A Lua script source does not work, user code plus upvalues cannot be extracted and re-run later. Some day this wants to be a plugin shaped like a command whose stdout becomes the bytes at apply time, and the result never lands in a bundle or a preview. Hooks cover it poorly today, a hook output is not file content and secrets in argv touch the process list. Anything running user code over credentials inherits security duty I do not want yet, so this parks at the bottom until unmanaged exists to build on.

What I have in my head:

```lua
confit.document.secret(
  confit.path.home(".ssh/id_ed25519"),
  { "op", "read", "op://private/github-key/private-key" }
)
```

---

Idea: Let tree callbacks return a boolean or a table
Importance: Mid
Pain: The tree callback returns a path today, and that return does triple duty as filter plus relocate plus implicit keep. My nerd fonts callback relocates only to flatten, which a boolean would say better. Lua is flexible and we do not take advantage of it. The shape I want is nil skips, true keeps at the same path, a table overrides. The table would carry path plus mode, because tarballs keep permissions but some compress formats drop them, and that half needs real research on which formats lose what before I bless a mode key.

What I have in my head:

```lua
local tree = confit.document.tree(archive, dest, function(path, _, _)
  if not path:match("%.ttf$") then
    return nil
  end
  return true
end)
```

```lua
return { path = "renamed.ttf", mode = "755" }
```

---

Idea: Decide what default plugins deserve to exist before 1.0
Importance: Low
Pain: Mise plus nerd fonts do specific things and read well thought. Merge plus template do different tiny things each, too small to group under one good name, which makes me ask if they need to ship at all. Anyone can write those helpers, but shipping them is DX like a stdlib. Maybe my plugins are the stdlib and these two ride there, maybe they fold into utils, maybe they drop to userland. Breaking in alpha is free, after 1.0 it is not, so this has a deadline even with no urgency.

What I have in my head:

```lua
local merged = confit.utils.merge(base, overlay)
tool:add_document(confit.utils.template(path, { src = "page.txt", vars = vars }))
```

---

Idea: Let profiles set the compression level
Importance: Low
Pain: My tests say level 6 is my sweet spot of time per compressed MB. Over 6 takes too long and saves few MBs, under grows several MBs for little time back. But those are my files and my data, other users feel different things. One profile field validated 0 to 9, defaults stay 6 for blobs plus 0 for the outer tar. Profile level, not per document, one knob nobody needs to measure per file. This rides under the memory card, streaming the bundle matters more than re-leveling it.

What I have in my head:

```lua
return {
  shells = { "bash" },
  documents = {},
  configs = { tools },
  compression = { blob = 4 },
}
```

---

Idea: Let trees group hand-built documents, not only archives
Importance: Mid
Pain: Related configs spray many small documents across the plan. One config needing 20 text documents in the same folder reads as noise next to the thing they serve. Separate documents diff better, grouped trees read better, both are valid, so the tool should offer the choice. Today tree only grows from compressed archives, so hand grouping has no home.

What I have in my head

Tree becomes a namespace with more than one source. The compressed road stays exactly as today, plus a from-data road that takes a list of documents and lands them under one destination. There are too many things open yet. I do not know if tree reuses documents and overwrites the paths or builds new constructors only for the tree. I am not even sure of the shape of the method. This is just a sketch, I will think on it when the moment of planning comes.

```lua
local bundle = confit.document.tree.from_data(dest, { doc1, doc2 })
```
