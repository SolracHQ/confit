//! Loader
//!
//! Scoped require with sandboxed chunk loading over one session.
//! Module tables ride read-only proxies, cycles fail naming the
//! loop, unknown names fail.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mlua::{Function, Lua, Table, Value};

use confit_driver as driver;
use confit_store::Stores;
use confit_surface_api::ApiError;

use crate::error::{EngineError, Result};

const AUDITED: &[&str] = &[
    "assert", "error", "ipairs", "next", "pairs", "pcall", "select", "tonumber", "tostring", "type",
];

const LIBRARIES: &[&str] = &["string", "table", "math", "utf8"];

/// Module cache key over builtin names with canonical file paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum CacheKey {
    /// Builtin module under its dotted name.
    Builtin(String),
    /// File module under its canonical path.
    File(PathBuf),
}

/// Shared loader state.
struct Session {
    /// Module values under builtin names with canonical paths.
    cache: RefCell<HashMap<CacheKey, Value>>,
    /// Module names under loading.
    loading: RefCell<Vec<String>>,
}

impl Session {
    fn new() -> Self {
        Self {
            cache: RefCell::new(HashMap::new()),
            loading: RefCell::new(Vec::new()),
        }
    }

    fn cached(&self, key: &CacheKey) -> Option<Value> {
        self.cache.borrow().get(key).cloned()
    }

    fn store(&self, key: CacheKey, value: Value) {
        self.cache.borrow_mut().insert(key, value);
    }

    fn push(&self, name: String) {
        self.loading.borrow_mut().push(name);
    }

    fn pop(&self) {
        let _ = self.loading.borrow_mut().pop();
    }

    fn cycle(&self, key: &str) -> Option<String> {
        let stack = self.loading.borrow();
        stack.iter().position(|held| held == key).map(|start| {
            let mut loop_ = stack[start..].join(" -> ");
            loop_.push_str(" -> ");
            loop_.push_str(key);
            loop_
        })
    }
}

/// Installs the scoped require function on the globals.
///
/// The bare confit global stays nil.
///
/// # Errors
///
/// - [`EngineError::Lua`] for failed app data and global writes.
pub fn install(lua: &Lua) -> Result<()> {
    lua.set_app_data(Session::new());
    confit_surface_api::config::reset(lua)?;
    let require = lua
        .create_function(|lua, name: String| run_require(lua, &name).map_err(mlua::Error::from))?;
    lua.globals().set("require", require)?;
    Ok(())
}

/// Loads one profile chunk under the sandbox.
///
/// # Errors
///
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
/// - [`EngineError::Lua`] for Lua syntax and runtime failures.
pub fn load_chunk(lua: &Lua, path: &Path) -> Result<Value> {
    let text = read_text(path)?;
    let require: Function = lua.globals().get("require")?;
    let env = environment(lua, require)?;
    let label = format!("@{}", path.display());
    Ok(compile(lua, &text, label, env)?.call::<Value>(())?)
}

/// Reads one chunk file as text.
///
/// # Errors
///
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
fn read_text(path: &Path) -> Result<String> {
    let display = path.display().to_string();
    let bytes = driver::fs::read(path).map_err(|error| EngineError::Read {
        path: display.clone(),
        reason: error.to_string(),
    })?;
    String::from_utf8(bytes).map_err(|_| EngineError::Utf8 { path: display })
}

/// Builds one sandboxed environment with audited globals and require.
///
/// Unknown globals read nil.
///
/// # Errors
///
/// - [`EngineError::Lua`] for failed global reads and table writes.
fn environment(lua: &Lua, require: Function) -> Result<Table> {
    let env = lua.create_table()?;
    let globals = lua.globals();
    for name in AUDITED {
        env.set(*name, globals.get::<Value>(*name)?)?;
    }
    for name in LIBRARIES {
        env.set(*name, globals.get::<Value>(*name)?)?;
    }
    env.set("require", require)?;
    Ok(env)
}

/// Compiles one source text.
///
/// # Errors
///
/// - [`EngineError::Lua`] for Lua syntax failures.
fn compile<'chunk>(
    lua: &'chunk Lua,
    source: &'chunk str,
    name: String,
    env: Table,
) -> Result<mlua::chunk::Chunk<'chunk>> {
    Ok(lua.load(source).set_name(name).set_environment(env))
}

/// Runs one require call for builtins with file modules.
///
/// # Errors
///
/// - [`ApiError::UnknownModule`] for unknown names.
/// - [`ApiError::Escape`] for escaping names.
/// - [`ApiError::Cycle`] for require cycles.
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
/// - [`EngineError::Lua`] for loader and chunk failures.
fn run_require(lua: &Lua, name: &str) -> Result<Value> {
    if name.starts_with("confit.") {
        return builtin_require(lua, name);
    }
    file_require(lua, name)
}

