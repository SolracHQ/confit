//! Resources
//!
//! Root-relative file loads plus table merge for Lua.

use std::path::{Component, Path, PathBuf};

use mlua::{Lua, MultiValue, Table, Value};
use serde_json::Value as Json;

use super::confit_table;

/// Named-registry key holding the project root string for Lua callbacks.
///
/// `install` stores the root under this key so `load_*` and
/// future path helpers resolve under the same `--root`. Readers use
/// `lua.named_registry_value::<String>(PROJECT_ROOT_KEY)`.
pub const PROJECT_ROOT_KEY: &str = "confit.project_root";

/// Tweaks for [`install`] merge behavior.
///
/// Both flags default to `false`. Unknown option keys are plan errors
/// naming the key; non-boolean values for known keys are Lua errors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeOpts {
    /// Merge top-level keys only, replacing nested tables wholesale.
    pub shallow: bool,
    /// Concatenate arrays instead of replacing, keeping every element.
    pub list_append: bool,
}

/// Installs the resources namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the namespace.
/// * `root` - project root for load resolution.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
pub fn install(lua: &Lua, root: PathBuf) -> mlua::Result<()> {
    lua.set_named_registry_value(PROJECT_ROOT_KEY, root.display().to_string())?;
    let confit = confit_table(lua)?;
    let resources = lua.create_table()?;

    let toml_root = root.clone();
    let load_toml =
        lua.create_function(move |lua, path: Value| load_toml_impl(lua, &toml_root, path))?;
    resources.set("load_toml", load_toml)?;

    let json_root = root.clone();
    let load_json =
        lua.create_function(move |lua, path: Value| load_json_impl(lua, &json_root, path))?;
    resources.set("load_json", load_json)?;

    let yaml_root = root.clone();
    let load_yaml =
        lua.create_function(move |lua, path: Value| load_yaml_impl(lua, &yaml_root, path))?;
    resources.set("load_yaml", load_yaml)?;

    let merge = lua.create_function(|lua, args: MultiValue| merge_impl(lua, args))?;
    resources.set("merge", merge)?;

    confit.set("resources", resources)?;
    Ok(())
}

/// Resolves a relative path under the project root.
///
/// # Arguments
///
/// * `root` - project root.
/// * `rel` - caller supplied path.
/// * `caller` - calling function name for errors.
///
/// # Returns
///
/// Resolved path under the root.
///
/// # Errors
///
/// Fails with plan errors for absolute paths and for paths escaping the root and for empty paths.
fn resolve_under_root(root: &Path, rel: &str, caller: &str) -> mlua::Result<PathBuf> {
    if rel.is_empty() {
        return Err(plan_err!("{caller}: path must not be empty"));
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(plan_err!(
            "{caller}: path '{rel}' must be project-root-relative, not absolute"
        ));
    }
    let mut stack: Vec<String> = Vec::new();
    for component in rel_path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                return Err(plan_err!(
                    "{caller}: path '{rel}' must be project-root-relative, not absolute"
                ));
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    return Err(plan_err!("{caller}: path '{rel}' escapes the project root"));
                }
            }
            Component::Normal(segment) => {
                stack.push(segment.to_string_lossy().into_owned());
            }
        }
    }
    let mut full = root.to_path_buf();
    for segment in &stack {
        full.push(segment);
    }
    if let (Ok(canonical_root), Ok(canonical_full)) = (root.canonicalize(), full.canonicalize())
        && !canonical_full.starts_with(&canonical_root)
    {
        return Err(plan_err!("{caller}: path '{rel}' escapes the project root"));
    }
    Ok(full)
}

/// Reads a path argument from a Lua value.
///
/// # Arguments
///
/// * `value` - raw Lua argument.
/// * `caller` - calling function name for errors.
///
/// # Returns
///
/// Path string.
///
/// # Errors
///
/// Fails with Lua errors for values of other shapes.
fn take_rel_path(value: Value, caller: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(lua_err!("{caller}: field 'path' must be a string")),
    }
}

