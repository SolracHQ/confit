//! Engine
//!
//! Lua profile evaluation into finished documents.

#![deny(missing_docs)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_core::document::ManifestDocument;
use confit_core::hook::Hook;
use confit_core::progress::ProgressSender;

use crate::fetch::Fetch;

mod error;
mod eval;
mod exec;
pub mod fetch;
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
/// remote downloads. The cache override keeps tests off the OS cache.
/// The fetcher override keeps tests off the network. The progress
/// sender stays silent while holding `None`.
///
/// # Examples
///
/// ```rust
/// use confit_engine::EvalOpts;
/// use std::path::PathBuf;
///
/// let opts = EvalOpts {
///     root: PathBuf::from("."),
///     plugins: PathBuf::from("plugins"),
///     re_fetch: false,
///     cache_dir: None,
///     fetcher: None,
///     progress: None,
/// };
/// assert!(matches!(opts.root.to_str(), Some(".")));
/// assert!(matches!(opts.plugins.to_str(), Some("plugins")));
/// ```
#[derive(Clone, Default)]
pub struct EvalOpts {
    /// Project root for resource reads plus module resolution.
    pub root: PathBuf,
    /// External plugin folder shaped `{user}/{name}/plugin.lua`.
    /// Missing folders read as embedded-only.
    pub plugins: PathBuf,
    /// Forces remote downloads past the sidecar cache.
    pub re_fetch: bool,
    /// Cache folder override for tests, holding `None` for OS cache.
    pub cache_dir: Option<PathBuf>,
    /// Network source override for tests, holding `None` for HTTP.
    pub fetcher: Option<Arc<dyn Fetch>>,
    /// Progress sender for fetch plus unpack plus patch facts.
    pub progress: Option<ProgressSender>,
}

impl std::fmt::Debug for EvalOpts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvalOpts")
            .field("root", &self.root)
            .field("plugins", &self.plugins)
            .field("re_fetch", &self.re_fetch)
            .field("cache_dir", &self.cache_dir)
            .field("fetcher", &self.fetcher.is_some())
            .field("progress", &self.progress.is_some())
            .finish()
    }
}

/// Finished evaluation holding documents plus blobs plus hooks.
///
/// Documents hold one rc document per shell in deterministic
/// order. Blobs hold raw opaque plus tree member bytes under
/// SHA-256 hex, one entry per referenced blob. Hooks hold merged
/// post-config steps in first-seen declaration order.
///
/// # Examples
///
/// ```rust
/// use confit_engine::Evaluation;
///
/// let evaluation = Evaluation {
///     documents: Vec::new(),
///     blobs: std::collections::BTreeMap::new(),
///     hooks: Vec::new(),
/// };
/// assert!(matches!(evaluation.documents.len(), 0));
/// ```
#[derive(Debug, Clone, Default)]
pub struct Evaluation {
    /// Holds finished documents in deterministic order.
    pub documents: Vec<ManifestDocument>,
    /// Holds raw blob bytes under SHA-256 hex hashes.
    pub blobs: BTreeMap<String, Vec<u8>>,
    /// Holds merged hooks in first-seen declaration order.
    pub hooks: Vec<Hook>,
}

/// Evaluates one profile file into finished documents plus hooks.
///
/// # Arguments
///
/// * `profile` - the profile file path.
/// * `opts` - root plus plugin folder inputs.
///
/// # Returns
///
/// Structured plus text plus link documents plus one rc document per
/// shell, in deterministic order, plus merged hooks.
///
/// # Errors
///
/// Missing files fail as io errors. Bad shapes plus conflicts plus
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
