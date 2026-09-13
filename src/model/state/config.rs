//! Config
//!
//! Named contribution bags built by `confit.config` for Lua.

use super::artifact::{ArtifactData, ArtifactKind};
use super::rc::{AliasEntry, EnvEntry, InitEntry, ProfileEntry};

/// Holds one file artifact pending fold into the plan.
///
/// Carries the merge key (`kind`, `path`) plus the payload (`data`); the plan service folds
/// pending entries into the artifact map keyed by `(kind, path)` with `merge_artifact`.
///
/// # Examples
///
/// ```rust
/// use confit::model::state::artifact::{ArtifactData, ArtifactKind};
/// use confit::model::state::config::PendingArtifact;
///
/// let pending = PendingArtifact { kind: ArtifactKind::File, path: "note.txt".into(), data: ArtifactData::File { content: "hi".into() }, priority: 0 };
/// assert_eq!(pending.path, "note.txt");
/// assert!(matches!(pending.kind, ArtifactKind::File));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingArtifact {
    /// Materialization kind, half of the `(kind, path)` merge key.
    pub kind: ArtifactKind,
    /// Destination path, half of the `(kind, path)` merge key.
    pub path: String,
    /// Merged payload for the plan service.
    pub data: ArtifactData,
    /// Merge priority for the artifact, defaulting to 0.
    pub priority: u32,
}

/// Accumulated per-config contribution built by `config:add_artifact`.
///
/// Each list keeps declaration order; every entry and artifact carries its own
/// merge priority for deterministic slot resolution.
///
/// # Examples
///
/// ```rust
/// use confit::model::state::config::ConfigContribution;
///
/// let config = ConfigContribution { name: "bat".into(), ..ConfigContribution::default() };
/// assert_eq!(config.name, "bat");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigContribution {
    /// Config name from `confit.config(name)`, unique per evaluation.
    pub name: String,
    /// Alias entries in declaration order, guarded entries keeping their `when`.
    pub aliases: Vec<AliasEntry>,
    /// Env entries in declaration order, guarded entries keeping their `when`.
    pub envs: Vec<EnvEntry>,
    /// Profile entries in declaration order, `profile_path` dirs appended as `PATH` prepends.
    pub profile: Vec<ProfileEntry>,
    /// Init entries in declaration order, guarded entries keeping their `when`.
    pub inits: Vec<InitEntry>,
    /// Pending file artifacts in declaration order.
    pub artifacts: Vec<PendingArtifact>,
}
