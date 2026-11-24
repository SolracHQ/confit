//! Collects profile hooks.

use std::collections::BTreeSet;

use confit_core::error::{
    Error, EvalError, Source, SourceKind, TerminalResult, contextual, error, kind::ErrorKind,
};
use confit_core::suggest;
use mlua::{Lua, Value};

use crate::api::profile::Profile;
use confit_model::document::Document;
use confit_model::hook::Hook;
use confit_model::routes::Route;
use lua_binding::value::{Decode, Encode};

use super::patches::split_patches;

/// Collects one profile's hooks.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a hook id
///   repeats, a patch target is unknown, or a changed condition targets an
///   unbuilt route.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a hook fails to
///   encode or a callback fails.
pub(super) fn collect_hooks(
    lua: &Lua,
    profile: &Profile,
    documents: &[Document],
) -> TerminalResult<Vec<Hook>> {
    validate_hook_ids(profile)?;
    let mut hooks = Vec::new();
    for config in &profile.configs {
        hooks.extend(config.hooks.iter().cloned());
    }
    apply_hook_patches(lua, profile, &mut hooks)?;
    validate_changed(&hooks, documents)?;
    Ok(hooks)
}

/// Rejects repeated hook ids.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when two hooks share
///   an id.
fn validate_hook_ids(profile: &Profile) -> TerminalResult<()> {
    let mut seen = BTreeSet::new();
    for config in &profile.configs {
        for hook in &config.hooks {
            if !seen.insert(hook.id.as_str().to_owned()) {
                let mut contextual = contextual(EvalError::DuplicateHook);
                for declaring in &profile.configs {
                    for held in &declaring.hooks {
                        if held.id == hook.id {
                            contextual = contextual.push_source(Source::new(
                                SourceKind::Config,
                                declaring.name.as_str(),
                                declared_at(held),
                            ));
                        }
                    }
                }
                return error(ErrorKind::Evaluate, contextual);
            }
        }
    }
    Ok(())
}

/// Returns the first declaration site.
fn declared_at(hook: &Hook) -> Option<String> {
    hook.id.trace.0.first().map(|site| site.to_string())
}

/// Applies the hook patches.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a target is
///   unknown or a patch has a bad shape.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a hook fails to
///   encode or a callback fails.
fn apply_hook_patches(lua: &Lua, profile: &Profile, hooks: &mut [Hook]) -> TerminalResult<()> {
    for owned in split_patches(profile).iter() {
        let Some(patch) = owned.hook() else {
            continue;
        };
        let Some(hook) = hooks
            .iter_mut()
            .find(|hook| hook.id.as_str() == patch.target.as_str())
        else {
            let ids: Vec<String> = hooks
                .iter()
                .map(|hook| hook.id.as_str().to_owned())
                .collect();
            let contextual = contextual(EvalError::HookTargetUnknown)
                .with_id(patch.target.as_str())
                .with_suggestions(suggest::nearby(patch.target.as_str(), &ids))
                .push_source(Source::new(SourceKind::Config, owned.owner.as_str(), None));
            return error(ErrorKind::Evaluate, contextual);
        };
        let table = match hook.encode(lua) {
            Ok(Value::Table(table)) => table,
            Ok(_) => {
                return error(ErrorKind::LuaFailed, contextual(EvalError::LuaFailed));
            }
            Err(fault) => {
                return error(ErrorKind::LuaFailed, fault);
            }
        };
        match patch.apply.call::<()>(lua, (table.clone(),)) {
            Ok(()) => {}
            Err(fault) => {
                if let Some(terminal) = fault.downcast_ref::<Error>() {
                    return Err(terminal.clone());
                }
                return error(ErrorKind::LuaFailed, fault);
            }
        }
        let back = match Hook::decode(&Value::Table(table), lua) {
            Ok(found) => found,
            Err(fault) => {
                let contextual = contextual(EvalError::BadShape)
                    .push_source(Source::new(SourceKind::Config, owned.owner.as_str(), None))
                    .with_document(patch.target.as_str())
                    .with_detail(fault.to_string());
                return error(ErrorKind::Evaluate, contextual);
            }
        };
        *hook = back;
    }
    Ok(())
}

/// Rejects changed conditions on unbuilt routes.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a changed
///   condition targets an unbuilt route.
fn validate_changed(hooks: &[Hook], documents: &[Document]) -> TerminalResult<()> {
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
    let candidates: Vec<String> = built.iter().map(|route| route.display()).collect();
    for path in paths {
        if !built.contains(&path) {
            let display = path.display();
            let contextual = contextual(EvalError::ChangedReferencesMissing)
                .with_suggestions(suggest::nearby(&display, &candidates))
                .with_document(display)
                .with_trace(path.trace.0.iter().map(|site| site.to_string()).collect());
            return error(ErrorKind::Evaluate, contextual);
        }
    }
    Ok(())
}