/// Loads a TOML file as a Lua table.
///
/// # Arguments
///
/// * `lua` - state building the table.
/// * `root` - project root.
/// * `path` - raw Lua path argument.
///
/// # Returns
///
/// Lua table holding file data.
///
/// # Errors
///
/// Fails with plan errors for read plus parse plus conversion failures.
fn load_toml_impl(lua: &Lua, root: &Path, path: Value) -> mlua::Result<Value> {
    const CALLER: &str = "resources.load_toml";
    let rel = take_rel_path(path, CALLER)?;
    let full = resolve_under_root(root, &rel, CALLER)?;
    let text = std::fs::read_to_string(&full)
        .map_err(|err| plan_err!("{CALLER}: cannot read '{rel}': {err}"))?;
    let parsed: toml::Value =
        toml::from_str(&text).map_err(|err| plan_err!("{CALLER}: cannot parse '{rel}': {err}"))?;
    let json = serde_json::to_value(&parsed)
        .map_err(|err| plan_err!("{CALLER}: cannot convert '{rel}': {err}"))?;
    json_to_lua(lua, &json)
}

/// Loads a JSON file as a Lua value.
///
/// # Arguments
///
/// * `lua` - state building the value.
/// * `root` - project root.
/// * `path` - raw Lua path argument.
///
/// # Returns
///
/// Lua value holding file data.
///
/// # Errors
///
/// Fails with plan errors for read plus parse failures.
fn load_json_impl(lua: &Lua, root: &Path, path: Value) -> mlua::Result<Value> {
    const CALLER: &str = "resources.load_json";
    let rel = take_rel_path(path, CALLER)?;
    let full = resolve_under_root(root, &rel, CALLER)?;
    let text = std::fs::read_to_string(&full)
        .map_err(|err| plan_err!("{CALLER}: cannot read '{rel}': {err}"))?;
    let json: Json = serde_json::from_str(&text)
        .map_err(|err| plan_err!("{CALLER}: cannot parse '{rel}': {err}"))?;
    json_to_lua(lua, &json)
}

/// Loads a YAML file as a Lua value.
///
/// # Arguments
///
/// * `lua` - state building the value.
/// * `root` - project root.
/// * `path` - raw Lua path argument.
///
/// # Returns
///
/// Lua value holding file data.
///
/// # Errors
///
/// Fails with plan errors for read plus parse failures.
fn load_yaml_impl(lua: &Lua, root: &Path, path: Value) -> mlua::Result<Value> {
    const CALLER: &str = "resources.load_yaml";
    let rel = take_rel_path(path, CALLER)?;
    let full = resolve_under_root(root, &rel, CALLER)?;
    let text = std::fs::read_to_string(&full)
        .map_err(|err| plan_err!("{CALLER}: cannot read '{rel}': {err}"))?;
    let json: Json = noyalib::from_str(&text)
        .map_err(|err| plan_err!("{CALLER}: cannot parse '{rel}': {err}"))?;
    json_to_lua(lua, &json)
}

/// Merges two Lua tables into a fresh table.
///
/// # Arguments
///
/// * `lua` - state building the result.
/// * `args` - raw call arguments holding base plus overlay plus options.
///
/// # Returns
///
/// Merged Lua value.
///
/// # Errors
///
/// Fails with Lua errors for table shape mismatches and with plan errors for unexpected option keys.
fn merge_impl(lua: &Lua, args: MultiValue) -> mlua::Result<Value> {
    const CALLER: &str = "resources.merge";
    let collected: Vec<Value> = args.into_iter().collect();
    if collected.len() < 2 || collected.len() > 3 {
        return Err(lua_err!("{CALLER}: expects (base, overlay, opts?)"));
    }
    let base = match collected.first() {
        Some(Value::Table(table)) => table.clone(),
        _ => {
            return Err(lua_err!("{CALLER}: argument 'base' must be a table"));
        }
    };
    let overlay = match collected.get(1) {
        Some(Value::Table(table)) => table.clone(),
        _ => {
            return Err(lua_err!("{CALLER}: argument 'overlay' must be a table"));
        }
    };
    let opts = match collected.get(2) {
        None | Some(Value::Nil) => MergeOpts::default(),
        Some(Value::Table(table)) => parse_merge_opts(table, CALLER)?,
        Some(_) => {
            return Err(lua_err!("{CALLER}: field 'opts' must be a table"));
        }
    };
    let base_json = table_to_json(&base, "resources.merge argument 'base'")?;
    let overlay_json = table_to_json(&overlay, "resources.merge argument 'overlay'")?;
    let merged = merge_value(&base_json, &overlay_json, &opts, 0);
    json_to_lua(lua, &merged)
}

