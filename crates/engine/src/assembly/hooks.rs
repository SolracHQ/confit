//! Collect profile hooks with hook patches over declared hooks.

use std::collections::BTreeSet;

use confit_core::error::{Error, EvalDetail, EvalError, Outcome};
use mlua::{Lua, Value};

use crate::api::profile::Profile;
use confit_binding::value::{Decode, Encode};
use confit_model::document::Document;
use confit_model::hook::Hook;
use confit_model::routes::Route;

use super::patches::split_patches;

/// Collect one profile's hooks in first-seen order.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for repeated ids, unknown targets, unbuilt
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for repeated ids, unknown targets, unbuilt
///   routes, and Lua failures.
pub(super) fn collect_hooks(
    lua: &Lua,
    profile: &Profile,
    documents: &[Document],
) -> Outcome<Vec<Hook>> {
    check_hook_ids(profile)?;
    let mut hooks = Vec::new();
    for config in &profile.configs {
        hooks.extend(config.hooks.iter().cloned());
    }
    apply_hook_patches(lua, profile, &mut hooks)?;
    validate_changed(&hooks, documents)?;
    Ok(hooks)
}

/// Reject repeated hook ids.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for repeated ids.
fn check_hook_ids(profile: &Profile) -> Outcome<()> {
    let mut seen = BTreeSet::new();
    for config in &profile.configs {
        for hook in &config.hooks {
            if let Some(id) = hook.id.as_ref()
                && !seen.insert(id.as_str().to_owned())
            {
                return Err(Error::evaluate(
                    EvalDetail::Eval(EvalError::DuplicateHook {
                        name: id.as_str().to_owned(),
                    }),
                    None,
                ));
            }
        }
    }
    Ok(())
}

/// Run hook patches over declared hooks.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unknown ids, bad shapes, and Lua failures.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for unknown ids, bad shapes, and Lua failures.
fn apply_hook_patches(lua: &Lua, profile: &Profile, hooks: &mut [Hook]) -> Outcome<()> {
    for patch in split_patches(profile)
        .iter()
        .filter_map(|patch| patch.hook())
    {
        let Some(hook) = hooks.iter_mut().find(|hook| {
            hook.id
                .as_ref()
                .is_some_and(|id| id.as_str() == patch.target.as_str())
        }) else {
            return Err(Error::evaluate(
                EvalDetail::Eval(EvalError::HookTargetUnknown {
                    id: patch.target.clone(),
                }),
                None,
            ));
        };
        let table = match hook.encode(lua) {
            Ok(Value::Table(table)) => table,
            Ok(_) => return Err(Error::lua_failed("hook views hold tables")),
            Err(error) => return Err(Error::lua_failed(error.to_string())),
        };
        match patch.apply.call::<()>(lua, (table.clone(),)) {
            Ok(()) => {}
            Err(error) => {
                if let Some(terminal) = error.downcast_ref::<Error>() {
                    return Err(terminal.clone());
                }
                return Err(Error::lua_failed(error.to_string()));
            }
        }
        let back = match Hook::decode(&Value::Table(table), lua) {
            Ok(found) => found,
            Err(mismatch) => {
                return Err(Error::evaluate(
                    EvalDetail::Face(mismatch),
                    Some(patch.target.clone()),
                ));
            }
        };
        *hook = back;
    }
    Ok(())
}

/// Reject changed conditions showing routes outside the built set.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for conditions showing unbuilt routes.
fn validate_changed(hooks: &[Hook], documents: &[Document]) -> Outcome<()> {
    let built: BTreeSet<Route> = documents
        .iter()
        .map(|document| document.path.clone())
        .collect();
    let mut paths = Vec::new();
    for hook in hooks {
        hook.requires.collect_changed(&mut paths);
        hook.when.collect_changed(&mut paths);
        hook.check.collect_changed(&mut paths);
    }
    for path in paths {
        if !built.contains(&path) {
            let display = path.display();
            return Err(Error::evaluate(
                EvalDetail::Eval(EvalError::ChangedReferencesMissing { document: display }),
                None,
            ));
        }
    }
    Ok(())
}
