//! Collect profile hooks with hook patches over declared hooks.

use std::collections::{BTreeMap, BTreeSet};

use mlua::{Lua, Value};

use crate::api::profile::Profile;
use confit_binding::value::{Decode, Encode};
use confit_model::document::Data;
use confit_model::hook::Hook;
use confit_model::routes::Route;

use super::patches::split_patches;
use crate::error::{EngineError, Result};

/// Collect one profile's hooks in first-seen order.
///
/// - Hook patches mutate live hooks in return order.
/// - Unknown hook targets and repeated ids fail the plan.
///
/// # Errors
///
/// - [`EngineError::DuplicateHook`] for repeated hook ids.
/// - [`EngineError::UnknownHookTarget`] for patches showing unknown ids.
/// - [`EngineError::ChangedUnknown`] for conditions showing unbuilt routes.
/// - [`EngineError::Lua`] for bridge and callback failures.
pub(super) fn collect_hooks(
    lua: &Lua,
    profile: &Profile,
    documents: &BTreeMap<Route, Data>,
) -> Result<Vec<Hook>> {
    check_hook_ids(profile)?;
    let mut hooks = Vec::new();
    for config in &profile.configs {
        hooks.extend(config.hooks.iter().cloned());
    }
    apply_hook_patches(lua, profile, &mut hooks)?;
    validate_changed(&hooks, documents)?;
    Ok(hooks)
}

/// Reject repeated hook ids. The error holds the first repeat.
///
/// # Errors
///
/// - [`EngineError::DuplicateHook`] for repeated ids.
fn check_hook_ids(profile: &Profile) -> Result<()> {
    let mut seen = BTreeSet::new();
    for config in &profile.configs {
        for hook in &config.hooks {
            if let Some(id) = hook.id.as_ref()
                && !seen.insert(id.clone())
            {
                return Err(EngineError::DuplicateHook { id: id.clone() });
            }
        }
    }
    Ok(())
}

/// Run hook patches over declared hooks mutating live tables in place.
///
/// - Patches apply in profile return order over matching ids.
///
/// # Errors
///
/// - [`EngineError::UnknownHookTarget`] for patches showing unknown ids.
/// - [`EngineError::Lua`] for bridge and callback failures.
fn apply_hook_patches(lua: &Lua, profile: &Profile, hooks: &mut [Hook]) -> Result<()> {
    for patch in split_patches(profile)
        .iter()
        .filter_map(|patch| patch.hook())
    {
        let Some(hook) = hooks
            .iter_mut()
            .find(|hook| hook.id.as_deref() == Some(patch.target.as_str()))
        else {
            return Err(EngineError::UnknownHookTarget {
                id: patch.target.clone(),
            });
        };
        let Value::Table(table) = hook.encode(lua)? else {
            return Err(EngineError::Lua(mlua::Error::runtime(
                "hook views hold tables",
            )));
        };
        patch.apply.call::<()>(lua, (table.clone(),))?;
        let back = Hook::decode(&Value::Table(table), lua).map_err(EngineError::from)?;
        *hook = back;
    }
    Ok(())
}

/// Reject changed conditions showing routes outside the built set.
///
/// # Errors
///
/// - [`EngineError::ChangedUnknown`] for conditions showing unbuilt routes.
fn validate_changed(hooks: &[Hook], documents: &BTreeMap<Route, Data>) -> Result<()> {
    let built: BTreeSet<Route> = documents.keys().cloned().collect();
    let mut paths = Vec::new();
    for hook in hooks {
        hook.requires.collect_changed(&mut paths);
        hook.when.collect_changed(&mut paths);
        hook.check.collect_changed(&mut paths);
    }
    for path in paths {
        if !built.contains(&path) {
            return Err(EngineError::ChangedUnknown {
                route: path.display(),
            });
        }
    }
    Ok(())
}
