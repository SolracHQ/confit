//! Document
//!
//! Document constructors over destination handles.

use mlua::{Function, Lua, MultiValue, Table, Value};

use super::confit_table;
use super::handles::{LuaBlobHandle, LuaRoute, blob_for_opaque, req_route};
use super::hook::{read_slots, write_slots};
use super::runtime::check_condition_json;
use crate::error::{EngineError, FieldRef, Scope, find_engine};
use crate::lua::{TableExt, ValueExt, set_marker};
use crate::model::{
    LinkDecl, OpaqueDecl, RcEntryDecl, StructuredDecl, TextDecl, TreeDecl, TreeMemberDecl,
};
use confit_model::arg::Arg;
use confit_model::document::{RcSection, StructuredFormat};

pub(crate) mod convert;

/// Declared document in registration form.
pub(crate) enum Declared {
    /// Structured declaration.
    Structured(StructuredDecl),
    /// Text declaration.
    Text(TextDecl),
    /// Link declaration.
    Link(LinkDecl),
    /// Opaque declaration holding a blob handle.
    Opaque(OpaqueDecl),
    /// Tree declaration holding one managed file set.
    Tree(TreeDecl),
    /// Rc base holding section buckets.
    Rc(Vec<RcEntryDecl>),
}

/// Installs the document and rc namespaces on a state.
pub(crate) fn install(session: &crate::eval::Session) -> mlua::Result<()> {
    let lua = &session.lua;
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    namespace.set(
        "structured",
        lua.create_function(|lua, args: (Value, Value)| structured_impl(lua, args))?,
    )?;
    namespace.set(
        "text",
        lua.create_function(|lua, args: (Value, Value, Option<Value>)| text_impl(lua, args))?,
    )?;
    namespace.set(
        "link",
        lua.create_function(|lua, args: (Value, Value)| link_impl(lua, args))?,
    )?;
    let opaque_stores = session.stores.clone();
    namespace.set(
        "opaque",
        lua.create_function(move |lua, args: (Value, Value, Option<Value>)| {
            opaque_impl(lua, &opaque_stores, args)
        })?,
    )?;
    install_rc(lua, &namespace)?;
    confit.set("document", namespace)?;
    Ok(())
}

/// Builds a structured document table from format and args.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
fn structured_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.structured";
    let scope = Scope::method(CTOR);
    let (format_value, args_value) = args;
    let format_name = format_value.req_str(&scope, "format")?;
    let table = args_value.req_table(&scope, "args")?;
    DocumentTables::structured(lua, &scope, format_name, table)
}

/// Builds a plain text document table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
fn text_impl(lua: &Lua, args: (Value, Value, Option<Value>)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.text";
    let scope = Scope::method(CTOR);
    let (dest_value, content_value, opts) = args;
    let destination = req_route(&dest_value, &scope, "path")?;
    let content = content_value.req_str(&scope, "content")?;
    let resolved = DocOpts::resolve(opts, &scope)?;
    DocumentTables::text(lua, destination, content, resolved.mode, resolved.unmanaged)
}

/// Builds a symlink document table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
fn link_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.link";
    let scope = Scope::method(CTOR);
    let (dest_value, target_value) = args;
    let destination = req_route(&dest_value, &scope, "path")?;
    let target = target_value.req_str(&scope, "target")?;
    DocumentTables::link(lua, destination, target)
}

/// Builds an opaque document table from a source handle.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
/// - [`EngineError::Blob`] for unreadable sources.
fn opaque_impl(
    lua: &Lua,
    stores: &confit_store::Stores,
    args: (Value, Value, Option<Value>),
) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.opaque";
    let scope = Scope::method(CTOR);
    let (dest_value, source_value, opts) = args;
    let destination = req_route(&dest_value, &scope, "path")?;
    let (blob, size) = blob_for_opaque(&source_value, stores, &scope)?;
    let resolved = DocOpts::resolve(opts, &scope)?;
    DocumentTables::opaque(
        lua,
        destination,
        blob,
        size,
        resolved.mode,
        resolved.unmanaged,
    )
}

/// Document table builders holding domain validation.
struct DocumentTables;

