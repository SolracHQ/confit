# Plan changes

`confit plan PROFILE` previews through reads alone. The profile
rides positionally. Stdout holds the diff. The payload lands in
a file.

```mermaid
flowchart TD
    A["Parse flags, expand tildes"] --> B["Require positional profile"]
    B --> C["Evaluate profile with Lua"]
    C --> D["Resolve state: --state, else slot"]
    D --> E["Load previous, missing reads empty"]
    E --> F["Snapshot disk, diff drift"]
    F --> G["Build: render, hash, count"]
    G --> H{"-o given?"}
    H -- yes --> I["Write plan file"]
    H -- no --> J["Write tmp plan, print path"]
    I --> K["Render summary: moving docs, drift, counts"]
    J --> K
    K --> L["Print log path"]
```

Evaluate runs the profile through the engine. State resolves to
the explicit file or the fixed slot at `state.json`. Missing state
reads empty, so first runs show everything as add. Drift compares
recorded documents against disk bytes. Build renders plus hashes
every document and counts create, update, delete against previous.
The summary prints creates, updates, deletes, drift notes, plus
counts. The tmp path serves
later `--plan` plus `--state` reuse.

Stdout carries the summary plus the `plan:` path through
anstream. Stderr carries the spinner plus the `log:` path.
Prompts never appear here: plan writes no documents.
