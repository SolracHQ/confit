//! Document
//!
//! Document constructors over destination handles.

use mlua::{Function, Lua, MultiValue, Table, Value};

use super::confit_table;
use super::handles::{LuaBlobHandle, LuaRoute, blob_for_opaque, req_route};
use super::hook::{read_slots, write_slots};
use super::runtime::check_condition_json;
use crate::error::plan_error;
use crate::lua::{TableExt, ValueExt, set_marker};
use crate::model::{
    LinkDecl, OpaqueDecl, RcEntryDecl, StructuredDecl, TextDecl, TreeDecl, TreeMemberDecl,
};
use confit_core::arg::Arg;
use confit_core::document::{RcData, StructuredFormat};

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
fn structured_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.structured";
    let (format_value, args_value) = args;
    let format_name = format_value.req_str(CTOR, "format")?;
    let table = args_value.req_table(CTOR, "args")?;
    DocumentTables::structured(lua, CTOR, format_name, table)
}

/// Builds a plain text document table.
fn text_impl(lua: &Lua, args: (Value, Value, Option<Value>)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.text";
    let (dest_value, content_value, opts) = args;
    let destination = req_route(&dest_value, CTOR, "path")?;
    let content = content_value.req_str(CTOR, "content")?;
    let resolved = DocOpts::resolve(opts, CTOR)?;
    DocumentTables::text(lua, destination, content, resolved.mode, resolved.unmanaged)
}

/// Builds a symlink document table.
fn link_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.link";
    let (dest_value, target_value) = args;
    let destination = req_route(&dest_value, CTOR, "path")?;
    let target = target_value.req_str(CTOR, "target")?;
    DocumentTables::link(lua, destination, target)
}

/// Builds an opaque document table from a source handle.
fn opaque_impl(
    lua: &Lua,
    stores: &confit_store::Stores,
    args: (Value, Value, Option<Value>),
) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.opaque";
    let (dest_value, source_value, opts) = args;
    let destination = req_route(&dest_value, CTOR, "path")?;
    let (blob, size) = blob_for_opaque(&source_value, stores, CTOR)?;
    let resolved = DocOpts::resolve(opts, CTOR)?;
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
    fn structured(lua: &Lua, ctor: &str, format_name: String, args: Table) -> mlua::Result<Table> {
        const KNOWN: &str = "'json', 'toml', or 'yaml'";
        let format = StructuredFormat::parse(&format_name)
            .ok_or_else(|| plan_error(format!("{ctor}: field 'format' must be one of {KNOWN}")))?;
        for pair in args.pairs::<Value, Value>() {
            let (key, _) = pair?;
            let Some(name) = key.opt_str() else {
                return Err(plan_error(format!(
                    "{ctor}: field 'args' must hold string keys"
                )));
            };
            if name != "path" && name != "data" {
                return Err(plan_error(format!(
                    "{ctor}: field 'args' unknown field '{name}'"
                )));
            }
        }
        let path_value: Value = args.get("path")?;
        req_route(&path_value, ctor, "path")?;
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
        destination: confit_core::handles::Route,
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
        destination: confit_core::handles::Route,
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
        destination: confit_core::handles::Route,
        blob: confit_core::handles::BlobHandle,
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
    destination: confit_core::handles::Route,
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
    fn resolve(opts: Option<Value>, ctor: &str) -> mlua::Result<Self> {
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
        let table = opts.req_table(ctor, "opts")?;
        for pair in table.pairs::<Value, Value>() {
            let (key, _) = pair?;
            let Some(name) = key.opt_str() else {
                return Err(plan_error(format!(
                    "{ctor}: field 'opts' must hold string keys"
                )));
            };
            if name != "mode" && name != "unmanaged" {
                return Err(plan_error(format!(
                    "{ctor}: field 'opts' unknown field '{name}'"
                )));
            }
        }
        let mode_value: Value = table.get("mode")?;
        let mode = if mode_value.is_nil() {
            None
        } else {
            let raw = mode_value.req_str(ctor, "mode")?;
            Some(
                confit_core::document::parse_mode(&raw)
                    .map_err(|error| plan_error(format!("{ctor}: {error}")))?,
            )
        };
        let unmanaged_value: Value = table.get("unmanaged")?;
        let unmanaged = match unmanaged_value {
            Value::Nil => false,
            Value::Boolean(flag) => flag,
            _ => {
                return Err(plan_error(format!(
                    "{ctor}: field 'unmanaged' must be a boolean"
                )));
            }
        };
        Ok(Self { mode, unmanaged })
    }
}