/// Parses merge options from a Lua table.
///
/// # Arguments
///
/// * `table` - raw option table.
/// * `caller` - calling function name for errors.
///
/// # Returns
///
/// Merge options holding flag values.
///
/// # Errors
///
/// Fails with plan errors for unexpected keys and with Lua errors for flag values of other shapes.
fn parse_merge_opts(table: &Table, caller: &str) -> mlua::Result<MergeOpts> {
    let mut opts = MergeOpts::default();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let name = match key {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(plan_err!("{caller}: unknown option '<non-string key>'"));
            }
        };
        match name.as_str() {
            "shallow" => match value {
                Value::Boolean(flag) => opts.shallow = flag,
                _ => {
                    return Err(lua_err!("{caller}: field 'shallow' must be a boolean"));
                }
            },
            "list_append" => match value {
                Value::Boolean(flag) => opts.list_append = flag,
                _ => {
                    return Err(lua_err!("{caller}: field 'list_append' must be a boolean"));
                }
            },
            _ => {
                return Err(plan_err!("{caller}: unknown option '{name}'"));
            }
        }
    }
    Ok(opts)
}

/// Merges an overlay JSON value over a base JSON value.
///
/// # Arguments
///
/// * `base` - earlier value.
/// * `overlay` - winning value.
/// * `opts` - tweaks for merge shape.
/// * `depth` - recursion level from the top call.
///
/// # Returns
///
/// Merged JSON value.
fn merge_value(base: &Json, overlay: &Json, opts: &MergeOpts, depth: usize) -> Json {
    match (base, overlay) {
        (Json::Object(base_map), Json::Object(overlay_map)) if !opts.shallow || depth == 0 => {
            let mut merged = base_map.clone();
            for (key, overlay_value) in overlay_map {
                match merged.remove(key) {
                    Some(base_value) => {
                        if opts.shallow {
                            match (&base_value, overlay_value) {
                                (Json::Array(base_items), Json::Array(overlay_items))
                                    if opts.list_append =>
                                {
                                    let mut items = base_items.clone();
                                    items.extend(overlay_items.clone());
                                    merged.insert(key.clone(), Json::Array(items));
                                }
                                _ => {
                                    merged.insert(key.clone(), overlay_value.clone());
                                }
                            }
                        } else {
                            merged.insert(
                                key.clone(),
                                merge_value(&base_value, overlay_value, opts, depth + 1),
                            );
                        }
                    }
                    None => {
                        merged.insert(key.clone(), overlay_value.clone());
                    }
                }
            }
            Json::Object(merged)
        }
        (Json::Array(base_items), Json::Array(overlay_items)) if opts.list_append => {
            let mut items = base_items.clone();
            items.extend(overlay_items.clone());
            Json::Array(items)
        }
        _ => overlay.clone(),
    }
}

