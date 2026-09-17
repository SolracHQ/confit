//! Progress
//!
//! Facts for one evaluation run.

use std::sync::Arc;

/// Shared sink for evaluation progress.
///
/// # Examples
///
/// ```rust
/// use confit_engine::ProgressEvent;
///
/// let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
/// let inner = seen.clone();
/// let sink: confit_engine::ProgressCallback = std::sync::Arc::new(move |event: ProgressEvent| {
///     match inner.lock() {
///         Ok(mut guard) => guard.push(event),
///         Err(poisoned) => poisoned.into_inner().push(event),
///     }
/// });
/// sink(ProgressEvent::Hashing);
/// assert!(matches!(seen.lock().map(|guard| guard.len()), Ok(1)));
/// ```
pub type ProgressCallback = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

/// Fact for one evaluation step.
///
/// # Examples
///
/// ```rust
/// use confit_engine::ProgressEvent;
///
/// let event = ProgressEvent::Hashing;
/// assert!(matches!(event, ProgressEvent::Hashing));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// Fetch started for one URL.
    FetchStarted {
        /// Remote address under fetching.
        url: String,
    },
    /// Fetch served from the sidecar cache.
    FetchCached {
        /// Remote address under fetching.
        url: String,
        /// Cached body size in bytes.
        bytes: usize,
    },
    /// Fetch downloaded fresh bytes.
    FetchDownloaded {
        /// Remote address under fetching.
        url: String,
        /// Fresh body size in bytes.
        bytes: usize,
    },
    /// Archive unpacked with kept over total members.
    Unpacked {
        /// Archive path under unpacking.
        archive: String,
        /// Members kept by the callback.
        kept: usize,
        /// Members seen in the archive.
        total: usize,
    },
    /// Patch callback finished for one target.
    PatchApplied {
        /// Contributing config name.
        owner: String,
        /// Target document path or rc.
        target: String,
    },
    /// Hash phase entered on the CLI side.
    Hashing,
    /// Plan file read entered on the CLI side.
    ReadingPlan {
        /// Plan file path under reading.
        path: String,
    },
    /// Plan payload serialization plus write entered.
    WritingPlan {
        /// Document count under serializing.
        documents: usize,
    },
    /// One document landed on disk.
    DocumentWritten {
        /// Destination path under writing.
        path: String,
    },
    /// One hook started on the CLI side.
    HookRunning {
        /// One-based hook position.
        position: usize,
        /// Total hooks under running.
        total: usize,
        /// Hook argv text under running.
        argv: String,
    },
}
