//! Provide stock require with project containment and plugin audit.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, Table, Value};

use confit_driver as driver;
use confit_store::Stores;

use crate::api::builtin;
use crate::project::PluginRoot;

use crate::error::{EngineError, Result};

const AUDITED: &[&str] = &[
    "assert", "error", "ipairs", "next", "pairs", "pcall", "select", "tonumber", "tostring", "type",
];

const LIBRARIES: &[&str] = &["string", "table", "math", "utf8"];

const PLUGIN_ROOT: &str = "plugins";

const INIT_ENTRY: &str = "init.lua";

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

/// Install the scoped require function on the globals.
///
/// - The global form resolves project relative as a project caller.
///
/// # Errors
///
/// - [`EngineError::Lua`] for failed app data and global writes.
pub fn install(lua: &Lua) -> Result<()> {
    lua.set_app_data(Session::new());
    let require = lua.create_function(|lua, name: String| {
        run_require(lua, &name, None).map_err(mlua::Error::from)
    })?;
    lua.globals().set("require", require)?;
    Ok(())
}

/// Load one profile chunk under the sandbox.
///
/// # Errors
///
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
/// - [`EngineError::Lua`] for Lua syntax and runtime failures.
pub fn load_chunk(lua: &Lua, path: &Path) -> Result<Value> {
    let text = read_text(path)?;
    let caller = path.parent().map(Path::to_path_buf);
    let env = environment(lua, caller)?;
    let label = format!("@{}", path.display());
    Ok(compile(lua, &text, label, env)?.call::<Value>(())?)
}

/// Read one chunk file as text.
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

/// Build one sandboxed environment with audited globals and require.
///
/// - Unknown globals are nil.
///
/// # Errors
///
/// - [`EngineError::Lua`] for failed global reads and table writes.
fn environment(lua: &Lua, caller: Option<PathBuf>) -> Result<Table> {
    let env = lua.create_table()?;
    let globals = lua.globals();
    for name in AUDITED {
        env.set(*name, globals.get::<Value>(*name)?)?;
    }
    for name in LIBRARIES {
        env.set(*name, globals.get::<Value>(*name)?)?;
    }
    let require = lua.create_function(move |lua, name: String| {
        run_require(lua, &name, caller.as_deref()).map_err(mlua::Error::from)
    })?;
    env.set("require", require)?;
    Ok(env)
}

/// Compile one source text.
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

/// Run one require call for builtins with file modules.
///
/// # Errors
///
/// - [`EngineError::UnknownModule`] for unknown names.
/// - [`EngineError::Escape`] for escaping names.
/// - [`EngineError::Cycle`] for require cycles.
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
/// - [`EngineError::Lua`] for loader and chunk failures.
fn run_require(lua: &Lua, name: &str, caller: Option<&Path>) -> Result<Value> {
    if name.starts_with("confit.") {
        return builtin_require(lua, name);
    }
    file_require(lua, name, caller)
}

/// Run one builtin require.
///
/// # Errors
///
/// - [`EngineError::UnknownModule`] for unknown names.
/// - [`EngineError::Cycle`] for require cycles.
/// - [`EngineError::Lua`] for module build failures.
fn builtin_require(lua: &Lua, name: &str) -> Result<Value> {
    let key = CacheKey::Builtin(name.to_owned());
    if let Some(hit) = with_session(lua, |session| session.cached(&key))? {
        return Ok(hit);
    }
    if let Some(loop_) = with_session(lua, |session| session.cycle(name))? {
        return Err(EngineError::Cycle { loop_ });
    }
    with_session(lua, |session| session.push(name.to_owned()))?;
    let built = builtin(lua, name);
    with_session(lua, |session| session.pop())?;
    let value = built?;
    with_session(lua, |session| session.store(key, value.clone()))?;
    Ok(value)
}

