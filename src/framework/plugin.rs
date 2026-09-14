//! Plugin
//!
//! Lazy user plugin loading under `confit.plugin` plus scoped sibling
//! requires and plan-domain helper errors. Embedded defaults live in
//! `plugins/{user}/{name}/plugin.lua` beside the crate root and compile in
//! through `include_str!`; external plugins mirror the same shape under the
//! `--plugins` folder and resolve on first access. A `plugin.d.lua` file may
//! sit beside any `plugin.lua` for editors; the loader reads `plugin.lua`
//! only. Collision notes go to stderr from the loader, as no warnings
//! channel reaches install time.

use std::path::{Component, Path, PathBuf};

use mlua::{Lua, Table, Value};

use super::confit_table;

/// Named-registry key holding the external plugins root string, empty while absent.
const PLUGINS_ROOT_KEY: &str = "confit.plugin.root";
/// Named-registry key holding the sibling require cache table.
const REQUIRE_CACHE_KEY: &str = "confit.plugin.require_cache";
/// Named-registry key holding the noted collision set table.
const NOTED_KEY: &str = "confit.plugin.noted";

/// Embedded defaults as `(user, name, source)` triples.
///
/// Each source compiles in from `plugins/{user}/{name}/plugin.lua` and
/// installs eagerly under `confit.plugin.{user}.{name}`. Later defaults
/// extend this list with their own `include_str!` line.
const EMBEDDED: &[(&str, &str, &str)] = &[
    (
        "solrachq",
        "mise",
        include_str!("../../plugins/solrachq/mise/plugin.lua"),
    ),
    (
        "solrachq",
        "merge",
        include_str!("../../plugins/solrachq/merge/plugin.lua"),
    ),
    (
        "solrachq",
        "template",
        include_str!("../../plugins/solrachq/template/plugin.lua"),
    ),
];

/// Installs the plugin namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the namespace.
/// * `plugins` - external plugins folder, empty keeps embedded defaults only.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
///
/// # Examples
///
/// ```rust
/// use confit::framework::plugin::install;
/// use mlua::{Lua, Value};
///
/// let lua = Lua::new();
/// assert!(install(&lua, None).is_ok());
/// let result: mlua::Result<String> =
///     lua.load(r#"return type(confit.plugin.solrachq.mise.package)"#).eval();
/// assert!(matches!(&result, Ok(kind) if kind == "function"));
/// ```
pub fn install(lua: &Lua, plugins: Option<PathBuf>) -> mlua::Result<()> {
    let root = plugins
        .map(|folder| folder.display().to_string())
        .unwrap_or_default();
    lua.set_named_registry_value(PLUGINS_ROOT_KEY, root.clone())?;
    lua.set_named_registry_value(REQUIRE_CACHE_KEY, lua.create_table()?)?;
    lua.set_named_registry_value(NOTED_KEY, lua.create_table()?)?;
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    let helpers = lua.create_table()?;
    helpers.set("error", lua.create_function(helpers_error_impl)?)?;
    namespace.set("helpers", helpers)?;
    let lazy = lua.create_table()?;
    let lazy_root = root.clone();
    lazy.set(
        "__index",
        lua.create_function(move |lua, (table, key): (Table, Value)| {
            users_index(lua, table, key, lazy_root.clone())
        })?,
    )?;
    namespace.set_metatable(Some(lazy))?;
    for (user, name, source) in EMBEDDED.iter().copied() {
        let users = ensure_user_table(lua, &namespace, &root, user)?;
        let value: Value = lua
            .load(source)
            .set_name(format!("@plugins/{user}/{name}/plugin.lua"))
            .call(())?;
        users.raw_set(name, value)?;
    }
    if !root.is_empty() {
        note_collisions(lua, Path::new(&root))?;
    }
    confit.set("plugin", namespace)?;
    Ok(())
}

/// Reports embedded status for a user plus plugin pair.
///
/// # Arguments
///
/// * `user` - plugin author namespace.
/// * `name` - plugin name.
///
/// # Returns
///
/// True while the pair ships as an embedded default.
fn is_embedded(user: &str, name: &str) -> bool {
    EMBEDDED
        .iter()
        .any(|(one, two, _)| *one == user && *two == name)
}