/// Rejects destination paths escaping the tree folder.
pub(crate) fn check_rel(ctor: &str, relpath: &str) -> mlua::Result<()> {
    if relpath.is_empty() {
        return Err(plan_error(format!(
            "{ctor}: callback must not return an empty destination path"
        )));
    }
    if relpath.starts_with('/') {
        return Err(plan_error(format!(
            "{ctor}: destination '{relpath}' must stay relative"
        )));
    }
    for segment in relpath.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(plan_error(format!(
                "{ctor}: destination '{relpath}' must name files under the folder"
            )));
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
fn rc_new_impl(lua: &Lua, sections: Value) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.new";
    let table = sections.req_table(CTOR, "sections")?;
    RcDocs::build(lua, CTOR, table)
}

/// Rc document builder holding domain validation.
struct RcDocs;

impl RcDocs {
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `ctor` - error prefix naming the constructor.
    /// * `sections` - section buckets table.
    ///
    /// # Returns
    ///
    /// Rc document table stamped with the rc marker.
    ///
    /// # Errors
    ///
    /// Non-string section names fail as plan errors. Unknown sections fail as plan errors.
    ///
    fn build(lua: &Lua, ctor: &str, sections: Table) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        for pair in sections.pairs::<Value, Value>() {
            let (key, value) = pair?;
            let Some(name) = key.opt_str() else {
                return Err(plan_error(format!(
                    "{ctor}: field 'sections' must hold section names"
                )));
            };
            if let Err(error) = RcData::check_section_name(&name) {
                match error {
                    confit_core::error::Error::Plan(message) => {
                        return Err(plan_error(format!("{ctor}: {message}")));
                    }
                    confit_core::error::Error::Io(error) => {
                        return Err(plan_error(format!("{ctor}: {error}")));
                    }
                }
            }
            out.set(name.as_str(), value)?;
        }
        set_marker(lua, &out, "rc", None)?;
        Ok(out)
    }
}

/// Builds one rc alias entry table.
fn rc_alias_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.alias";
    let (name_value, value_value, opts) = args;
    let name = name_value.req_str(CTOR, "name")?;
    let value = value_value.req_str(CTOR, "value")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::alias(lua, name, value, when)
}

/// Builds one rc env entry table.
fn rc_env_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.env";
    let (name_value, value_value, opts) = args;
    let name = name_value.req_str(CTOR, "name")?;
    let value = value_value.req_str(CTOR, "value")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::env(lua, name, value, when)
}

/// Builds one rc path prepend entry table from dir or var and dir.
fn rc_prepend_impl(lua: &Lua, args: MultiValue) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.prepend";
    let collected: Vec<Value> = args.into_iter().collect();
    let (name, dir, opts) = match collected.as_slice() {
        [dir_value] => ("PATH".to_string(), dir_value.clone(), Value::Nil),
        [first, second] => {
            if is_route_slot(second) {
                (
                    first.clone().req_str(CTOR, "var")?,
                    second.clone(),
                    Value::Nil,
                )
            } else {
                ("PATH".to_string(), first.clone(), second.clone())
            }
        }
        [var_value, dir_value, opts] => (
            var_value.clone().req_str(CTOR, "var")?,
            dir_value.clone(),
            opts.clone(),
        ),
        _ => {
            return Err(plan_error(format!(
                "{CTOR}: 'prepend' expects (dir, opts?) or (var, dir, opts?)"
            )));
        }
    };
    check_route_slot(&dir, CTOR, "dir")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
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
/// # Arguments
///
/// * `value` - the slot value under checking.
/// * `ctor` - error prefix naming the constructor.
/// * `field` - field name under reading.
///
/// # Returns
///
/// Unit for strings and route userdata.
///
/// # Errors
///
/// Anything else fails as a plan error naming the field.
fn check_route_slot(value: &Value, ctor: &str, field: &str) -> mlua::Result<()> {
    if is_route_slot(value) {
        return Ok(());
    }
    Err(plan_error(format!(
        "{ctor}: field '{field}' must be a string or a confit.path value"
    )))
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
        inner.set("op", "prepend")?;
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
fn rc_eval_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.eval";
    let (argv_value, opts) = args;
    let argv = read_slots(&argv_value, CTOR, "argv")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::init(lua, "eval", argv, None, when)
}