/// Converts a Lua value into JSON.
///
/// # Arguments
///
/// * `value` - Lua value.
/// * `ctx` - field path for errors.
///
/// # Returns
///
/// JSON holding value data.
///
/// # Errors
///
/// Fails with Lua errors for values carrying executable shapes.
fn lua_to_json(value: Value, ctx: &str) -> mlua::Result<Json> {
    match value {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(flag) => Ok(Json::Bool(flag)),
        Value::Integer(number) => Ok(Json::Number(number.into())),
        Value::Number(number) => match serde_json::Number::from_f64(number) {
            Some(parsed) => Ok(Json::Number(parsed)),
            None => Err(lua_err!("{ctx} must be a finite number")),
        },
        Value::String(text) => Ok(Json::String(text.to_string_lossy())),
        Value::Table(table) => table_to_json(&table, ctx),
        Value::Function(_) => Err(lua_err!("{ctx} must be data-only (function not allowed)")),
        Value::UserData(_) => Err(lua_err!("{ctx} must be data-only (userdata not allowed)")),
        Value::LightUserData(_) => Err(lua_err!("{ctx} must be data-only (userdata not allowed)")),
        Value::Thread(_) => Err(lua_err!("{ctx} must be data-only (thread not allowed)")),
        Value::Error(_) => Err(lua_err!("{ctx} must be data-only")),
        Value::Other(_) => Err(lua_err!("{ctx} must be data-only")),
    }
}

/// Converts a Lua table into a JSON value.
///
/// # Arguments
///
/// * `table` - Lua table.
/// * `ctx` - field path for errors.
///
/// # Returns
///
/// JSON array for dense arrays, else JSON object.
///
/// # Errors
///
/// Fails with Lua errors for keys of other shapes and for values carrying executable shapes.
fn table_to_json(table: &Table, ctx: &str) -> mlua::Result<Json> {
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        entries.push(pair?);
    }
    if entries.is_empty() {
        return Ok(Json::Object(serde_json::Map::new()));
    }
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    let mut all_integer = true;
    for (key, value) in &entries {
        match key {
            Value::Integer(index) => indexed.push((*index, value.clone())),
            _ => {
                all_integer = false;
                break;
            }
        }
    }
    if all_integer {
        indexed.sort_by_key(|(index, _)| *index);
        let dense = indexed
            .iter()
            .enumerate()
            .all(|(position, (index, _))| *index == position as i64 + 1);
        if dense {
            let mut items = Vec::with_capacity(indexed.len());
            for (index, value) in &indexed {
                let child = format!("{ctx}[{index}]");
                items.push(lua_to_json(value.clone(), &child)?);
            }
            return Ok(Json::Array(items));
        }
    }
    let mut map = serde_json::Map::new();
    for (key, value) in &entries {
        let name = match key {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(lua_err!("{ctx} must be a table with string keys"));
            }
        };
        let child = format!("{ctx}.{name}");
        map.insert(name, lua_to_json(value.clone(), &child)?);
    }
    Ok(Json::Object(map))
}

