//! Actions
//!
//! Orchestration from profile flags to built plans.

use std::path::Path;

use confit_core::document::Document;
use confit_core::error::{Error, Result};
use confit_core::ids::{DocPath, ReadOutcome};
use confit_core::plan::{BuiltPlan, Plan, State, build};

use crate::cli::{SharedArgs, resolve_root};
use crate::fs::{Filesystem, OsFs, snapshot};

/// Outcome of one profile run with its previous state.
#[derive(Debug)]
pub struct Outcome {
    /// Holds the built plan with counts plus warnings.
    pub built: BuiltPlan,
    /// Holds the previous state backing lifecycle marks.
    pub previous: State,
}

/// Builds a plan from evaluated documents.
///
/// # Arguments
///
/// * `documents` - desired documents in engine pipeline order.
/// * `previous` - the last apply record.
/// * `snapshot` - the disk reader mapping paths to outcomes.
///
/// # Returns
///
/// The built plan with counts plus warnings.
///
/// # Errors
///
/// Repeated declarations plus render failures fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::run_plan;
/// use confit_core::ids::ReadOutcome;
/// use confit_core::plan::State;
///
/// let outcome = run_plan(Vec::new(), &State::empty(), &|_| ReadOutcome::Absent);
/// assert!(matches!(outcome, Ok(built) if built.summary.create == 0));
/// ```
pub fn run_plan(
    documents: Vec<Document>,
    previous: &State,
    snapshot: &dyn Fn(&DocPath) -> ReadOutcome,
) -> Result<BuiltPlan> {
    build(documents, previous, snapshot)
}

/// Loads previous state, treating missing files as empty.
///
/// # Arguments
///
/// * `path` - the state file, holding `None` for empty previous.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The parsed state, else empty for missing inputs.
///
/// # Errors
///
/// Unreadable present files plus bad JSON fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::load_state;
/// use confit_cli::fs::OsFs;
///
/// let outcome = load_state(None, &OsFs);
/// assert!(matches!(outcome, Ok(state) if state.documents.is_empty()));
/// ```
pub fn load_state(path: Option<&Path>, fs: &dyn Filesystem) -> Result<State> {
    let Some(file) = path else {
        return Ok(State::empty());
    };
    let bytes = match fs.read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(State::empty());
        }
        Err(error) => return Err(Error::from(error)),
    };
    serde_json::from_slice(&bytes)
        .map_err(|error| Error::Plan(format!("read state '{}': {error}", file.display())))
}

/// Writes the plan payload to a file or stdout.
///
/// # Arguments
///
/// * `plan` - the versioned desired state.
/// * `out` - the destination, holding `None` for stdout.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// Unit once the payload lands.
///
/// # Errors
///
/// Serializer plus io failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::write_plan;
/// use confit_cli::fs::OsFs;
/// use confit_core::plan::{PLAN_VERSION, Plan};
///
/// let plan = Plan { version: PLAN_VERSION, documents: Vec::new(), created_at: String::new() };
/// assert!(matches!(write_plan(&plan, None, &OsFs), Ok(())));
/// ```
pub fn write_plan(plan: &Plan, out: Option<&Path>, fs: &dyn Filesystem) -> Result<()> {
    let text = serde_json::to_string_pretty(plan)
        .map_err(|error| Error::Plan(format!("render plan: {error}")))?;
    match out {
        Some(dest) => fs.write(dest, text.as_bytes()).map_err(Error::from),
        None => {
            println!("{text}");
            Ok(())
        }
    }
}

/// Runs one profile from shared flags into a built plan.
///
/// Evaluates the engine, loads previous state, snapshots disk
/// paths, builds the core plan, and writes the payload when the
/// caller passes a destination.
///
/// # Arguments
///
/// * `shared` - the profile plus seam flags.
/// * `output` - the plan destination, holding `None` for stdout.
///
/// # Returns
///
/// The built plan with its previous state.
///
/// # Errors
///
/// Evaluation plus state plus build plus write failures surface
/// as plan or io errors.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::run;
/// use confit_cli::cli::SharedArgs;
/// use std::path::PathBuf;
///
/// let shared = SharedArgs {
///     profile: PathBuf::from("profile.lua"),
///     root: None,
///     state: None,
///     plugins: None,
///     log_file: None,
/// };
/// let outcome = run(&shared, None);
/// assert!(matches!(outcome, Ok(_) | Err(_)));
/// ```
pub fn run(shared: &SharedArgs, output: Option<&Path>) -> Result<Outcome> {
    let root = resolve_root(&shared.root, &shared.profile);
    let documents = confit_engine::evaluate(
        &shared.profile,
        confit_engine::EvalOpts {
            root,
            plugins: shared.plugins.clone(),
        },
    )?;
    let previous = load_state(shared.state.as_deref(), &OsFs)?;
    let fs = OsFs;
    let built = run_plan(documents, &previous, &|path| snapshot(path, &fs))?;
    if let Some(dest) = output {
        write_plan(&built.plan, Some(dest), &OsFs)?;
    }
    Ok(Outcome { built, previous })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemoryFs;
    use confit_core::plan::PLAN_VERSION;

    #[test]
    fn missing_state_file_means_empty_previous() {
        let fs = MemoryFs::new();
        match load_state(None, &fs) {
            Ok(state) => assert!(state.documents.is_empty()),
            Err(error) => panic!("empty state loads: {error}"),
        }
        match load_state(Some(Path::new("state.json")), &fs) {
            Ok(state) => assert!(state.documents.is_empty()),
            Err(error) => panic!("missing file loads empty: {error}"),
        }
    }

    #[test]
    fn plan_output_lands_in_memory() {
        let fs = MemoryFs::new();
        let plan = Plan {
            version: PLAN_VERSION,
            documents: Vec::new(),
            created_at: String::new(),
        };
        let dest = Path::new("plan.json");
        match write_plan(&plan, Some(dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
        }
        assert!(fs.exists(dest));
    }
}
