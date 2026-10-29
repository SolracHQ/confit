//! Provide stock require with project containment and plugin audit.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, Table, Value};

use confit_core::error::{
    ContextualCause, ContextualResult, Error, ErrorCause, EvalError, FilesystemError,
    RequireNameRule, Terminal, TerminalResult, WithContext, contextual, error, kind::ErrorKind,
};
use confit_store::Stores;

use crate::api::builtin;
use crate::api::global::Global;
use crate::project::PluginRoot;

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

    fn cycle(&self, key: &str) -> Option<Vec<String>> {
        let stack = self.loading.borrow();
        stack.iter().position(|held| held == key).map(|start| {
            let mut chain = stack[start..].to_vec();
            chain.push(key.to_owned());
            chain
        })
    }
}

/// Install the scoped require function.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for failed app data and global writes.
pub fn install(lua: &Lua) -> TerminalResult<()> {
    lua.set_app_data(Session::new());
    let require = lua
        .create_function(|lua, name: String| {
            run_require(lua, &name, None).map_err(mlua::Error::external)
        })
        .map_err(ContextualCause::from)
        .terminal(ErrorKind::LuaFailed)?;
    lua.globals()
        .set("require", require)
        .map_err(ContextualCause::from)
        .terminal(ErrorKind::LuaFailed)?;
    Ok(())
}

/// Load one profile chunk under the sandbox.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::OpenProfile`] for unreadable profiles and Lua failures.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for unreadable profiles and Lua failures.
pub fn load_chunk(lua: &Lua, path: &Path) -> TerminalResult<Value> {
    let text = read_text(path)
        .with_profile(path)
        .terminal(ErrorKind::OpenProfile)?;
    let caller = path.parent().map(Path::to_path_buf);
    let env = environment(lua, caller)?;
    let label = format!("@{}", path.display());
    match compile(lua, &text, label, env).call::<Value>(()) {
        Ok(found) => Ok(found),
        Err(fault) => {
            if let Some(terminal) = fault.downcast_ref::<Error>() {
                return Err(terminal.clone());
            }
            error(ErrorKind::LuaFailed, fault)
        }
    }
}

/// Read one chunk file as text.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] for unreadable files.
/// - [`confit_core::error::DocumentError::ParseFailed`] for non-UTF8 files.
fn read_text(path: &Path) -> ContextualResult<String> {
    let bytes = std::fs::read(path)
        .map_err(ContextualCause::from)
        .with_fs_path(path)?;
    String::from_utf8(bytes)
        .map_err(ContextualCause::from)
        .with_fs_path(path)
}

/// Build one sandboxed environment.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for unreadable globals and unwritable tables.
fn environment(lua: &Lua, caller: Option<PathBuf>) -> TerminalResult<Table> {
    let env = lua
        .create_table()
        .map_err(ContextualCause::from)
        .terminal(ErrorKind::LuaFailed)?;
    let globals = lua.globals();
    for name in AUDITED {
        let held: Value = globals
            .get(*name)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed)?;
        env.set(*name, held)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed)?;
    }
    for name in LIBRARIES {
        let held: Value = globals
            .get(*name)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed)?;
        env.set(*name, held)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed)?;
    }
    let require = lua
        .create_function(move |lua, name: String| {
            run_require(lua, &name, caller.as_deref()).map_err(mlua::Error::external)
        })
        .map_err(ContextualCause::from)
        .terminal(ErrorKind::LuaFailed)?;
    env.set("require", require)
        .map_err(ContextualCause::from)
        .terminal(ErrorKind::LuaFailed)?;
    Global::install(lua, &env)
        .map_err(ContextualCause::from)
        .terminal(ErrorKind::LuaFailed)?;
    Ok(env)
}

/// Compile one source text.
fn compile<'chunk>(
    lua: &'chunk Lua,
    source: &'chunk str,
    name: String,
    env: Table,
) -> mlua::chunk::Chunk<'chunk> {
    lua.load(source).set_name(name).set_environment(env)
}

/// Run one require call for builtins with file modules.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unknown names, bad names, escapes,
/// - [`confit_core::error::kind::ErrorKind::OpenModule`] for unknown names, bad names, escapes,
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for unknown names, bad names, escapes,
///   cycles, unreadable files, and Lua failures.
fn run_require(lua: &Lua, name: &str, caller: Option<&Path>) -> TerminalResult<Value> {
    if name.starts_with("confit.") {
        return builtin_require(lua, name);
    }
    file_require(lua, name, caller)
}

