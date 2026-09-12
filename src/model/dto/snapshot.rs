//! Snapshot
//!
//! Actual bytes behind each artifact path.

/// Disk state behind one artifact path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snapshot {
    /// Empty path; the artifact awaits creation.
    Absent,
    /// Bytes plus their hex SHA-256.
    Present {
        /// Raw disk bytes (link target text for links).
        bytes: Vec<u8>,
        /// Hex SHA-256 over `bytes`.
        hash: String,
    },
    /// Path with failing reads; carries the reason.
    Unreadable {
        /// Path plus IO reason, ready for stderr.
        reason: String,
    },
}
