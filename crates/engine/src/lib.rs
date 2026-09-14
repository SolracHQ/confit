//! Engine
//!
//! Lua profile evaluation into finished documents.

#![deny(missing_docs)]

use std::path::{Path, PathBuf};

use confit_core::document::Document;

mod eval;
mod exec;
mod level;
mod model;
mod surface;
mod values;

/// Evaluation inputs for one profile run.
///
/// The root jails resource reads. The plugins folder adds external
/// namespaces beside the embedded defaults.
///
/// # Examples
///
/// ```rust
/// use confit_engine::EvalOpts;
/// use std::path::PathBuf;
///
/// let opts = EvalOpts {
///     root: PathBuf::from("."),
///     plugins: None,
/// };
/// assert!(matches!(opts.root.to_str(), Some(".")));
/// assert!(matches!(opts.plugins, None));
/// ```
#[derive(Debug, Clone, Default)]
pub struct EvalOpts {
    /// Project root for resource reads plus module resolution.
    pub root: PathBuf,
    /// External plugin folder shaped `{user}/{name}/plugin.lua`.
    pub plugins: Option<PathBuf>,
}

/// Evaluates one profile file into finished documents.
///
/// # Arguments
///
/// * `profile` - the profile file path.
/// * `opts` - root plus plugin folder inputs.
///
/// # Returns
///
/// Structured plus text plus link documents plus one rc document per
/// shell, in deterministic order.
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
pub fn evaluate(profile: &Path, opts: EvalOpts) -> confit_core::error::Result<Vec<Document>> {
    eval::run(profile, opts)
}