/// Run one builtin require.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unknown names, cycles, and Lua failures.
/// - [`confit_core::error::kind::ErrorKind::OpenModule`] for unknown names, cycles, and Lua failures.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for unknown names, cycles, and Lua failures.
fn builtin_require(lua: &Lua, name: &str) -> TerminalResult<Value> {
    let key = CacheKey::Builtin(name.to_owned());
    if let Some(hit) = with_session(lua, |session| session.cached(&key))? {
        return Ok(hit);
    }
    if let Some(chain) = with_session(lua, |session| session.cycle(name))? {
        return error(
            ErrorKind::Evaluate,
            contextual(EvalError::CycleDetected).with_cycle(chain),
        );
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
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for bad names, escapes, cycles, missing
/// - [`confit_core::error::kind::ErrorKind::OpenModule`] for bad names, escapes, cycles, missing
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for bad names, escapes, cycles, missing
///   files, unreadable files, and Lua failures.
fn file_require(lua: &Lua, name: &str, caller: Option<&Path>) -> TerminalResult<Value> {
    let root = stored_root(lua)?;
    let root_real = canonical_root(&root)
        .with_module(name)
        .terminal(ErrorKind::OpenModule)?;
    let held = stored_plugins(lua);
    let fallback = root_real.join(PLUGIN_ROOT);
    let plugins = held.as_ref().map(|scope| &scope.dir).unwrap_or(&fallback);
    let parts = map_name(name).terminal(ErrorKind::Evaluate)?;
    if parts.is_empty() {
        return error(
            ErrorKind::OpenModule,
            contextual(FilesystemError::FileMissing).with_module(name),
        );
    }
    let (base, scope, rest) = match parts.first() {
        Some(first) if first == PLUGIN_ROOT => (plugins.as_path(), plugins.as_path(), &parts[1..]),
        _ => (root.as_path(), root_real.as_path(), &parts[..]),
    };
    let path = match resolve(base, scope, rest) {
        Ok(found) => found,
        Err(contextual) => match contextual.cause() {
            ErrorCause::EvalError(EvalError::RequireEscapesScope) => {
                return error(ErrorKind::Evaluate, contextual.with_module(name));
            }
            _ => {
                return error(ErrorKind::OpenModule, contextual.with_module(name));
            }
        },
    };
    if let Err(contextual) = audit(plugins, caller, &path) {
        return error(
            ErrorKind::Evaluate,
            contextual.with_module(name).with_fs_path(&path),
        );
    }
    let key = CacheKey::File(path.clone());
    if let Some(hit) = with_session(lua, |session| session.cached(&key))? {
        return Ok(hit);
    }
    let tag = path.display().to_string();
    if let Some(chain) = with_session(lua, |session| session.cycle(&tag))? {
        return error(
            ErrorKind::Evaluate,
            contextual(EvalError::CycleDetected).with_cycle(chain),
        );
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
/// - [`confit_core::error::FilesystemError`] for unreadable roots.
fn canonical_root(root: &Path) -> ContextualResult<PathBuf> {
    std::fs::canonicalize(root)
        .map_err(ContextualCause::from)
        .with_fs_path(root)
}

/// Resolve path parts to their contained canonical path.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::RequireEscapesScope`] for escaping names.
/// - [`confit_core::error::FilesystemError::FileMissing`] for missing files.
/// - [`confit_core::error::FilesystemError`] for unreadable files.
fn resolve(base: &Path, scope: &Path, parts: &[String]) -> ContextualResult<PathBuf> {
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
    let found = match std::fs::canonicalize(&direct) {
        Ok(hit) => hit,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::canonicalize(&entry)
            .map_err(ContextualCause::from)
            .with_fs_path(&entry)?,
        Err(error) => {
            return Err(ContextualCause::from(error).with_fs_path(&direct));
        }
    };
    scoped(scope, found)
}

/// Map one require name to its path parts.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::BadRequireName`] for malformed names.
fn map_name(name: &str) -> ContextualResult<Vec<String>> {
    if name.contains('\\') {
        return Err(contextual(EvalError::BadRequireName {
            rule: RequireNameRule::Backslash,
        })
        .with_module(name));
    }
    if name.len() >= 2 && name.as_bytes()[0].is_ascii_alphabetic() && name.as_bytes()[1] == b':' {
        return Err(contextual(EvalError::BadRequireName {
            rule: RequireNameRule::DrivePrefix,
        })
        .with_module(name));
    }
    if name.starts_with('/') {
        return Err(contextual(EvalError::BadRequireName {
            rule: RequireNameRule::AbsolutePath,
        })
        .with_module(name));
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
/// # Errors
///
/// - [`confit_core::error::EvalError::RequireEscapesScope`] for plugin callers outside the tree.
fn audit(plugins: &Path, caller: Option<&Path>, target: &Path) -> ContextualResult<()> {
    let Some(caller) = caller else {
        return Ok(());
    };
    let Ok(caller_real) = std::fs::canonicalize(caller) else {
        return Ok(());
    };
    if caller_real.starts_with(plugins) && !target.starts_with(plugins) {
        return Err(contextual(EvalError::RequireEscapesScope).with_scope(plugins));
    }
    Ok(())
}

/// Hold one canonical path inside its canonical scope.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::RequireEscapesScope`] for escaping names.
fn scoped(scope: &Path, full: PathBuf) -> ContextualResult<PathBuf> {
    if !full.starts_with(scope) {
        return Err(contextual(EvalError::RequireEscapesScope)
            .with_scope(scope)
            .with_fs_path(full));
    }
    Ok(full)
}

/// Load one file module.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::OpenModule`] for missing files, unreadable files,
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for missing files, unreadable files,
///   non-UTF8 files, and Lua failures.
fn load_file(lua: &Lua, path: &Path, name: &str) -> TerminalResult<Value> {
    let bytes = std::fs::read(path)
        .map_err(ContextualCause::from)
        .with_fs_path(path)
        .with_module(name)
        .terminal(ErrorKind::OpenModule)?;
    let text = String::from_utf8(bytes)
        .map_err(ContextualCause::from)
        .with_fs_path(path)
        .with_module(name)
        .terminal(ErrorKind::OpenModule)?;
    let caller = path.parent().map(Path::to_path_buf);
    let env = environment(lua, caller)?;
    let label = format!("@{}", path.display());
    match compile(lua, &text, label, env).call::<Value>(()) {
        Ok(found) => Ok(found),
        Err(fault) => {
            if let Some(terminal) = fault.downcast_ref::<Error>() {
                return Err(terminal.clone());
            }
            error(ErrorKind::LuaFailed, fault)
        }
    }
}

/// Run one closure against shared loader state.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for an uninstalled loader.
fn with_session<T>(lua: &Lua, op: impl FnOnce(&Session) -> T) -> TerminalResult<T> {
    match lua.app_data_ref::<Session>() {
        Some(session) => Ok(op(&session)),
        None => error(ErrorKind::LuaFailed, contextual(EvalError::LuaFailed)),
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
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for an uninstalled api.
fn stored_root(lua: &Lua) -> TerminalResult<PathBuf> {
    match lua.app_data_ref::<Stores>() {
        Some(stores) => Ok(stores.resources().roots.project_root.clone()),
        None => error(ErrorKind::LuaFailed, contextual(EvalError::LuaFailed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        lua.set_app_data(stores.clone());
        install(&lua).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        let env = environment(lua, None).map_err(mlua::Error::external)?;
        compile(lua, source, "@test".to_owned(), env).call(())
    }

    #[test]
    fn missing_profiles_fail_structured() {
        use confit_core::error::{ErrorCause, FilesystemError};

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let lua = test_lua(&stores);
        let missing = root.join("absent.lua");
        match load_chunk(&lua, &missing) {
            Ok(_) => panic!("missing profile passes"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the cause shows the miss: {error}"
                );
                assert!(
                    error.to_string().contains("absent.lua"),
                    "the line shows the file: {error}"
                );
                match &error.context().profile {
                    Some(found) => {
                        assert_eq!(found, &missing, "the profile slot shows the file: {error}")
                    }
                    None => panic!("the profile slot shows the file: {error}"),
                }
            }
        }
    }

    #[test]
    fn encoded_profiles_fail_structured() {
        use confit_core::error::{DocumentError, ErrorCause, Require};

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let profile = root.join("profile.lua");
        std::fs::write(&profile, [0xff, 0xfe]).unwrap();
        let lua = test_lua(&stores);
        match load_chunk(&lua, &profile) {
            Ok(_) => panic!("encoded profile passes"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::DocumentError(DocumentError::ParseFailed {
                            require: Require::Utf8Text,
                        })
                    ),
                    "the cause shows the encoding: {error}"
                );
                assert!(
                    error.to_string().contains("profile.lua"),
                    "the line shows the file: {error}"
                );
                match &error.context().profile {
                    Some(found) => {
                        assert_eq!(found, &profile, "the profile slot shows the file: {error}")
                    }
                    None => panic!("the profile slot shows the file: {error}"),
                }
            }
        }
    }

    #[test]
    fn encoded_modules_fail_structured() {
        use confit_core::error::{DocumentError, ErrorCause, Require};

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        std::fs::write(root.join("broken.lua"), [0xff, 0xfe]).unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"broken\")") {
            Ok(_) => panic!("encoded module passes"),
            Err(error) => match error.downcast_ref::<Error>() {
                Some(terminal) => {
                    assert!(
                        matches!(
                            terminal.cause(),
                            ErrorCause::DocumentError(DocumentError::ParseFailed {
                                require: Require::Utf8Text,
                            })
                        ),
                        "the cause shows the encoding: {terminal}"
                    );
                    assert!(
                        terminal.to_string().contains("broken"),
                        "the line shows the module: {terminal}"
                    );
                    match &terminal.context().module {
                        Some(module) => assert_eq!(
                            module, "broken",
                            "the module slot shows the name: {terminal}"
                        ),
                        None => panic!("the module slot shows the name: {terminal}"),
                    }
                    match &terminal.context().fs_path {
                        Some(path) => assert!(
                            path.ends_with("broken.lua"),
                            "the fs slot shows the file: {terminal}"
                        ),
                        None => panic!("the fs slot shows the file: {terminal}"),
                    }
                }
                None => panic!("encoded module stays terminal: {error}"),
            },
        }
    }

    #[test]
    fn unknown_names_fail() {
        use confit_core::error::{ErrorCause, FilesystemError};

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        for name in ["confit.nope", "absent"] {
            let source = format!("return require({name:?})");
            match run(&lua, &source) {
                Ok(_) => panic!("unknown module passes: {name}"),
                Err(error) => match error.downcast_ref::<Error>() {
                    Some(terminal) => {
                        assert!(
                            matches!(
                                terminal.cause(),
                                ErrorCause::FilesystemError(FilesystemError::FileMissing)
                            ),
                            "the cause shows the miss: {terminal}"
                        );
                        assert!(
                            terminal.to_string().contains(name),
                            "the line shows the module: {terminal}"
                        );
                        match &terminal.context().module {
                            Some(found) => assert_eq!(
                                found, name,
                                "the module slot shows the name: {terminal}"
                            ),
                            None => panic!("the module slot shows the name: {terminal}"),
                        }
                    }
                    None => panic!("unknown module stays terminal: {error}"),
                },
            }
        }
    }

    #[test]
    fn file_modules_load_under_root() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        std::fs::write(root.join("helper.lua"), b"return 41").unwrap();
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
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let folder = root.join("nested");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("helper.lua"), b"return 41").unwrap();
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
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let folder = root.join("nested");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("helper.lua"), b"return 41").unwrap();
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
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        std::fs::write(root.join("tools.lua"), b"return 3").unwrap();
        let nested = root.join("profiles");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("mainframe.lua"),
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
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        std::fs::write(root.join("helper.lua"), b"return 41").unwrap();
        let lua = test_lua(&stores);
        assert!(
            run(&lua, "return require(\"helper.lua\")").is_err(),
            "suffixed name fails"
        );
    }

    #[test]
    fn cycles_name_the_loop() {
        use confit_core::error::{ErrorCause, EvalError};

        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        std::fs::write(
            root.join("loop_a.lua"),
            b"local _ = require(\"loop_b\") return 1",
        )
        .unwrap();
        std::fs::write(
            root.join("loop_b.lua"),
            b"local _ = require(\"loop_a\") return 2",
        )
        .unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"loop_a\")") {
            Ok(_) => panic!("require cycle passes"),
            Err(error) => match error.downcast_ref::<Error>() {
                Some(terminal) => {
                    assert!(
                        matches!(
                            terminal.cause(),
                            ErrorCause::EvalError(EvalError::CycleDetected)
                        ),
                        "the cause shows the cycle: {terminal}"
                    );
                    let chain = &terminal.context().cycle;
                    assert_eq!(chain.len(), 3, "the chain closes the loop: {terminal}");
                    assert_eq!(
                        chain.first(),
                        chain.last(),
                        "the chain returns to its start: {terminal}"
                    );
                }
                None => panic!("require cycle stays terminal: {error}"),
            },
        }
    }

    #[test]
    fn session_cycle_keeps_ordered_names() {
        let session = Session::new();
        session.push("loop_a".to_owned());
        session.push("loop_b".to_owned());
        assert_eq!(
            session.cycle("loop_a"),
            Some(vec![
                "loop_a".to_owned(),
                "loop_b".to_owned(),
                "loop_a".to_owned(),
            ]),
            "the cycle closes over the stack"
        );
        assert_eq!(
            session.cycle("absent"),
            None,
            "names outside the stack stay clean"
        );
    }

    #[test]
    fn bare_confit_global_stays_nil() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        match run(&lua, "return confit") {
            Ok(Value::Nil) => {}
            Ok(_) => panic!("bare confit global stays nil"),
            Err(error) => panic!("sandbox loads the global: {error}"),
        }
    }

    #[test]
    fn unknown_globals_read_nil() {
        let lua = Lua::new();
        match run(&lua, "return type(os)") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "nil"),
            Ok(_) => panic!("unknown globals return strings"),
            Err(error) => panic!("sandbox loads unknown globals: {error}"),
        }
    }

    #[test]
    fn audited_globals_with_libraries_return() {
        let lua = Lua::new();
        match run(&lua, "return type(pairs)") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "function"),
            Ok(_) => panic!("audited globals return strings"),
            Err(error) => panic!("sandbox loads audited globals: {error}"),
        }
        match run(&lua, "return (\"hi\"):upper()") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "HI"),
            Ok(_) => panic!("string libraries return strings"),
            Err(error) => panic!("sandbox loads libraries: {error}"),
        }
    }

    #[test]
    fn proxies_reject_writes_showing_readonly() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        assert!(
            run(
                &lua,
                "local validate = require(\"confit.validate\") validate.document = 1",
            )
            .is_err(),
            "proxy write fails"
        );
    }

    #[test]
    fn init_entries_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let entry = root.join("plugins").join("alice").join("tool");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("init.lua"), b"return 77").unwrap();
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
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let entry = root.join("plugins").join("_").join("sugar");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("helper.lua"), b"return 7").unwrap();
        std::fs::write(
            entry.join("init.lua"),
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
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let tools = root.join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(tools.join("helper.lua"), b"return 1").unwrap();
        let entry = root.join("plugins").join("_").join("sugar");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("init.lua"), b"return require(\"tools.helper\")").unwrap();
        let lua = test_lua(&stores);
        assert!(
            run(&lua, "return require(\"plugins._.sugar\")").is_err(),
            "plugin escape fails"
        );
    }

    #[test]
    fn project_callers_require_anything_contained() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let entry = root.join("plugins").join("_").join("sugar");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("init.lua"), b"return 9").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"plugins._.sugar\")") {
            Ok(Value::Integer(found)) => assert_eq!(found, 9),
            Ok(_) => panic!("project caller returns an integer"),
            Err(error) => panic!("project callers load plugins: {error}"),
        }
    }

    #[test]
    fn scoped_paths_hold_inside_scope() {
        use confit_core::error::EvalError;

        let scope = PathBuf::from("/project");
        match scoped(&scope, PathBuf::from("/project/tools.lua")) {
            Ok(_) => {}
            Err(error) => panic!("contained path fails: {error}"),
        }
        match scoped(&scope, PathBuf::from("/outside/secret.lua")) {
            Ok(_) => panic!("escaping path passes"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        confit_core::error::ErrorCause::EvalError(EvalError::RequireEscapesScope)
                    ),
                    "the cause shows the escape: {error}"
                );
                match &error.context().scope {
                    Some(found) => {
                        assert_eq!(found, &scope, "the scope slot shows the scope: {error}")
                    }
                    None => panic!("the scope slot shows the scope: {error}"),
                }
            }
        }
    }

    #[test]
    fn nested_requires_stay_root_relative() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let folder = root.join("nested");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("helper.lua"), b"return 41").unwrap();
        let lua = test_lua(&stores);
        match run(&lua, "return require(\"nested/helper\")") {
            Ok(Value::Integer(found)) => assert_eq!(found, 41),
            Ok(_) => panic!("nested module returns an integer"),
            Err(error) => panic!("nested modules load: {error}"),
        }
    }
}
