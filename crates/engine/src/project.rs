//! Project config from `project.toml` beside the profile.

use std::path::{Path, PathBuf};

use confit_core::error::{
    ContextualCause, ContextualResult, DocumentError, FilesystemError, ProjectError,
    TerminalResult, contextual, error, kind::ErrorKind,
};
use serde::{Deserialize, Serialize};

/// Project file name under the project root.
pub const FILE: &str = "project.toml";

/// Project config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Project {
    /// Scaffolded confit version.
    pub version: String,
    /// Plugin tree config.
    pub plugins: PluginConf,
    /// Stubs folder config.
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

/// Plugin tree config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PluginConf {
    /// Plugin tree folder, project relative unless absolute.
    pub folder: PathBuf,
    /// Whether the folder is outside the project root.
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

/// Stubs folder config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StubConf {
    /// Stubs folder, project relative unless absolute.
    pub folder: PathBuf,
}

impl Default for StubConf {
    fn default() -> Self {
        Self {
            folder: PathBuf::from("stubs"),
        }
    }
}

/// Resolved plugin tree root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRoot {
    /// Canonical plugin folder.
    pub dir: PathBuf,
}

impl Project {
    /// Read one project file with defaults for missing files.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable and malformed files.
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

    /// Parse one project text.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for malformed text.
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

    /// Render one project text.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::RenderFailed`] for unrenderable projects.
    pub fn render(&self) -> ContextualResult<String> {
        toml::to_string_pretty(self)
            .map_err(|_| contextual(DocumentError::RenderFailed).with_fs_path(PathBuf::from(FILE)))
    }

    /// Resolve the plugin tree against one project root.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable roots and outside folders.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unreadable roots and outside folders.
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
    fn missing_fields_read_defaults() {
        let project = Project::parse("", Path::new("project.toml")).unwrap();
        assert_eq!(project.plugins.folder, PathBuf::from("plugins"));
        assert_eq!(project.stubs.folder, PathBuf::from("stubs"));
        assert!(!project.plugins.allow_external_folders);
    }

    #[test]
    fn unknown_fields_fail() {
        let result = Project::parse("[plugins]\nbogus = true", Path::new("project.toml"));
        assert!(result.is_err(), "unknown fields fail");
    }

    #[test]
    fn explicit_folders_parse() {
        let text = "[plugins]\nfolder = \"../shared\"\nallow_external_folders = true\n[stubs]\nfolder = \"types\"";
        let project = Project::parse(text, Path::new("project.toml")).unwrap();
        assert_eq!(project.plugins.folder, PathBuf::from("../shared"));
        assert_eq!(project.stubs.folder, PathBuf::from("types"));
        assert!(project.plugins.allow_external_folders);
    }

    #[test]
    fn malformed_text_fails_showing_path() {
        use confit_core::error::{ErrorCause, ProjectError};

        match Project::parse("[plugins", Path::new("some/project.toml")) {
            Ok(_) => panic!("malformed text passes"),
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
                    None => panic!("the fs slot shows the file: {error}"),
                }
            }
        }
    }
}