/// Runs one builtin require.
///
/// # Errors
///
/// - [`ApiError::UnknownModule`] for unknown names.
/// - [`ApiError::Cycle`] for require cycles.
/// - [`EngineError::Lua`] for module build failures.
fn builtin_require(lua: &Lua, name: &str) -> Result<Value> {
    let key = CacheKey::Builtin(name.to_owned());
    if let Some(hit) = with_session(lua, |session| session.cached(&key))? {
        return Ok(hit);
    }
    if let Some(loop_) = with_session(lua, |session| session.cycle(name))? {
        return Err(ApiError::Cycle { loop_ }.into());
    }
    with_session(lua, |session| session.push(name.to_owned()))?;
    let built = confit_surface_api::builtin(lua, name);
    with_session(lua, |session| session.pop())?;
    let value = built?;
    with_session(lua, |session| session.store(key, value.clone()))?;
    Ok(value)
}

/// Runs one file require.
///
/// Names spell root-relative files with an optional lua suffix.
///
/// # Errors
///
/// - [`ApiError::Escape`] for escaping names.
/// - [`ApiError::Cycle`] for require cycles.
/// - [`ApiError::UnknownModule`] for missing files.
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
/// - [`EngineError::Lua`] for read and chunk failures.
fn file_require(lua: &Lua, name: &str) -> Result<Value> {
    let root = stored_root(lua)?;
    let path = resolve(&root, name)?;
    let key = CacheKey::File(path.clone());
    if let Some(hit) = with_session(lua, |session| session.cached(&key))? {
        return Ok(hit);
    }
    let tag = path.display().to_string();
    if let Some(loop_) = with_session(lua, |session| session.cycle(&tag))? {
        return Err(ApiError::Cycle { loop_ }.into());
    }
    with_session(lua, |session| session.push(tag))?;
    let loaded = load_file(lua, &path, name);
    with_session(lua, |session| session.pop())?;
    let value = loaded?;
    if !value.is_nil() {
        with_session(lua, |session| session.store(key, value.clone()))?;
    }
    Ok(value)
}

/// Resolves one file module name to its jailed canonical path.
///
/// # Errors
///
/// - [`ApiError::Escape`] for escaping names.
fn resolve(root: &Path, name: &str) -> Result<PathBuf> {
    const CALLER: &str = "require";
    let rel = name.strip_suffix(".lua").unwrap_or(name);
    let parts =
        confit_surface_api::paths::split_checked(rel, false).map_err(|fault| ApiError::Escape {
            caller: CALLER.to_owned(),
            input: name.to_owned(),
            reason: fault.reason().to_owned(),
        })?;
    let mut full = root.to_path_buf();
    for part in &parts {
        full.push(part);
    }
    full.set_extension("lua");
    if !confit_surface_api::resource::normalize(&full)
        .starts_with(confit_surface_api::resource::normalize(root))
    {
        return Err(ApiError::Escape {
            caller: CALLER.to_owned(),
            input: name.to_owned(),
            reason: format!("'{name}' escapes its root"),
        }
        .into());
    }
    Ok(full)
}

/// Loads one file module.
///
/// # Errors
///
/// - [`ApiError::UnknownModule`] for missing files.
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
/// - [`EngineError::Lua`] for Lua syntax and runtime failures.
fn load_file(lua: &Lua, path: &Path, name: &str) -> Result<Value> {
    let display = path.display().to_string();
    let bytes = driver::fs::read(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => ApiError::UnknownModule {
            name: name.to_owned(),
        }
        .into(),
        _ => EngineError::Read {
            path: display.clone(),
            reason: error.to_string(),
        },
    })?;
    let text = String::from_utf8(bytes).map_err(|_| EngineError::Utf8 { path: display })?;
    let require: Function = lua.globals().get("require")?;
    let env = environment(lua, require)?;
    let label = format!("@{}", path.display());
    Ok(compile(lua, &text, label, env)?.call::<Value>(())?)
}

/// Runs one closure against shared loader state.
///
/// # Errors
///
/// - [`EngineError::Lua`] while the loader stays uninstalled.
fn with_session<T>(lua: &Lua, op: impl FnOnce(&Session) -> T) -> Result<T> {
    match lua.app_data_ref::<Session>() {
        Some(session) => Ok(op(&session)),
        None => Err(EngineError::Lua(mlua::Error::RuntimeError(
            "confit loader is not installed".to_owned(),
        ))),
    }
}

