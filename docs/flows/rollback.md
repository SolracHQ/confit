# Apply the past

`confit apply %N` re-applies one history entry newest-first
from one. `confit apply @name` re-applies one named slot.
Both run the standard apply flow with preview plus prompts.
Applying the past is apply with older desired documents.

```mermaid
flowchart TD
    A["Parse flags, expand tildes"] --> B["Sniff positional shape"]
    B --> C["Load slot plan through the pool"]
    C --> D["Unknown pick fails naming the count"]
    C --> F["Load fixed slot state"]
    F --> G["Trust stored hashes, no render"]
    G --> H["Drift baseline against disk"]
    H --> I["Render preview"]
    I --> J{"--force?"}
    J -- no --> K["Prompt, literal yes continues"]
    J -- yes --> L["Fresh drift check"]
    K -- abort --> Z["Abort, nothing written"]
    K -- yes --> L
    L --> M{"World moved?"}
    M -- yes --> N["Show drift, prompt again"]
    M -- no --> O["Write documents"]
    N -- abort --> Z
    N -- yes --> O
    O --> P["Remove recorded orphans plus dropped tree members"]
    P --> Q["Write fixed slot state"]
    Q --> R["Archive rotation entry"]
    R --> S["Run hooks in order"]
    S --> T["Report written, removed, stored"]
```

Picks count newest-first from one, so `%1` names the
just-previous entry. A past apply records a fresh rotation
entry like any other apply, so history keeps moving
forward. Applying the past restores exactly what the
stored plan holds.

Stdout carries `applied:` plus `previous:` through anstream
for a picked slot. Stderr carries the preview plus prompts
plus the spinner plus the `log:` path. The preview and
drift lines land on stderr through the seams output; the
report lands on stdout.
