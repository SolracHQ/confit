//! Workspace
//!
//! Disk snapshots, document writes, scaffolding, and text reads.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use confit_core::arg::Arg;
use confit_core::document::ManifestDocument;
use confit_core::error::{Error, Result};
use confit_core::fs::snapshot::TreeMemberRead;
use confit_core::handles::{ResourceHandle, Route};
use confit_core::ids::ReadOutcome;

use super::blob::BlobStore;

pub mod file;
pub mod memory;

/// Managed disk behind snapshots, writes, and text reads.
///
/// Snapshot outcomes mirror the drift shapes: absent for
/// missing destinations, present bytes and mode else.
/// Every destination resolves through `resolve` from its
/// manifest route, so snapshots, writes, and removals share
/// one expansion.
pub trait Workspace {
    /// Expands one destination route to its host path.
    fn resolve(&self, route: &Route) -> PathBuf;

    /// Births a resource handle for one project file.
    ///
    /// # Errors
    ///
    /// Escaping, missing, and unreadable files fail as plan or io errors.
    fn resource(&self, exec_root: &Path, path: &Path) -> Result<ResourceHandle>;

    /// Snapshots one document destination through its kind-aware reader.
    fn snapshot_doc(&self, document: &ManifestDocument) -> ReadOutcome;

    /// Snapshots one tree destination into relative member reads.
    fn snapshot_tree(&self, document: &ManifestDocument) -> BTreeMap<String, TreeMemberRead>;

    /// Writes every document to its resolved destination.
    ///
    /// Present unmanaged documents stay untouched while
    /// their destination reads absent from the changed set.
    /// Secret documents execute their command at apply time;
    /// stdout bytes land on disk and never enter a bundle or preview.
    ///
    /// # Errors
    ///
    /// Render, command, and io failures surface as plan or io errors.
    fn write_documents(
        &self,
        documents: &[ManifestDocument],
        blobs: &dyn BlobStore,
        changed: &BTreeSet<Route>,
        on_written: Option<&dyn Fn(&Route)>,
    ) -> Result<usize>;

    /// Removes recorded destinations absent from desired documents.
    ///
    /// # Errors
    ///
    /// Removal failures surface as io errors.
    fn remove_orphans(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize>;

    /// Removes dropped tree members between recorded and desired manifests.
    ///
    /// # Errors
    ///
    /// Removal failures surface as io errors.
    fn remove_tree_members(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize>;

    /// Scaffolds one project holding profiles and stubs.
    ///
    /// # Errors
    ///
    /// Write failures surface as plan or io errors.
    fn scaffold(&self, dest: &Route, name: &str) -> Result<()>;

    /// Reads one profile file as text.
    ///
    /// # Errors
    ///
    /// Missing files and invalid text fail as plan errors.
    fn read_profile(&self, handle: &ResourceHandle) -> Result<String>;

    /// Reads one module file as text.
    ///
    /// # Errors
    ///
    /// Missing files and invalid text fail as plan errors.
    fn read_module(&self, handle: &ResourceHandle) -> Result<String>;
}

/// Executes one secret command, returning its stdout bytes.
///
/// Bytes arrive at apply time alone and never persist
/// elsewhere.
///
/// # Arguments
///
/// * `argv` - the command and arguments in order.
/// * `destination` - the destination route naming failures.
/// * `resolve` - the route resolver under expanding.
///
/// # Returns
///
/// Stdout bytes from a successful run.
///
/// # Errors
///
/// Empty commands fail as plan errors. Spawn failures and
/// non-zero statuses fail as plan errors naming the
/// destination.
pub fn run_secret_command(
    argv: &[Arg],
    destination: &Route,
    resolve: &dyn Fn(&Route) -> PathBuf,
) -> Result<Vec<u8>> {
    if argv.is_empty() {
        return Err(Error::Plan(format!(
            "secret '{}': command reads empty",
            destination.display()
        )));
    }
    let display = argv.iter().map(Arg::display).collect::<Vec<_>>().join(" ");
    let expanded: Vec<String> = argv
        .iter()
        .map(|slot| match slot {
            Arg::Text(text) => text.clone(),
            Arg::Route(route) => resolve(route).to_string_lossy().into_owned(),
        })
        .collect();
    let Some((program, args)) = expanded.split_first() else {
        return Err(Error::Plan(format!(
            "secret '{}': command reads empty",
            destination.display()
        )));
    };
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|error| {
            Error::Plan(format!(
                "secret '{}': cannot run '{display}': {error}",
                destination.display(),
            ))
        })?;
    if !output.status.success() {
        return Err(Error::Plan(format!(
            "secret '{}': command '{display}' failed with status {}",
            destination.display(),
            output.status
        )));
    }
    Ok(output.stdout)
}
