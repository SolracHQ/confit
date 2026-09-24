//! Store
//!
//! Persisted manifests behind slot and bundle backends.

pub mod manifest;

/// One slot kind selecting applied, named, or history bundles.
///
/// Applied holds the fixed state slot. Named holds one
/// `@name` slot without the sigil. History holds one `%N`
/// pick newest-first from one.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotKind {
    /// Holds the applied slot.
    Applied,
    /// Holds one named slot without the `@` sigil.
    Named(String),
    /// Holds one history pick newest-first from one.
    History(usize),
}
