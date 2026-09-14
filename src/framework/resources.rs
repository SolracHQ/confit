//! Resources
//!
//! Root-relative file loads for Lua.

use std::path::{Component, Path, PathBuf};

use mlua::{Lua, Value};

use serde_json::Value as Json;

use super::confit_table;

/// Named-registry key holding the project root string for Lua callbacks.
///
/// `install` stores the root under this key so `load_*` and
/// future path helpers resolve under the same `--root`. Readers use
/// `lua.named_registry_value::<String>(PROJECT_ROOT_KEY)`.
pub const PROJECT_ROOT_KEY: &str = "confit.project_root";

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

    let text_root = root.clone();
    let load_text =
        lua.create_function(move |lua, path: Value| load_text_impl(lua, &text_root, path))?;
    resources.set("load_text", load_text)?;

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

/// Loads a text file as a Lua string.
///
/// # Arguments
///
/// * `_lua` - state, unused for plain text.
/// * `root` - project root.
/// * `path` - raw Lua path argument.
///
/// # Returns
///
/// File text.
///
/// # Errors
///
/// Fails with plan errors for read failures.
fn load_text_impl(_lua: &Lua, root: &Path, path: Value) -> mlua::Result<String> {
    const CALLER: &str = "resources.load_text";
    let rel = take_rel_path(path, CALLER)?;
    let full = resolve_under_root(root, &rel, CALLER)?;
    std::fs::read_to_string(&full).map_err(|err| plan_err!("{CALLER}: cannot read '{rel}': {err}"))
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
            Value::Table(table) => {
                super::super::document::table_to_json(&table, "test").expect("convert")
            }
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
    fn load_text_returns_file_bytes() {
        let (_dir, lua) = setup(&[("note.txt", "hello {{ name }}\n")]);
        let text: String = lua
            .load(r#"return confit.resources.load_text("note.txt")"#)
            .eval()
            .expect("evaluate");
        assert_eq!(text, "hello {{ name }}\n");
    }

    #[test]
    fn load_text_escape_and_bad_types_fail() {
        let (_dir, lua) = setup(&[("inside.txt", "hi\n")]);
        for expr in [
            r#"return confit.resources.load_text("../escape.txt")"#,
            r#"return confit.resources.load_text("/abs.txt")"#,
            r#"return confit.resources.load_text(42)"#,
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            if expr.contains("42") {
                assert!(is_lua_error(&err), "bad type is Lua: {err}");
            } else {
                assert!(is_plan_error(&err), "escape is plan: {err}");
            }
        }
        let missing = lua
            .load(r#"return confit.resources.load_text("missing.txt")"#)
            .eval::<Value>()
            .expect_err("must fail");
        assert!(is_plan_error(&missing), "missing file is plan: {missing}");
    }
}
