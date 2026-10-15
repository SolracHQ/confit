//! Init run
//! Project scaffolding from live faces and embedded resources.

use std::path::PathBuf;

use confit_driver as driver;
use confit_engine::project::{FILE as PROJECT_FILE, Project};

use crate::error::{CliError, Result};

use crate::cli::InitArgs;

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
    /// - [`CliError::Exists`] for present scaffolds.
    /// - [`CliError::Engine`] for stub generation failures.
    /// - [`CliError::Write`] with [`CliError::WriteUnknown`]
    ///   for scaffold writes.
    pub fn execute(self) -> Result<Vec<InitOutcome>> {
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
/// - [`CliError::Exists`] for present scaffolds.
/// - [`CliError::Engine`] for stub generation failures.
/// - [`CliError::Write`] with [`CliError::WriteUnknown`]
///   for scaffold writes.
fn scaffold(root: &std::path::Path, project: &Project) -> Result<InitReport> {
    let profile = root.join(crate::cli::DEFAULT_PROFILE);
    let stubs_dir = root.join(&project.stubs.folder);
    let project_path = root.join(PROJECT_FILE);
    let plugins_dir = root.join(&project.plugins.folder);
    let documents = confit_engine::stubs::generate_stubs()?;
    let stub_destinations: Vec<PathBuf> = documents
        .iter()
        .map(|document| stubs_dir.join(format!("{}.d.lua", document.namespace)))
        .collect();
    let mut destinations = vec![profile.clone(), project_path.clone()];
    destinations.extend(stub_destinations.iter().cloned());
    destinations.extend(PLUGIN_FILES.iter().map(|entry| plugins_dir.join(entry.0)));
    if driver::fs::exists(&stubs_dir) {
        return Err(CliError::Exists { path: stubs_dir });
    }
    for destination in &destinations {
        if driver::fs::exists(destination) {
            return Err(CliError::Exists {
                path: destination.clone(),
            });
        }
    }
    write_file(&profile, PROFILE_TEXT.as_bytes())?;
    write_file(&project_path, project.render()?.as_bytes())?;
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
/// Report with the stubs folder and the written stub file count.
///
/// # Errors
///
/// - [`CliError::Engine`] for stub generation failures.
/// - [`CliError::Write`] with [`CliError::WriteUnknown`]
///   for stub writes.
fn write_stubs(dir: &std::path::Path) -> Result<InitReport> {
    let documents = confit_engine::stubs::generate_stubs()?;
    for document in &documents {
        let destination = dir.join(format!("{}.d.lua", document.namespace));
        write_file(&destination, document.text.as_bytes())?;
    }
    Ok(InitReport {
        path: dir.to_path_buf(),
        written: documents.len(),
    })
}

/// Write the core plugins alone, overwriting present files.
///
/// # Errors
///
/// - [`CliError::Write`] with [`CliError::WriteUnknown`]
///   for plugin writes.
fn write_plugins(root: &std::path::Path, folder: &std::path::Path) -> Result<InitReport> {
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
/// - [`CliError::Write`] with [`CliError::WriteUnknown`]
///   for config writes.
fn write_lsp(root: &std::path::Path, project: &Project) -> Result<InitReport> {
    let path = root.join(LSP_FILE);
    let context = minijinja::context!(
        plugins => escape(&project.plugins.folder.display().to_string()),
        stubs => escape(&project.stubs.folder.display().to_string()),
    );
    let mut text = minijinja::Environment::new()
        .render_str(LSP_TEXT, context)
        .map_err(|error| CliError::WriteUnknown {
            path: path.clone(),
            message: error.to_string(),
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
/// - [`CliError::Write`] with [`CliError::WriteUnknown`]
///   for folder and file writes.
fn write_file(destination: &std::path::Path, text: &[u8]) -> Result<()> {
    driver::fs::create_parent(destination)
        .map_err(|error| CliError::from_write_io(destination, error))?;
    driver::fs::write(destination, text)
        .map_err(|error| CliError::from_write_io(destination, error))
}
