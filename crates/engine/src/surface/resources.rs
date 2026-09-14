//! Resources
//!
//! Root-relative file loads for Lua.

use std::path::{Component, Path, PathBuf};

use mlua::{Lua, Value};
use serde_json::Value as Json;

use super::confit_table;
use crate::values::{json_to_lua, plan_error};

/// Installs the resources namespace on a state.
pub(crate) fn install(lua: &Lua, root: &Path) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let resources = lua.create_table()?;
    for name in ["load_toml", "load_json", "load_yaml", "load_text"] {
        let root = root.to_path_buf();
        let loader =
            lua.create_function(move |lua, path: Value| load_impl(lua, &root, name, path))?;
        resources.set(name, loader)?;
    }
    confit.set("resources", resources)?;
    Ok(())
}

/// Loads one root-relative file through the matching decoder.
fn load_impl(lua: &Lua, root: &Path, name: &'static str, path: Value) -> mlua::Result<Value> {
    let caller = match name {
        "load_toml" => "resources.load_toml",
        "load_json" => "resources.load_json",
        "load_yaml" => "resources.load_yaml",
        _ => "resources.load_text",
    };
    let rel = match path {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(plan_error(format!(
                "{caller}: field 'path' must be a string"
            )));
        }
    };
    let full = resolve_under_root(root, &rel, caller)?;
    let text = std::fs::read_to_string(&full)
        .map_err(|error| plan_error(format!("{caller}: cannot read '{rel}': {error}")))?;
    if name == "load_text" {
        return Ok(Value::String(lua.create_string(&text)?));
    }
    let json = match name {
        "load_toml" => {
            let parsed: toml::Value = toml::from_str(&text)
                .map_err(|error| plan_error(format!("{caller}: cannot parse '{rel}': {error}")))?;
            serde_json::to_value(&parsed)
                .map_err(|error| plan_error(format!("{caller}: cannot convert '{rel}': {error}")))?
        }
        "load_json" => serde_json::from_str::<Json>(&text)
            .map_err(|error| plan_error(format!("{caller}: cannot parse '{rel}': {error}")))?,
        _ => noyalib::from_str::<Json>(&text)
            .map_err(|error| plan_error(format!("{caller}: cannot parse '{rel}': {error}")))?,
    };
    json_to_lua(lua, &json)
}

/// Resolves one relative path under the project root.
fn resolve_under_root(root: &Path, rel: &str, caller: &str) -> mlua::Result<PathBuf> {
    if rel.is_empty() {
        return Err(plan_error(format!("{caller}: path must not be empty")));
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(plan_error(format!(
            "{caller}: path '{rel}' must be project-root-relative, not absolute"
        )));
    }
    let mut stack: Vec<String> = Vec::new();
    for component in rel_path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                return Err(plan_error(format!(
                    "{caller}: path '{rel}' must be project-root-relative, not absolute"
                )));
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    return Err(plan_error(format!(
                        "{caller}: path '{rel}' escapes the project root"
                    )));
                }
            }
            Component::Normal(segment) => stack.push(segment.to_string_lossy().into_owned()),
        }
    }
    let mut full = root.to_path_buf();
    for segment in &stack {
        full.push(segment);
    }
    if let (Ok(canonical_root), Ok(canonical_full)) = (root.canonicalize(), full.canonicalize())
        && !canonical_full.starts_with(&canonical_root)
    {
        return Err(plan_error(format!(
            "{caller}: path '{rel}' escapes the project root"
        )));
    }
    Ok(full)
}