/// Validates one `confit.plugin` index segment.
///
/// # Arguments
///
/// * `segment` - user or plugin key from Lua.
/// * `what` - segment kind for errors.
///
/// # Errors
///
/// Fails with plan errors for empty segments and for separators and for dot segments.
fn validate_segment(segment: &str, what: &str) -> mlua::Result<()> {
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains('/')
        || segment.contains('\\')
    {
        return Err(plan_err!(
            "confit.plugin: {what} '{segment}' must be a single path segment"
        ));
    }
    Ok(())
}

/// Builds a fresh user table carrying the lazy plugin loader.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `root` - external plugins folder string, empty while absent.
/// * `user` - author namespace owning the table.
///
/// # Returns
///
/// User table resolving plugin names on first access.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
fn new_user_table(lua: &Lua, root: String, user: String) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let frame = lua.create_table()?;
    frame.set(
        "__index",
        lua.create_function(move |lua, (owned, key): (Table, Value)| {
            plugins_index(lua, owned, key, root.clone(), user.clone())
        })?,
    )?;
    table.set_metatable(Some(frame))?;
    Ok(table)
}

/// Fetches the user table, creating plus caching it while absent.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `namespace` - plugin table holding user tables.
/// * `root` - external plugins folder string, empty while absent.
/// * `user` - author namespace.
///
/// # Returns
///
/// User table for the namespace.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures and with Lua errors
/// for user keys holding values of other shapes.
fn ensure_user_table(lua: &Lua, namespace: &Table, root: &str, user: &str) -> mlua::Result<Table> {
    let existing: Value = namespace.raw_get(user)?;
    match existing {
        Value::Table(table) => Ok(table),
        Value::Nil => {
            let table = new_user_table(lua, root.to_string(), user.to_string())?;
            namespace.raw_set(user, table.clone())?;
            Ok(table)
        }
        _ => Err(lua_err!("confit.plugin: user '{user}' must be a table")),
    }
}

/// Resolves a username access on the plugin table.
///
/// # Arguments
///
/// * `lua` - state owning the tables.
/// * `namespace` - plugin table receiving the user table.
/// * `key` - raw username key.
/// * `root` - external plugins folder string, empty while absent.
///
/// # Returns
///
/// User table for present user folders, nil otherwise.
///
/// # Errors
///
/// Fails with plan errors for keys escaping the plugins folder and with
/// mlua errors for table creation failures.
fn users_index(lua: &Lua, namespace: Table, key: Value, root: String) -> mlua::Result<Value> {
    let user = match key {
        Value::String(text) => text.to_string_lossy(),
        _ => return Ok(Value::Nil),
    };
    validate_segment(&user, "user")?;
    if root.is_empty() {
        return Ok(Value::Nil);
    }
    let folder = Path::new(&root).join(user.as_str());
    match (Path::new(&root).canonicalize(), folder.canonicalize()) {
        (Ok(home), Ok(full)) if !full.starts_with(&home) => {
            return Err(plan_err!(
                "confit.plugin: user '{user}' escapes the plugins folder"
            ));
        }
        _ => {}
    }
    if !folder.is_dir() || std::fs::read_dir(&folder).is_err() {
        return Ok(Value::Nil);
    }
    let table = new_user_table(lua, root, user.clone())?;
    namespace.raw_set(user.as_str(), table.clone())?;
    Ok(Value::Table(table))
}

/// Resolves a plugin name access on a user table.
///
/// # Arguments
///
/// * `lua` - state owning the tables.
/// * `users` - user table receiving the plugin value.
/// * `key` - raw plugin name key.
/// * `root` - external plugins folder string, empty while absent.
/// * `user` - author namespace.
///
/// # Returns
///
/// Plugin value for present plugin files, nil otherwise.
///
/// # Errors
///
/// Fails with plan errors for keys escaping the plugins folder and for
/// unreadable plugin files, and with Lua errors for plugin chunk failures.
fn plugins_index(
    lua: &Lua,
    users: Table,
    key: Value,
    root: String,
    user: String,
) -> mlua::Result<Value> {
    let name = match key {
        Value::String(text) => text.to_string_lossy(),
        _ => return Ok(Value::Nil),
    };
    validate_segment(&name, "plugin")?;
    if is_embedded(&user, &name) {
        if noted(lua, &format!("{user}/{name}"))? {
            eprintln!(
                "confit: note: external plugin '{user}/{name}' collides with an embedded default; keeping the embedded one"
            );
        }
        return Ok(Value::Nil);
    }
    if root.is_empty() {
        return Ok(Value::Nil);
    }
    let file = Path::new(&root)
        .join(user.as_str())
        .join(name.as_str())
        .join("plugin.lua");
    if !file.is_file() {
        return Ok(Value::Nil);
    }
    let value = load_external(lua, &root, &user, &name)?;
    users.raw_set(name.as_str(), value.clone())?;
    Ok(value)
}

