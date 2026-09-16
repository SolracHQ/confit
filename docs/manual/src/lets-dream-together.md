# Lets dream together

This page holds the crazy ideas. None of them sits near a
plan document. Too much ground work pends. The tool stays
too green: commands arrive with hooks.

Plugin git projects: each `username/name` plugin lives in its
own repo, required straight from the profile. Shared tooling
ships as repos.

Require guards: plugins declare what they need, and the
loader explains the missing piece instead of failing. Plugin
orders resolve themselves.

Dylib libraries: native extensions for real capability
expansion. Security needs thought first: signing, sandboxing,
explicit grants. Risky plan, open future.

Hooks: user-space commands running at plan or apply time.
The kitty tarball route needs them: download, extract, chmod,
link. Hooks never escalate: no root step, no privilege path.
confit manages config, external binaries stay external. Until
hooks exist, binaries install by hand and confit owns
everything around them.