impl DocumentTables {
    /// Builds a structured document table from format and args.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for misshaped arguments.
    /// - [`EngineError::OptUnknown`] for unknown fields.
    fn structured(
        lua: &Lua,
        scope: &Scope,
        format_name: String,
        args: Table,
    ) -> mlua::Result<Table> {
        let format = StructuredFormat::parse(&format_name).ok_or_else(|| EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("format"),
            want: "must be one of 'json', 'toml', or 'yaml'",
        })?;
        for pair in args.pairs::<Value, Value>() {
            let (key, _) = pair?;
            let Some(name) = key.opt_str() else {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("args"),
                    want: "must hold string keys",
                }
                .into());
            };
            if name != "path" && name != "data" {
                return Err(EngineError::OptUnknown {
                    scope: scope.clone(),
                    field: FieldRef::name("args"),
                    name,
                }
                .into());
            }
        }
        let path_value: Value = args.get("path")?;
        req_route(&path_value, scope, "path")?;
        let out = lua.create_table()?;
        out.set("path", path_value)?;
        let data: Value = args.get("data")?;
        out.set("data", data)?;
        set_marker(lua, &out, "structured", Some(("__format", format.name())))?;
        Ok(out)
    }

    /// Builds a plain text document table.
    fn text(
        lua: &Lua,
        destination: confit_model::routes::Route,
        content: String,
        mode: Option<u32>,
        unmanaged: bool,
    ) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        out.set("path", lua.create_userdata(LuaRoute::from(destination))?)?;
        out.set("content", content)?;
        out.set("unmanaged", unmanaged)?;
        let text = mode.map(|bits| bits.to_string());
        let extra = text.as_deref().map(|bits| ("__mode", bits));
        set_marker(lua, &out, "text", extra)?;
        Ok(out)
    }

    /// Builds a symlink document table.
    fn link(
        lua: &Lua,
        destination: confit_model::routes::Route,
        target: String,
    ) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        out.set("path", lua.create_userdata(LuaRoute::from(destination))?)?;
        out.set("target", target)?;
        set_marker(lua, &out, "link", None)?;
        Ok(out)
    }

    /// Builds an opaque document table holding a sealed blob handle.
    fn opaque(
        lua: &Lua,
        destination: confit_model::routes::Route,
        blob: confit_store::handles::BlobHandle,
        size: u64,
        mode: Option<u32>,
        unmanaged: bool,
    ) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        out.set("path", lua.create_userdata(LuaRoute::from(destination))?)?;
        out.set("blob", lua.create_userdata(LuaBlobHandle::new(blob))?)?;
        let size = size.min(i64::MAX as u64) as i64;
        out.set("size", size)?;
        out.set("unmanaged", unmanaged)?;
        let text = mode.map(|bits| bits.to_string());
        let extra = text.as_deref().map(|bits| ("__mode", bits));
        set_marker(lua, &out, "opaque", extra)?;
        Ok(out)
    }
}

/// Builds one tree document table from kept members.
pub(crate) fn tree_table(
    lua: &Lua,
    destination: confit_model::routes::Route,
    members: Vec<TreeMemberDecl>,
) -> mlua::Result<Table> {
    let out = lua.create_table()?;
    out.set("path", lua.create_userdata(LuaRoute::from(destination))?)?;
    let list = lua.create_table()?;
    for (index, member) in members.iter().enumerate() {
        let item = lua.create_table()?;
        item.set("rel", member.rel.as_str())?;
        item.set(
            "blob",
            lua.create_userdata(LuaBlobHandle::new(member.blob.clone()))?,
        )?;
        let size = member.size.min(i64::MAX as u64) as i64;
        item.set("size", size)?;
        item.set("mode", i64::from(member.mode))?;
        list.set(index + 1, item)?;
    }
    out.set("members", list)?;
    set_marker(lua, &out, "tree", None)?;
    Ok(out)
}

/// Document opts holding mode and the unmanaged flag.
struct DocOpts {
    /// Unix permission bits, holding `None` for default handling.
    mode: Option<u32>,
    /// True while presence alone satisfies the document.
    unmanaged: bool,
}