/// Loads one external plugin file with a scoped require environment.
///
/// # Arguments
///
/// * `lua` - state running the chunk.
/// * `root` - external plugins folder string.
/// * `user` - author namespace.
/// * `name` - plugin name.
///
/// # Returns
///
/// Plugin chunk return value.
///
/// # Errors
///
/// Fails with plan errors for paths escaping the plugins folder and for
/// unreadable plugin files, and with Lua errors for plugin chunk failures.
fn load_external(lua: &Lua, root: &str, user: &str, name: &str) -> mlua::Result<Value> {
    let folder = Path::new(root).join(user).join(name);
    let file = folder.join("plugin.lua");
    match (Path::new(root).canonicalize(), file.canonicalize()) {
        (Ok(home), Ok(full)) if !full.starts_with(&home) => {
            return Err(plan_err!(
                "confit.plugin.{user}.{name}: path escapes the plugins folder"
            ));
        }
        _ => {}
    }
    let source = std::fs::read_to_string(&file)
        .map_err(|err| plan_err!("confit.plugin.{user}.{name}: cannot read 'plugin.lua': {err}"))?;
    let requirer = make_require(lua, folder.clone(), folder)?;
    let env = chunk_env(lua, requirer)?;
    let value: Value = lua
        .load(source.as_str())
        .set_name(format!("@{}", file.display()))
        .set_environment(env)
        .call(())?;
    Ok(value)
}

/// Builds a chunk environment holding a scoped require.
///
/// # Arguments
///
/// * `lua` - state owning the table.
/// * `requirer` - scoped require function for the chunk.
///
/// # Returns
///
/// Environment table falling back to globals for other names.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
fn chunk_env(lua: &Lua, requirer: mlua::Function) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    env.set("require", requirer)?;
    let fallback = lua.create_table()?;
    fallback.set("__index", lua.globals())?;
    env.set_metatable(Some(fallback))?;
    Ok(env)
}

/// Builds a scoped require resolving sibling files inside one plugin folder.
///
/// # Arguments
///
/// * `lua` - state owning the function.
/// * `current` - folder of the requiring file.
/// * `folder` - plugin folder jailing every read.
///
/// # Returns
///
/// Require function cached per file with chunk names carrying `@` paths.
///
/// # Errors
///
/// Fails with mlua errors for function creation failures.
fn make_require(lua: &Lua, current: PathBuf, folder: PathBuf) -> mlua::Result<mlua::Function> {
    lua.create_function(move |lua, request: Value| {
        let request = match request {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(lua_err!("plugin.require: field 'module' must be a string"));
            }
        };
        require_impl(lua, &current, &folder, &request)
    })
}

