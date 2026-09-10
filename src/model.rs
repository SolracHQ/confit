//! Defines the pure data model: conditions, shell rc entries, artifacts,
//! plans.
//!
//! Holds data types plus construction rules. Lua serves as a leaf adapter
//! building these types; rendering runs in `apply`, planning stays in
//! `plan`.

pub mod artifact;
pub mod condition;
pub mod plan;
pub mod rc;

pub use artifact::{
    AliasShadow, Artifact, ArtifactData, ArtifactKind, BlameSet, Contribution, Shadowed,
    ShadowedSet, Table,
};
pub use condition::{Condition, when_eq};
pub use plan::Plan;
pub use rc::{EnvEntry, InitEntry, PathOp, ProfileEntry, RcData};
