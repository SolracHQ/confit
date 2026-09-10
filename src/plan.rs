//! Plan service: tool contributions become hashed artifacts.
//!
//! Layering: the CLI calls into this service; the service calls the pure
//! `model`/`merge`/`canonical` layers plus the `lua` leaf adapter. Plan
//! works on data; `apply` renders bytes (minijinja, shell codegen).

use std::collections::BTreeMap;

use serde_json::Value;

use crate::canonical::data_hash;
use crate::diff::{ArtifactDetail, ArtifactStatus, ChangeKind, EntryChange};
use crate::error::Result;
use crate::lua::{InitEntryShape, ProfileGraph, ToolContribution};
use crate::merge::{merge_rc, merge_toml};
use crate::model::artifact::BlameSet;
use crate::model::{
    Artifact, ArtifactData, ArtifactKind, Contribution, EnvEntry, InitEntry, PathOp, Plan,
    ProfileEntry, RcData, ShadowedSet, Table,
};
use crate::store::State;

/// Shared mise artifact path.
///
/// Invariants: every mise package from every tool folds into this one TOML
/// artifact; the plan holds exactly one mise artifact.
const MISE_PATH: &str = "~/.config/mise/config.toml";

/// Hook recorded once when the plan holds at least one mise entry.
///
/// Invariants: pushed at most once per plan, in first-seen order; the hook
/// set holds this entry.
const MISE_HOOK: &str = "mise install";

/// Per-artifact diff counts of a plan against the previous state.
///
/// Invariants: the three fields sum to the plan's artifact count; each
/// artifact contributes exactly one status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSummary {
    /// Artifacts absent from the previous state.
    pub create: usize,
    /// Artifacts whose `data_hash` differs from the previous state.
    pub update: usize,
    /// Artifacts whose `data_hash` matches the previous state.
    pub unchanged: usize,
}

