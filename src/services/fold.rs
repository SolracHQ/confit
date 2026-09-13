//! Fold
//!
//! Single driver folding pending contributions into plan artifacts: rc
//! per-shell fan-out plus path-grouped file fold under one winner rule.

use std::collections::BTreeMap;

use crate::binding::ProfileGraph;
use crate::error::Result;
use crate::model::state::artifact::Artifact;
use crate::model::state::artifact::ArtifactData;
use crate::model::state::artifact::ArtifactKind;
use crate::model::state::artifact::Table;
use crate::model::state::config::ConfigContribution;
use crate::model::state::rc::{InitEntry, RcData};
use crate::security::sha256_hex;
use crate::services::merge::{ArtifactWinners, RcWinners, merge_artifact, merge_rc, walk_leaves};

/// Folds profile contributions into plan artifacts.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding shells plus configs.
///
/// # Returns
///
/// Rc artifacts in shell order first, then file artifacts in contribution
/// order, with rc hashes filled and file hashes left empty for the caller.
///
/// # Errors
///
/// Fails with merge plus hashing plus template errors.
pub fn fold_profile(graph: &ProfileGraph) -> Result<Vec<Artifact>> {
    let mut artifacts = fold_rc_artifacts(graph)?;
    artifacts.extend(fold_file_artifacts(graph)?);
    Ok(artifacts)
}

/// Folds one rc artifact per declared shell.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding shells plus configs.
///
/// # Returns
///
/// Rc artifacts in shell order, holding hashed data.
///
/// # Errors
///
/// Fails with merge plus hashing plus template errors.
fn fold_rc_artifacts(graph: &ProfileGraph) -> Result<Vec<Artifact>> {
    let mut artifacts = Vec::with_capacity(graph.shells.len());
    for shell in &graph.shells {
        artifacts.push(fold_shell_rc(graph, shell)?);
    }
    Ok(artifacts)
}

/// Builds the rc artifact for a shell.
///
/// Owner names thread through the fold as merge-time locals so slots resolve
/// against the true base winner; the artifact carries data alone.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding configs.
/// * `shell` - shell name for artifact path.
///
/// # Returns
///
/// Rc artifact holding hashed data.
///
/// # Errors
///
/// Fails with merge plus hashing errors.
fn fold_shell_rc(graph: &ProfileGraph, shell: &str) -> Result<Artifact> {
    let mut overlays: Vec<(String, RcData)> = Vec::with_capacity(graph.configs.len());
    for config in &graph.configs {
        let mut overlay = config_overlay(config);
        materialize_inits(&mut overlay.init, shell)?;
        overlays.push((config.name.clone(), overlay));
    }
    let mut data = RcData::default();
    let mut owners = RcWinners::default();
    for (name, overlay) in &overlays {
        if overlay_is_empty(overlay) {
            continue;
        }
        let (merged, updated) = merge_rc(&data, &owners, overlay, name)?;
        data = merged;
        owners = updated;
    }
    let mut artifact = Artifact {
        kind: ArtifactKind::Rc,
        path: rc_path(shell),
        data: ArtifactData::Rc(data),
        data_hash: String::new(),
    };
    artifact.data_hash = sha256_hex(&artifact.data.to_bytes()?);
    Ok(artifact)
}