/// Run one file require.
///
/// - Names resolve project relative with entry fallback.
/// - Plugin callers hold inside the plugin tree.
///
/// # Errors
///
/// - [`EngineError::Escape`] for escaping names.
/// - [`EngineError::Escape`] for plugin callers outside the tree.
/// - [`EngineError::Cycle`] for require cycles.
/// - [`EngineError::UnknownModule`] for missing files.
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
/// - [`EngineError::Lua`] for read and chunk failures.
fn file_require(lua: &Lua, name: &str, caller: Option<&Path>) -> Result<Value> {
    let root = stored_root(lua)?;
    let root_real = canonical_root(&root)?;
    let held = stored_plugins(lua);
    let fallback = root_real.join(PLUGIN_ROOT);
    let plugins = held.as_ref().map(|scope| &scope.dir).unwrap_or(&fallback);
    let parts = map_name(name)?;
    if parts.is_empty() {
        return Err(EngineError::UnknownModule {
            name: name.to_owned(),
        });
    }
    let (base, scope, rest) = match parts.first() {
        Some(first) if first == PLUGIN_ROOT => (plugins.as_path(), plugins.as_path(), &parts[1..]),
        _ => (root.as_path(), root_real.as_path(), &parts[..]),
    };
    let path = resolve(base, scope, rest, name)?;
    audit(plugins, caller, &path, name)?;
    let key = CacheKey::File(path.clone());
    if let Some(hit) = with_session(lua, |session| session.cached(&key))? {
        return Ok(hit);
    }
    let tag = path.display().to_string();
    if let Some(loop_) = with_session(lua, |session| session.cycle(&tag))? {
        return Err(EngineError::Cycle { loop_ });
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

/// Canonicalize one project root.
///
/// # Errors
///
/// - [`EngineError::Read`] for unreadable roots.
fn canonical_root(root: &Path) -> Result<PathBuf> {
    driver::fs::canonicalize(root).map_err(|error| EngineError::Read {
        path: root.display().to_string(),
        reason: error.to_string(),
    })
}

/// Resolve path parts to their contained canonical path.
///
/// - Parts resolve under the base with entry fallback.
///
/// # Errors
///
/// - [`EngineError::Escape`] for escaping names.
/// - [`EngineError::UnknownModule`] for missing files.
/// - [`EngineError::Read`] for unreadable files.
fn resolve(base: &Path, scope: &Path, parts: &[String], name: &str) -> Result<PathBuf> {
    let mut direct = base.to_path_buf();
    for part in parts {
        direct.push(part);
    }
    direct.set_extension("lua");
    let mut entry = base.to_path_buf();
    for part in parts {
        entry.push(part);
    }
    entry.push(INIT_ENTRY);
    let found = match driver::fs::canonicalize(&direct) {
        Ok(hit) => hit,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            driver::fs::canonicalize(&entry).map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => EngineError::UnknownModule {
                    name: name.to_owned(),
                },
                _ => EngineError::Read {
                    path: entry.display().to_string(),
                    reason: error.to_string(),
                },
            })?
        }
        Err(error) => {
            return Err(EngineError::Read {
                path: direct.display().to_string(),
                reason: error.to_string(),
            });
        }
    };
    scoped(scope, found, name)
}

