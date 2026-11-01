//! Loads the project config.

use std::path::{Path, PathBuf};

use confit_core::error::{
    ContextualCause, ContextualResult, DocumentError, FilesystemError, ProjectError,
    TerminalResult, contextual, error, kind::ErrorKind,
};
use serde::{Deserialize, Serialize};

/// The project file name.
pub const FILE: &str = "project.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Project {
    /// The scaffolded confit version.
    pub version: String,
    /// The plugin tree config.
    pub plugins: PluginConf,
    /// The stubs folder config.
    pub stubs: StubConf,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            plugins: PluginConf::default(),
            stubs: StubConf::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PluginConf {
    /// The plugin tree folder, project relative unless absolute.
    pub folder: PathBuf,
    /// Whether the folder sits outside the project root.
    pub allow_external_folders: bool,
}

impl Default for PluginConf {
    fn default() -> Self {
        Self {
            folder: PathBuf::from("plugins"),
            allow_external_folders: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StubConf {
    /// The stubs folder, project relative unless absolute.
    pub folder: PathBuf,
}

impl Default for StubConf {
    fn default() -> Self {
        Self {
            folder: PathBuf::from("stubs"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRoot {
    /// The canonical plugin folder.
    pub dir: PathBuf,
}

impl Project {
    /// Loads one project file.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the file
    ///   is unreadable or malformed.
    pub fn read(root: &Path) -> TerminalResult<Self> {
        let path = root.join(FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(fault) => {
                return error(
                    ErrorKind::LoadProject,
                    ContextualCause::from(fault).with_fs_path(&path),
                );
            }
        };
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(fault) => {
                return error(
                    ErrorKind::LoadProject,
                    ContextualCause::from(fault).with_fs_path(&path),
                );
            }
        };
        Self::parse(&text, &path)
    }

    /// Parses one project text.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the text
    ///   is malformed.
    pub fn parse(text: &str, path: &Path) -> TerminalResult<Self> {
        match toml::from_str(text) {
            Ok(found) => Ok(found),
            Err(fault) => error(
                ErrorKind::LoadProject,
                contextual(ProjectError::InvalidConfig)
                    .with_fs_path(path)
                    .with_detail(fault.to_string()),
            ),
        }
    }

    /// Renders the project as TOML.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::RenderFailed`] when the
    ///   project fails to render.
    pub fn render(&self) -> ContextualResult<String> {
        toml::to_string_pretty(self)
            .map_err(|_| contextual(DocumentError::RenderFailed).with_fs_path(PathBuf::from(FILE)))
    }

    /// Resolves the plugin root.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the root
    ///   or the folder is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the folder
    ///   sits outside the project root.
    pub fn plugin_root(&self, root: &Path) -> TerminalResult<PluginRoot> {
        let joined = root.join(&self.plugins.folder);
        let root_real = match std::fs::canonicalize(root) {
            Ok(found) => found,
            Err(fault) => {
                return error(
                    ErrorKind::LoadProject,
                    ContextualCause::from(fault).with_fs_path(root.join(FILE)),
                );
            }
        };
        let dir = match std::fs::canonicalize(&joined) {
            Ok(dir) => dir,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => joined,
            Err(fault) => {
                return error(
                    ErrorKind::LoadProject,
                    ContextualCause::from(fault).with_fs_path(root.join(FILE)),
                );
            }
        };
        if !dir.starts_with(&root_real) && !self.plugins.allow_external_folders {
            return error(
                ErrorKind::Evaluate,
                contextual(FilesystemError::OutsideRoot)
                    .with_project(root)
                    .with_scope(&dir),
            );
        }
        Ok(PluginRoot { dir })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_fields_take_their_defaults() {
        let project = Project::parse("", Path::new("project.toml")).unwrap();
        assert_eq!(project.plugins.folder, PathBuf::from("plugins"));
        assert_eq!(project.stubs.folder, PathBuf::from("stubs"));
        assert!(!project.plugins.allow_external_folders);
    }

    #[test]
    fn project_parse_rejects_unknown_fields() {
        let result = Project::parse("[plugins]\nbogus = true", Path::new("project.toml"));
        assert!(
            result.is_err(),
            "the project parser rejects an unknown field"
        );
    }

    #[test]
    fn project_parse_accepts_explicit_folders() {
        let text = "[plugins]\nfolder = \"../shared\"\nallow_external_folders = true\n[stubs]\nfolder = \"types\"";
        let project = Project::parse(text, Path::new("project.toml")).unwrap();
        assert_eq!(project.plugins.folder, PathBuf::from("../shared"));
        assert_eq!(project.stubs.folder, PathBuf::from("types"));
        assert!(project.plugins.allow_external_folders);
    }

    #[test]
    fn project_parse_rejects_malformed_text() {
        use confit_core::error::{ErrorCause, ProjectError};

        match Project::parse("[plugins", Path::new("some/project.toml")) {
            Ok(_) => panic!("the project parser accepts malformed text"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::ProjectError(ProjectError::InvalidConfig)
                    ),
                    "the cause shows the config: {error}"
                );
                match &error.context().fs_path {
                    Some(found) => assert_eq!(
                        found,
                        &Path::new("some/project.toml"),
                        "the fs slot shows the file: {error}"
                    ),
                    None => panic!("expected a recorded file path: {error}"),
                }
            }
        }
    }
}
