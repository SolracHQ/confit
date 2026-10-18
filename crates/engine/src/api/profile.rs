//! Profiles
//! Plain profile decode into configs and documents.

use confit_binding_macros::Face;

use super::config::Config;
use confit_model::document::Document;

/// One profile.
#[derive(Debug, Clone, Default, Face)]
#[face(table)]
pub struct Profile {
    /// Configs in profile order.
    #[face(default)]
    pub configs: Vec<Config>,
    /// Documents in declaration order with destination paths.
    #[face(default)]
    pub documents: Vec<Document>,
}

impl Profile {
    /// Build one profile from configs and documents.
    pub fn new(configs: Vec<Config>, documents: Vec<Document>) -> Self {
        Self { configs, documents }
    }
}
