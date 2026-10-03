//! Engine
//!
//! Lua profile evaluation into finished documents.

use std::path::Path;

use mlua::Lua;

use confit_model::document::Document;
use confit_model::hook::Hook;
use confit_store::Stores;

use crate::error::Result;

/// Finished evaluation.
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Finished documents in deterministic order.
    pub documents: Vec<Document>,
    /// Merged hooks in first-seen order.
    pub hooks: Vec<Hook>,
}

/// Profile evaluator.
pub struct Engine {
    /// The Lua instance.
    lua: Lua,
    /// Shared store access.
    stores: Stores,
}

impl Engine {
    /// Builds one engine from stores.
    pub fn new(stores: Stores) -> Self {
        Self {
            lua: Lua::new(),
            stores,
        }
    }

    /// Evaluates one profile file into finished documents and hooks.
    ///
    /// Declarations finish with filled hashes in destination
    /// display order.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::Surface`] for surface failures.
    /// - [`crate::error::EngineError::Model`] for unhashable documents.
    /// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
    /// - [`crate::error::EngineError::Read`] for unreadable files.
    /// - [`crate::error::EngineError::Utf8`] for non-UTF8 files.
    /// - [`crate::error::EngineError::Lua`] for Lua failures.
    pub fn evaluate(&self, profile: &Path) -> Result<Evaluation> {
        self.lua.set_app_data(self.stores.clone());
        crate::loader::install(&self.lua)?;
        let value = crate::loader::load_chunk(&self.lua, profile)?;
        let decoded = confit_surface_api::decode_profile(value)?;
        let documents = crate::assembly::assemble(decoded.documents)?;
        Ok(Evaluation {
            documents,
            hooks: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::fs::TestGuard;
    use confit_model::document::{Data, StructuredFormat};
    use confit_store::{StoreRoots, Stores};
    use std::path::PathBuf;

    const PAIR: &str = r#"
        local config = require("confit.config")
        local path = require("confit.path")
        local first = config("first")
        first:add_document(path.home("zebra"), { text = "z" })
        local second = config("second")
        second:add_document(path.home("apple"), { text = "a" })
        return { first, second }
    "#;

    const DUPLICATE: &str = r#"
        local config = require("confit.config")
        local path = require("confit.path")
        local mine = config("tools/bat")
        mine:add_document(path.home(".bashrc"), { text = "one" })
        mine:add_document(path.home(".bashrc"), { text = "two" })
        return mine
    "#;

    const STRUCTURED: &str = r#"
        local config = require("confit.config")
        local path = require("confit.path")
        local mine = config("tools/bat")
        mine:add_document(path.home("config.toml"), {
            format = "toml",
            data = { profile = { shell = "bash" } },
        })
        return mine
    "#;

    const PROJECT_TEXT: &str = r#"
        local config = require("confit.config")
        local path = require("confit.path")
        local resource = require("confit.resource")
        local handle = assert(resource("note.txt"))
        local mine = config("tools/bat")
        mine:add_document(path.home(".bashrc"), { text = assert(handle:text()) })
        return mine
    "#;

    fn test_engine(dir: &Path) -> Engine {
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                cache_base: dir.join("cache"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            sender,
        );
        Engine::new(stores)
    }

    fn project_root(dir: &Path) -> PathBuf {
        let root = dir.join("project");
        confit_driver::fs::create_dir_all(&root).unwrap();
        root
    }

    fn profile_with(dir: &Path, source: &str) -> (PathBuf, PathBuf) {
        let root = project_root(dir);
        let profile = root.join("profile.lua");
        confit_driver::fs::write(&profile, source.as_bytes()).unwrap();
        (root, profile)
    }

    fn text_of(document: &Document) -> &str {
        match &document.data {
            Data::Text { content, .. } => content,
            _ => panic!("expected a text document"),
        }
    }

    #[test]
    fn evaluates_text_in_destination_order() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let engine = test_engine(dir.path());
        let (_, profile) = profile_with(dir.path(), PAIR);
        match engine.evaluate(&profile) {
            Ok(found) => {
                assert_eq!(found.documents.len(), 2);
                assert_eq!(found.documents[0].destination.display(), "home:apple");
                assert_eq!(found.documents[1].destination.display(), "home:zebra");
                assert_eq!(text_of(&found.documents[0]), "a");
                assert_eq!(text_of(&found.documents[1]), "z");
                assert!(
                    !found.documents[0].data_hash.is_empty(),
                    "hashes fill past the edge"
                );
            }
            Err(error) => panic!("profile evaluates: {error}"),
        }
    }

    #[test]
    fn evaluates_structured_with_filled_hash() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let engine = test_engine(dir.path());
        let (_, profile) = profile_with(dir.path(), STRUCTURED);
        match engine.evaluate(&profile) {
            Ok(found) => {
                assert_eq!(found.documents.len(), 1);
                assert_eq!(found.documents[0].destination.display(), "home:config.toml");
                match &found.documents[0].data {
                    Data::Structured { format, data } => {
                        assert_eq!(*format, StructuredFormat::Toml);
                        match data.get("profile").and_then(|item| item.get("shell")) {
                            Some(shell) => assert_eq!(shell.as_str(), Some("bash")),
                            None => panic!("structured data carries the shell"),
                        }
                    }
                    _ => panic!("structured document evaluates"),
                }
                assert!(
                    !found.documents[0].data_hash.is_empty(),
                    "hashes fill past the edge"
                );
            }
            Err(error) => panic!("structured profile evaluates: {error}"),
        }
    }

    #[test]
    fn duplicate_destinations_fail() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let engine = test_engine(dir.path());
        let (_, profile) = profile_with(dir.path(), DUPLICATE);
        match engine.evaluate(&profile) {
            Ok(_) => panic!("duplicate destinations pass"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("home:.bashrc"),
                    "names the destination: {text}"
                );
            }
        }
    }

    #[test]
    fn root_rides_stores() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let engine = test_engine(dir.path());
        let root = project_root(dir.path());
        confit_driver::fs::write(&root.join("note.txt"), b"hello").unwrap();
        let elsewhere = dir.path().join("elsewhere");
        confit_driver::fs::create_dir_all(&elsewhere).unwrap();
        let profile = elsewhere.join("profile.lua");
        confit_driver::fs::write(&profile, PROJECT_TEXT.as_bytes()).unwrap();
        match engine.evaluate(&profile) {
            Ok(found) => {
                assert_eq!(found.documents.len(), 1);
                assert_eq!(text_of(&found.documents[0]), "hello");
            }
            Err(error) => panic!("stored root evaluates: {error}"),
        }
    }

    #[test]
    fn project_text_flows_into_documents() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let engine = test_engine(dir.path());
        let root = project_root(dir.path());
        confit_driver::fs::write(&root.join("note.txt"), b"hello").unwrap();
        let profile = root.join("profile.lua");
        confit_driver::fs::write(&profile, PROJECT_TEXT.as_bytes()).unwrap();
        match engine.evaluate(&profile) {
            Ok(found) => {
                assert_eq!(found.documents.len(), 1);
                assert_eq!(text_of(&found.documents[0]), "hello");
            }
            Err(error) => panic!("project text evaluates: {error}"),
        }
    }
}
