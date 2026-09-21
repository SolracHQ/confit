//! Require
//!
//! Scoped Lua file loading jailed to one folder.

use std::path::{Component, Path, PathBuf};

use mlua::{Function, Lua, Table, Value};

use crate::error::plan_error;

/// Registry key holding scoped require results by file.
const REQUIRE_CACHE_KEY: &str = "confit.require_cache";

/// Builds one chunk environment holding a scoped require.
pub(crate) fn chunk_env(lua: &Lua, requirer: Function) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    env.set("require", requirer)?;
    let fallback = lua.create_table()?;
    fallback.set("__index", lua.globals())?;
    env.set_metatable(Some(fallback))?;
    Ok(env)
}

/// Scoped require owner holding the resolution triple.
///
/// The current folder, the jail root, and the caller scope travel
/// together, so resolution methods read them from self.
///
#[derive(Debug, Clone)]
pub(crate) struct Requirer {
    /// Folder holding the requiring file.
    pub(crate) current: PathBuf,
    /// Jail root holding every loadable module.
    pub(crate) folder: PathBuf,
    /// Caller name for error lines, like `profile` or `plugin`.
    pub(crate) scope: &'static str,
}

impl Requirer {
    /// Builds one scoped require resolving inside a folder.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the require closure.
    ///
    /// # Returns
    ///
    /// Lua function resolving dotted modules under the jail root.
    ///
    /// # Errors
    ///
    /// Closure creation failures fail as Lua errors.
    ///
    pub(crate) fn make(&self, lua: &Lua) -> mlua::Result<Function> {
        let owner = self.clone();
        lua.create_function(move |lua, request: Value| {
            let request = match request {
                Value::String(text) => text.to_string_lossy(),
                _ => {
                    return Err(plan_error(format!(
                        "{}.require: field 'module' must be a string",
                        owner.scope
                    )));
                }
            };
            owner.require(lua, &request)
        })
    }

    /// Resolves one scoped require request into a cached value.
    fn require(&self, lua: &Lua, request: &str) -> mlua::Result<Value> {
        let caller = format!("{}.require", self.scope);
        let file = self.resolve(request)?;
        let cache: Table = lua.named_registry_value(REQUIRE_CACHE_KEY)?;
        let slot = file.display().to_string();
        if let value @ (Value::Table(_) | Value::Function(_) | Value::String(_)) =
            cache.get::<Value>(slot.as_str())?
        {
            return Ok(value);
        }
        let source = std::fs::read_to_string(&file)
            .map_err(|error| plan_error(format!("{caller}: cannot read '{request}': {error}")))?;
        let parent = file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.folder.clone());
        let child = Requirer {
            current: parent,
            folder: self.folder.clone(),
            scope: self.scope,
        };
        let requirer = child.make(lua)?;
        let env = chunk_env(lua, requirer)?;
        let value: Value = lua
            .load(&source)
            .set_name(format!("@{}", file.display()))
            .set_environment(env)
            .call(())?;
        cache.set(slot.as_str(), value.clone())?;
        Ok(value)
    }

    /// Resolves one scoped require request inside a folder.
    ///
    /// Both sides normalize lexically, so roots holding `..`
    /// segments compare correctly.
    fn resolve(&self, request: &str) -> mlua::Result<PathBuf> {
        let caller = format!("{}.require", self.scope);
        if request.is_empty() {
            return Err(plan_error(format!(
                "{caller}: field 'module' must not be empty"
            )));
        }
        let mut stack: Vec<String> = Vec::new();
        for part in request.split('.') {
            if part.is_empty() {
                return Err(plan_error(format!("{caller}: bad module '{request}'")));
            }
            stack.push(part.to_string());
        }
        let mut base = self.current.clone();
        for part in &stack {
            base.push(part);
        }
        base.set_extension("lua");
        let normalized = Self::normalize(&base)
            .ok_or_else(|| plan_error(format!("{caller}: module '{request}' escapes its root")))?;
        let root = Self::normalize(&self.folder).unwrap_or_else(|| self.folder.clone());
        if !normalized.starts_with(&root) {
            return Err(plan_error(format!(
                "{caller}: module '{request}' escapes its root"
            )));
        }
        Ok(normalized)
    }

    /// Normalizes one path lexically, resolving `.` and `..`.
    ///
    /// Returns None while `..` climbs above the path start.
    fn normalize(path: &Path) -> Option<PathBuf> {
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    if !out.pop() {
                        return None;
                    }
                }
                Component::CurDir => {}
                other => out.push(other),
            }
        }
        Some(out)
    }
}