impl DocOpts {
    /// Resolves the document opts from an opts value.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for misshaped options.
    /// - [`EngineError::OptUnknown`] for unknown fields.
    /// - [`EngineError::ModeBits`] for bad mode text.
    fn resolve(opts: Option<Value>, scope: &Scope) -> mlua::Result<Self> {
        let Some(opts) = opts else {
            return Ok(Self {
                mode: None,
                unmanaged: false,
            });
        };
        if opts.is_nil() {
            return Ok(Self {
                mode: None,
                unmanaged: false,
            });
        }
        let table = opts.req_table(scope, "opts")?;
        for pair in table.pairs::<Value, Value>() {
            let (key, _) = pair?;
            let Some(name) = key.opt_str() else {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("opts"),
                    want: "must hold string keys",
                }
                .into());
            };
            if name != "mode" && name != "unmanaged" {
                return Err(EngineError::OptUnknown {
                    scope: scope.clone(),
                    field: FieldRef::name("opts"),
                    name,
                }
                .into());
            }
        }
        let mode_value: Value = table.get("mode")?;
        let mode = if mode_value.is_nil() {
            None
        } else {
            let raw = mode_value.req_str(scope, "mode")?;
            Some(mode_bits(&raw, scope)?)
        };
        let unmanaged_value: Value = table.get("unmanaged")?;
        let unmanaged = match unmanaged_value {
            Value::Nil => false,
            Value::Boolean(flag) => flag,
            _ => {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("unmanaged"),
                    want: "must be a boolean",
                }
                .into());
            }
        };
        Ok(Self { mode, unmanaged })
    }
}

/// Parses octal or symbolic permission text under a scope.
///
/// Octal text holds three digits like `755` or four digits with
/// a leading zero like `0755`. Symbolic text holds nine
/// characters like `rwxr-xr-x`, one `rwx` triple per class.
///
/// # Errors
///
/// - [`EngineError::ModeBits`] for leading `d`, wrong
///   lengths, and bad characters.
pub(crate) fn mode_bits(text: &str, scope: &Scope) -> mlua::Result<u32> {
    const SHAPE: &str = "octal like 755 or symbolic like rwxr-xr-x";
    const SYMBOLIC: &str = "nine rwx characters like rwxr-xr-x";
    const FOUR_DIGIT: &str = "four digit octal starting with 0 like 0755";
    const DIGITS: &str = "octal digits 0-7";
    let invalid = |want: &'static str| EngineError::ModeBits {
        scope: scope.clone(),
        text: text.to_owned(),
        want,
    };
    if text.starts_with('d') {
        return Err(invalid(SHAPE).into());
    }
    match text.len() {
        9 => {
            let mut bits: u32 = 0;
            for (index, byte) in text.as_bytes().iter().enumerate() {
                let bit: u32 = match (index % 3, byte) {
                    (0, b'r') => 4,
                    (1, b'w') => 2,
                    (2, b'x') => 1,
                    (_, b'-') => 0,
                    _ => return Err(invalid(SYMBOLIC).into()),
                };
                bits |= bit << ((2 - index / 3) * 3);
            }
            Ok(bits)
        }
        3 | 4 => {
            let body = match text.len() {
                3 => text,
                _ => text.strip_prefix('0').ok_or_else(|| invalid(FOUR_DIGIT))?,
            };
            if !body.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
                return Err(invalid(DIGITS).into());
            }
            u32::from_str_radix(body, 8).map_err(|_| invalid(DIGITS).into())
        }
        _ => Err(invalid(SHAPE).into()),
    }
}

/// Rejects destination paths escaping the tree folder.
///
/// # Errors
///
/// - [`EngineError::Detail`] for empty destination paths.
/// - [`EngineError::Destination`] for absolute and
///   non-file destinations.
pub(crate) fn check_rel(scope: &Scope, relpath: &str) -> mlua::Result<()> {
    if relpath.is_empty() {
        return Err(EngineError::Detail {
            scope: scope.clone(),
            want: "callback must not return an empty destination path",
        }
        .into());
    }
    if relpath.starts_with('/') {
        return Err(EngineError::Destination {
            scope: scope.clone(),
            path: relpath.to_owned(),
            want: "must stay relative",
        }
        .into());
    }
    for segment in relpath.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(EngineError::Destination {
                scope: scope.clone(),
                path: relpath.to_owned(),
                want: "must name files under the folder",
            }
            .into());
        }
    }
    Ok(())
}

