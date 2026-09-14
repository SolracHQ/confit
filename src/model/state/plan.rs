//! Plan
//!
//! One versioned desired-state document per plan run.

use serde::{Deserialize, Serialize};

use super::document::Document;

/// Plan format version written by every plan run.
pub const PLAN_VERSION: u32 = 1;

/// Holds the versioned desired-state document written by `plan`.
///
/// `documents` hold canonical merged data; `created_at` holds an RFC3339 timestamp assigned by
/// the service, the single clock source.
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
    /// Holds merged documents in plan order.
    pub documents: Vec<Document>,
}
