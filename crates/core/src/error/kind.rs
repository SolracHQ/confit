//! Terminal error kinds for required operations.

/// The required operation that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// A bundle read that failed.
    ReadBundle,
    /// A bundle write that failed.
    WriteBundle,
    /// A member unpack that failed.
    UnpackMember,
    /// A manifest render that failed.
    RenderManifest,
    /// A document that appears twice.
    DuplicateDocument,
    /// A module open that failed.
    OpenModule,
    /// A hook run that failed.
    RunHook,
    /// A hook process spawn that failed.
    SpawnHook,
    /// A Lua evaluation that failed.
    LuaFailed,
    /// A profile evaluation that failed.
    Evaluate,
    /// A stored manifest that is corrupt.
    CorruptManifest,
    /// A manifest version that the binary rejects.
    ManifestVersion,
    /// A manifest with no version.
    ManifestNoVersion,
    /// A delete input with no name.
    DeleteNonNamed,
    /// Flags that conflict.
    FlagsConflict,
    /// A target that is already present.
    OverwriteRefused,
    /// A project folder that is absent.
    MissingProject,
    /// A profile file that is absent.
    MissingProfile,
    /// A project config load that failed.
    LoadProject,
    /// A profile open that failed.
    OpenProfile,
    /// A state delete that failed.
    DeleteState,
    /// A state apply that failed.
    ApplyState,
    /// A bundle-side state operation that failed.
    BundleState,
    /// A state export that failed.
    ExportState,
    /// A state restore that failed.
    RestoreState,
    /// A history rotation that failed.
    RotateHistory,
    /// An old blob cleanup that failed.
    PruneBlobs,
    /// A state store that failed.
    StoreState,
    /// A project init that failed.
    InitProject,
    /// A prompt reply that fails its read.
    ReadReply,
    /// A hook log write that failed.
    WriteHookLog,
    /// A destination write that failed.
    WriteDestination,
    /// A destination removal that failed.
    RemoveDestination,
}
