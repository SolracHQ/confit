# Apply without a plan

`confit apply [PROFILE]` evaluates first, then follows
the same guarded write path. The profile rides positionally,
required unless `--plan` passes. The preview renders for review
on the spot.

```mermaid
flowchart TD
    A["Parse flags, expand tildes"] --> B["Require positional profile"]
    B --> C["Evaluate profile with Lua engine"]
    C --> D["Load fixed slot state"]
    D --> E["Missing slot reads empty, marks first run"]
    E --> F["Build: render, hash, count"]
    F --> G["Drift baseline against disk"]
    G --> H["First run: preview impact desired versus disk"]
    H --> I["Render preview"]
    H --> I{"--force?"}
    I -- no --> J["Prompt, literal yes continues"]
    I -- yes --> K["Fresh drift check"]
    J -- abort --> Z["Abort, nothing written"]
    J -- yes --> K
    K --> L{"World moved?"}
    L -- yes --> M["Show drift, prompt again"]
    L -- no --> N["Write documents"]
    M -- abort --> Z
    M -- yes --> N
    N --> O["Remove recorded orphans plus dropped tree members"]
    O --> P["Write fixed slot state"]
    P --> Q["Archive rotation entry"]
    Q --> R["Run hooks in order"]
    R --> S["Report written, removed, stored"]
```

`--plan FILE` runs instead on the file alone with no profile
and no preview; see apply-plan.md. That branch sets preview
to false and skips the preview render.

Apply always writes the fixed slot state. The user owns the
result after every apply. The next plan reads the slot and
shows zero changes while disk matches. Switching profiles
converges through the same slot: last applied wins, orphans
from the earlier profile delete. Tree destinations never
delete, dropped members delete per manifest. Hooks run after
state plus history land, in plan order with pre-check skips
plus post-check failure aborts.

Stdout carries `applied:` plus `previous:` through anstream.
Stderr carries the preview plus prompts plus the spinner plus
the `log:` path. The preview and drift lines land on stderr
through the seams output; the report lands on stdout.
