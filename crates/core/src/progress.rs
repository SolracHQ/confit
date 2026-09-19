//! Progress
//!
//! Log facts for one run. Every layer reports. Only the CLI renders.

/// Unbounded channel sender for progress facts.
///
/// # Examples
///
/// ```rust
/// use confit_core::progress::Event;
///
/// let (sender, receiver) = crossbeam_channel::unbounded::<Event>();
/// sender.send(Event::Hashing);
/// assert!(matches!(receiver.try_recv(), Ok(Event::Hashing)));
/// ```
pub type ProgressSender = crossbeam_channel::Sender<Event>;

/// Fact for one run step.
///
/// # Examples
///
/// ```rust
/// use confit_core::progress::Event;
///
/// let event = Event::Hashing;
/// assert!(matches!(event, Event::Hashing));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
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
        /// Finished patch count including this patch.
        done: usize,
        /// Total patches under running.
        total: usize,
    },
    /// Patch run started with a known total.
    PatchesStarted {
        /// Patch count under running.
        patches: usize,
    },
    /// Hash phase entered.
    Hashing,
    /// Plan file read entered.
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
    /// One hook started.
    HookRunning {
        /// One-based hook position.
        position: usize,
        /// Total hooks under running.
        total: usize,
        /// Hook argv text under running.
        argv: String,
    },
    /// Compression started with known totals.
    CompressStarted {
        /// Blob count under compressing.
        blobs: usize,
        /// Total raw bytes under compressing.
        bytes: u64,
    },
    /// One blob finished compressing.
    BlobCompressed {
        /// Finished blob count including this blob.
        done: usize,
        /// Total blobs under compressing.
        total: usize,
        /// Raw bytes finished including this blob.
        bytes: u64,
    },
}
