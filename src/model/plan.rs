//! Describes the planned desired state: one versioned document per `plan`
//! run.

use serde::{Deserialize, Serialize};

use super::artifact::Artifact;

/// Holds the versioned desired-state document written by `plan`.
///
/// Invariants: `artifacts` hold canonical merged data; `created_at` holds
/// an RFC3339 timestamp assigned by the service, the single clock source;
/// `hooks` hold deduplicated entries in first-seen order from the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Holds the plan format version.
    pub version: u32,
    /// Holds the RFC3339 creation timestamp, set by the service.
    pub created_at: String,
    /// Holds the confit project root the plan was built from.
    pub root: String,
    /// Holds the active profile name.
    pub profile: String,
    /// Holds merged artifacts in plan order.
    pub artifacts: Vec<Artifact>,
    /// Holds hooks running once each at apply time, in first-seen order.
    pub hooks: Vec<String>,
}
