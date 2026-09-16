# Apply without a plan

`confit apply [PROFILE]` evaluates first, then follows
the same guarded write path. The profile rides positionally,
required unless `--plan` passes. The preview renders for review
on the spot.

```mermaid
flowchart TD
    A["Parse flags, expand tildes"] --> B["Require positional profile"]
    B --> C["Evaluate profile with Lua engine"]
    C --> D["Resolve state: --state, else slot"]
    D --> E["Load previous, missing reads empty"]
    E --> F["Build: render, hash, count"]
    F --> G["Drift baseline against disk"]
    G --> H["Render preview"]
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
    N --> O["Remove recorded orphans"]
    O --> P["Write state file"]
    P --> Q["Archive rotation entry"]
    Q --> R["Report written, removed, stored"]
```

`--plan FILE` runs instead on the file alone with no profile
and no preview; see apply-plan.md. That branch sets preview
to false and skips the preview render.

Apply always writes the state file: explicit `--state` wins,
else the fixed slot records the result. The machine owns the
result after every apply. The next plan reads the slot and
shows zero changes while disk matches. Switching profiles
converges through the same slot: last applied wins, orphans
from the earlier profile delete.

Stdout carries `applied:` plus `previous:` through anstream.
Stderr carries the preview plus prompts plus the spinner plus
the `log:` path. The preview and drift lines land on stderr
through the seams output; the report lands on stdout.
