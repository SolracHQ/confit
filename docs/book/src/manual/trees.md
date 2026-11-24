# Trees

Now that we installed the starship binary by hand, we will install a whole font
folder with one tree document.

I really like nerd fonts, they make the terminal experience by far better, but
installing them is a headache. One release holds 96 `.ttf` files, and every one
needs to land under `~/.local/share/fonts/JetBrainsMono`.

The naive way is one opaque document per file in a loop:

```lua
--- more code here
for _, name in ipairs(names) do
   table.insert(documents, { path = folder:join(name), handle = handles[name]:ref() })
end
--- more code here
```

It works, but the preview lists 96 documents, and the hook carries 96 checks,
one per file. You cannot read that preview, and a dropped member hides in the
noise.

Build the tree by hand instead. One document holds a member map, and a loop
fills it:

```lua
--- more code here
local members = {}
for _, name in ipairs(names) do
   members[name] = handles[name]:ref()
end
--- more code here
local tree = { path = folder, members = members }
--- more code here
```

The preview is one line now, and the hook checks the folder once. But the fetch,
the `.ttf` filter, the version pin, and the refresh hook are still yours to
maintain. The plugin folds all of it into two arguments:

```lua
local nerd_fonts = require("plugins._.nerd_fonts")

return nerd_fonts.font("JetBrainsMono", "3.5.1")
```

`tools/fonts.lua` returns the config directly. The function fetches the pinned
release and keeps the `.ttf` members under one folder. One document, one preview
line, one hook. The tree holds the 96 members inside:

```lua
members = {
   ["JetBrainsMonoNerdFont-Regular.ttf"] = handle:ref(),
}
```

A ref without a mode takes 644. Members sort by relative path.
Apply writes each member under the destination folder.

The check counts the tree as one document:

```text
+ data('fonts/JetBrainsMono'): tree
  + tree (96 files)
```

```text
Summary
Documents: 7 to add, 0 to change, 0 to destroy.
Hooks: 2 to add, 0 to change, 0 to destroy.
```

```sh
confit apply ~/dotfiles
```

```text
applied: 7 files, 0 removed
restore: confit restore %1
```

Apply writes the 96 members under the folder. A refresh hook runs `fc-cache`
after the write.

A file that you place in the folder stays. It belongs to no recorded member. A
release that drops a member removes it on the next apply.
