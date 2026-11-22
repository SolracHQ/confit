//! Scaffolds a project from current shapes and embedded resources.

use std::path::{Path, PathBuf};

use confit_core::error::{
    ContextualCause, DocumentError, Error, ErrorCause, EvalError, FilesystemError, Terminal,
    TerminalResult, WithContext, contextual, error, kind::ErrorKind,
};
use confit_driver as driver;
use confit_engine::project::{FILE as PROJECT_FILE, Project};
use confit_engine::stubs::generate_stubs;

use crate::cli::{DEFAULT_PROFILE, InitArgs};

const PROFILE_TEXT: &str = include_str!("../../resources/main.lua");

/// The editor config file that the execution writes at the project root.
const LSP_FILE: &str = ".luarc.json";

/// The editor config template that maps the plugin and stubs folders.
const LSP_TEXT: &str = include_str!("../../resources/luarc.json");

// Core plugins.
include!(concat!(env!("OUT_DIR"), "/plugin_files.rs"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// The output path.
    pub path: PathBuf,
    /// The number of files the execution wrote.
    pub written: usize,
}

/// One init outcome.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InitItem {
    /// The stage label.
    pub stage: String,
    /// The written path text.
    pub path: String,
    /// The written file count.
    pub written: usize,
}

pub struct InitRunner<'a> {
    /// The flags that choose the folder and the artifacts to refresh.
    pub args: &'a InitArgs,
}

impl InitRunner<'_> {
    /// Runs the scaffold, or refreshes each flagged artifact.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::OverwriteRefused`] when a scaffold file is present.
    /// - [`confit_core::error::kind::ErrorKind::InitProject`] when a scaffold write fails.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the project read fails.
    pub fn execute(self) -> TerminalResult<Vec<InitItem>> {
        let root = self.args.dir.clone().unwrap_or_else(|| PathBuf::from("."));
        let project = Project::read(&root)?;
        if !self.args.stubs && !self.args.plugins && !self.args.lsp {
            let report = scaffold(&root, &project)?;
            return Ok(vec![InitItem {
                stage: "init".to_owned(),
                path: report.path.display().to_string(),
                written: report.written,
            }]);
        }
        let mut items = Vec::new();
        if self.args.stubs {
            let report = write_stubs(&root.join(&project.stubs.folder))?;
            items.push(InitItem {
                stage: "stubs".to_owned(),
                path: report.path.display().to_string(),
                written: report.written,
            });
        }
        if self.args.plugins {
            let report = write_plugins(&root, &project.plugins.folder)?;
            items.push(InitItem {
                stage: "plugins".to_owned(),
                path: report.path.display().to_string(),
                written: report.written,
            });
        }
        if self.args.lsp {
            let report = write_lsp(&root, &project)?;
            items.push(InitItem {
                stage: "lsp".to_owned(),
                path: report.path.display().to_string(),
                written: report.written,
            });
        }
        Ok(items)
    }
}

/// Writes the profile, the current stubs, the core plugins, the project
/// file, and the editor config.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::OverwriteRefused`] when a scaffold file is present.
/// - [`confit_core::error::kind::ErrorKind::InitProject`] when a scaffold write fails.
/// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the project read fails.
fn scaffold(root: &Path, project: &Project) -> TerminalResult<InitReport> {
    let profile = root.join(DEFAULT_PROFILE);
    let stubs_dir = root.join(&project.stubs.folder);
    let project_path = root.join(PROJECT_FILE);
    let plugins_dir = root.join(&project.plugins.folder);
    let (lsp_path, lsp_text) = render_lsp(root, project)?;
    let documents = generate_stubs().map_err(map_stubs)?;
    let stub_destinations: Vec<PathBuf> = documents
        .iter()
        .map(|document| stubs_dir.join(format!("{}.d.lua", document.module)))
        .collect();
    let mut destinations = vec![profile.clone(), project_path.clone()];
    destinations.extend(stub_destinations.iter().cloned());
    destinations.extend(PLUGIN_FILES.iter().map(|entry| plugins_dir.join(entry.0)));
    destinations.push(lsp_path.clone());
    if stubs_dir.exists() {
        return error(
            ErrorKind::OverwriteRefused,
            contextual(FilesystemError::FileExists).with_fs_path(stubs_dir),
        );
    }
    for destination in &destinations {
        if destination.exists() {
            return error(
                ErrorKind::OverwriteRefused,
                contextual(FilesystemError::FileExists).with_fs_path(destination),
            );
        }
    }
    write_file(&profile, PROFILE_TEXT.as_bytes())?;
    write_file(
        &project_path,
        &project
            .render()
            .terminal(ErrorKind::InitProject)?
            .into_bytes(),
    )?;
    for (document, destination) in documents.iter().zip(stub_destinations.iter()) {
        write_file(destination, document.text.as_bytes())?;
    }
    for (relative, text) in PLUGIN_FILES.iter().copied() {
        write_file(&plugins_dir.join(relative), text.as_bytes())?;
    }
    write_file(&lsp_path, lsp_text.as_bytes())?;
    Ok(InitReport {
        path: profile,
        written: destinations.len(),
    })
}