/// Build the desired-state plan for a profile graph.
///
/// Groups each tool's contributions into artifacts: one rc artifact per
/// declared shell plus a single shared mise TOML artifact. Contributions fold
/// in profile tool order; `Contribution.order` is an incrementing `u64`
/// assigned per fold (mise folds in tool order first, then each shell's rc
/// folds in tool order). Each merged artifact gets a fresh `data_hash` via
/// `canonical::data_hash`.
///
/// Invariants: `version` is always 1; `created_at` is a fresh RFC3339
/// timestamp; `root`/`profile` echo the strings passed in; `hooks` holds
/// `"mise install"` at most once, iff at least one mise entry exists.
/// Paths stay literal; expansion belongs to a later layer. The `previous`
/// state stays reserved for later use: diffing runs separately via [`diff`],
/// and the filesystem snapshot belongs to a later layer.
///
/// Args: `graph` is the evaluated profile, `previous` the last apply record
/// (reserved for later use), `root` the project-root label, `profile` the
/// profile label.
///
/// Example:
/// ```rust,no_run
/// use std::path::Path;
/// use confit::lua::evaluate;
/// use confit::plan::orchestrate;
/// use confit::store::State;
///
/// let graph = evaluate(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua")).unwrap();
/// let plan = orchestrate(&graph, &State::empty(), "examples/0-basic_tool", "examples/0-basic_tool/profile.lua").unwrap();
/// assert_eq!(plan.version, 1);
/// ```
pub fn orchestrate(
    graph: &ProfileGraph,
    previous: &State,
    root: &str,
    profile: &str,
) -> Result<Plan> {
    // The previous state stays reserved: diffing runs separately via `diff()`.
    let _ = previous;
    let mut order: u64 = 0;

    let mut mise_table = Table::new();
    let mut mise_blame: BTreeMap<String, String> = BTreeMap::new();
    let mut mise_contributions = Vec::new();
    for tool in &graph.tools {
        if let Some(spec) = &tool.mise {
            let mut inner = serde_json::Map::new();
            inner.insert(spec.name.clone(), Value::String(spec.version.clone()));
            let mut overlay = Table::new();
            overlay.insert("tools".to_string(), Value::Object(inner));
            let (merged, blame) = merge_toml(&mise_table, &overlay, &mise_blame, &tool.tool)?;
            mise_table = merged;
            mise_blame = blame;
            mise_contributions.push(Contribution {
                tool: tool.tool.clone(),
                order,
            });
            order += 1;
        }
    }

    let mut artifacts = Vec::new();
    for shell in &graph.shells {
        let mut data = RcData::default();
        let mut shadowed = ShadowedSet::default();
        let mut blame = BlameSet::default();
        let mut contributions = Vec::new();
        for tool in &graph.tools {
            let overlay = tool_rc(tool);
            if overlay_is_empty(&overlay) {
                continue;
            }
            let (merged, produced, produced_blame) = merge_rc(&data, &blame, &overlay, &tool.tool)?;
            data = merged;
            blame = produced_blame;
            shadowed.env.extend(produced.env);
            shadowed.profile.extend(produced.profile);
            shadowed.init.extend(produced.init);
            shadowed.aliases.extend(produced.aliases);
            contributions.push(Contribution {
                tool: tool.tool.clone(),
                order,
            });
            order += 1;
        }
        let mut artifact = Artifact {
            kind: ArtifactKind::Rc,
            path: rc_path(shell),
            data: ArtifactData::Rc(data),
            contributions,
            shadowed,
            blame,
            data_hash: String::new(),
        };
        artifact.data_hash = data_hash(&artifact.data)?;
        artifacts.push(artifact);
    }

    let mut hooks = Vec::new();
    if !mise_contributions.is_empty() {
        let mut artifact = Artifact {
            kind: ArtifactKind::Toml,
            path: MISE_PATH.to_string(),
            data: ArtifactData::Toml(mise_table),
            contributions: mise_contributions,
            shadowed: ShadowedSet::default(),
            blame: BlameSet {
                toml: mise_blame,
                ..BlameSet::default()
            },
            data_hash: String::new(),
        };
        artifact.data_hash = data_hash(&artifact.data)?;
        artifacts.push(artifact);
        hooks.push(MISE_HOOK.to_string());
    }

    Ok(Plan {
        version: 1,
        created_at: chrono::Utc::now().to_rfc3339(),
        root: root.to_string(),
        profile: profile.to_string(),
        artifacts,
        hooks,
    })
}

/// Diff a plan against the previous state, counting per-artifact status.
///
/// Compares each plan artifact's `data_hash` to the previous [`State`] keyed
/// `"kind:path"` (lowercase kind, e.g. `"rc:~/.bashrc"`): absent means
/// create, differing hash means update, equal hash means unchanged.
///
/// Invariants: diffing compares `data_hash` values; `output_hash`
/// comparison belongs to the filesystem-snapshot layer. Args: `plan` is the
/// desired state, `previous` the last apply record.
///
/// Example:
/// ```rust,no_run
/// use std::path::Path;
/// use confit::lua::evaluate;
/// use confit::plan::{diff, orchestrate};
/// use confit::store::State;
///
/// let graph = evaluate(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua")).unwrap();
/// let plan = orchestrate(&graph, &State::empty(), "root", "profile").unwrap();
/// let summary = diff(&plan, &State::empty());
/// assert_eq!((summary.create, summary.update, summary.unchanged), (2, 0, 0));
/// ```
pub fn diff(plan: &Plan, previous: &State) -> DiffSummary {
    let mut summary = DiffSummary {
        create: 0,
        update: 0,
        unchanged: 0,
    };
    for artifact in &plan.artifacts {
        let key = format!("{}:{}", artifact.kind, artifact.path);
        match previous.artifacts.get(&key) {
            None => summary.create += 1,
            Some(entry) if entry.data_hash == artifact.data_hash => summary.unchanged += 1,
            Some(_) => summary.update += 1,
        }
    }
    summary
}

