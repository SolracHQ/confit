//! Plan
//!
//! Diff counts and disk warnings over desired and recorded state.
//! Identical inputs yield identical outputs so every path stays testable.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::binding::ProfileGraph;
use crate::error::{Error, Result};
use crate::model::dto::diff::DiffSummary;
use crate::model::dto::diff::PlanSummary;
use crate::model::dto::snapshot::Snapshot;
use crate::model::dto::warning::PlanWarning;
use crate::model::dto::warning::WarningKind;
use crate::model::state::State;
use crate::model::state::document::{Document, DocumentData, DocumentKind, Table};
use crate::model::state::plan::PLAN_VERSION;
use crate::model::state::plan::Plan;
use crate::model::state::rc::{InitEntry, InitSpec};
use crate::repository::Filesystem;
use crate::security::sha256_hex;
use crate::services::path::expand_tilde;

/// Builds the desired state plan for a profile graph.
///
/// Live execution already merged structured plus rc into `merged`.
/// Text plus link merge here with warnings. Slice 2 handles conversion.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding shells plus configs.
/// * `root` - project root label recorded in the plan.
/// * `profile` - profile label recorded in the plan.
/// * `strict` - true escalates text plus link conflicts to plan errors.
///
/// # Returns
///
/// Versioned plan holding hashed documents plus warnings.
///
/// # Errors
///
/// Fails with merge plus hashing plus strict errors.
///
/// # Examples
/// ```rust,no_run
/// use std::path::Path;
/// use confit::binding::evaluate;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::services::plan::build_plan;
///
/// let graph = match evaluate(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua"), None) {
///     Ok(graph) => graph,
///     Err(error) => panic!("fixture evaluates: {error}"),
/// };
/// let plan = match build_plan(&graph, "examples/0-basic_tool", "examples/0-basic_tool/profile.lua", false) {
///     Ok((plan, _)) => plan,
///     Err(error) => panic!("plan builds: {error}"),
/// };
/// assert_eq!(plan.version, PLAN_VERSION);
/// ```
pub fn build_plan(
    graph: &ProfileGraph,
    root: &str,
    profile: &str,
    strict: bool,
) -> Result<(Plan, Vec<PlanWarning>)> {
    check_file_kinds(graph)?;
    let mut warnings = Vec::new();
    let mut documents = fold_rc(graph)?;
    let mut files = fold_structured(graph);
    files.extend(fold_text_link(graph, strict, &mut warnings)?);
    files.sort_by(|left, right| left.path.cmp(&right.path));
    documents.extend(files);
    for document in &mut documents {
        if document.data_hash.is_empty() {
            document.data_hash = sha256_hex(&document.data.to_bytes()?);
        }
    }
    Ok((
        Plan {
            version: PLAN_VERSION,
            created_at: chrono::Utc::now().to_rfc3339(),
            root: root.to_string(),
            profile: profile.to_string(),
            documents,
        },
        warnings,
    ))
}

/// Rejects same-path declarations with mismatched file kinds.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding documents.
///
/// # Returns
///
/// Unit for consistent kinds.
///
/// # Errors
///
/// Kind mismatches yield merge errors naming the path.
fn check_file_kinds(graph: &ProfileGraph) -> Result<()> {
    let mut kinds: BTreeMap<&str, DocumentKind> = BTreeMap::new();
    for pending in &graph.documents {
        if matches!(pending.kind, DocumentKind::Rc) {
            continue;
        }
        match kinds.insert(pending.path.as_str(), pending.kind) {
            None => {}
            Some(first) if first == pending.kind => {}
            Some(_) => {
                return Err(Error::Merge(format!(
                    "cannot merge document at '{}': kind mismatch",
                    pending.path
                )));
            }
        }
    }
    for config in &graph.configs {
        for pending in &config.documents {
            if matches!(pending.kind, DocumentKind::Rc) {
                continue;
            }
            match kinds.insert(pending.path.as_str(), pending.kind) {
                None => {}
                Some(first) if first == pending.kind => {}
                Some(_) => {
                    return Err(Error::Merge(format!(
                        "cannot merge document at '{}': kind mismatch",
                        pending.path
                    )));
                }
            }
        }
    }
    for merged in &graph.merged {
        if matches!(merged.kind, DocumentKind::Rc) {
            continue;
        }
        match kinds.insert(merged.path.as_str(), merged.kind) {
            None => {}
            Some(first) if first == merged.kind => {}
            Some(_) => {
                return Err(Error::Merge(format!(
                    "cannot merge document at '{}': kind mismatch",
                    merged.path
                )));
            }
        }
    }
    Ok(())
}

