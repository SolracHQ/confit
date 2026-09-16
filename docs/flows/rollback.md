# Rollback

`confit recover` lists stored plans. `confit recover INDEX`
re-applies the picked one through the standard apply flow.
Rollback is apply with older desired documents.

```mermaid
flowchart TD
    A["Parse flags, expand tildes"] --> B{"Index given?"}
    B -- no --> C["List index plus timestamp lines"]
    B -- yes --> D["Load stored plan by index"]
    D --> E["Unknown index fails"]
    D --> F["Load previous: --state, else slot"]
    F --> G["Build: render, hash, count"]
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
    O --> P["Remove recorded orphans"]
    P --> Q["Write state file"]
    Q --> R["Archive rotation entry"]
    R --> S["Report written, removed, stored"]
```

The listing shows `index @ timestamp` lines, oldest first.
Unknown indices fail naming the range. A recovered apply
records a fresh rotation entry like any other apply, so
history keeps moving forward. Recovery restores exactly what
the stored plan holds.

Stdout carries `applied:` plus `previous:` through anstream
for a picked index. Stderr carries the listing lines plus the
preview plus prompts plus the spinner plus the `log:` path.
The listing, preview, and drift lines land on stderr through
the seams output; the report lands on stdout.
