//! Terminal error kinds naming required operations.
//!
//! Kinds name the operation. Causes name the failed result and context
//! names where the operation ran.

/// Terminal error kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Failed bundle read.
    ReadBundle,
    /// Failed bundle write.
    WriteBundle,
    /// Failed member unpack.
    UnpackMember,
    /// Failed manifest render.
    RenderManifest,
    /// Repeated document.
    DuplicateDocument,
    /// Failed module open.
    OpenModule,
    /// Failed hook run.
    RunHook,
    /// Failed hook process spawn.
    SpawnHook,
    /// Failed Lua evaluation.
    LuaFailed,
    /// Failed profile evaluation.
    Evaluate,
    /// Corrupt stored manifest.
    CorruptManifest,
    /// Unsupported manifest version.
    ManifestVersion,
    /// Manifest missing its version.
    ManifestNoVersion,
    /// Non named delete input.
    DeleteNonNamed,
    /// Conflicting flags.
    FlagsConflict,
    /// Present target.
    OverwriteRefused,
    /// Missing project folder.
    MissingProject,
    /// Missing profile file.
    MissingProfile,
    /// Failed project config load.
    LoadProject,
    /// Failed profile open.
    OpenProfile,
    /// Failed slot delete.
    DeleteSlot,
    /// Failed slot apply.
    ApplySlot,
    /// Failed slot export.
    ExportSlot,
    /// Failed slot bundle.
    BundleSlot,
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
    ReadReply,
    /// Failed hook log write.
    WriteHookLog,
    /// Failed destination write.
    WriteDestination,
    /// Failed destination removal.
    RemoveDestination,
}
