//! Project config from `project.toml` beside the profile.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, Result};

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
    /// - [`EngineError::Read`] for unreadable files.
    /// - [`EngineError::Utf8`] for non-UTF8 files.
    /// - [`EngineError::Project`] for malformed files.
    pub fn read(root: &Path) -> Result<Self> {
        let path = root.join(FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(EngineError::Read {
                    path: path.display().to_string(),
                    reason: error.to_string(),
                });
            }
        };
        let text = String::from_utf8(bytes).map_err(|_| EngineError::Utf8 {
            path: path.display().to_string(),
        })?;
        Self::parse(&text, &path)
    }

    /// Parse one project text.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Project`] for malformed text.
    pub fn parse(text: &str, path: &Path) -> Result<Self> {
        toml::from_str(text).map_err(|error| EngineError::Project {
            path: path.display().to_string(),
            reason: error.to_string(),
        })
    }

    /// Render one project text.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Project`] for unrenderable projects.
    pub fn render(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|error| EngineError::Project {
            path: FILE.to_owned(),
            reason: error.to_string(),
        })
    }

    /// Resolve the plugin tree against one project root.
    ///
    /// Missing folders fall back lexical, so requires fail as unknown
    /// modules exactly as without config. Folders outside the root fail
    /// without the allow flag.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Project`] for outside folders without allow.
    /// - [`EngineError::Read`] for unreadable roots.
    pub fn plugin_root(&self, root: &Path) -> Result<PluginRoot> {
        let joined = root.join(&self.plugins.folder);
        let root_real = std::fs::canonicalize(root).map_err(|error| EngineError::Read {
            path: root.display().to_string(),
            reason: error.to_string(),
        })?;
        let dir = match std::fs::canonicalize(&joined) {
            Ok(dir) => dir,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => joined,
            Err(error) => {
                return Err(EngineError::Read {
                    path: joined.display().to_string(),
                    reason: error.to_string(),
                });
            }
        };
        if !dir.starts_with(&root_real) && !self.plugins.allow_external_folders {
            return Err(EngineError::Project {
                path: self.plugins.folder.display().to_string(),
                reason: format!("folder is outside '{}' without allow", root_real.display()),
            });
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
        let error = Project::parse("[plugins", Path::new("some/project.toml")).unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("some/project.toml"),
            "the text shows the path: {text}"
        );
    }

    #[test]
    fn render_round_trips() {
        let project = Project::default();
        let back = Project::parse(&project.render().unwrap(), Path::new("project.toml")).unwrap();
        assert_eq!(back, project);
    }
}
