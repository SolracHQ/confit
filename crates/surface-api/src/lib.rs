//! Surface api
//!
//! Domain surface over the face machinery.

pub mod api;
pub mod config;
pub mod declare;
pub mod definitions;
pub mod error;
pub mod fetch;
pub mod handles;
pub mod paths;
pub mod resource;
pub mod version;

pub use api::{builtin, decode_profile};
pub use declare::{Body, Declaration, Profile, SerializableValue, StructuredBody, TextBody};
pub use definitions::definitions;
pub use error::{ApiError, Result};
pub use handles::ResourceHandle;
pub use paths::PathHandle;
pub use version::API_VERSION;
