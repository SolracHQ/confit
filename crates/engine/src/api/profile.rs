//! Plain profile decode.

use lua_binding::Shape;

use super::config::Config;
use confit_model::document::Document;

/// One profile.
#[derive(Debug, Clone, Default, Shape)]
#[shape(table)]
pub struct Profile {
    /// Configs in profile order.
    #[shape(default)]
    pub configs: Vec<Config>,
    /// Documents in declaration order with destination paths.
    #[shape(default)]
    pub documents: Vec<Document>,
}

impl Profile {
    /// Build one profile.
    pub fn new(configs: Vec<Config>, documents: Vec<Document>) -> Self {
        Self { configs, documents }
    }
}
