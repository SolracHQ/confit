# Philosophy

You stand at the end of the demo from init to managed machines. This chapter tells why the tool reads this way.

confit compares three states:

- Desired state comes from the profile.
- Previous state comes from the last apply.
- Actual state comes from the disk.

Every change reads from its own source. The world moved since the last apply, or the declared state changed.

I aim for these objectives:

- One user per profile, shared pool of configs across users.
- User-space files, external tools handling system software.
- Same profile yields the same documents, every run.
- Full programming language shaping the config, Lua today.
- Preview before write, always. Plan shows the change first.
- Post-config steps delegate to the tools themselves, hooks run after files land.
- Small binary, sharp edges, integration over ownership.

confit takes inspiration from several tools. It keeps the best of each for these objectives. For readers who like comparisons, here is a small comparison table:

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

I manage dotfiles across machines. I looked for tools that fit that work. I tried chezmoi first. It manages files through templates. I wanted Lua code shaping files plus profiles for each machine. I tried home-manager next. It manages user files with a real diff through Nix. I wanted a small binary with Lua alone. I tried Ansible next. It automates fleets through YAML playbooks. I wanted user-space files with a preview first. I tried custom scripts next. They shaped files with direct code. I wanted shared patterns with steady behavior across runs. I built confit for that work.