/// Converts a JSON value into a Lua value.
///
/// # Arguments
///
/// * `lua` - state building tables plus strings.
/// * `value` - JSON source.
///
/// # Returns
///
/// Lua value holding JSON data.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
fn json_to_lua(lua: &Lua, value: &Json) -> mlua::Result<Value> {
    match value {
        Json::Null => Ok(Value::Nil),
        Json::Bool(flag) => Ok(Value::Boolean(*flag)),
        Json::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(Value::Integer(integer))
            } else if let Some(unsigned) = number.as_u64() {
                if unsigned <= i64::MAX as u64 {
                    Ok(Value::Integer(unsigned as i64))
                } else {
                    Ok(Value::Number(unsigned as f64))
                }
            } else if let Some(float) = number.as_f64() {
                Ok(Value::Number(float))
            } else {
                Ok(Value::Nil)
            }
        }
        Json::String(text) => Ok(Value::String(lua.create_string(text)?)),
        Json::Array(items) => {
            let table = lua.create_table()?;
            for (position, item) in items.iter().enumerate() {
                table.raw_set(position as i64 + 1, json_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
        Json::Object(map) => {
            let table = lua.create_table()?;
            for (key, item) in map {
                table.raw_set(key.clone(), json_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::Error;

    /// Builds a temp root with resources installed.
    ///
    /// # Arguments
    ///
    /// * `files` - relative paths plus contents.
    ///
    /// # Returns
    ///
    /// Temp directory plus Lua state.
    fn setup(files: &[(&str, &str)]) -> (tempfile::TempDir, Lua) {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, contents) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("mkdirs");
            }
            std::fs::write(&path, contents).expect("write");
        }
        let lua = Lua::new();
        install(&lua, dir.path().to_path_buf()).expect("install");
        (dir, lua)
    }

    /// Reports plan domain status for a Lua failure.
    ///
    /// # Arguments
    ///
    /// * `err` - Lua failure.
    ///
    /// # Returns
    ///
    /// True for plan domain failures, false for other failures.
    fn is_plan_error(err: &mlua::Error) -> bool {
        if let Some(Error::Plan(_)) = err.downcast_ref::<Error>() {
            return true;
        }
        format!("{err}").contains("plan error:")
    }

    /// Reports Lua domain status for a Lua failure.
    ///
    /// # Arguments
    ///
    /// * `err` - Lua failure.
    ///
    /// # Returns
    ///
    /// True for Lua domain failures, false for other failures.
    fn is_lua_error(err: &mlua::Error) -> bool {
        if let Some(Error::Lua(_)) = err.downcast_ref::<Error>() {
            return true;
        }
        format!("{err}").contains("lua error:")
    }

    /// Evaluates a Lua chunk into JSON.
    ///
    /// # Arguments
    ///
    /// * `lua` - prepared state.
    /// * `expr` - Lua chunk returning a table.
    ///
    /// # Returns
    ///
    /// JSON holding table data.
    fn eval_table(lua: &Lua, expr: &str) -> Json {
        let value: Value = lua.load(expr).eval().expect("evaluate");
        match value {
            Value::Table(table) => table_to_json(&table, "test").expect("convert"),
            other => panic!("expected table, got {other:?}"),
        }
    }

    #[test]
    fn path_jail_matrix() {
        let (_dir, lua) = setup(&[]);
        let root = lua
            .named_registry_value::<String>(PROJECT_ROOT_KEY)
            .expect("root stored");
        assert!(!root.is_empty());
        let root_path = Path::new(&root);
        for rel in [
            "resources/a.toml",
            "a.toml",
            "a/b/../c.toml",
            "./a.toml",
            "a/./b.toml",
        ] {
            let resolved = resolve_under_root(root_path, rel, "test").expect("inside");
            assert!(
                resolved.starts_with(root_path),
                "{rel} stays under root: {}",
                resolved.display()
            );
        }
        for rel in [
            "",
            "/abs.toml",
            "../escape.toml",
            "a/../../escape.toml",
            "..",
        ] {
            let err = resolve_under_root(root_path, rel, "test").expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain for {rel}: {err}");
        }
    }

    #[test]
    fn loads_all_three_formats() {
        let (_dir, lua) = setup(&[
            ("base.toml", "x = 1\n[n]\ny = 2\n"),
            ("extra.json", r#"{"j": true, "n": {"z": 3}}"#),
            ("extra.yaml", "y: 1\nn:\n  w: 4\n"),
        ]);
        let toml = eval_table(&lua, r#"return confit.resources.load_toml("base.toml")"#);
        assert_eq!(toml.get("x").and_then(Json::as_i64), Some(1));
        assert_eq!(
            toml.get("n")
                .and_then(|n| n.get("y"))
                .and_then(Json::as_i64),
            Some(2)
        );
        let json = eval_table(&lua, r#"return confit.resources.load_json("extra.json")"#);
        assert_eq!(json.get("j").and_then(Json::as_bool), Some(true));
        let yaml = eval_table(&lua, r#"return confit.resources.load_yaml("extra.yaml")"#);
        assert_eq!(yaml.get("y").and_then(Json::as_i64), Some(1));
        assert_eq!(
            yaml.get("n")
                .and_then(|n| n.get("w"))
                .and_then(Json::as_i64),
            Some(4)
        );
    }

    #[test]
    fn load_escape_and_absolute_are_plan_errors() {
        let (_dir, lua) = setup(&[("inside.toml", "x = 1\n")]);
        for expr in [
            r#"return confit.resources.load_toml("../escape.toml")"#,
            r#"return confit.resources.load_toml("/abs.toml")"#,
            r#"return confit.resources.load_json("a/../../escape.json")"#,
            r#"return confit.resources.load_yaml(42)"#,
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            if expr.contains("42") {
                assert!(is_lua_error(&err), "bad type is Lua: {err}");
            } else {
                assert!(is_plan_error(&err), "escape is plan: {err}");
                assert!(
                    format!("{err}").contains("escape.toml")
                        || format!("{err}").contains("abs.toml")
                        || format!("{err}").contains("escape.json"),
                    "names path: {err}"
                );
            }
        }
    }

    #[test]
    fn merge_deep_last_wins_arrays() {
        let (_dir, lua) = setup(&[]);
        let merged = eval_table(
            &lua,
            r#"return confit.resources.merge({a = 1, n = {x = 1, y = 1}, l = {1, 2}}, {n = {y = 2}, l = {3}})"#,
        );
        assert_eq!(merged.get("a").and_then(Json::as_i64), Some(1));
        let nested = merged.get("n").expect("nested");
        assert_eq!(nested.get("x").and_then(Json::as_i64), Some(1));
        assert_eq!(nested.get("y").and_then(Json::as_i64), Some(2));
        assert_eq!(
            merged.get("l").expect("list"),
            &Json::Array(vec![Json::from(3)])
        );
    }

    #[test]
    fn merge_list_append_concatenates_without_dedup() {
        let (_dir, lua) = setup(&[]);
        let merged = eval_table(
            &lua,
            r#"return confit.resources.merge({l = {1, 2}}, {l = {2, 3}}, {list_append = true})"#,
        );
        assert_eq!(
            merged.get("l").expect("list"),
            &Json::Array(vec![
                Json::from(1),
                Json::from(2),
                Json::from(2),
                Json::from(3)
            ])
        );
        let nested = eval_table(
            &lua,
            r#"return confit.resources.merge({n = {l = {1}}}, {n = {l = {2}}}, {list_append = true})"#,
        );
        assert_eq!(
            nested
                .get("n")
                .and_then(|n| n.get("l"))
                .expect("nested list"),
            &Json::Array(vec![Json::from(1), Json::from(2)])
        );
    }

    #[test]
    fn merge_shallow_replaces_nested_wholesale() {
        let (_dir, lua) = setup(&[]);
        let merged = eval_table(
            &lua,
            r#"return confit.resources.merge({n = {x = 1, y = 1}, keep = 1}, {n = {y = 2}}, {shallow = true})"#,
        );
        let nested = merged.get("n").expect("nested");
        assert!(
            nested.get("x").is_none(),
            "shallow drops base-only nested keys"
        );
        assert_eq!(nested.get("y").and_then(Json::as_i64), Some(2));
        assert_eq!(merged.get("keep").and_then(Json::as_i64), Some(1));
    }

    #[test]
    fn merge_unknown_option_is_plan_error_naming_key() {
        let (_dir, lua) = setup(&[]);
        let err = lua
            .load(r#"return confit.resources.merge({a = 1}, {b = 2}, {bogus = true})"#)
            .eval::<Value>()
            .expect_err("unknown key must fail");
        assert!(is_plan_error(&err), "unknown key is plan: {err}");
        assert!(format!("{err}").contains("bogus"), "names key: {err}");
    }

    #[test]
    fn merge_bad_types_are_lua_errors() {
        let (_dir, lua) = setup(&[]);
        for expr in [
            r#"return confit.resources.merge(42, {})"#,
            r#"return confit.resources.merge({}, "nope")"#,
            r#"return confit.resources.merge({}, {}, 42)"#,
            r#"return confit.resources.merge({f = function() end}, {})"#,
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_lua_error(&err), "bad types are Lua: {expr}: {err}");
        }
    }
}