/// Installs the rc entry constructors on the document namespace.
fn install_rc(lua: &Lua, namespace: &Table) -> mlua::Result<()> {
    let rc = lua.create_table()?;
    rc.set(
        "new",
        lua.create_function(|lua, sections: Value| rc_new_impl(lua, sections))?,
    )?;
    rc.set(
        "alias",
        lua.create_function(|lua, args: (Value, Value, Value)| rc_alias_impl(lua, args))?,
    )?;
    rc.set(
        "env",
        lua.create_function(|lua, args: (Value, Value, Value)| rc_env_impl(lua, args))?,
    )?;
    rc.set(
        "prepend",
        lua.create_function(|lua, args: MultiValue| rc_prepend_impl(lua, args))?,
    )?;
    rc.set(
        "eval",
        lua.create_function(|lua, args: (Value, Value)| rc_eval_impl(lua, args))?,
    )?;
    rc.set(
        "cmd",
        lua.create_function(|lua, args: (Value, Value)| rc_cmd_impl(lua, args))?,
    )?;
    rc.set(
        "source",
        lua.create_function(|lua, args: (Value, Value)| rc_source_impl(lua, args))?,
    )?;
    namespace.set("rc", rc)
}

/// Builds the single rc document table from section lists.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped sections.
fn rc_new_impl(lua: &Lua, sections: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.new";
    let scope = Scope::method(CTOR);
    let table = sections.req_table(&scope, "sections")?;
    RcDocs::build(lua, &scope, table)
}

/// Rc document builder holding domain validation.
struct RcDocs;

impl RcDocs {
    /// Builds one rc document table from section buckets.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for non-string section names.
    /// - [`EngineError::NestScope`] for bad section names.
    ///
    fn build(lua: &Lua, scope: &Scope, sections: Table) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        for pair in sections.pairs::<Value, Value>() {
            let (key, value) = pair?;
            let Some(name) = key.opt_str() else {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("sections"),
                    want: "must hold section names",
                }
                .into());
            };
            if let Err(parsed) = RcSection::parse(&name) {
                return Err(EngineError::NestScope {
                    scope: scope.clone(),
                    reason: parsed.to_string(),
                }
                .into());
            }
            out.set(name.as_str(), value)?;
        }
        set_marker(lua, &out, "rc", None)?;
        Ok(out)
    }
}

/// Builds one rc alias entry table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
fn rc_alias_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.alias";
    let scope = Scope::method(CTOR);
    let (name_value, value_value, opts) = args;
    let name = name_value.req_str(&scope, "name")?;
    let value = value_value.req_str(&scope, "value")?;
    let when = OptsGuard::resolve(lua, &scope, opts)?;
    RcEntries::alias(lua, name, value, when)
}

/// Builds one rc env entry table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
fn rc_env_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.env";
    let scope = Scope::method(CTOR);
    let (name_value, value_value, opts) = args;
    let name = name_value.req_str(&scope, "name")?;
    let value = value_value.req_str(&scope, "value")?;
    let when = OptsGuard::resolve(lua, &scope, opts)?;
    RcEntries::env(lua, name, value, when)
}

/// Builds one rc path prepend entry table from dir or var and dir.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
/// - [`EngineError::OpArity`] for wrong arity.
fn rc_prepend_impl(lua: &Lua, args: MultiValue) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.prepend";
    let scope = Scope::method(CTOR);
    let collected: Vec<Value> = args.into_iter().collect();
    let (name, dir, opts) = match collected.as_slice() {
        [dir_value] => ("PATH".to_string(), dir_value.clone(), Value::Nil),
        [first, second] => {
            if is_route_slot(second) {
                (
                    first.clone().req_str(&scope, "var")?,
                    second.clone(),
                    Value::Nil,
                )
            } else {
                ("PATH".to_string(), first.clone(), second.clone())
            }
        }
        [var_value, dir_value, opts] => (
            var_value.clone().req_str(&scope, "var")?,
            dir_value.clone(),
            opts.clone(),
        ),
        _ => {
            return Err(EngineError::OpArity {
                scope: scope.clone(),
                op: "prepend".to_owned(),
                want: "(dir, opts?) or (var, dir, opts?)",
            }
            .into());
        }
    };
    check_route_slot(&dir, &scope, "dir")?;
    let when = OptsGuard::resolve(lua, &scope, opts)?;
    RcEntries::path(lua, name, dir, when)
}