/// Render the terminal plan summary with entry-level transitions.
///
/// Emits one header per artifact (`<path>: <kind>[ ~ update] ← tool, tool`,
/// tools ordered by contribution order, `~ update` only for updates), then
/// entry lines for created/updated artifacts (`+` added, `~` changed as
/// `<from> → <to>`, `-` removed, four-space unchanged context), collapsing
/// unchanged artifacts to the header alone. Changed lines show the winner
/// tool (`(tool)`), expanding to `(WINNER wins over LOSER)` under
/// `show_conflicts` when shadow history names a loser. Rc env entries group
/// by name via [`env_winners`] with the conditional-override suffix; all
/// other entries render in detail order. Ends with the
/// `plan: N create, M update, K unchanged` line.
///
/// Invariants: headers list every artifact in plan order; unchanged artifacts
/// never emit entry lines; removed lines never carry tools; the final plan
/// line matches [`diff`] counts byte-for-byte.
///
/// Args: `plan` is the desired state, `counts` its diff counts, `details`
/// the entry diffs aligned by `"kind:path"` key, `show_conflicts` selects
/// winner-over-loser rendering for changed lines.
///
/// Example:
/// ```rust,no_run
/// use std::path::Path;
/// use confit::diff::detail;
/// use confit::lua::evaluate;
/// use confit::plan::{diff, orchestrate, summarize};
/// use confit::store::State;
///
/// let graph = evaluate(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua")).unwrap();
/// let plan = orchestrate(&graph, &State::empty(), "root", "profile").unwrap();
/// let counts = diff(&plan, &State::empty());
/// let details = detail(&plan, &State::empty());
/// let text = summarize(&plan, &counts, &details, false);
/// assert!(text.ends_with("plan: 2 create, 0 update, 0 unchanged"));
/// ```
pub fn summarize(
    plan: &Plan,
    counts: &DiffSummary,
    details: &[ArtifactDetail],
    show_conflicts: bool,
) -> String {
    let mut lines = Vec::new();
    for artifact in &plan.artifacts {
        let key = format!("{}:{}", artifact.kind, artifact.path);
        let detail = details.iter().find(|detail| detail.key == key);
        let status = detail.map(|detail| detail.status);
        lines.push(header_line(artifact, status));
        let Some(detail) = detail else {
            continue;
        };
        if detail.status == ArtifactStatus::Unchanged {
            continue;
        }
        if let ArtifactData::Rc(rc) = &artifact.data {
            lines.extend(rc_entry_lines(rc, &detail.entries, show_conflicts));
        } else {
            for entry in &detail.entries {
                lines.push(render_entry(entry, show_conflicts));
            }
        }
    }
    lines.push(format!(
        "plan: {} create, {} update, {} unchanged",
        counts.create, counts.update, counts.unchanged
    ));
    lines.join("\n")
}

/// Render one artifact header with contributing tools in order.
fn header_line(artifact: &Artifact, status: Option<ArtifactStatus>) -> String {
    let mut tools: Vec<(&u64, &str)> = artifact
        .contributions
        .iter()
        .map(|contribution| (&contribution.order, contribution.tool.as_str()))
        .collect();
    tools.sort();
    let names: Vec<&str> = tools.into_iter().map(|(_, tool)| tool).collect();
    let mut header = format!("{}: {}", artifact.path, artifact.kind);
    if status == Some(ArtifactStatus::Update) {
        header.push_str(" ~ update");
    }
    if !names.is_empty() {
        header.push_str(&format!(" ← {}", names.join(", ")));
    }
    header
}

/// Render one detail entry with its marker and winner attribution.
fn render_entry(entry: &EntryChange, show_conflicts: bool) -> String {
    match &entry.change {
        ChangeKind::Added { value } => match &entry.tool {
            Some(tool) => format!("  + {} = {value} ({tool})", entry.label),
            None => format!("  + {} = {value}", entry.label),
        },
        ChangeKind::Changed { from, to } => {
            let suffix = match (&entry.tool, &entry.over) {
                (Some(tool), Some(over)) if show_conflicts => {
                    format!(" ({tool} wins over {over})")
                }
                (Some(tool), _) => format!(" ({tool})"),
                (None, _) => String::new(),
            };
            format!("  ~ {} = {from} → {to}{suffix}", entry.label)
        }
        ChangeKind::Removed { value } => {
            format!("  - {} = {value}", entry.label)
        }
        ChangeKind::Unchanged { value } => match &entry.tool {
            Some(tool) => format!("    {} = {value} ({tool})", entry.label),
            None => format!("    {} = {value}", entry.label),
        },
    }
}

