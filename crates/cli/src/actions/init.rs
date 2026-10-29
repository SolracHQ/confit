//! Init run
//! Project scaffolding from live faces and embedded resources.

use std::path::PathBuf;

use confit_core::error::{
    ContextualCause, DocumentError, Error, ErrorCause, EvalError, FilesystemError, Terminal,
    TerminalResult, WithContext, contextual, error, kind::ErrorKind,
};
use confit_driver as driver;
use confit_engine::project::{FILE as PROJECT_FILE, Project};
use confit_engine::stubs::generate_stubs;

use crate::cli::{DEFAULT_PROFILE, InitArgs};

const PROFILE_TEXT: &str = include_str!("../../resources/main.lua");

// Core plugins.
include!(concat!(env!("OUT_DIR"), "/plugin_files.rs"));

/// Outcome of one init run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitOutcome {
    /// Full scaffold.
    Scaffold(InitReport),
    /// Stubs refresh.
    Stubs(InitReport),
    /// Plugins refresh.
    Plugins(InitReport),
    /// Editor config write.
    Lsp(InitReport),
}

/// One init report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// Written path.
    pub path: PathBuf,
    /// Written file count.
    pub written: usize,
}

/// Init run from flags on the host driver.
pub struct InitRunner<'a> {
    /// Init flags for the run.
    pub args: &'a InitArgs,
}

impl InitRunner<'_> {
    /// Run one scaffold or refresh the flagged artifacts.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::OverwriteRefused`] for present scaffolds.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::InitProject`] for scaffold writes.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for scaffold writes.
    pub fn execute(self) -> TerminalResult<Vec<InitOutcome>> {
        let root = self.args.dir.clone().unwrap_or_else(|| PathBuf::from("."));
        let project = Project::read(&root)?;
        if !self.args.stubs && !self.args.plugins && !self.args.lsp {
            return Ok(vec![InitOutcome::Scaffold(scaffold(&root, &project)?)]);
        }
        let mut outcomes = Vec::new();
        if self.args.stubs {
            outcomes.push(InitOutcome::Stubs(write_stubs(
                &root.join(&project.stubs.folder),
            )?));
        }
        if self.args.plugins {
            outcomes.push(InitOutcome::Plugins(write_plugins(
                &root,
                &project.plugins.folder,
            )?));
        }
        if self.args.lsp {
            outcomes.push(InitOutcome::Lsp(write_lsp(&root, &project)?));
        }
        Ok(outcomes)
    }
}

/// Write one profile with live stubs, the core plugins and the project file.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::OverwriteRefused`] for present scaffolds.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
/// - [`confit_core::error::kind::ErrorKind::InitProject`] for scaffold writes.
/// - [`confit_core::error::kind::ErrorKind::LoadProject`] for scaffold writes.
fn scaffold(root: &std::path::Path, project: &Project) -> TerminalResult<InitReport> {
    let profile = root.join(DEFAULT_PROFILE);
    let stubs_dir = root.join(&project.stubs.folder);
    let project_path = root.join(PROJECT_FILE);
    let plugins_dir = root.join(&project.plugins.folder);
    let documents = generate_stubs().map_err(map_stubs)?;
    let stub_destinations: Vec<PathBuf> = documents
        .iter()
        .map(|document| stubs_dir.join(format!("{}.d.lua", document.namespace)))
        .collect();
    let mut destinations = vec![profile.clone(), project_path.clone()];
    destinations.extend(stub_destinations.iter().cloned());
    destinations.extend(PLUGIN_FILES.iter().map(|entry| plugins_dir.join(entry.0)));
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
    Ok(InitReport {
        path: profile,
        written: destinations.len(),
    })
}

/// Write the generated faces alone, overwriting present stubs.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
/// - [`confit_core::error::kind::ErrorKind::InitProject`] for stub writes.
fn write_stubs(dir: &std::path::Path) -> TerminalResult<InitReport> {
    let documents = generate_stubs().map_err(map_stubs)?;
    for document in &documents {
        let destination = dir.join(format!("{}.d.lua", document.namespace));
        write_file(&destination, document.text.as_bytes())?;
    }
    Ok(InitReport {
        path: dir.to_path_buf(),
        written: documents.len(),
    })
}

/// Map one stub failure to its terminal shape.
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

/// Write the core plugins alone, overwriting present files.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::InitProject`] for plugin writes.
fn write_plugins(root: &std::path::Path, folder: &std::path::Path) -> TerminalResult<InitReport> {
    let dir = root.join(folder);
    for (relative, text) in PLUGIN_FILES.iter().copied() {
        write_file(&dir.join(relative), text.as_bytes())?;
    }
    Ok(InitReport {
        path: dir,
        written: PLUGIN_FILES.len(),
    })
}

/// Editor config file name under the project root.
const LSP_FILE: &str = ".luarc.json";

/// Editor config template mapping the plugin and stubs folders.
const LSP_TEXT: &str = include_str!("../../resources/luarc.json");

/// Write the editor config mapping the plugin and stubs folders.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::InitProject`] for config writes.
fn write_lsp(root: &std::path::Path, project: &Project) -> TerminalResult<InitReport> {
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
    write_file(&path, text.as_bytes())?;
    Ok(InitReport { path, written: 1 })
}

/// Escape one string for double quotes.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Write one scaffold file with parent folders first.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::InitProject`] for folder and file writes.
fn write_file(destination: &std::path::Path, text: &[u8]) -> TerminalResult<()> {
    driver::fs::create_parent(destination).terminal(ErrorKind::InitProject)?;
    std::fs::write(destination, text)
        .map_err(ContextualCause::from)
        .with_fs_path(destination)
        .terminal(ErrorKind::InitProject)
}
