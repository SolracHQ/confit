# Introduction

Configure It (confit) is a CaC (Configuration as Code) tool
scoped in the user space. Its headline idea is a three-way
diff. Desired state comes from the profile. Previous state
comes from the last apply. Actual state comes from the disk.
Most tools compare two of these. Confit compares all three,
so every change reads from its own source. The world moved
since the last apply, or the declared state changed. Aim to reach
the following objectives.

- One user per profile, shared pool of configs across users.
- User-space files, external tools handling system software.
- Same profile yields the same documents, every run.
- Full programming language shaping the config, Lua today.
- Preview before write, always. Plan shows the change first.
- Post-config steps delegate to the tools themselves, hooks
  run after files land.
- Small binary, sharp edges, integration over ownership.

confit takes inspiration from several tools, keeping the best
of each for these specific objectives. It seeks different
objectives, but for those who like comparisons here is a small
comparison table.

| tool | scope | language | philosophy |
| --- | --- | --- | --- |
| confit | one user's files | Lua | preview first, idempotent files |
| home-manager | one user's files through nix | Nix language | declarative user env, Nix store tax included |
| chezmoi | dotfiles across machines | templates | template-driven file management |
| ansible | fleets plus systems | YAML | playbook automation at scale |
| nix | whole systems | Nix language | reproducible system builds |
| terraform | infrastructure | HCL | provisioned resources as state |
| pulumi | infrastructure | real languages | provisioned resources in code |

## Why it exists

Other tools manage dotfiles, so why confit exists is the
question you could be thinking. I looked for alternatives. I
found chezmoi, but it reads too template oriented for the
flexibility I want. Profiles stay missing
too. Branches in the source config approximate them, but the
idea was always more than dotfiles. Something flexible, able
to integrate with other tools. Home-manager comes nearest,
declarative user files with a real diff, but it asks for the
whole Nix store plus its language, more than one user
needs.

Ansible entered the picture next. Fetching the whole ansible
runtime for a simple user config reads overkill. I tried a
small set of custom scripts after that. They grew complex too
fast, repeated patterns emerged everywhere, and standardizing
them broke on exceptional cases. That leads here.