/// Folds merged structured documents into output order.
///
/// Live execution already merged patches, so this clones finals.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding merged finals.
///
/// # Returns
///
/// Structured documents in path order.
fn fold_structured(graph: &ProfileGraph) -> Vec<Document> {
    let mut out: Vec<Document> = graph
        .merged
        .iter()
        .filter(|item| matches!(item.kind, DocumentKind::Structured))
        .cloned()
        .collect();
    out.sort_by(|left, right| left.path.cmp(&right.path));
    out
}

/// Folds one rc document per declared shell.
///
/// Merged rc holds unmaterialized inits, materialization runs per shell.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding shells plus merged rc.
///
/// # Returns
///
/// Rc documents in shell order.
///
/// # Errors
///
/// Template failures yield plan errors.
fn fold_rc(graph: &ProfileGraph) -> Result<Vec<Document>> {
    let base = graph
        .merged
        .iter()
        .find(|item| matches!(item.kind, DocumentKind::Rc) && item.path == "rc");
    let Some(base) = base else {
        return Ok(Vec::new());
    };
    let DocumentData::Rc(data) = &base.data else {
        return Ok(Vec::new());
    };
    let mut documents = Vec::with_capacity(graph.shells.len());
    for shell in &graph.shells {
        let mut cloned = data.clone();
        materialize_inits(&mut cloned.init, shell)?;
        let mut document = Document {
            kind: DocumentKind::Rc,
            path: rc_path(shell),
            data: DocumentData::Rc(cloned),
            data_hash: String::new(),
        };
        document.data_hash = sha256_hex(&document.data.to_bytes()?);
        documents.push(document);
    }
    Ok(documents)
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
/// Renders every argv element and source path with shell facts.
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
        match &mut entry.spec {
            InitSpec::Eval { argv, .. } | InitSpec::Cmd { argv, .. } => {
                for arg in argv {
                    *arg = render_init_template(arg, &facts)?;
                }
            }
            InitSpec::Source { path, .. } => {
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

/// Folds text plus link declarations into documents with warnings.
///
/// Same path with different bytes warns naming path plus owners. Strict
/// mode fails the same conflict as a plan error.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding documents.
/// * `strict` - true escalates conflicts to plan errors.
/// * `warnings` - warnings under extension, mutated in place.
///
/// # Returns
///
/// Text plus link documents in path order.
///
/// # Errors
///
/// Kind mismatches plus strict conflicts yield errors naming the path.
fn fold_text_link(
    graph: &ProfileGraph,
    strict: bool,
    warnings: &mut Vec<PlanWarning>,
) -> Result<Vec<Document>> {
    let mut grouped: BTreeMap<String, Vec<(String, DocumentKind, DocumentData)>> = BTreeMap::new();
    for pending in &graph.documents {
        match &pending.data {
            DocumentData::Text { .. } | DocumentData::Link { .. } => {
                grouped.entry(pending.path.clone()).or_default().push((
                    "profile".to_string(),
                    pending.kind,
                    pending.data.clone(),
                ));
            }
            _ => {}
        }
    }
    for config in &graph.configs {
        for pending in &config.documents {
            match &pending.data {
                DocumentData::Text { .. } | DocumentData::Link { .. } => {
                    grouped.entry(pending.path.clone()).or_default().push((
                        config.name.clone(),
                        pending.kind,
                        pending.data.clone(),
                    ));
                }
                _ => {}
            }
        }
    }
    let mut documents = Vec::new();
    for (path, entries) in grouped {
        let mut ordered = entries;
        ordered.sort_by(|left, right| left.0.cmp(&right.0));
        let first_kind = ordered[0].1;
        if ordered.iter().any(|(_, kind, _)| *kind != first_kind) {
            return Err(Error::Merge(format!(
                "cannot merge document at '{path}': kind mismatch"
            )));
        }
        let first_data = ordered[0].2.clone();
        let same = ordered.iter().all(|(_, _, data)| data == &first_data);
        if same {
            documents.push(Document {
                kind: first_kind,
                path: path.clone(),
                data: first_data,
                data_hash: String::new(),
            });
            continue;
        }
        let mut owners: Vec<String> = ordered.iter().map(|(owner, _, _)| owner.clone()).collect();
        owners.sort();
        owners.dedup();
        if strict {
            return Err(Error::Plan(format!(
                "strict: document '{path}' has conflicting declarations ({})",
                owners
                    .iter()
                    .map(|owner| format!("'{owner}'"))
                    .collect::<Vec<_>>()
                    .join(" plus ")
            )));
        }
        warnings.push(PlanWarning {
            path: path.clone(),
            kind: WarningKind::DeclarationConflict { owners },
        });
        documents.push(Document {
            kind: first_kind,
            path: path.clone(),
            data: first_data,
            data_hash: String::new(),
        });
    }
    Ok(documents)
}

/// Loads the previous apply record.
///
/// # Arguments
///
/// * `fs` - the filesystem backend.
/// * `state` - the state file path, holding `None` for an empty source.
///
/// # Returns
///
/// Previous state, empty while the path stays absent or unconfigured.
///
/// # Errors
///
/// Corrupt files yield store errors naming the path. Read failures yield store errors naming
/// the path.
pub fn load_state(fs: &dyn Filesystem, state: Option<&Path>) -> Result<State> {
    let Some(path) = state else {
        return Ok(State::empty());
    };
    let key = path.display().to_string();
    let bytes = match fs.read_bytes(&key) {
        Ok(None) => return Ok(State::empty()),
        Ok(Some(bytes)) => bytes,
        Err(error) => {
            let detail = match error {
                Error::Store(inner) => inner,
                error => error.to_string(),
            };
            return Err(Error::Store(format!(
                "read state file '{}': {detail}",
                path.display()
            )));
        }
    };
    serde_json::from_slice(&bytes)
        .map_err(|e| Error::Store(format!("parse state file '{}': {e}", path.display())))
}

/// Snapshots every document path in plan order.
///
/// # Arguments
///
/// * `fs` - the filesystem backend.
/// * `plan` - supplies kind plus path keys.
///
/// # Returns
///
/// Disk states keyed by `kind:path`, one entry per document in plan order. Unreadable paths
/// arrive as warning snapshots.
pub fn snapshot_current(fs: &dyn Filesystem, plan: &Plan) -> BTreeMap<String, Snapshot> {
    let mut out = BTreeMap::new();
    for document in &plan.documents {
        let key = document.key_string();
        let expanded = expand_tilde(&document.path);
        let expanded_key = expanded.display().to_string();
        let read = if document.kind == DocumentKind::Link {
            fs.read_link(&expanded_key)
        } else {
            fs.read_bytes(&expanded_key)
        };
        let snapshot = match read {
            Ok(None) => Snapshot::Absent,
            Ok(Some(bytes)) => {
                let hash = sha256_hex(&bytes);
                Snapshot::Present { bytes, hash }
            }
            Err(error) => {
                let detail = match error {
                    Error::Store(inner) => inner,
                    error => error.to_string(),
                };
                Snapshot::Unreadable {
                    reason: crate::presentation::unreadable_reason(&document.path, &detail),
                }
            }
        };
        out.insert(key, snapshot);
    }
    out
}

/// Writes pre-serialized plan bytes to the destination.
///
/// # Arguments
///
/// * `fs` - the destination backend.
/// * `bytes` - the serialized payload.
/// * `dest` - the output path.
///
/// # Returns
///
/// Unit on success.
///
/// # Errors
///
/// IO failures yield store errors naming the path.
pub fn write_plan(fs: &dyn Filesystem, bytes: &[u8], dest: &Path) -> Result<()> {
    fs.write_bytes_tmp(dest, bytes)
}

/// Diffs a plan against the previous state.
///
/// # Arguments
///
/// * `plan` - the desired state.
/// * `previous` - the last apply record keyed by `kind:path`.
///
/// # Returns
///
/// Lifecycle counts per document status.
///
/// # Examples
/// ```rust
/// use confit::model::state::document::Document;
/// use confit::model::state::document::DocumentData;
/// use confit::model::state::document::DocumentKind;
/// use confit::model::state::plan::Plan;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::model::state::State;
/// use confit::services::plan::diff;
///
/// fn file_document(path: &str) -> Document {
///     Document {
///         kind: DocumentKind::Text,
///         path: path.into(),
///         data: DocumentData::Text { content: "hi".into() },
///         data_hash: "hash".into(),
///     }
/// }
/// let plan = Plan { version: PLAN_VERSION, created_at: String::new(), root: String::new(), profile: String::new(), documents: vec![file_document("a"), file_document("b")] };
/// let summary = diff(&plan, &State::empty());
/// assert_eq!(summary.create, 2);
/// ```
pub fn diff(plan: &Plan, previous: &State) -> DiffSummary {
    let mut summary = DiffSummary {
        create: 0,
        update: 0,
        delete: 0,
    };
    let mut planned = BTreeSet::new();
    for document in &plan.documents {
        let key = document.key_string();
        planned.insert(key.clone());
        match previous.documents.get(&key) {
            None => summary.create += 1,
            Some(entry) if entry.data_hash != document.data_hash => summary.update += 1,
            Some(_) => {}
        }
    }
    for key in previous.documents.keys() {
        if !planned.contains(key) {
            summary.delete += 1;
        }
    }
    summary
}

/// Collects disk warnings from record gated disk comparison.
///
/// # Arguments
///
/// * `plan` - the desired state.
/// * `previous` - the last apply record.
/// * `snapshots` - the disk states keyed by `kind:path`.
/// * `rendered` - the desired bytes keyed by `kind:path`.
///
/// # Returns
///
/// Filesystem warnings in plan order.
pub fn disk_warnings(
    plan: &Plan,
    previous: &State,
    snapshots: &BTreeMap<String, Snapshot>,
    rendered: &BTreeMap<String, Vec<u8>>,
) -> Vec<PlanWarning> {
    let mut warnings = Vec::new();
    for document in &plan.documents {
        let key = document.key_string();
        let Some(desired) = rendered.get(&key) else {
            continue;
        };
        let rendered_hash = sha256_hex(desired);
        let record = previous.documents.get(&key);
        let snapshot = snapshots.get(&key);
        match record {
            None => match snapshot {
                None | Some(Snapshot::Absent) => {}
                Some(Snapshot::Present { .. }) => warnings.push(PlanWarning {
                    path: document.path.clone(),
                    kind: WarningKind::OverwriteUntracked,
                }),
                Some(Snapshot::Unreadable { reason }) => warnings.push(PlanWarning {
                    path: document.path.clone(),
                    kind: WarningKind::Unreadable {
                        reason: reason.clone(),
                    },
                }),
            },
            Some(entry) if entry.data_hash != document.data_hash => {}
            Some(_) => match snapshot {
                None | Some(Snapshot::Absent) => {}
                Some(Snapshot::Present { hash, .. }) if *hash == rendered_hash => {}
                Some(Snapshot::Present { .. }) => warnings.push(PlanWarning {
                    path: document.path.clone(),
                    kind: WarningKind::ManualModification,
                }),
                Some(Snapshot::Unreadable { reason }) => warnings.push(PlanWarning {
                    path: document.path.clone(),
                    kind: WarningKind::Unreadable {
                        reason: reason.clone(),
                    },
                }),
            },
        }
    }
    warnings
}

/// Shapes diff counts for presentation.
///
/// # Arguments
///
/// * `counts` - the two-way diff counts.
///
/// # Returns
///
/// Plan summary carrying create, update, and delete counts.
pub fn summarize(counts: &DiffSummary) -> PlanSummary {
    PlanSummary {
        create: counts.create,
        update: counts.update,
        delete: counts.delete,
    }
}
