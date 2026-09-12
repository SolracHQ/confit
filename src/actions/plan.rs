//! Plan
//!
//! Builds the desired-state plan from an evaluated profile.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::binding::ProfileGraph;
use crate::error::Result;
use crate::framework::{InitEntryShape, ToolContribution};
use crate::model::state::artifact::Artifact;
use crate::model::state::artifact::ArtifactData;
use crate::model::state::artifact::ArtifactKind;
use crate::model::state::artifact::BlameSet;
use crate::model::state::artifact::Contribution;
use crate::model::state::artifact::ShadowedSet;
use crate::model::state::artifact::Table;
use crate::model::state::plan::PLAN_VERSION;
use crate::model::state::plan::Plan;
use crate::model::state::rc::EnvEntry;
use crate::model::state::rc::InitEntry;
use crate::model::state::rc::PathOp;
use crate::model::state::rc::ProfileEntry;
use crate::model::state::rc::RcData;
use crate::security::sha256_hex;
use crate::services::merge::{merge_artifact, merge_rc, merge_toml};

/// Shared mise artifact path.
///
/// Every mise package from every tool folds into this one TOML artifact; the plan holds exactly
/// one mise artifact.
const MISE_PATH: &str = "~/.config/mise/config.toml";

/// Builds the desired state plan for a profile graph.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding shells plus tools.
/// * `root` - project root label recorded in the plan.
/// * `profile` - profile label recorded in the plan.
///
/// # Returns
///
/// Versioned plan holding hashed artifacts in contribution order.
///
/// # Errors
///
/// Fails with merge plus hashing errors.
///
/// # Examples
/// ```rust,no_run
/// use std::path::Path;
/// use confit::actions::plan;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::services::plan::evaluate_profile;
///
/// let graph = match evaluate_profile(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua")) {
///     Ok(graph) => graph,
///     Err(error) => panic!("fixture evaluates: {error}"),
/// };
/// let plan = match plan(&graph, "examples/0-basic_tool", "examples/0-basic_tool/profile.lua") {
///     Ok(plan) => plan,
///     Err(error) => panic!("plan builds: {error}"),
/// };
/// assert_eq!(plan.version, PLAN_VERSION);
/// ```
pub fn plan(graph: &ProfileGraph, root: &str, profile: &str) -> Result<Plan> {
    let mut order = 0u64;
    let (mise_table, mise_blame, mise_contributions) = fold_mise_table(graph, &mut order)?;
    let mut artifacts = Vec::new();
    for shell in &graph.shells {
        artifacts.push(fold_shell_rc(graph, shell, &mut order)?);
    }
    if !mise_contributions.is_empty() {
        artifacts.push(mise_artifact(mise_table, mise_blame, mise_contributions)?);
    }
    fold_appended(graph, &mut artifacts, &mut order)?;
    for artifact in &mut artifacts {
        if artifact.data_hash.is_empty() {
            artifact.data_hash = sha256_hex(&artifact.data.to_bytes()?);
        }
    }
    Ok(Plan {
        version: PLAN_VERSION,
        created_at: chrono::Utc::now().to_rfc3339(),
        root: root.to_string(),
        profile: profile.to_string(),
        artifacts,
    })
}

/// Folds tool mise specs into a shared table.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding tools.
/// * `order` - running contribution counter.
///
/// # Returns
///
/// Merged table plus blame plus contributions.
///
/// # Errors
///
/// Fails with merge errors.
fn fold_mise_table(
    graph: &ProfileGraph,
    order: &mut u64,
) -> Result<(Table, BTreeMap<String, String>, Vec<Contribution>)> {
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
                order: *order,
            });
            *order += 1;
        }
    }
    Ok((mise_table, mise_blame, mise_contributions))
}

/// Wraps the shared mise table as a TOML artifact.
///
/// # Arguments
///
/// * `mise_table` - merged mise data.
/// * `mise_blame` - blame map for merged keys.
/// * `mise_contributions` - contributions for the artifact.
///
/// # Returns
///
/// Mise artifact holding hashed data.
///
/// # Errors
///
/// Fails with hashing errors.
fn mise_artifact(
    mise_table: Table,
    mise_blame: BTreeMap<String, String>,
    mise_contributions: Vec<Contribution>,
) -> Result<Artifact> {
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
    artifact.data_hash = sha256_hex(&artifact.data.to_bytes()?);
    Ok(artifact)
}

