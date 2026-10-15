//! Configs
//! Plain config decode into names with documents and patches.

use std::collections::HashMap;

use mlua::{Lua, Value};

use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;

use super::patch::Patch;
use confit_model::document::Data;
use confit_model::hook::Hook;
use confit_model::routes::Route;

/// One decoded config.
#[derive(Debug, Clone, Default, Face)]
#[face(table)]
pub struct Config {
    /// The config name.
    pub name: String,
    /// Documents keyed by destination route.
    #[face(default, key_convert = decode_document_key)]
    pub documents: HashMap<Route, Data>,
    /// Patches in profile return order.
    #[face(default)]
    pub patches: Vec<Patch>,
    /// Hooks in profile return order.
    #[face(default)]
    pub hooks: Vec<Hook>,
    /// Requirements keyed by config name with hints.
    #[face(default)]
    pub requirements: HashMap<String, String>,
}

impl Config {
    /// Build one config from its name with documents, patches, hooks, and requirements.
    pub fn new(
        name: String,
        documents: HashMap<Route, Data>,
        patches: Vec<Patch>,
        hooks: Vec<Hook>,
        requirements: HashMap<String, String>,
    ) -> Self {
        Self {
            name,
            documents,
            patches,
            hooks,
            requirements,
        }
    }
}

/// Decode one documents key.
///
/// # Returns
///
/// - Return the destination route.
///
/// # Errors
///
/// - [`Mismatch`] for values outside route tables and relative paths.
pub(crate) fn decode_document_key(
    lua: &Lua,
    value: &Value,
) -> std::result::Result<Route, Mismatch> {
    Route::decode(value, lua).map_err(|error| Mismatch {
        path: "document destination".to_owned(),
        want: error.want,
        found: error.found,
    })
}
