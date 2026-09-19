# Ideas

Cards for future work. None sits near a plan document.
Shipped ideas graduate off this page.

---

Idea: Add a runtime check for hooks, that checks if certain documents changed since last execution either by new desired state or by fs drift
Importance: High
Pain: Every time I apply the bundle, even when I dont change the fonts or mise tools hooks get triggered

---

Idea: Add plan time interactive variables
Importance: Mid/Low
Pain: Being able to have plan time facts could be good for dynamic behaviors as have exactly the same tools and language and fonts but maybe customize if use one starship theme or other, or a python version, small things, this is technically already cover by profiles but is a good to have so is not really a priority

---

Idea: Host facts (plan time conditions)
Importance: Mid
Pain: All my personal machines are linux, but sometimes I install windows in my gaming pc and I need to think in others that could want to use the tool, so the profiles should know the host facts to be able to use scoop or chocolatey instead of mise or even brew with simple lua ifs, is something simple to implement but that i did not do until now because was not yet necessary for my pcs, since all are x64 linux pcs, but the world is a jungle

---

Idea: Stop holding the whole run in memory
Importance: Critical
Pain: My dotfiles plan peaks at 1.7GB RSS in 6.6s and example 3 alone hits 1.3GB, and with the ram prices that is a big pain. We are using memory for use it not because we need it, I have almost everything in memory all the time.

My suspects so far:

- On store I am storing whole gzips in vectors. Raw blobs get cloned beside the originals, the parallel compression holds all inputs plus all outputs at once, and the whole bundle gzip gets built in memory before a single byte hits disk.
- On compress we pass all the childs of the compressed files as literal lua strings putting pressure in the gc, while Rust keeps the same bytes alive. There is not one explicit GC call in the whole run.
- Built plus previous bundles stay alive together through presentation, so all the peaks overlap instead of sequencing.

---

Idea: Apply over pre-existing symlinks instead of half writing through them
Importance: High
Pain: My old dotfiles symlink starship.toml and mise.toml, and apply gets really confused when the file exists but is a symlink. It says wrote but does nothing, it does not remove the link, it just lands in a strange state. I delete them manually and re-run and it works, so something in the write path needs solving.

My suspects so far:

- The snapshot reads links without following them, and creating a link document replaces present files. So the app knows what a symlink is.
- The plain file write path apparently does not. It probably opens through the link or past it, reports wrote, and the bytes never land where the plan thinks they did.
- The drift side likely reads one thing while the write side does another, which is how you get a confident report plus a strange disk.
