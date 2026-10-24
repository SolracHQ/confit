//! Progress
//! Log facts for one run. Every layer reports. Only the CLI renders.

/// Unbounded channel sender for progress facts.
pub type ProgressSender = crossbeam_channel::Sender<Event>;

/// Fact for one run step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Fetch started for one URL.
    FetchStarted {
        /// Remote address under fetching.
        url: String,
    },
    /// Fetch reports in-flight bytes for one URL.
    FetchProgress {
        /// Remote address under fetching.
        url: String,
        /// Bytes under writing.
        bytes_so_far: u64,
        /// Body length under downloading.
        total: Option<u64>,
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
    /// Archive unpacked with total members.
    Unpacked {
        /// Archive path under unpacking.
        archive: String,
        /// Members seen in the archive.
        total: usize,
    },
    /// One document landed on disk.
    DocumentWritten {
        /// Destination path under writing.
        path: String,
        /// Finished write count including this document.
        done: usize,
        /// Documents under writing.
        total: usize,
    },
    /// One document left disk.
    DocumentRemoved {
        /// Destination path under removing.
        path: String,
        /// Finished removal count including this document.
        done: usize,
        /// Documents under scanning.
        total: usize,
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
    /// One pool entry finished compressing.
    PoolStored {
        /// Finished pool entry count including this entry.
        done: usize,
    },
    /// Promotion started with a known pool entry count.
    Promoting {
        /// Pool entry count under promoting.
        blobs: usize,
    },
}
