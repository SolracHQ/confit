# Schemas

This page lists the JSON shapes with TypeScript types.

## Document

The document pairs one route with one payload, see documents.md for semantics.

```ts
type Route = {
  base: string;
  relative: string;
};
type Condition =
  | { env_eq: { key: string; value: string } }
  | { env_set: { key: string } }
  | { in_path: { name: string } }
  | { exists: { route: Route } }
  | { changed: { route: Route } }
  | { all: Condition[] }
  | { any: Condition[] }
  | { nop: Condition };
type RcEnv = {
  name: string;
  value: string;
  when: Condition | null;
};
type RcPath = {
  name: string;
  dir: Route;
  when: Condition | null;
};
type RcAlias = {
  name: string;
  expansion: string;
  when: Condition | null;
};
type RcEval = {
  argv: (string | Route)[];
  when: Condition | null;
};
type RcCmd = {
  argv: (string | Route)[];
  when: Condition | null;
};
type RcSource = {
  path: Route;
  when: Condition | null;
};
type RcEntry =
  | { env: RcEnv }
  | { path: RcPath }
  | { alias: RcAlias }
  | { eval: RcEval }
  | { cmd: RcCmd }
  | { source: RcSource };
type TreeFull = {
  handle: string;
  mode: number;
};
type TreeMember = string | TreeFull;
type Document =
  | { path: Route; unmanaged: boolean; format: string;
    data: Record<string, unknown> }
  | { path: Route; unmanaged: boolean; content: string;
    mode: number }
  | { path: Route; unmanaged: boolean; target: Route }
  | { path: Route; unmanaged: boolean; profile: RcEntry[];
    config: RcEntry[]; final: RcEntry[] }
  | { path: Route; unmanaged: boolean; handle: string;
    mode: number }
  | { path: Route; unmanaged: boolean;
    members: Record<string, TreeMember> };
```

## Hook

The hook holds one command with its guards and limit.

```ts
type Hook = {
  argv: (string | Route)[];
  path: (string | Route)[];
  id: string;
  requires: Condition;
  when: Condition;
  check: Condition;
  timeout_secs: number;
};
```

## Manifest

The manifest holds the version with documents and hooks.

```ts
type Manifest = {
  version: [number, number, number];
  documents: Document[];
  hooks: Hook[];
};
```

## Change

The change holds one route diff with before and after states.

```ts
type Side = {
  sha: string;
  mode: number;
};
type Change = {
  path: Route;
  expanded: string;
  kind: "addition" | "change" | "removal";
  comparability: "comparable" | {
    incomparable: "unreadable" | "dir_there" |
      "blocked_by_dir" | "blocked_by_special_file";
  };
  group: Route | null;
  before: Side | null;
  after: Side | null;
  drifted: boolean;
};
```

## View

The view holds one named change list.

```ts
type View = {
  kind: "desired" | "drift" | "effective";
  changes: Change[];
};
```

## Bundle result

The bundle result holds the output path with the manifest.

```ts
type BundleResult = {
  output: string;
  manifest: Manifest;
};
```

## Check result

The check result holds the full diff with three views.

```ts
type CheckResult = {
  wanted: Manifest;
  recorded: Manifest;
  desired: View;
  drift: View;
  effective: View;
};
```

## Apply result

The apply result holds write counts with per path outcomes.

```ts
type AppliedChange = {
  path: string;
  outcome: "added" | "changed" | "removed" |
    "failed";
};
type ApplyResult = {
  added: number;
  changed: number;
  removed: number;
  changes: AppliedChange[];
  saved: number;
};
```

## Restore result

The restore result holds write counts for one package with per path outcomes.

```ts
type RestoredChange = {
  path: string;
  outcome: "added" | "changed" | "removed" |
    "failed";
};
type RestoreResult = {
  added: number;
  changed: number;
  removed: number;
  changes: RestoredChange[];
  saved: number;
};
```

## Delete result

The delete result holds the deleted name with the cleanup count.

```ts
type DeleteResult = {
  name: string;
  pruned: number;
};
```

## Init result

The init result holds one scaffold item with its path and count.

```ts
type InitItem = {
  stage: string;
  path: string;
  written: number;
};
type InitResult = InitItem | InitItem[];
```
