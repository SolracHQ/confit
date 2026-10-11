//! Surface api
//! Domain surface for the Lua surface.

pub mod api;
pub mod config;
pub mod document;
pub mod entries;
pub mod error;
pub mod fetch;
pub mod handles;
pub mod hook;
pub mod patch;
pub mod paths;
pub mod resource;
pub mod validate;
pub mod version;

pub use api::{Profile, builtin};
pub use config::Config;
pub use document::{
    Document, LinkDocument, Mode, OpaqueDocument, ParseMode, RcDocument, SerializableValue,
    StructuredDocument, TextDocument, Tree, TreeDocument, TreeSlot,
};
pub use error::{ApiError, Result};
pub use handles::{Archive, Handle};
pub use hook::HookFace;
pub use patch::{FilterArgs, Level, Patch, PatchTarget, RcPatch, StructuredPatch};
pub use paths::route_value;
pub use version::API_VERSION;