/// Builds the rc artifact for a shell.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding tools.
/// * `shell` - shell name for artifact path.
/// * `order` - running contribution counter.
///
/// # Returns
///
/// Rc artifact holding hashed data.
///
/// # Errors
///
/// Fails with merge plus hashing errors.
fn fold_shell_rc(graph: &ProfileGraph, shell: &str, order: &mut u64) -> Result<Artifact> {
    let mut data = RcData::default();
    let mut shadowed = ShadowedSet::default();
    let mut blame = BlameSet::default();
    let mut contributions = Vec::new();
    for tool in &graph.tools {
        let overlay = tool_rc(tool, shell);
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
            order: *order,
        });
        *order += 1;
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
    artifact.data_hash = sha256_hex(&artifact.data.to_bytes()?);
    Ok(artifact)
}

/// Folds appended artifacts into the artifact list.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding pending artifacts.
/// * `artifacts` - artifact list receiving folded entries.
/// * `order` - running contribution counter.
///
/// # Errors
///
/// Fails with merge errors for kind mismatches on a shared path.
fn fold_appended(
    graph: &ProfileGraph,
    artifacts: &mut Vec<Artifact>,
    order: &mut u64,
) -> Result<()> {
    for tool in &graph.tools {
        for pending in &tool.artifacts {
            let incoming = Artifact {
                kind: pending.kind,
                path: pending.path.clone(),
                data: pending.data.clone(),
                contributions: vec![Contribution {
                    tool: tool.tool.clone(),
                    order: *order,
                }],
                shadowed: ShadowedSet::default(),
                blame: BlameSet::default(),
                data_hash: String::new(),
            };
            *order += 1;
            match artifacts
                .iter()
                .position(|existing| existing.path == incoming.path)
            {
                Some(index) => {
                    let existing = artifacts.remove(index);
                    let merged = merge_artifact(existing, incoming)?;
                    artifacts.insert(index, merged);
                }
                None => artifacts.push(incoming),
            }
        }
    }
    Ok(())
}

/// Derives the rc path for a shell name.
///
/// # Arguments
///
/// * `shell` - shell name.
///
/// # Returns
///
/// Rc path for the shell.
fn rc_path(shell: &str) -> String {
    match shell {
        "bash" => "~/.bashrc".to_string(),
        "zsh" => "~/.zshrc".to_string(),
        other => format!("~/.{other}rc"),
    }
}

/// Derives per-shell rc data for a tool contribution.
///
/// # Arguments
///
/// * `contribution` - tool contribution holding aliases plus envs plus profile entries plus inits.
/// * `shell` - shell name for shell specific init entries.
///
/// # Returns
///
/// Rc data for the shell.
fn tool_rc(contribution: &ToolContribution, shell: &str) -> RcData {
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
    let mut init = Vec::with_capacity(contribution.inits.len() + 1);
    if contribution.mise.is_some() {
        init.push(InitEntry::Eval {
            argv: vec![
                "mise".to_string(),
                "activate".to_string(),
                shell.to_string(),
            ],
        });
    }
    init.extend(contribution.inits.iter().map(|shape| match shape {
        InitEntryShape::Eval(argv) => InitEntry::Eval { argv: argv.clone() },
        InitEntryShape::Cmd(argv) => InitEntry::Cmd { argv: argv.clone() },
        InitEntryShape::Source(path) => InitEntry::Source { path: path.clone() },
    }));
    RcData {
        profile,
        env,
        aliases,
        init,
    }
}

/// Reports empty status for an rc overlay.
///
/// # Arguments
///
/// * `overlay` - rc data for inspection.
///
/// # Returns
///
/// True for empty overlays, false for overlays holding entries.
fn overlay_is_empty(overlay: &RcData) -> bool {
    overlay.profile.is_empty()
        && overlay.env.is_empty()
        && overlay.aliases.is_empty()
        && overlay.init.is_empty()
}