/// Builds one rc cmd init entry table.
fn rc_cmd_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.cmd";
    let (argv_value, opts) = args;
    let argv = read_slots(&argv_value, CTOR, "argv")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::init(lua, "cmd", argv, None, when)
}

/// Builds one rc source init entry table.
fn rc_source_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.source";
    let (path_value, opts) = args;
    check_route_slot(&path_value, CTOR, "path")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::init(lua, "source", Vec::new(), Some(path_value), when)
}

/// Opts guard resolver holding domain validation.
struct OptsGuard;

impl OptsGuard {
    /// Resolves the `when` guard from an opts value.
    ///
    /// # Arguments
    ///
    /// * `lua` - state calling builder functions.
    /// * `ctor` - error prefix naming the constructor.
    /// * `opts` - opts value holding nil or a table with a `when` field.
    ///
    /// # Returns
    ///
    /// Guard table holding `None` for no guard.
    ///
    /// # Errors
    ///
    /// Non-table opts fail as plan errors. Unknown opts fields fail as plan errors.
    /// Bad guard shapes fail as plan errors.
    ///
    fn resolve(lua: &Lua, ctor: &str, opts: Value) -> mlua::Result<Option<Table>> {
        if opts.is_nil() {
            return Ok(None);
        }
        let table = opts.req_table(ctor, "opts")?;
        for pair in table.pairs::<Value, Value>() {
            let (key, _) = pair?;
            let Some(name) = key.opt_str() else {
                return Err(plan_error(format!(
                    "{ctor}: field 'opts' must hold string keys"
                )));
            };
            if name != "when" {
                return Err(plan_error(format!(
                    "{ctor}: field 'opts' unknown field '{name}'"
                )));
            }
        }
        let when_value: Value = table.get("when")?;
        if let Some(func) = when_value.clone().opt_func() {
            let resolved = call_when_function(lua, ctor, &func)?;
            table.set("when", resolved)?;
        }
        let guard_value: Value = table.get("when")?;
        if guard_value.is_nil() {
            return Ok(None);
        }
        let Some(guard) = guard_value.opt_table() else {
            return Err(plan_error(format!(
                "{ctor}: field 'when' must be a condition table"
            )));
        };
        let json = guard
            .to_json(&format!("{ctor}: field 'when'"))
            .map_err(|error| plan_error(format!("{ctor}: field 'when' {error}")))?;
        check_condition_json(&json, &format!("{ctor}: field 'when'"))
            .map_err(|detail| plan_error(format!("{ctor}: field 'when' {detail}")))?;
        Ok(Some(guard))
    }
}

/// Calls one `when` builder function with the runtime namespace.
fn call_when_function(lua: &Lua, ctor: &str, func: &Function) -> mlua::Result<Table> {
    let missing = || {
        plan_error(format!(
            "{ctor}: field 'when' needs the confit.runtime table"
        ))
    };
    let confit: Value = lua.globals().get("confit")?;
    let confit = confit.req_table(ctor, "when").map_err(|_| missing())?;
    let runtime: Value = confit.get("runtime")?;
    let runtime = runtime.req_table(ctor, "when").map_err(|_| missing())?;
    match func.call::<Table>(runtime) {
        Ok(table) => Ok(table),
        Err(error) => {
            if crate::error::find_plan(&error).is_some() {
                return Err(error);
            }
            Err(plan_error(format!("{ctor}: field 'when' failed: {error}")))
        }
    }
}