/// Map one require name to its path parts.
///
/// - Every dot becomes a slash with empty components collapsing.
///
/// # Errors
///
/// - [`EngineError::Escape`] for escaping names.
fn map_name(name: &str) -> Result<Vec<String>> {
    if name.contains('\\') {
        return Err(refuse(name, "backslashes are not supported"));
    }
    if name.len() >= 2 && name.as_bytes()[0].is_ascii_alphabetic() && name.as_bytes()[1] == b':' {
        return Err(refuse(name, "drive prefixes are not supported"));
    }
    if name.starts_with('/') {
        return Err(refuse(name, "expected a relative path"));
    }
    Ok(name
        .replace('.', "/")
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Hold one plugin caller inside the plugin tree.
///
/// - Project callers require anything under the project root.
///
/// # Errors
///
/// - [`EngineError::Escape`] for plugin callers outside the tree.
fn audit(plugins: &Path, caller: Option<&Path>, target: &Path, name: &str) -> Result<()> {
    const CALLER: &str = "require";
    let Some(caller) = caller else {
        return Ok(());
    };
    let Ok(caller_real) = driver::fs::canonicalize(caller) else {
        return Ok(());
    };
    if caller_real.starts_with(plugins) && !target.starts_with(plugins) {
        return Err(EngineError::Escape {
            caller: CALLER.to_owned(),
            input: name.to_owned(),
            reason: format!("'{name}' escapes '{}'", plugins.display()),
        });
    }
    Ok(())
}

/// Refuse one require name with its scope reason.
fn refuse(name: &str, reason: &str) -> EngineError {
    EngineError::Escape {
        caller: "require".to_owned(),
        input: name.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Hold one canonical path inside its canonical scope.
///
/// # Errors
///
/// - [`EngineError::Escape`] for escaping names.
fn scoped(scope: &Path, full: PathBuf, name: &str) -> Result<PathBuf> {
    const CALLER: &str = "require";
    if !full.starts_with(scope) {
        return Err(EngineError::Escape {
            caller: CALLER.to_owned(),
            input: name.to_owned(),
            reason: format!("'{name}' is outside '{}'", scope.display()),
        });
    }
    Ok(full)
}

/// Load one file module.
///
/// # Errors
///
/// - [`EngineError::UnknownModule`] for missing files.
/// - [`EngineError::Read`] for unreadable files.
/// - [`EngineError::Utf8`] for non-UTF8 files.
/// - [`EngineError::Lua`] for Lua syntax and runtime failures.
fn load_file(lua: &Lua, path: &Path, name: &str) -> Result<Value> {
    let display = path.display().to_string();
    let bytes = driver::fs::read(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => EngineError::UnknownModule {
            name: name.to_owned(),
        },
        _ => EngineError::Read {
            path: display.clone(),
            reason: error.to_string(),
        },
    })?;
    let text = String::from_utf8(bytes).map_err(|_| EngineError::Utf8 { path: display })?;
    let caller = path.parent().map(Path::to_path_buf);
    let env = environment(lua, caller)?;
    let label = format!("@{}", path.display());
    Ok(compile(lua, &text, label, env)?.call::<Value>(())?)
}

/// Run one closure against shared loader state.
///
/// # Errors
///
/// - [`EngineError::Lua`] for an uninstalled loader.
fn with_session<T>(lua: &Lua, op: impl FnOnce(&Session) -> T) -> Result<T> {
    match lua.app_data_ref::<Session>() {
        Some(session) => Ok(op(&session)),
        None => Err(EngineError::Lua(mlua::Error::RuntimeError(
            "confit loader is not installed".to_owned(),
        ))),
    }
}

/// Resolve the plugin root from the shared app data.
fn stored_plugins(lua: &Lua) -> Option<PluginRoot> {
    lua.app_data_ref::<PluginRoot>().map(|scope| scope.clone())
}

/// Resolve the project root from the shared stores.
///
/// # Errors
///
/// - [`EngineError::Lua`] for an uninstalled api.
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
        let env = environment(lua, None)?;
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
                assert!(
                    path.contains("absent.lua"),
                    "the path shows the file: {path}"
                );
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
                assert!(
                    path.contains("profile.lua"),
                    "the path shows the file: {path}"
                );
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
                    assert!(
                        path.contains("broken.lua"),
                        "the path shows the file: {path}"
                    );
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
                    assert!(
                        text.contains("unknown module"),
                        "the text shows the miss: {text}"
                    );
                    assert!(text.contains(name), "the text shows the module: {text}");
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
            Ok(_) => panic!("file module returns an integer"),
            Err(error) => panic!("file modules load: {error}"),
        }
    }

    #[test]
    fn dotted_names_map_to_slashes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let folder = root.join("nested");
        driver::fs::create_dir_all(&folder).unwrap();
        driver::fs::write(&folder.join("helper.lua"), b"return 41").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"nested.helper\")") {
            Ok(Value::Integer(found)) => assert_eq!(found, 41),
            Ok(_) => panic!("dotted module returns an integer"),
            Err(error) => panic!("dotted names load: {error}"),
        }
    }

    #[test]
    fn empty_components_collapse() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let folder = root.join("nested");
        driver::fs::create_dir_all(&folder).unwrap();
        driver::fs::write(&folder.join("helper.lua"), b"return 41").unwrap();
        let lua = test_lua(&stores);
        for name in ["nested..helper", "nested//helper"] {
            let source = format!("return require({name:?})");
            match run(&lua, &source) {
                Ok(Value::Integer(found)) => assert_eq!(found, 41),
                Ok(_) => panic!("collapsed module returns an integer: {name}"),
                Err(error) => panic!("collapsed names load: {error}"),
            }
        }
    }

    #[test]
    fn dotdot_collapses_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        driver::fs::write(&root.join("tools.lua"), b"return 3").unwrap();
        let nested = root.join("profiles");
        driver::fs::create_dir_all(&nested).unwrap();
        driver::fs::write(
            &nested.join("mainframe.lua"),
            b"return require(\"../tools\") * 14",
        )
        .unwrap();
        let lua = test_lua(&stores);
        match load_chunk(&lua, &nested.join("mainframe.lua")) {
            Ok(Value::Integer(found)) => assert_eq!(found, 42),
            Ok(_) => panic!("collapsed parent returns an integer"),
            Err(error) => panic!("collapsed parents load: {error}"),
        }
    }

    #[test]
    fn lua_suffix_misses_like_stock() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        driver::fs::write(&root.join("helper.lua"), b"return 41").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"helper.lua\")") {
            Ok(_) => panic!("suffixed name passes"),
            Err(error) => {
                let text = crate::error::EngineError::from(error).to_string();
                assert!(
                    text.contains("unknown module"),
                    "suffixed names miss: {text}"
                );
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
                assert!(
                    text.contains("require cycle"),
                    "the text shows the loop: {text}"
                );
                assert!(text.contains("loop_a"), "the text shows the member: {text}");
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
            Ok(_) => panic!("bare confit global stays nil"),
            Err(error) => panic!("sandbox reads the global: {error}"),
        }
    }

    #[test]
    fn unknown_globals_read_nil() {
        let lua = Lua::new();
        match run(&lua, "return type(os)") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "nil"),
            Ok(_) => panic!("unknown globals return strings"),
            Err(error) => panic!("sandbox reads unknown globals: {error}"),
        }
    }

    #[test]
    fn audited_globals_with_libraries_return() {
        let lua = Lua::new();
        match run(&lua, "return type(pairs)") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "function"),
            Ok(_) => panic!("audited globals return strings"),
            Err(error) => panic!("sandbox reads audited globals: {error}"),
        }
        match run(&lua, "return (\"hi\"):upper()") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "HI"),
            Ok(_) => panic!("string libraries return strings"),
            Err(error) => panic!("sandbox reads libraries: {error}"),
        }
    }

    #[test]
    fn proxies_reject_writes_showing_readonly() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        match run(
            &lua,
            "local validate = require(\"confit.validate\") validate.document = 1",
        ) {
            Ok(_) => panic!("proxy write passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("read-only"),
                    "the text shows the proxy: {text}"
                );
                assert!(
                    text.contains("confit.validate"),
                    "the text shows the module: {text}"
                );
            }
        }
    }

    #[test]
    fn init_entries_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let entry = root.join("plugins").join("alice").join("tool");
        driver::fs::create_dir_all(&entry).unwrap();
        driver::fs::write(&entry.join("init.lua"), b"return 77").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"plugins.alice.tool\")") {
            Ok(Value::Integer(found)) => assert_eq!(found, 77),
            Ok(_) => panic!("plugin entry returns an integer"),
            Err(error) => panic!("plugin entry loads: {error}"),
        }
    }

    #[test]
    fn deep_plugin_paths_load() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let entry = root.join("plugins").join("_").join("sugar");
        driver::fs::create_dir_all(&entry).unwrap();
        driver::fs::write(&entry.join("helper.lua"), b"return 7").unwrap();
        driver::fs::write(
            &entry.join("init.lua"),
            b"local helper = require(\"plugins._.sugar.helper\") return helper * 6",
        )
        .unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"plugins._.sugar\")") {
            Ok(Value::Integer(found)) => assert_eq!(found, 42),
            Ok(_) => panic!("deep plugin path returns an integer"),
            Err(error) => panic!("deep plugin paths load: {error}"),
        }
    }

    #[test]
    fn plugin_callers_hold_inside_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let tools = root.join("tools");
        driver::fs::create_dir_all(&tools).unwrap();
        driver::fs::write(&tools.join("helper.lua"), b"return 1").unwrap();
        let entry = root.join("plugins").join("_").join("sugar");
        driver::fs::create_dir_all(&entry).unwrap();
        driver::fs::write(&entry.join("init.lua"), b"return require(\"tools.helper\")").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"plugins._.sugar\")") {
            Ok(_) => panic!("plugin escape passes"),
            Err(error) => {
                let text = crate::error::EngineError::from(error).to_string();
                assert!(text.contains("plugins"), "the text shows the tree: {text}");
            }
        }
    }

    #[test]
    fn project_callers_require_anything_contained() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let entry = root.join("plugins").join("_").join("sugar");
        driver::fs::create_dir_all(&entry).unwrap();
        driver::fs::write(&entry.join("init.lua"), b"return 9").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"plugins._.sugar\")") {
            Ok(Value::Integer(found)) => assert_eq!(found, 9),
            Ok(_) => panic!("project caller returns an integer"),
            Err(error) => panic!("project callers load plugins: {error}"),
        }
    }

    #[test]
    fn scoped_paths_hold_inside_scope() {
        let scope = PathBuf::from("/project");
        match scoped(&scope, PathBuf::from("/project/tools.lua"), "tools") {
            Ok(_) => {}
            Err(error) => panic!("contained path fails: {error}"),
        }
        match scoped(&scope, PathBuf::from("/outside/secret.lua"), "tools.link") {
            Ok(_) => panic!("escaping path passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("outside"),
                    "the text shows the boundary: {text}"
                );
            }
        }
    }

    #[test]
    fn nested_requires_stay_root_relative() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let folder = root.join("nested");
        driver::fs::create_dir_all(&folder).unwrap();
        driver::fs::write(&folder.join("helper.lua"), b"return 41").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"nested/helper\")") {
            Ok(Value::Integer(found)) => assert_eq!(found, 41),
            Ok(_) => panic!("nested module returns an integer"),
            Err(error) => panic!("nested modules load: {error}"),
        }
    }
}
