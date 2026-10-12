//! Profiles
//! Plain profile decode into configs and documents.

use std::collections::HashMap;

use confit_binding_macros::Face;

use super::config::{Config, decode_document_key};
use confit_model::document::Data;
use confit_model::routes::Route;

/// One profile.
#[derive(Debug, Clone, Default, Face)]
#[face(table)]
pub struct Profile {
    /// Configs in profile order.
    #[face(default)]
    pub configs: Vec<Config>,
    /// Documents keyed by destination route.
    #[face(default, key_convert = decode_document_key)]
    pub documents: HashMap<Route, Data>,
}

impl Profile {
    /// Build one profile from configs and documents.
    pub fn new(configs: Vec<Config>, documents: HashMap<Route, Data>) -> Self {
        Self { configs, documents }
    }
}