/// Reports whether one value holds a string or a route.
///
/// Strings run verbatim. Route userdata carries the
/// destination route. Anything else reads as opts.
fn is_route_slot(value: &Value) -> bool {
    if value.clone().opt_str().is_some() {
        return true;
    }
    match value.as_userdata() {
        Some(data) => data.borrow::<LuaRoute>().is_ok(),
        None => false,
    }
}

/// Rejects one route slot holding neither string nor route.
///
/// # Errors
///
/// - [`EngineError::Field`] for non-route slots.
fn check_route_slot(value: &Value, scope: &Scope, field: &str) -> mlua::Result<()> {
    if is_route_slot(value) {
        return Ok(());
    }
    Err(EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name(field),
        want: "must be a string or a confit.path value",
    }
    .into())
}

/// Rc entry table builders holding shared shapes.
struct RcEntries;

impl RcEntries {
    /// Builds one rc alias entry table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `name` - alias name.
    /// * `value` - alias expansion.
    /// * `when` - guard table holding `None` for no guard.
    ///
    /// # Returns
    ///
    /// Entry table stamped with the rc-entry marker.
    ///
    fn alias(lua: &Lua, name: String, value: String, when: Option<Table>) -> mlua::Result<Table> {
        let inner = lua.create_table()?;
        inner.set("name", name)?;
        inner.set("expansion", value)?;
        Self::tagged(lua, "alias", inner, when)
    }

    /// Builds one rc env entry table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `name` - variable name.
    /// * `value` - variable value.
    /// * `when` - guard table holding `None` for no guard.
    ///
    /// # Returns
    ///
    /// Entry table stamped with the rc-entry marker.
    ///
    fn env(lua: &Lua, name: String, value: String, when: Option<Table>) -> mlua::Result<Table> {
        let inner = lua.create_table()?;
        inner.set("name", name)?;
        inner.set("value", value)?;
        Self::tagged(lua, "env", inner, when)
    }

    /// Builds one shared PATH prepend entry table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `name` - variable name.
    /// * `dir` - directory value holding a string or a route.
    /// * `when` - guard table holding `None` for no guard.
    ///
    /// # Returns
    ///
    /// Entry table stamped with the rc-entry marker.
    ///
    fn path(lua: &Lua, name: String, dir: Value, when: Option<Table>) -> mlua::Result<Table> {
        let inner = lua.create_table()?;
        inner.set("name", name)?;
        inner.set("dir", dir)?;
        Self::tagged(lua, "path", inner, when)
    }

    /// Builds one shared init entry table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `shape` - entry shape naming the op.
    /// * `argv` - command slots, empty for source entries.
    /// * `source` - sourced value holding a string or a route.
    /// * `when` - guard table holding `None` for no guard.
    ///
    /// # Returns
    ///
    /// Entry table stamped with the rc-entry marker.
    ///
    fn init(
        lua: &Lua,
        shape: &str,
        argv: Vec<Arg>,
        source: Option<Value>,
        when: Option<Table>,
    ) -> mlua::Result<Table> {
        let inner = lua.create_table()?;
        if shape == "source" {
            inner.set("path", source.unwrap_or_default())?;
        } else {
            write_slots(lua, &inner, "argv", &argv)?;
        }
        Self::tagged(lua, shape, inner, when)
    }