/// Render rc entry lines: grouped env plus direct sections.
///
/// Groups desired env entries by name (base display with override counts),
/// placing removed env lines with the group; aliases, profile, and init
/// render in detail order with other removed lines trailing.
fn rc_entry_lines(rc: &RcData, entries: &[EntryChange], show_conflicts: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let is_env_label = |label: &str| {
        !label.starts_with("alias ")
            && !label.starts_with("profile ")
            && !label.starts_with("init[")
            && !label.starts_with("vars.")
            && *label != *"src"
            && *label != *"content"
            && *label != *"target"
            && *label != *"source"
            && !label.contains('.')
            && !label.contains('[')
    };
    // Aliases desired, in detail order.
    for entry in entries {
        if entry.label.starts_with("alias ") && !matches!(entry.change, ChangeKind::Removed { .. })
        {
            lines.push(render_entry(entry, show_conflicts));
        }
    }
    // Grouped env desired.
    let env_entries: Vec<&EntryChange> = entries
        .iter()
        .filter(|entry| {
            is_env_label(&entry.label) && !matches!(entry.change, ChangeKind::Removed { .. })
        })
        .collect();
    for (name, value, conditional) in env_winners(rc.env.iter()) {
        let group: Vec<&&EntryChange> = env_entries
            .iter()
            .filter(|entry| entry.label == name)
            .collect();
        if group.is_empty() {
            continue;
        }
        lines.push(render_grouped_env(
            name,
            value,
            conditional,
            &group,
            show_conflicts,
        ));
    }
    // Removed env with the group.
    for entry in entries {
        if is_env_label(&entry.label) && matches!(entry.change, ChangeKind::Removed { .. }) {
            lines.push(render_entry(entry, show_conflicts));
        }
    }
    // Profile, init desired, in detail order.
    for entry in entries {
        if (entry.label.starts_with("profile ") || entry.label.starts_with("init["))
            && !matches!(entry.change, ChangeKind::Removed { .. })
        {
            lines.push(render_entry(entry, show_conflicts));
        }
    }
    // Other removed lines trailing.
    for entry in entries {
        if matches!(entry.change, ChangeKind::Removed { .. }) && !is_env_label(&entry.label) {
            lines.push(render_entry(entry, show_conflicts));
        }
    }
    lines
}

/// Render one grouped env line with override counts and markers.
fn render_grouped_env(
    name: &str,
    value: &str,
    conditional: usize,
    group: &[&&EntryChange],
    show_conflicts: bool,
) -> String {
    let overrides = if conditional == 0 {
        String::new()
    } else {
        format!(" (+ {conditional} conditional overrides)")
    };
    let changed = group.iter().find_map(|entry| match &entry.change {
        ChangeKind::Changed { from, to } => Some((
            from.clone(),
            to.clone(),
            entry.tool.clone(),
            entry.over.clone(),
        )),
        _ => None,
    });
    if let Some((from, to, tool, over)) = changed {
        let suffix = match (&tool, &over) {
            (Some(winner), Some(loser)) if show_conflicts => {
                format!("{overrides} ({winner} wins over {loser})")
            }
            (Some(winner), _) => format!("{overrides} ({winner})"),
            (None, _) => overrides,
        };
        return format!("  ~ {name} = {from} → {to}{suffix}");
    }
    if let Some(added) = group
        .iter()
        .find(|entry| matches!(entry.change, ChangeKind::Added { .. }))
    {
        let suffix = match &added.tool {
            Some(tool) => format!("{overrides} ({tool})"),
            None => overrides,
        };
        return format!("  + {name} = \"{value}\"{suffix}");
    }
    let tool = group
        .iter()
        .find_map(|entry| match &entry.change {
            ChangeKind::Unchanged { .. } => entry.tool.clone(),
            _ => None,
        })
        .or_else(|| group.first().and_then(|entry| entry.tool.clone()));
    let suffix = match &tool {
        Some(tool) => format!("{overrides} ({tool})"),
        None => overrides,
    };
    format!("    {name} = \"{value}\"{suffix}")
}