/// Resolves one scoped require request into a cached file value.
///
/// # Arguments
///
/// * `lua` - state running the chunk.
/// * `current` - folder of the requiring file.
/// * `folder` - plugin folder jailing every read.
/// * `request` - module request; dots separate folders, an optional `.lua`
///   suffix stays honored, and a leading `./` stays stripped.
///
/// # Returns
///
/// Required file return value, shared across repeat requests for one file.
///
/// # Errors
///
/// Fails with plan errors for requests escaping the plugin folder and for
/// missing files and for unreadable files, and with Lua errors for sibling
/// chunk failures.
fn require_impl(lua: &Lua, current: &Path, folder: &Path, request: &str) -> mlua::Result<Value> {
    const CALLER: &str = "plugin.require";
    if request.is_empty() {
        return Err(plan_err!("{CALLER}: module name must not be empty"));
    }
    if Path::new(request).is_absolute() {
        return Err(plan_err!(
            "{CALLER}: module '{request}' must be plugin-relative, not absolute"
        ));
    }
    let file = resolve_require(current, folder, request)?;
    let key = file.display().to_string();
    let cache: Table = match lua.named_registry_value(REQUIRE_CACHE_KEY) {
        Ok(table) => table,
        Err(_) => {
            let table = lua.create_table()?;
            lua.set_named_registry_value(REQUIRE_CACHE_KEY, table.clone())?;
            table
        }
    };
    let cached: Value = cache.raw_get(key.as_str())?;
    if !matches!(cached, Value::Nil) {
        return Ok(cached);
    }
    let source = std::fs::read_to_string(&file)
        .map_err(|err| plan_err!("{CALLER}: cannot read '{request}': {err}"))?;
    let parent = match file.parent() {
        Some(parent) => parent.to_path_buf(),
        None => {
            return Err(plan_err!(
                "{CALLER}: module '{request}' has no parent folder"
            ));
        }
    };
    let requirer = make_require(lua, parent, folder.to_path_buf())?;
    let env = chunk_env(lua, requirer)?;
    let value: Value = lua
        .load(source.as_str())
        .set_name(format!("@{}", file.display()))
        .set_environment(env)
        .call(())?;
    cache.raw_set(key.as_str(), value.clone())?;
    Ok(value)
}

/// Resolves one scoped require request to a file inside the plugin folder.
///
/// # Arguments
///
/// * `current` - folder of the requiring file.
/// * `folder` - plugin folder jailing every read.
/// * `request` - module request.
///
/// # Returns
///
/// Sibling file path, preferring `<request>.lua` over `<request>/init.lua`.
///
/// # Errors
///
/// Fails with plan errors for requests escaping the plugin folder and for
/// requests matching no file.
fn resolve_require(current: &Path, folder: &Path, request: &str) -> mlua::Result<PathBuf> {
    const CALLER: &str = "plugin.require";
    let trimmed = request.strip_prefix("./").unwrap_or(request);
    let stem = trimmed.strip_suffix(".lua").unwrap_or(trimmed);
    if stem.is_empty() {
        return Err(plan_err!("{CALLER}: module '{request}' must not be empty"));
    }
    let rel = stem.replace('.', "/");
    let base = current
        .strip_prefix(folder)
        .map_err(|_| plan_err!("{CALLER}: module '{request}' escapes the plugin folder"))?;
    let mut stack: Vec<String> = Vec::new();
    for component in base.components() {
        match component {
            Component::Normal(segment) => stack.push(segment.to_string_lossy().into_owned()),
            _ => {
                return Err(plan_err!(
                    "{CALLER}: module '{request}' escapes the plugin folder"
                ));
            }
        }
    }
    for component in Path::new(&rel).components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                return Err(plan_err!(
                    "{CALLER}: module '{request}' must be plugin-relative, not absolute"
                ));
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    return Err(plan_err!(
                        "{CALLER}: module '{request}' escapes the plugin folder"
                    ));
                }
            }
            Component::Normal(segment) => stack.push(segment.to_string_lossy().into_owned()),
        }
    }
    let mut full = folder.to_path_buf();
    for segment in &stack {
        full.push(segment);
    }
    let mut direct = full.clone().into_os_string();
    direct.push(".lua");
    let direct = PathBuf::from(direct);
    if direct.is_file() {
        return jail_canonical(folder, &direct, request);
    }
    let nested = full.join("init.lua");
    if nested.is_file() {
        return jail_canonical(folder, &nested, request);
    }
    Err(plan_err!(
        "{CALLER}: module '{request}' matches no file in the plugin folder"
    ))
}

/// Verifies a resolved sibling file stays inside the plugin folder.
///
/// # Arguments
///
/// * `folder` - plugin folder jailing the read.
/// * `file` - resolved sibling file.
/// * `request` - module request for errors.
///
/// # Returns
///
/// The sibling file path.
///
/// # Errors
///
/// Fails with plan errors for symlink escapes from the plugin folder.
fn jail_canonical(folder: &Path, file: &Path, request: &str) -> mlua::Result<PathBuf> {
    const CALLER: &str = "plugin.require";
    match (folder.canonicalize(), file.canonicalize()) {
        (Ok(home), Ok(full)) if !full.starts_with(&home) => Err(plan_err!(
            "{CALLER}: module '{request}' escapes the plugin folder"
        )),
        _ => Ok(file.to_path_buf()),
    }
}

