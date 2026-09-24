//! Engine
//!
//! Lua profile evaluation into finished documents.

#![deny(missing_docs)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit_core::document::ManifestDocument;
use confit_core::handles::BlobHandle;
use confit_core::hook::Hook;
use confit_core::progress::ProgressSender;
use confit_store::Stores;

mod error;
mod eval;
mod exec;
mod level;
mod lua;
mod model;
mod path_expr;
mod require;
mod surface;

/// Evaluation inputs for one profile run.
///
/// The root jails resource reads. The plugins folder adds external
/// namespaces beside the embedded defaults. The re-fetch flag forces
/// remote downloads. The stores override keeps tests on memory
/// backends, holding `None` for host backends. The progress
/// sender stays silent while holding `None`.
///
#[derive(Clone, Default)]
pub struct EvalOpts {
    /// Project root for resource reads and module resolution.
    pub root: PathBuf,
    /// External plugin folder shaped `{user}/{name}/plugin.lua`.
    /// Missing folders read as embedded-only.
    pub plugins: PathBuf,
    /// Forces remote downloads past the sidecar cache.
    pub re_fetch: bool,
    /// Store override for tests, holding `None` for host stores.
    pub stores: Option<Stores>,
    /// Progress sender for fetch, unpack, and patch facts.
    pub progress: Option<ProgressSender>,
}

impl std::fmt::Debug for EvalOpts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalOpts")
            .field("root", &self.root)
            .field("plugins", &self.plugins)
            .field("re_fetch", &self.re_fetch)
            .field("stores", &self.stores.is_some())
            .field("progress", &self.progress.is_some())
            .finish()
    }
}

/// Finished evaluation holding documents, blobs, and hooks.
///
/// Documents hold one rc document per shell in deterministic
/// order. Blobs hold blob handles under SHA-256 hex, one
/// entry per referenced blob. Hooks hold merged
/// post-config steps in first-seen declaration order.
///
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Holds finished documents in deterministic order.
    pub documents: Vec<ManifestDocument>,
    /// Holds blob handles under SHA-256 hex hashes.
    pub blobs: BTreeMap<String, BlobHandle>,
    /// Holds merged hooks in first-seen declaration order.
    pub hooks: Vec<Hook>,
}

/// Evaluates one profile file into finished documents and hooks.
///
/// # Arguments
///
/// * `profile` - the profile file path.
/// * `opts` - root and plugin folder inputs.
///
/// # Returns
///
/// Structured, text, link, secret documents, one rc document per
/// shell, in deterministic order, and merged hooks.
///
/// # Errors
///
/// Missing files fail as io errors. Bad shapes, conflicts, and
/// unreadable graphs fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_engine::{EvalOpts, evaluate};
/// use std::path::Path;
///
/// let outcome = evaluate(Path::new("/nonexistent-profile.lua"), EvalOpts::default());
/// assert!(matches!(outcome, Err(_)));
/// ```
pub fn evaluate(profile: &Path, opts: EvalOpts) -> confit_core::error::Result<Evaluation> {
    eval::Session::run(profile, opts)
}