/// Writes the generated stubs and overwrites present files.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the evaluation fails.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when Lua fails.
/// - [`confit_core::error::kind::ErrorKind::InitProject`] when a stub write fails.
fn write_stubs(dir: &Path) -> TerminalResult<InitReport> {
    let documents = generate_stubs().map_err(map_stubs)?;
    for document in &documents {
        let destination = dir.join(format!("{}.d.lua", document.module));
        write_file(&destination, document.text.as_bytes())?;
    }
    Ok(InitReport {
        path: dir.to_path_buf(),
        written: documents.len(),
    })
}

/// Maps one stub failure to the stub error kind.
fn map_stubs(contextual: ContextualCause) -> Error {
    let kind = match contextual.cause() {
        ErrorCause::EvalError(EvalError::LuaFailed) => ErrorKind::LuaFailed,
        ErrorCause::EvalError(_) | ErrorCause::DocumentError(_) => ErrorKind::Evaluate,
        _ => ErrorKind::InitProject,
    };
    if contextual.context.detail.is_none() {
        let message = contextual.cause().to_string();
        return contextual.with_detail(message).terminal(kind);
    }
    contextual.terminal(kind)
}

/// Writes the core plugins and overwrites present files.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::InitProject`] when a plugin write fails.
fn write_plugins(root: &Path, folder: &Path) -> TerminalResult<InitReport> {
    let dir = root.join(folder);
    for (relative, text) in PLUGIN_FILES.iter().copied() {
        write_file(&dir.join(relative), text.as_bytes())?;
    }
    Ok(InitReport {
        path: dir,
        written: PLUGIN_FILES.len(),
    })
}

/// Writes the editor config that maps the plugin and stubs folders.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::InitProject`] when a config write fails.
fn write_lsp(root: &Path, project: &Project) -> TerminalResult<InitReport> {
    let (path, text) = render_lsp(root, project)?;
    write_file(&path, text.as_bytes())?;
    Ok(InitReport { path, written: 1 })
}

/// Renders the editor config that maps the plugin and stubs folders.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::InitProject`] when the template fails to render.
fn render_lsp(root: &Path, project: &Project) -> TerminalResult<(PathBuf, String)> {
    let path = root.join(LSP_FILE);
    let context = minijinja::context!(
        plugins => escape(&project.plugins.folder.display().to_string()),
        stubs => escape(&project.stubs.folder.display().to_string()),
    );
    let mut text = minijinja::Environment::new()
        .render_str(LSP_TEXT, context)
        .map_err(|error| {
            contextual(DocumentError::RenderFailed)
                .with_fs_path(&path)
                .with_detail(error.to_string())
                .terminal(ErrorKind::InitProject)
        })?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok((path, text))
}

/// Escapes the backslashes and the double quotes in one string.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Writes one scaffold file and creates the parent folders first.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::InitProject`] when a folder or file write fails.
fn write_file(destination: &Path, text: &[u8]) -> TerminalResult<()> {
    driver::fs::create_parent(destination).terminal(ErrorKind::InitProject)?;
    std::fs::write(destination, text)
        .map_err(ContextualCause::from)
        .with_fs_path(destination)
        .terminal(ErrorKind::InitProject)
}
