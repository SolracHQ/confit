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