    /// Wraps one op inner table and guard into an entry table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `shape` - entry shape naming the op.
    /// * `inner` - op inner table.
    /// * `when` - guard table holding `None` for no guard.
    ///
    /// # Returns
    ///
    /// Entry table stamped with the rc-entry marker.
    ///
    fn tagged(lua: &Lua, shape: &str, inner: Table, when: Option<Table>) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        out.set(shape, inner)?;
        if let Some(guard) = when {
            out.set("when", guard)?;
        }
        set_marker(lua, &out, "rc-entry", None)?;
        Ok(out)
    }
}

/// Builds one rc eval init entry table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
fn rc_eval_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.eval";
    let scope = Scope::method(CTOR);
    let (argv_value, opts) = args;
    let argv = read_slots(&argv_value, &scope, "argv")?;
    let when = OptsGuard::resolve(lua, &scope, opts)?;
    RcEntries::init(lua, "eval", argv, None, when)
}

/// Builds one rc cmd init entry table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
fn rc_cmd_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.cmd";
    let scope = Scope::method(CTOR);
    let (argv_value, opts) = args;
    let argv = read_slots(&argv_value, &scope, "argv")?;
    let when = OptsGuard::resolve(lua, &scope, opts)?;
    RcEntries::init(lua, "cmd", argv, None, when)
}

/// Builds one rc source init entry table.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped arguments.
fn rc_source_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.source";
    let scope = Scope::method(CTOR);
    let (path_value, opts) = args;
    check_route_slot(&path_value, &scope, "path")?;
    let when = OptsGuard::resolve(lua, &scope, opts)?;
    RcEntries::init(lua, "source", Vec::new(), Some(path_value), when)
}

/// Opts guard resolver holding domain validation.
struct OptsGuard;

impl OptsGuard {
    /// Resolves the `when` guard from an opts value.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for misshaped options.
    /// - [`EngineError::OptUnknown`] for unknown fields.
    /// - [`EngineError::GateFailed`] for failing guard calls.
    ///
    fn resolve(lua: &Lua, scope: &Scope, opts: Value) -> mlua::Result<Option<Table>> {
        if opts.is_nil() {
            return Ok(None);
        }
        let table = opts.req_table(scope, "opts")?;
        for pair in table.pairs::<Value, Value>() {
            let (key, _) = pair?;
            let Some(name) = key.opt_str() else {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::name("opts"),
                    want: "must hold string keys",
                }
                .into());
            };
            if name != "when" {
                return Err(EngineError::OptUnknown {
                    scope: scope.clone(),
                    field: FieldRef::name("opts"),
                    name,
                }
                .into());
            }
        }
        let when_value: Value = table.get("when")?;
        if let Some(func) = when_value.clone().opt_func() {
            let resolved = call_when_function(lua, scope, &func)?;
            table.set("when", resolved)?;
        }
        let guard_value: Value = table.get("when")?;
        if guard_value.is_nil() {
            return Ok(None);
        }
        let Some(guard) = guard_value.opt_table() else {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("when"),
                want: "must be a condition table",
            }
            .into());
        };
        let nested = scope.slot(FieldRef::name("when"));
        let json = guard.to_json(&nested)?;
        check_condition_json(&json, &nested)?;
        Ok(Some(guard))
    }
}

/// Calls one `when` builder function with the runtime namespace.
///
/// # Errors
///
/// - [`EngineError::Field`] for missing runtime tables.
/// - [`EngineError::GateFailed`] for failing guard calls.
fn call_when_function(lua: &Lua, scope: &Scope, func: &Function) -> mlua::Result<Table> {
    let field = FieldRef::name("when");
    let missing = || EngineError::Field {
        scope: scope.clone(),
        field: field.clone(),
        want: "needs the confit.runtime table",
    };
    let confit: Value = lua.globals().get("confit")?;
    let confit = confit
        .req_table(scope, &field.to_string())
        .map_err(|_| missing())?;
    let runtime: Value = confit.get("runtime")?;
    let runtime = runtime
        .req_table(scope, &field.to_string())
        .map_err(|_| missing())?;
    match func.call::<Table>(runtime) {
        Ok(table) => Ok(table),
        Err(error) => {
            if find_engine(&error).is_some() {
                return Err(error);
            }
            Err(EngineError::GateFailed {
                scope: scope.clone(),
                field,
                reason: error.to_string(),
            }
            .into())
        }
    }
}
