# Apply a source

`confit apply SOURCE` sniffs its shape first, then follows
the same guarded write path. Profiles evaluate through the
engine, bundles plus slots load through the pool. The preview
renders for review on the spot.

```mermaid
flowchart TD
    A["Parse flags, expand tildes"] --> B["Sniff positional shape"]
    B --> C["Evaluate profile or load slot plan"]
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
    Q --> Q2["Prune unreferenced pool blobs"]
    Q2 --> R["Run hooks in order"]
    R --> S["Report written, removed, stored"]
```

A `.cb` source runs on the file alone with no profile
and no preview; see apply-plan.md. That branch sets preview
to false and skips the preview render.

Apply always writes the fixed slot state. The user owns the
result after every apply. The next plan reads the slot and
shows zero changes while disk matches. Switching profiles
converges through the same slot: last applied wins, orphans
from the earlier profile delete. Tree destinations never
delete, dropped members delete per manifest. Prune drops pool
blobs referenced by no slot after archiving, so rotated-out
entries release their bytes at once. Hooks run after
state plus history land, in plan order with pre-check skips
plus post-check failure aborts.

Stdout carries `applied:` plus `previous:` through anstream.
Stderr carries the preview plus prompts plus the spinner plus
the `log:` path. The preview and drift lines land on stderr
through the seams output; the report lands on stdout.
