//! Filesystem [`StateStore`] and [`PlanWriter`] backends.
//!
//! State files are always JSON on disk; TOML is export-only for plans.
//! Plan writes are atomic (temp file plus rename in the destination
//! directory).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::Plan;

use super::traits::{PlanFormat, PlanWriter, State, StateStore};

/// JSON state file reader.
///
/// Invariants: `None` resolves to the empty state directly; a `Some` path
/// naming an absent file also resolves to empty; a corrupt file produces
/// [`Error::Store`] naming the path.
#[derive(Debug, Clone, Default)]
pub struct FsStateStore {
    path: Option<PathBuf>,
}

impl FsStateStore {
    /// Reader for the state file at `path`; `None` selects the empty-state
    /// source.
    ///
    /// Args: `path` is the JSON state file location, if any.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }
}

impl StateStore for FsStateStore {
    /// Load and parse the JSON state file.
    fn load(&self) -> Result<State> {
        let Some(path) = self.path.as_ref() else {
            return Ok(State::empty());
        };
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(State::empty()),
            Err(e) => {
                return Err(Error::Store(format!(
                    "read state file '{}': {e}",
                    path.display()
                )));
            }
        };
        serde_json::from_str(&text)
            .map_err(|e| Error::Store(format!("parse state file '{}': {e}", path.display())))
    }
}

/// Atomic plan file writer.
///
/// Invariants: creates parent dirs as needed; publishes via temp file plus
/// rename in the destination directory, so readers observe fully published
/// plans; every IO failure produces [`Error::Store`] naming the path.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsPlanWriter;

impl FsPlanWriter {
    /// Stateless writer; all inputs arrive per [`write`](PlanWriter::write) call.
    pub fn new() -> Self {
        Self
    }
}

impl PlanWriter for FsPlanWriter {
    /// Serialize `plan` in `format` and publish it at `dest` atomically.
    ///
    /// Args: `plan` is the merged plan, `dest` the output path,
    /// `format` the export format.
    fn write(&self, plan: &Plan, dest: &Path, format: PlanFormat) -> Result<()> {
        let rendered = format.serialize(plan)?;
        if let Some(parent) = dest.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| {
                Error::Store(format!("create plan dir '{}': {e}", parent.display()))
            })?;
        }
        let tmp = PathBuf::from(format!("{}.tmp-{}", dest.display(), std::process::id()));
        let result: Result<()> = (|| {
            fs::write(&tmp, rendered.as_bytes())
                .map_err(|e| Error::Store(format!("write plan file '{}': {e}", tmp.display())))?;
            fs::rename(&tmp, dest).map_err(|e| {
                Error::Store(format!("publish plan file '{}': {e}", dest.display()))
            })?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use super::super::traits::StateEntry;
    use crate::model::{Artifact, ArtifactData, ArtifactKind, Contribution};

    fn sample_plan() -> Plan {
        Plan {
            version: 1,
            created_at: "2026-09-09T00:00:00Z".into(),
            root: "/home/tester/confit".into(),
            profile: "desktop".into(),
            artifacts: vec![Artifact {
                kind: ArtifactKind::File,
                path: "/home/tester/.bashrc".into(),
                data: ArtifactData::File {
                    content: "export X=1\n".into(),
                },
                contributions: vec![Contribution {
                    tool: "base".into(),
                    order: 0,
                }],
                shadowed: Default::default(),
                blame: Default::default(),
                data_hash: String::new(),
            }],
            hooks: vec!["mise install".into()],
        }
    }

    fn stray_tmp_files(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(".tmp-"))
            })
            .collect()
    }

    #[test]
    fn json_round_trip_through_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("nested").join("plan.json");
        let plan = sample_plan();
        FsPlanWriter::new()
            .write(&plan, &dest, PlanFormat::Json)
            .unwrap();
        let text = fs::read_to_string(&dest).unwrap();
        assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
        assert!(stray_tmp_files(&dir.path().join("nested")).is_empty());
    }

    #[test]
    fn toml_export_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("plan.toml");
        FsPlanWriter::new()
            .write(&sample_plan(), &dest, PlanFormat::Toml)
            .unwrap();
        let text = fs::read_to_string(&dest).unwrap();
        let value: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(value["profile"].as_str().unwrap(), "desktop");
    }

    #[test]
    fn missing_state_returns_empty() {
        assert!(FsStateStore::new(None).load().unwrap().artifacts.is_empty());
        let dir = tempfile::tempdir().unwrap();
        assert!(
            FsStateStore::new(Some(dir.path().join("nope.json")))
                .load()
                .unwrap()
                .artifacts
                .is_empty()
        );
    }

    #[test]
    fn corrupt_state_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(FsStateStore::new(Some(path)).load().is_err());
    }

    #[test]
    fn existing_state_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut artifacts = BTreeMap::new();
        artifacts.insert(
            "rc:/home/tester/.bashrc".into(),
            StateEntry {
                data_hash: "abc".into(),
                output_hash: "def".into(),
                data: None,
            },
        );
        fs::write(&path, serde_json::to_string(&State { artifacts }).unwrap()).unwrap();
        let loaded = FsStateStore::new(Some(path)).load().unwrap();
        assert_eq!(
            loaded.artifacts["rc:/home/tester/.bashrc"].output_hash,
            "def"
        );
    }
}
