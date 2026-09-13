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
use crate::model::state::artifact::ArtifactKind;
use crate::model::state::plan::PLAN_VERSION;
use crate::model::state::plan::Plan;
use crate::repository::Filesystem;
use crate::security::sha256_hex;
use crate::services::fold::fold_profile;
use crate::services::path::expand_tilde;

/// Builds the desired state plan for a profile graph.
///
/// # Arguments
///
/// * `graph` - evaluated profile holding shells plus configs.
/// * `root` - project root label recorded in the plan.
/// * `profile` - profile label recorded in the plan.
///
/// # Returns
///
/// Versioned plan holding hashed artifacts in fold order.
///
/// # Errors
///
/// Fails with merge plus hashing errors.
///
/// # Examples
/// ```rust,no_run
/// use std::path::Path;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::services::plan::build_plan;
/// use confit::services::plan::evaluate_profile;
///
/// let graph = match evaluate_profile(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua")) {
///     Ok(graph) => graph,
///     Err(error) => panic!("fixture evaluates: {error}"),
/// };
/// let plan = match build_plan(&graph, "examples/0-basic_tool", "examples/0-basic_tool/profile.lua") {
///     Ok(plan) => plan,
///     Err(error) => panic!("plan builds: {error}"),
/// };
/// assert_eq!(plan.version, PLAN_VERSION);
/// ```
pub fn build_plan(graph: &ProfileGraph, root: &str, profile: &str) -> Result<Plan> {
    let mut artifacts = fold_profile(graph)?;
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

/// Evaluates the profile file into a graph.
///
/// # Arguments
///
/// * `root` - the project root for module resolution.
/// * `profile` - the profile file path.
///
/// # Returns
///
/// Declared shells plus config contributions in profile order.
///
/// # Errors
///
/// Script failures surface as Lua errors, malformed tables as config errors, missing files as
/// IO errors.
pub fn evaluate_profile(root: &Path, profile: &Path) -> Result<ProfileGraph> {
    crate::binding::evaluate(root, profile)
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

/// Snapshots every artifact path in plan order.
///
/// # Arguments
///
/// * `fs` - the filesystem backend.
/// * `plan` - supplies kind plus path keys.
///
/// # Returns
///
/// Disk states keyed by `kind:path`, one entry per artifact in plan order. Unreadable paths
/// arrive as warning snapshots.
pub fn snapshot_current(fs: &dyn Filesystem, plan: &Plan) -> BTreeMap<String, Snapshot> {
    let mut out = BTreeMap::new();
    for artifact in &plan.artifacts {
        let key = artifact.key_string();
        let expanded = expand_tilde(&artifact.path);
        let expanded_key = expanded.display().to_string();
        let read = if artifact.kind == ArtifactKind::Link {
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
                    reason: crate::presentation::unreadable_reason(&artifact.path, &detail),
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
/// Lifecycle counts per artifact status.
///
/// # Examples
/// ```rust
/// use confit::model::state::artifact::Artifact;
/// use confit::model::state::artifact::ArtifactData;
/// use confit::model::state::artifact::ArtifactKind;
/// use confit::model::state::plan::Plan;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::model::state::State;
/// use confit::services::plan::diff;
///
/// fn file_artifact(path: &str) -> Artifact {
///     Artifact {
///         kind: ArtifactKind::File,
///         path: path.into(),
///         data: ArtifactData::File { content: "hi".into() },
///         data_hash: "hash".into(),
///     }
/// }
/// let plan = Plan { version: PLAN_VERSION, created_at: String::new(), root: String::new(), profile: String::new(), artifacts: vec![file_artifact("a"), file_artifact("b")] };
/// let summary = diff(&plan, &State::empty());
/// assert_eq!((summary.create, summary.update, summary.unchanged), (2, 0, 0));
/// ```
pub fn diff(plan: &Plan, previous: &State) -> DiffSummary {
    let mut summary = DiffSummary {
        create: 0,
        update: 0,
        unchanged: 0,
        delete: 0,
    };
    let mut planned = BTreeSet::new();
    for artifact in &plan.artifacts {
        let key = artifact.key_string();
        planned.insert(key.clone());
        match previous.artifacts.get(&key) {
            None => summary.create += 1,
            Some(entry) if entry.data_hash == artifact.data_hash => summary.unchanged += 1,
            Some(_) => summary.update += 1,
        }
    }
    for key in previous.artifacts.keys() {
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
    for artifact in &plan.artifacts {
        let key = artifact.key_string();
        let Some(desired) = rendered.get(&key) else {
            continue;
        };
        let rendered_hash = sha256_hex(desired);
        let record = previous.artifacts.get(&key);
        let snapshot = snapshots.get(&key);
        match record {
            None => match snapshot {
                None | Some(Snapshot::Absent) => {}
                Some(Snapshot::Present { .. }) => warnings.push(PlanWarning {
                    path: artifact.path.clone(),
                    kind: WarningKind::OverwriteUntracked,
                }),
                Some(Snapshot::Unreadable { reason }) => warnings.push(PlanWarning {
                    path: artifact.path.clone(),
                    kind: WarningKind::Unreadable {
                        reason: reason.clone(),
                    },
                }),
            },
            Some(entry) if entry.data_hash != artifact.data_hash => {}
            Some(_) => match snapshot {
                None | Some(Snapshot::Absent) => {}
                Some(Snapshot::Present { hash, .. }) if *hash == rendered_hash => {}
                Some(Snapshot::Present { .. }) => warnings.push(PlanWarning {
                    path: artifact.path.clone(),
                    kind: WarningKind::ManualModification,
                }),
                Some(Snapshot::Unreadable { reason }) => warnings.push(PlanWarning {
                    path: artifact.path.clone(),
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

#[cfg(test)]
mod three_way_tests {
    use super::*;
    use std::cell::RefCell;

    use crate::model::state::StateEntry;
    use crate::model::state::artifact::Artifact;
    use crate::model::state::artifact::ArtifactData;
    use crate::model::state::artifact::ArtifactKind;
    use crate::model::state::artifact::Table;
    use crate::model::state::plan::PLAN_VERSION;
    use crate::model::state::rc::EnvEntry;
    use crate::repository::MemoryFilesystem;
    use crate::security::sha256_hex;
    use crate::services::render::{rc, render_baseline};

    const PATH: &str = "demo.toml";
    const KEY: &str = "toml:demo.toml";

    fn table_a() -> Table {
        [("a".to_string(), serde_json::json!(1))]
            .into_iter()
            .collect()
    }

    fn toml_artifact() -> Artifact {
        let data = ArtifactData::Toml(table_a());
        let hash = sha256_hex(&data.to_bytes().unwrap());
        Artifact {
            kind: ArtifactKind::Toml,
            path: PATH.into(),
            data,
            data_hash: hash,
        }
    }

    fn plan_with(artifacts: Vec<Artifact>) -> Plan {
        Plan {
            version: PLAN_VERSION,
            created_at: "2026-09-09T00:00:00Z".into(),
            root: "root".into(),
            profile: "profile".into(),
            artifacts,
        }
    }

    fn state_with(entries: Vec<(String, String)>) -> State {
        State {
            artifacts: entries
                .into_iter()
                .map(|(key, data_hash)| {
                    (
                        key,
                        StateEntry {
                            data_hash,
                            output_hash: String::new(),
                            data: None,
                        },
                    )
                })
                .collect(),
        }
    }

    fn present(bytes: &[u8]) -> Snapshot {
        Snapshot::Present {
            bytes: bytes.to_vec(),
            hash: sha256_hex(bytes),
        }
    }

    fn plan_previous(artifact: Artifact, record: Option<String>) -> (Plan, State) {
        let plan = plan_with(vec![artifact]);
        let previous = state_with(
            record
                .map(|data_hash| (KEY.to_string(), data_hash))
                .into_iter()
                .collect(),
        );
        (plan, previous)
    }

    fn snapshots_for(snapshot: Option<Snapshot>) -> BTreeMap<String, Snapshot> {
        snapshot
            .map(|snapshot| [(KEY.to_string(), snapshot)].into_iter().collect())
            .unwrap_or_default()
    }

    fn rendered_for(plan: &Plan) -> BTreeMap<String, Vec<u8>> {
        let files = MemoryFilesystem::default();
        render_baseline(plan, Path::new("root"), &files).expect("renders")
    }

    fn warnings_for(
        artifact: Artifact,
        record: Option<String>,
        snapshot: Option<Snapshot>,
    ) -> Vec<PlanWarning> {
        let (plan, previous) = plan_previous(artifact, record);
        let snapshots = snapshots_for(snapshot);
        let rendered = rendered_for(&plan);
        disk_warnings(&plan, &previous, &snapshots, &rendered)
    }

    #[test]
    fn no_record_absent_is_plain_create() {
        let (plan, previous) = plan_previous(toml_artifact(), None);
        let counts = diff(&plan, &previous);
        assert_eq!(
            (
                counts.create,
                counts.update,
                counts.unchanged,
                counts.delete
            ),
            (1, 0, 0, 0)
        );
        let warnings = warnings_for(toml_artifact(), None, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn no_record_present_warns_overwrite_untracked() {
        let (plan, previous) = plan_previous(toml_artifact(), None);
        let counts = diff(&plan, &previous);
        assert_eq!(
            (
                counts.create,
                counts.update,
                counts.unchanged,
                counts.delete
            ),
            (1, 0, 0, 0)
        );
        let warnings = warnings_for(toml_artifact(), None, Some(present(b"a = 9\n")));
        assert_eq!(warnings.len(), 1);
        let warning = &warnings[0];
        assert_eq!(warning.path, PATH);
        assert_eq!(warning.kind, WarningKind::OverwriteUntracked);
    }

    #[test]
    fn no_record_unreadable_warns_with_reason() {
        let reason = "cannot read 'demo.toml': denied".to_string();
        let (plan, previous) = plan_previous(toml_artifact(), None);
        let counts = diff(&plan, &previous);
        assert_eq!(
            (
                counts.create,
                counts.update,
                counts.unchanged,
                counts.delete
            ),
            (1, 0, 0, 0)
        );
        let warnings = warnings_for(
            toml_artifact(),
            None,
            Some(Snapshot::Unreadable {
                reason: reason.clone(),
            }),
        );
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].path, PATH);
        assert_eq!(
            warnings[0].kind,
            WarningKind::Unreadable {
                reason: reason.clone()
            }
        );
    }

    #[test]
    fn differing_record_is_update_whatever_disk_holds() {
        for snapshot in [
            None,
            Some(Snapshot::Absent),
            Some(present(b"a = 1\n")),
            Some(Snapshot::Unreadable {
                reason: "cannot read 'demo.toml': denied".into(),
            }),
        ] {
            let (plan, previous) = plan_previous(toml_artifact(), Some("stale".into()));
            let counts = diff(&plan, &previous);
            assert_eq!(
                (
                    counts.create,
                    counts.update,
                    counts.unchanged,
                    counts.delete
                ),
                (0, 1, 0, 0)
            );
            let warnings = warnings_for(toml_artifact(), Some("stale".into()), snapshot);
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn matching_record_absent_is_unchanged() {
        let hash = toml_artifact().data_hash.clone();
        let (plan, previous) = plan_previous(toml_artifact(), Some(hash.clone()));
        let counts = diff(&plan, &previous);
        assert_eq!(
            (
                counts.create,
                counts.update,
                counts.unchanged,
                counts.delete
            ),
            (0, 0, 1, 0)
        );
        let warnings = warnings_for(toml_artifact(), Some(hash), None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn matching_record_clean_disk_is_unchanged() {
        let artifact = toml_artifact();
        let (plan, previous) = plan_previous(artifact.clone(), Some(artifact.data_hash.clone()));
        let counts = diff(&plan, &previous);
        assert_eq!(
            (
                counts.create,
                counts.update,
                counts.unchanged,
                counts.delete
            ),
            (0, 0, 1, 0)
        );
        let rendered = rendered_for(&plan);
        let snapshots = [(KEY.to_string(), present(&rendered[KEY]))]
            .into_iter()
            .collect();
        let warnings = disk_warnings(&plan, &previous, &snapshots, &rendered);
        assert!(warnings.is_empty());
    }

    #[test]
    fn matching_record_edited_disk_is_update_with_manual_warning() {
        let artifact = toml_artifact();
        let (plan, previous) = plan_previous(artifact.clone(), Some(artifact.data_hash.clone()));
        let counts = diff(&plan, &previous);
        assert_eq!(
            (
                counts.create,
                counts.update,
                counts.unchanged,
                counts.delete
            ),
            (0, 0, 1, 0)
        );
        let warnings = warnings_for(
            artifact.clone(),
            Some(artifact.data_hash),
            Some(present(b"a = 9\n")),
        );
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].path, PATH);
        assert_eq!(warnings[0].kind, WarningKind::ManualModification);
    }

    #[test]
    fn matching_record_unreadable_is_unchanged_with_warning() {
        let artifact = toml_artifact();
        let reason = "cannot read 'demo.toml': denied".to_string();
        let (plan, previous) = plan_previous(artifact.clone(), Some(artifact.data_hash.clone()));
        let counts = diff(&plan, &previous);
        assert_eq!(
            (
                counts.create,
                counts.update,
                counts.unchanged,
                counts.delete
            ),
            (0, 0, 1, 0)
        );
        let warnings = warnings_for(
            artifact.clone(),
            Some(artifact.data_hash),
            Some(Snapshot::Unreadable {
                reason: reason.clone(),
            }),
        );
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].kind,
            WarningKind::Unreadable {
                reason: reason.clone()
            }
        );
    }

    #[test]
    fn previous_only_keys_count_as_delete() {
        let plan = plan_with(vec![toml_artifact()]);
        let mut previous = state_with(vec![(KEY.to_string(), toml_artifact().data_hash)]);
        previous.artifacts.insert(
            "toml:gone.toml".to_string(),
            StateEntry {
                data_hash: "old".into(),
                output_hash: String::new(),
                data: None,
            },
        );
        let counts = diff(&plan, &previous);
        assert_eq!(counts.delete, 1);
        assert_eq!((counts.create, counts.update, counts.unchanged), (0, 0, 1));
    }

    #[test]
    fn missing_snapshot_reads_absent() {
        let artifact = toml_artifact();
        let plan = plan_with(vec![artifact.clone()]);
        let previous = state_with(vec![(KEY.to_string(), artifact.data_hash.clone())]);
        let snapshots = BTreeMap::new();
        let counts = diff(&plan, &previous);
        assert_eq!((counts.update, counts.unchanged), (0, 1));
        let rendered = rendered_for(&plan);
        let warnings = disk_warnings(&plan, &previous, &snapshots, &rendered);
        assert!(warnings.is_empty());
    }

    #[test]
    fn render_failure_returns_err() {
        let data = ArtifactData::Template {
            src: "{{ unclosed".into(),
            vars: Table::new(),
        };
        let artifact = Artifact {
            kind: ArtifactKind::Template,
            path: "app.conf".into(),
            data_hash: sha256_hex(&data.to_bytes().unwrap()),
            data,
        };
        let plan = plan_with(vec![artifact]);
        let files = MemoryFilesystem::default();
        let error = render_baseline(&plan, Path::new("root"), &files).unwrap_err();
        assert!(matches!(error, crate::error::Error::Plan(_)), "{error}");
    }

    #[test]
    fn rc_disk_compares_against_rendered_markers() {
        let data = crate::model::state::rc::RcData {
            profile: Vec::new(),
            env: vec![EnvEntry {
                name: "A".into(),
                value: "1".into(),
                when: None,
                priority: 0,
            }],
            aliases: Vec::new(),
            init: Vec::new(),
        };
        let artifact = Artifact {
            kind: ArtifactKind::Rc,
            path: "~/.bashrc".into(),
            data_hash: sha256_hex(&ArtifactData::Rc(data.clone()).to_bytes().unwrap()),
            data: ArtifactData::Rc(data.clone()),
        };
        let plan = plan_with(vec![artifact.clone()]);
        let previous = state_with(vec![(
            "rc:~/.bashrc".to_string(),
            artifact.data_hash.clone(),
        )]);
        let rendered_bytes = rc::render_rc(&data).into_bytes();
        assert!(String::from_utf8_lossy(&rendered_bytes).contains("export A=1"));
        let files = MemoryFilesystem::default();
        let rendered = render_baseline(&plan, Path::new("root"), &files).expect("renders");
        assert_eq!(rendered["rc:~/.bashrc"], rendered_bytes);
        let counts = diff(&plan, &previous);
        assert_eq!((counts.update, counts.unchanged), (0, 1));
        let snapshots: BTreeMap<String, Snapshot> =
            [("rc:~/.bashrc".to_string(), present(&rendered_bytes))]
                .into_iter()
                .collect();
        let clean = disk_warnings(&plan, &previous, &snapshots, &rendered);
        assert!(clean.is_empty());

        let edited = b"# edited bashrc\n".to_vec();
        assert!(
            String::from_utf8_lossy(&edited).contains("edited"),
            "edited bytes differ"
        );
        let edited_snapshots: BTreeMap<String, Snapshot> =
            [("rc:~/.bashrc".to_string(), present(&edited))]
                .into_iter()
                .collect();
        let warned = disk_warnings(&plan, &previous, &edited_snapshots, &rendered);
        assert_eq!(warned.len(), 1);
        assert_eq!(warned[0].kind, WarningKind::ManualModification);
    }

    #[test]
    fn snapshot_current_keys_by_kind_path() {
        let plan = plan_with(vec![toml_artifact()]);
        let files = MemoryFilesystem {
            files: RefCell::new([(PATH.to_string(), b"a = 1\n".to_vec())].into()),
            failures: RefCell::new(BTreeMap::new()),
        };
        let snapshots = snapshot_current(&files, &plan);
        assert_eq!(snapshots.len(), 1);
        assert!(snapshots.contains_key(KEY));
        assert!(matches!(snapshots[KEY], Snapshot::Present { .. }));
    }

    #[test]
    fn summarize_shapes_counts_for_presentation() {
        let counts = DiffSummary {
            create: 1,
            update: 0,
            unchanged: 0,
            delete: 0,
        };
        let shaped = summarize(&counts);
        assert_eq!((shaped.create, shaped.update, shaped.delete), (1, 0, 0));
        let counts = DiffSummary {
            delete: 2,
            ..counts
        };
        let shaped = summarize(&counts);
        assert_eq!((shaped.create, shaped.update, shaped.delete), (1, 0, 2));
    }
}

#[cfg(test)]
mod serialize_tests {
    use super::*;
    use crate::model::state::artifact::Artifact;
    use crate::model::state::artifact::ArtifactData;
    use crate::model::state::artifact::ArtifactKind;
    use crate::model::state::plan::PLAN_VERSION;

    fn sample_plan() -> Plan {
        Plan {
            version: PLAN_VERSION,
            created_at: "2026-09-09T00:00:00Z".into(),
            root: "/home/tester/confit".into(),
            profile: "desktop".into(),
            artifacts: vec![Artifact {
                kind: ArtifactKind::File,
                path: "/home/tester/.bashrc".into(),
                data: ArtifactData::File {
                    content: "export X=1\n".into(),
                },
                data_hash: String::new(),
            }],
        }
    }

    #[test]
    fn json_serialize_round_trips() {
        let plan = sample_plan();
        let text = serde_json::to_string_pretty(&plan).unwrap();
        assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
    }
}