/// Reads the project root from the shared stores.
///
/// # Errors
///
/// - [`EngineError::Lua`] while the api stays uninstalled.
fn stored_root(lua: &Lua) -> Result<PathBuf> {
    match lua.app_data_ref::<Stores>() {
        Some(stores) => Ok(stores.resources().roots.project_root.clone()),
        None => Err(EngineError::Lua(mlua::Error::RuntimeError(
            "confit api is not installed".to_owned(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::fs::TestGuard;
    use confit_store::{StoreRoots, Stores};

    fn test_stores(dir: &Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                cache_base: dir.join("cache"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            sender,
        )
    }

    fn project_root(dir: &Path) -> PathBuf {
        let root = dir.join("project");
        driver::fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        lua.set_app_data(stores.clone());
        install(&lua).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        let require: Function = lua.globals().get("require")?;
        let env = environment(lua, require)?;
        compile(lua, source, "@test".to_owned(), env)?.call(())
    }

    #[test]
    fn missing_profiles_fail_structured() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let lua = test_lua(&stores);
        let missing = root.join("absent.lua");
        match load_chunk(&lua, &missing) {
            Ok(_) => panic!("missing profile passes"),
            Err(crate::error::EngineError::Read { path, .. }) => {
                assert!(path.contains("absent.lua"), "names the file: {path}");
            }
            Err(other) => panic!("missing profile flattens: {other}"),
        }
    }

    #[test]
    fn encoded_profiles_fail_structured() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let profile = root.join("profile.lua");
        driver::fs::write(&profile, &[0xff, 0xfe]).unwrap();
        let lua = test_lua(&stores);
        match load_chunk(&lua, &profile) {
            Ok(_) => panic!("encoded profile passes"),
            Err(crate::error::EngineError::Utf8 { path }) => {
                assert!(path.contains("profile.lua"), "names the file: {path}");
            }
            Err(other) => panic!("encoded profile flattens: {other}"),
        }
    }

    #[test]
    fn encoded_modules_fail_structured() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        driver::fs::write(&root.join("broken.lua"), &[0xff, 0xfe]).unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"broken\")") {
            Ok(_) => panic!("encoded module passes"),
            Err(error) => match crate::error::EngineError::from(error) {
                crate::error::EngineError::Utf8 { path } => {
                    assert!(path.contains("broken.lua"), "names the file: {path}");
                }
                other => panic!("encoded module flattens: {other}"),
            },
        }
    }

    #[test]
    fn unknown_names_fail() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        for name in ["confit.nope", "absent"] {
            let source = format!("return require({name:?})");
            match run(&lua, &source) {
                Ok(_) => panic!("unknown module passes: {name}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains("unknown module"), "names the miss: {text}");
                    assert!(text.contains(name), "names the module: {text}");
                }
            }
        }
    }

    #[test]
    fn file_modules_load_under_root() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        driver::fs::write(&root.join("helper.lua"), b"return 41").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"helper\")") {
            Ok(Value::Integer(found)) => assert_eq!(found, 41),
            Ok(_) => panic!("file module answers its return"),
            Err(error) => panic!("file modules load: {error}"),
        }
    }

    #[test]
    fn escaping_names_fail() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"../outside\")") {
            Ok(_) => panic!("escaping require passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("escape"), "names the jail: {text}");
            }
        }
    }

    #[test]
    fn cycles_name_the_loop() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        driver::fs::write(
            &root.join("loop_a.lua"),
            b"local _ = require(\"loop_b\") return 1",
        )
        .unwrap();
        driver::fs::write(
            &root.join("loop_b.lua"),
            b"local _ = require(\"loop_a\") return 2",
        )
        .unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"loop_a\")") {
            Ok(_) => panic!("require cycle passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("require cycle"), "names the loop: {text}");
                assert!(text.contains("loop_a"), "names the member: {text}");
            }
        }
    }

    #[test]
    fn bare_confit_global_stays_nil() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        match run(&lua, "return confit") {
            Ok(Value::Nil) => {}
            Ok(_) => panic!("bare confit global answers a value"),
            Err(error) => panic!("sandbox reads the global: {error}"),
        }
    }

    #[test]
    fn unknown_globals_read_nil() {
        let lua = Lua::new();
        match run(&lua, "return type(os)") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "nil"),
            Ok(_) => panic!("unknown globals answer strings"),
            Err(error) => panic!("sandbox reads unknown globals: {error}"),
        }
    }

    #[test]
    fn audited_globals_with_libraries_answer() {
        let lua = Lua::new();
        match run(&lua, "return type(pairs)") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "function"),
            Ok(_) => panic!("audited globals answer strings"),
            Err(error) => panic!("sandbox reads audited globals: {error}"),
        }
        match run(&lua, "return (\"hi\"):upper()") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "HI"),
            Ok(_) => panic!("string libraries answer strings"),
            Err(error) => panic!("sandbox reads libraries: {error}"),
        }
    }

    #[test]
    fn proxies_reject_writes_naming_readonly() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        match run(&lua, "local path = require(\"confit.path\") path.home = 1") {
            Ok(_) => panic!("proxy write passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("read-only"), "names the proxy: {text}");
                assert!(text.contains("confit.path"), "names the module: {text}");
            }
        }
    }
}
