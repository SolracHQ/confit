//! Project config from `project.toml` beside the profile.

use std::path::{Path, PathBuf};

use confit_core::error::{
    DocumentError, Error, ErrorContext, EvalDetail, Outcome, ProjectError, Require, Result,
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
    pub fn read(root: &Path) -> Outcome<Self> {
        let path = root.join(FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(Error::load_project(path, ErrorContext::from(error))),
        };
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => return Err(Error::load_project(path, ErrorContext::from(error))),
        };
        Self::parse(&text, &path)
    }

    /// Parse one project text.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for malformed text.
    pub fn parse(text: &str, path: &Path) -> Outcome<Self> {
        match toml::from_str(text) {
            Ok(found) => Ok(found),
            Err(error) => Err(Error::load_project(
                path.to_path_buf(),
                ErrorContext::from(ProjectError::InvalidConfig {
                    detail: error.to_string(),
                }),
            )),
        }
    }

    /// Render one project text.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::RenderFailed`] for unrenderable projects.
    pub fn render(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|error| {
            ErrorContext::from(DocumentError::RenderFailed {
                reason: error.to_string(),
            })
            .path(PathBuf::from(FILE))
        })
    }

    /// Resolve the plugin tree against one project root.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable roots and outside folders.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unreadable roots and outside folders.
    pub fn plugin_root(&self, root: &Path) -> Outcome<PluginRoot> {
        let joined = root.join(&self.plugins.folder);
        let root_real = match std::fs::canonicalize(root) {
            Ok(found) => found,
            Err(error) => {
                return Err(Error::load_project(
                    root.join(FILE),
                    ErrorContext::from(error),
                ));
            }
        };
        let dir = match std::fs::canonicalize(&joined) {
            Ok(dir) => dir,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => joined,
            Err(error) => {
                return Err(Error::load_project(
                    root.join(FILE),
                    ErrorContext::from(error),
                ));
            }
        };
        if !dir.starts_with(&root_real) && !self.plugins.allow_external_folders {
            return Err(Error::evaluate(
                EvalDetail::Parse {
                    input: self.plugins.folder.display().to_string(),
                    require: Require::ProjectFolder,
                },
                None,
            ));
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
        assert!(
            Project::parse("[plugins", Path::new("some/project.toml")).is_err(),
            "malformed text fails"
        );
    }
}