/// Derives rc data for a config contribution.
///
/// # Arguments
///
/// * `config` - config contribution holding rc entries.
///
/// # Returns
///
/// Rc data holding the config profile plus env plus aliases plus init.
fn config_overlay(config: &ConfigContribution) -> RcData {
    RcData {
        profile: config.profile.clone(),
        env: config.envs.clone(),
        aliases: config.aliases.clone(),
        init: config.inits.clone(),
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

/// Materializes init entries for one shell.
///
/// Renders every argv element and source path through minijinja with the
/// shell facts, so `{{shell}}` (see `confit.shell.SHELL`) lands as the
/// declared shell name. Entries without template syntax pass through
/// byte-identical.
///
/// # Arguments
///
/// * `inits` - init entries under materialization, mutated in place.
/// * `shell` - declared shell name feeding the facts.
///
/// # Errors
///
/// Template syntax failures yield plan errors.
fn materialize_inits(inits: &mut [InitEntry], shell: &str) -> Result<()> {
    let mut facts = Table::new();
    facts.insert(
        "shell".to_string(),
        serde_json::Value::String(shell.to_string()),
    );
    for entry in inits {
        match entry {
            InitEntry::Eval { argv, .. } | InitEntry::Cmd { argv, .. } => {
                for arg in argv {
                    *arg = render_init_template(arg, &facts)?;
                }
            }
            InitEntry::Source { path, .. } => {
                *path = render_init_template(path, &facts)?;
            }
        }
    }
    Ok(())
}

/// Renders one init string with the shell facts.
///
/// # Arguments
///
/// * `text` - the init string under materialization.
/// * `facts` - the shell facts feeding template slots.
///
/// # Returns
///
/// Materialized string.
///
/// # Errors
///
/// Template syntax failures yield plan errors.
fn render_init_template(text: &str, facts: &Table) -> Result<String> {
    super::render::render_str(text, facts, "render init: ")
}

/// Folds pending file artifacts from every config into the artifact list.
///
/// Winner state threads per path as merge-time locals so slots resolve against
/// the true base winner; artifacts carry data alone.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding configs.
///
/// # Returns
///
/// Folded artifacts in contribution order, hashes left empty for the caller.
///
/// # Errors
///
/// Fails with merge errors for kind mismatches on a shared path.
fn fold_file_artifacts(graph: &ProfileGraph) -> Result<Vec<Artifact>> {
    let mut artifacts: Vec<Artifact> = Vec::new();
    let mut winners: BTreeMap<String, ArtifactWinners> = BTreeMap::new();
    for config in &graph.configs {
        for pending in &config.artifacts {
            match artifacts
                .iter()
                .position(|existing| existing.path == pending.path)
            {
                Some(index) => {
                    let existing = artifacts.remove(index);
                    let state = winners.entry(pending.path.clone()).or_default();
                    let merged = merge_artifact(
                        existing,
                        pending.data.clone(),
                        &config.name,
                        pending.priority,
                        state,
                    )?;
                    artifacts.insert(index, merged);
                }
                None => {
                    let mut state = ArtifactWinners::default();
                    state
                        .contributors
                        .push((config.name.clone(), pending.priority));
                    let table = match &pending.data {
                        ArtifactData::Toml(table)
                        | ArtifactData::Json(table)
                        | ArtifactData::Yaml(table) => Some(table),
                        _ => None,
                    };
                    if let Some(table) = table {
                        for (key, value) in table {
                            insert_fold_winners(
                                value,
                                key,
                                &mut state.leaves,
                                &config.name,
                                pending.priority,
                            );
                        }
                    }
                    winners.insert(pending.path.clone(), state);
                    artifacts.push(Artifact {
                        kind: pending.kind,
                        path: pending.path.clone(),
                        data: pending.data.clone(),
                        data_hash: String::new(),
                    });
                }
            }
        }
    }
    Ok(artifacts)
}

/// Records winner pairs for fresh table leaves.
///
/// # Arguments
///
/// * `value` - the subtree under recording.
/// * `path` - the dotted location of the subtree.
/// * `winners` - the winner map under update.
/// * `name` - names the contributing config.
/// * `priority` - merge priority carried by the contributing artifact.
fn insert_fold_winners(
    value: &serde_json::Value,
    path: &str,
    winners: &mut BTreeMap<String, (String, u32)>,
    name: &str,
    priority: u32,
) {
    walk_leaves(value, path, &mut |leaf_path, _| {
        winners.insert(leaf_path.to_string(), (name.to_string(), priority));
    });
}