/// Raises a plan-domain error attributing the calling plugin chunk.
///
/// # Arguments
///
/// * `lua` - state exposing the call stack.
/// * `message` - raw message value.
///
/// # Returns
///
/// Never returns a value; always fails.
///
/// # Errors
///
/// Fails with plan errors carrying the calling chunk name plus the message,
/// and with Lua errors for messages of other shapes. Attribution falls back
/// to `plugin '<unknown>'` while stack inspection finds no `@` chunk, so
/// direct profile calls attribute the profile chunk itself.
///
/// # Examples
///
/// ```rust
/// use confit::framework::plugin::install;
/// use mlua::{Lua, Value};
///
/// let lua = Lua::new();
/// assert!(install(&lua, None).is_ok());
/// let result: mlua::Result<Value> =
///     lua.load(r#"return confit.plugin.helpers.error("boom")"#).eval();
/// match result {
///     Err(err) => assert!(format!("{err}").contains("boom")),
///     Ok(_) => panic!("helpers.error must fail"),
/// }
/// ```
fn helpers_error_impl(lua: &Lua, message: Value) -> mlua::Result<Value> {
    const CALLER: &str = "confit.plugin.helpers.error";
    let message = match message {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(lua_err!("{CALLER}: field 'message' must be a string"));
        }
    };
    Err(plan_err!("{}: {message}", caller_attribution(lua)))
}

/// Names the calling plugin chunk for helper errors.
///
/// # Arguments
///
/// * `lua` - state exposing the call stack.
///
/// # Returns
///
/// `plugin '<chunk>'` for the nearest `@` chunk on the stack, else
/// `plugin '<unknown>'`.
fn caller_attribution(lua: &Lua) -> String {
    for level in 1..8 {
        let found = lua.inspect_stack(level, |debug| match debug.source().source {
            Some(text) if text.starts_with('@') => Some(text.into_owned()),
            _ => None,
        });
        match found {
            Some(Some(text)) => return format!("plugin '{text}'"),
            Some(None) => {}
            None => break,
        }
    }
    "plugin '<unknown>'".to_string()
}

/// Notes external plugins colliding with embedded defaults.
///
/// # Arguments
///
/// * `lua` - state holding the noted set.
/// * `root` - external plugins folder.
///
/// # Returns
///
/// Always succeeds; missing or unreadable folders read as absent. Each
/// collision prints one stderr note on first sighting, as no warnings
/// channel reaches install time.
///
/// # Errors
///
/// Fails with mlua errors for noted-set failures.
fn note_collisions(lua: &Lua, root: &Path) -> mlua::Result<()> {
    let users = match std::fs::read_dir(root) {
        Ok(users) => users,
        Err(_) => return Ok(()),
    };
    for user_entry in users {
        let user_entry = match user_entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !user_entry
            .file_type()
            .map(|kind| kind.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let user = user_entry.file_name().to_string_lossy().into_owned();
        let names = match std::fs::read_dir(root.join(&user)) {
            Ok(names) => names,
            Err(_) => continue,
        };
        for name_entry in names {
            let name_entry = match name_entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if !name_entry
                .file_type()
                .map(|kind| kind.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let name = name_entry.file_name().to_string_lossy().into_owned();
            if !root.join(&user).join(&name).join("plugin.lua").is_file() {
                continue;
            }
            if is_embedded(&user, &name) && noted(lua, &format!("{user}/{name}"))? {
                eprintln!(
                    "confit: note: external plugin '{user}/{name}' collides with an embedded default; keeping the embedded one"
                );
            }
        }
    }
    Ok(())
}

/// Records a collision key, reporting first sighting.
///
/// # Arguments
///
/// * `lua` - state holding the noted set.
/// * `key` - `user/name` collision key.
///
/// # Returns
///
/// True on first sighting, false for repeats.
///
/// # Errors
///
/// Fails with mlua errors for noted-set failures.
fn noted(lua: &Lua, key: &str) -> mlua::Result<bool> {
    let seen: Table = match lua.named_registry_value(NOTED_KEY) {
        Ok(table) => table,
        Err(_) => return Ok(true),
    };
    let known: Value = seen.raw_get(key)?;
    if matches!(known, Value::Nil) {
        seen.raw_set(key, true)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
