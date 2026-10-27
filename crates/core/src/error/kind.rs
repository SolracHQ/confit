//! Terminal error kinds with line data.

use std::path::PathBuf;

use super::cause::{EvalDetail, HookReason, SlotReason};

/// Terminal error kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Failed bundle read.
    ReadBundle {
        /// The bundle path under reading.
        path: PathBuf,
    },
    /// Failed bundle write.
    WriteBundle {
        /// The bundle path under writing.
        path: PathBuf,
    },
    /// Failed member unpack.
    UnpackMember {
        /// The member name under unpacking.
        member: String,
        /// The requiring document route, when the manifest names one.
        required_by: Option<String>,
    },
    /// Failed manifest render.
    RenderManifest,
    /// Repeated document.
    DuplicateDocument {
        /// The repeated document name.
        name: String,
    },
    /// Failed module open.
    OpenModule {
        /// The module name under opening.
        name: String,
    },
    /// Failed hook run.
    RunHook {
        /// The hook argv under running.
        argv: String,
        /// The hook failure reason.
        reason: HookReason,
    },
    /// Failed hook process spawn.
    SpawnHook {
        /// The hook argv under spawning.
        argv: String,
    },
    /// Failed Lua evaluation.
    LuaFailed {
        /// The Lua failure text.
        message: String,
    },
    /// Failed profile evaluation.
    Evaluate {
        /// The evaluation detail.
        detail: EvalDetail,
        /// The route under evaluation, when one names it.
        route: Option<String>,
    },
    /// Corrupt stored manifest.
    CorruptManifest {
        /// The manifest path under loading.
        path: PathBuf,
        /// The serde detail text.
        detail: String,
    },
    /// Unsupported manifest version.
    ManifestVersion {
        /// The manifest path under loading.
        path: PathBuf,
        /// The found version number.
        got: u64,
    },
    /// Manifest missing its version.
    ManifestNoVersion {
        /// The manifest path under loading.
        path: PathBuf,
    },
    /// Non named delete input.
    DeleteNonNamed {
        /// The offending input value.
        input: String,
    },
    /// Conflicting flags.
    FlagsConflict {
        /// The command under calling.
        command: String,
        /// The conflicting flags under calling.
        flags: Vec<String>,
    },
    /// Present target.
    OverwriteRefused {
        /// The present target path.
        path: PathBuf,
    },
    /// Missing project folder.
    MissingProject {
        /// The missing project path.
        path: PathBuf,
    },
    /// Missing profile file.
    MissingProfile {
        /// The missing profile path.
        path: PathBuf,
    },
    /// Failed project config load.
    LoadProject {
        /// The project file under loading.
        path: PathBuf,
    },
    /// Failed profile open.
    OpenProfile {
        /// The profile file under opening.
        path: PathBuf,
    },
    /// Failed slot delete.
    DeleteSlot {
        /// The rejected slot text.
        slot: String,
        /// The rejection reason.
        reason: SlotReason,
    },
    /// Failed slot apply.
    ApplySlot {
        /// The rejected slot text.
        slot: String,
        /// The rejection reason.
        reason: SlotReason,
    },
    /// Failed slot export.
    ExportSlot {
        /// The rejected slot text.
        slot: String,
        /// The rejection reason.
        reason: SlotReason,
    },
    /// Failed slot bundle.
    BundleSlot {
        /// The rejected slot text.
        slot: String,
        /// The rejection reason.
        reason: SlotReason,
    },
    /// Failed state delete.
    DeleteState,
    /// Failed state apply.
    ApplyState,
    /// Failed state plan.
    PlanState,
    /// Failed state export.
    ExportState,
    /// Failed history rotation.
    RotateHistory,
    /// Failed old blob cleanup.
    PruneBlobs,
    /// Failed state store.
    StoreState,
    /// Failed project init.
    InitProject,
    /// Unreadable prompt reply.
    ReadReply {
        /// The unreadable reply detail.
        detail: String,
    },
    /// Failed hook log write.
    WriteHookLog {
        /// The hook log path under writing.
        path: PathBuf,
    },
    /// Failed destination write.
    WriteDestination {
        /// The destination path under writing.
        path: PathBuf,
    },
    /// Failed destination removal.
    RemoveDestination {
        /// The destination path under removing.
        path: PathBuf,
    },
}