/// Derive the rc path for a shell name.
///
/// `bash` maps to `~/.bashrc`, `zsh` to `~/.zshrc`; any other name maps to
/// `~/.<shell>rc`. Paths stay literal; expansion belongs to a later layer.
fn rc_path(shell: &str) -> String {
    match shell {
        "bash" => "~/.bashrc".to_string(),
        "zsh" => "~/.zshrc".to_string(),
        other => format!("~/.{other}rc"),
    }
}

/// Map one tool contribution onto its per-shell rc data.
///
/// Aliases fill the map (later merges win key by key); envs append as
/// unconditional entries (the Lua surface produces unconditional entries);
/// `profile_entries` append as `Prepend` profile entries (the Lua surface
/// produces `Prepend` entries); `profile_paths` append as `PATH` `Prepend`
/// entries after `profile_entries` (the bindings keep the two lists
/// separate, so each list keeps its own declaration order); inits map
/// preserving order.
fn tool_rc(contribution: &ToolContribution) -> RcData {
    let mut aliases = BTreeMap::new();
    for (name, value) in &contribution.aliases {
        aliases.insert(name.clone(), value.clone());
    }
    let env = contribution
        .envs
        .iter()
        .map(|(name, value)| EnvEntry {
            name: name.clone(),
            value: value.clone(),
            when: None,
        })
        .collect();
    let mut profile =
        Vec::with_capacity(contribution.profile_entries.len() + contribution.profile_paths.len());
    for (name, value) in &contribution.profile_entries {
        profile.push(ProfileEntry {
            name: name.clone(),
            value: value.clone(),
            op: PathOp::Prepend,
            when: None,
        });
    }
    for dir in &contribution.profile_paths {
        profile.push(ProfileEntry {
            name: "PATH".to_string(),
            value: dir.clone(),
            op: PathOp::Prepend,
            when: None,
        });
    }
    let init = contribution
        .inits
        .iter()
        .map(|shape| match shape {
            InitEntryShape::Eval(argv) => InitEntry::Eval { argv: argv.clone() },
            InitEntryShape::Cmd(argv) => InitEntry::Cmd { argv: argv.clone() },
        })
        .collect();
    RcData {
        profile,
        env,
        aliases,
        init,
    }
}

/// True when every section of the rc overlay reads empty.
///
/// Tools with empty overlays record an empty contribution list on the shell
/// artifact: contributions track data flow, with empty overlays adding zero
/// entries.
fn overlay_is_empty(overlay: &RcData) -> bool {
    overlay.profile.is_empty()
        && overlay.env.is_empty()
        && overlay.aliases.is_empty()
        && overlay.init.is_empty()
}

/// Group env winners by name in first-seen order for display.
///
/// Returns one `(name, value, conditional_count)` tuple per distinct name:
/// the value prefers the unconditional entry when one exists, else the first
/// entry; the count tallies entries carrying a `when` guard.
fn env_winners<'a>(entries: impl Iterator<Item = &'a EnvEntry>) -> Vec<(&'a str, &'a str, usize)> {
    let mut groups: Vec<(&'a str, Vec<&'a EnvEntry>)> = Vec::new();
    for entry in entries {
        match groups.iter_mut().find(|(name, _)| *name == entry.name) {
            Some((_, members)) => members.push(entry),
            None => groups.push((entry.name.as_str(), vec![entry])),
        }
    }
    groups
        .into_iter()
        .map(|(name, members)| {
            let conditional = members.iter().filter(|entry| entry.when.is_some()).count();
            let display = members
                .iter()
                .find(|entry| entry.when.is_none())
                .or(members.first())
                .map(|entry| entry.value.as_str())
                .unwrap_or("");
            (name, display, conditional)
        })
        .collect()
}
