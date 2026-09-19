//! Document
//!
//! Document plus rc entry tables for Lua.

use std::path::{Path, PathBuf};

use mlua::{Function, Lua, MultiValue, Table, Value};

use super::confit_table;
use super::runtime::check_condition_json;
use crate::error::plan_error;
use crate::lua::{TableExt, ValueExt, read_marker, set_marker};
use crate::model::{LinkDecl, OpaqueDecl, RcEntryDecl, StructuredDecl, TextDecl, TreeDecl};
use confit_core::document::{RcData, StructuredFormat};
use confit_core::progress::{Event, ProgressSender};

pub(crate) mod archive;
pub(crate) mod convert;

use self::archive::read_members;

/// Declared document in registration form.
pub(crate) enum Declared {
    /// Structured declaration.
    Structured(StructuredDecl),
    /// Text declaration.
    Text(TextDecl),
    /// Link declaration.
    Link(LinkDecl),
    /// Opaque declaration holding raw bytes.
    Opaque(OpaqueDecl),
    /// Tree declaration holding one managed file set.
    Tree(TreeDecl),
    /// Rc base holding section buckets.
    Rc(Vec<RcEntryDecl>),
}

/// Installs the document plus rc namespaces on a state.
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
    namespace.set(
        "opaque",
        lua.create_function(|lua, args: (Value, Value, Option<Value>)| opaque_impl(lua, args))?,
    )?;
    let comp_root: PathBuf = session.root.clone();
    let comp_cache: PathBuf = session.cache.clone();
    let comp_progress = session.progress.clone();
    namespace.set(
        "compressed",
        lua.create_function(move |lua, args: (Value, Value)| {
            compressed_impl(lua, &comp_root, &comp_cache, args, comp_progress.clone())
        })?,
    )?;
    let tree_root: PathBuf = session.root.clone();
    let tree_cache: PathBuf = session.cache.clone();
    let tree_progress = session.progress.clone();
    namespace.set(
        "tree",
        lua.create_function(move |lua, args: (Value, Value, Value)| {
            tree_impl(lua, &tree_root, &tree_cache, args, tree_progress.clone())
        })?,
    )?;
    install_rc(lua, &namespace)?;
    confit.set("document", namespace)?;
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

/// Builds a structured document table from format plus args.
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
    let (path_value, content_value, opts) = args;
    let path = path_value.req_str(CTOR, "path")?;
    let content = content_value.req_str(CTOR, "content")?;
    let mode = ModeOpts::resolve(opts, CTOR)?;
    DocumentTables::text(lua, path, content, mode)
}

/// Builds a symlink document table.
fn link_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.link";
    let (path_value, target_value) = args;
    let path = path_value.req_str(CTOR, "path")?;
    let target = target_value.req_str(CTOR, "target")?;
    DocumentTables::link(lua, path, target)
}

/// Builds an opaque document table holding raw bytes.
fn opaque_impl(lua: &Lua, args: (Value, Value, Option<Value>)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.opaque";
    let (path_value, content_value, opts) = args;
    let path = path_value.req_str(CTOR, "path")?;
    let content = content_value.req_bytes(CTOR, "content")?;
    let mode = ModeOpts::resolve(opts, CTOR)?;
    DocumentTables::opaque(lua, path, content, mode)
}

/// Document table builders holding domain validation.
struct DocumentTables;

impl DocumentTables {
    /// Builds a structured document table from format plus args.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `ctor` - error prefix naming the constructor.
    /// * `format_name` - raw format name under parsing.
    /// * `args` - args table holding path plus data values.
    ///
    /// # Returns
    ///
    /// Document table stamped with the structured marker.
    ///
    /// # Errors
    ///
    /// Unknown formats fail as plan errors. Unknown args fields fail as plan errors.
    /// Non-string args keys fail as plan errors.
    ///
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
        let out = lua.create_table()?;
        let path: Value = args.get("path")?;
        out.set("path", path)?;
        let data: Value = args.get("data")?;
        out.set("data", data)?;
        set_marker(lua, &out, "structured", Some(("__format", format.name())))?;
        Ok(out)
    }

    /// Builds a plain text document table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `path` - destination path.
    /// * `content` - exact file text.
    /// * `mode` - unix permission bits, holding `None` for default handling.
    ///
    /// # Returns
    ///
    /// Document table stamped with the text marker plus the mode marker.
    ///
    fn text(lua: &Lua, path: String, content: String, mode: Option<u32>) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        out.set("path", path)?;
        out.set("content", content)?;
        let text = mode.map(|bits| bits.to_string());
        let extra = text.as_deref().map(|bits| ("__mode", bits));
        set_marker(lua, &out, "text", extra)?;
        Ok(out)
    }

    /// Builds a symlink document table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `path` - link path.
    /// * `target` - link target.
    ///
    /// # Returns
    ///
    /// Document table stamped with the link marker.
    ///
    fn link(lua: &Lua, path: String, target: String) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        out.set("path", path)?;
        out.set("target", target)?;
        set_marker(lua, &out, "link", None)?;
        Ok(out)
    }

    /// Builds an opaque document table holding raw bytes.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `path` - destination path.
    /// * `content` - raw file bytes.
    /// * `mode` - unix permission bits, holding `None` for default handling.
    ///
    /// # Returns
    ///
    /// Document table stamped with the opaque marker plus the mode marker.
    ///
    fn opaque(lua: &Lua, path: String, content: Vec<u8>, mode: Option<u32>) -> mlua::Result<Table> {
        let out = lua.create_table()?;
        out.set("path", path)?;
        out.set("content", lua.create_string(&content)?)?;
        let text = mode.map(|bits| bits.to_string());
        let extra = text.as_deref().map(|bits| ("__mode", bits));
        set_marker(lua, &out, "opaque", extra)?;
        Ok(out)
    }
}

/// Mode opts resolver holding domain validation.
struct ModeOpts;

impl ModeOpts {
    /// Resolves the unix permission bits from an opts value.
    ///
    /// # Arguments
    ///
    /// * `opts` - opts value holding nil, missing, or a table with a `mode` field.
    /// * `ctor` - error prefix naming the constructor.
    ///
    /// # Returns
    ///
    /// Mode bits holding `None` for missing plus nil opts.
    ///
    /// # Errors
    ///
    /// Non-table opts fail as plan errors. Unknown opts fields fail as plan errors.
    /// Non-string modes fail as plan errors. Unparsable modes fail as plan errors.
    ///
    fn resolve(opts: Option<Value>, ctor: &str) -> mlua::Result<Option<u32>> {
        let Some(opts) = opts else {
            return Ok(None);
        };
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
            if name != "mode" {
                return Err(plan_error(format!(
                    "{ctor}: field 'opts' unknown field '{name}'"
                )));
            }
        }
        let mode_value: Value = table.get("mode")?;
        if mode_value.is_nil() {
            return Ok(None);
        }
        let raw = mode_value.req_str(ctor, "mode")?;
        confit_core::document::parse_mode(&raw)
            .map(Some)
            .map_err(|error| plan_error(format!("{ctor}: {error}")))
    }
}

/// Unpacks one archive through a per-member callback.
fn compressed_impl(
    lua: &Lua,
    root: &Path,
    cache: &Path,
    args: (Value, Value),
    progress: Option<ProgressSender>,
) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.compressed";
    let (path_value, callback_value) = args;
    let rel = path_value.req_str(CTOR, "path")?;
    let callback = callback_value.req_func(CTOR, "callback")?;
    CompressedDocs::build(lua, root, cache, CTOR, rel, callback, progress)
}

/// Archive unpacker holding domain validation.
struct CompressedDocs;

impl CompressedDocs {
    /// Unpacks one archive through a per-member callback.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `root` - project root for archive reads.
    /// * `cache` - cache folder for cache-absolute reads.
    /// * `ctor` - error prefix naming the constructor.
    /// * `rel` - archive path under reading.
    /// * `callback` - per-member document picker.
    /// * `progress` - progress sender holding `None` for silence.
    ///
    /// # Returns
    ///
    /// Kept document tables in callback order.
    ///
    /// # Errors
    ///
    /// Empty paths fail as plan errors. Unreadable archives fail as plan errors.
    /// Non-document callback returns fail as plan errors.
    ///
    #[allow(clippy::too_many_arguments)]
    fn build(
        lua: &Lua,
        root: &Path,
        cache: &Path,
        ctor: &str,
        rel: String,
        callback: Function,
        progress: Option<ProgressSender>,
    ) -> mlua::Result<Table> {
        if rel.is_empty() {
            return Err(plan_error(format!(
                "{ctor}: field 'path' must not be empty"
            )));
        }
        let full = super::resources::resolve_under_root(root, cache, &rel, ctor)?;
        let start = std::time::Instant::now();
        let bytes = std::fs::read(&full)
            .map_err(|error| plan_error(format!("{ctor}: cannot read '{rel}': {error}")))?;
        let members = read_members(&bytes, &rel, ctor)?;
        let out = lua.create_table()?;
        let mut kept: i64 = 1;
        for member in &members {
            let info = lua.create_table()?;
            let size = member.size.min(i64::MAX as u64) as i64;
            info.set("size", size)?;
            info.set("executable", member.executable)?;
            let content = lua.create_string(&member.content)?;
            let returned: Value = callback.call((member.name.as_str(), info, content))?;
            if returned.is_nil() {
                continue;
            }
            let Some(table) = returned.opt_table() else {
                return Err(plan_error(format!(
                    "{ctor}: callback must return a document or nil"
                )));
            };
            if read_marker(&table, "__kind").is_none() {
                return Err(plan_error(format!(
                    "{ctor}: callback must return a document or nil"
                )));
            }
            out.set(kept, table)?;
            kept += 1;
        }
        log::debug!(
            "unpack archive={rel} kept={} total={} took {}ms",
            out.raw_len(),
            members.len(),
            start.elapsed().as_millis()
        );
        if let Some(sender) = progress.as_ref() {
            let _ = sender.send(Event::Unpacked {
                archive: rel.clone(),
                kept: out.raw_len(),
                total: members.len(),
            });
        }
        Ok(out)
    }
}

/// Builds one tree document from an archive through a path picker.
fn tree_impl(
    lua: &Lua,
    root: &Path,
    cache: &Path,
    args: (Value, Value, Value),
    progress: Option<ProgressSender>,
) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.tree";
    let (archive_value, dest_value, callback_value) = args;
    let rel = archive_value.req_str(CTOR, "archive")?;
    let dest = dest_value.req_str(CTOR, "dest")?;
    let callback = callback_value.req_func(CTOR, "callback")?;
    TreeDocs::build(lua, root, cache, CTOR, rel, dest, callback, progress)
}

/// Tree builder holding domain validation.
struct TreeDocs;

impl TreeDocs {
    /// Builds one tree document from an archive through a path picker.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the output table.
    /// * `root` - project root for archive reads.
    /// * `cache` - cache folder for cache-absolute reads.
    /// * `ctor` - error prefix naming the constructor.
    /// * `rel` - archive path under reading.
    /// * `dest` - destination folder holding the members.
    /// * `callback` - per-member destination picker.
    /// * `progress` - progress sender holding `None` for silence.
    ///
    /// # Returns
    ///
    /// One tree document table holding the manifest in
    /// relative path order.
    ///
    /// # Errors
    ///
    /// Empty archive paths plus empty destinations fail as plan
    /// errors. Unreadable archives fail as plan errors.
    /// Non-string callback returns fail as plan errors. Empty,
    /// absolute, plus dot-dot relative paths fail as plan
    /// errors. Repeated relative paths fail as plan errors.
    /// Empty picks fail as plan errors naming the filter.
    ///
    #[allow(clippy::too_many_arguments)]
    fn build(
        lua: &Lua,
        root: &Path,
        cache: &Path,
        ctor: &str,
        rel: String,
        dest: String,
        callback: Function,
        progress: Option<ProgressSender>,
    ) -> mlua::Result<Table> {
        if rel.is_empty() {
            return Err(plan_error(format!(
                "{ctor}: field 'archive' must not be empty"
            )));
        }
        if dest.is_empty() {
            return Err(plan_error(format!(
                "{ctor}: field 'dest' must not be empty"
            )));
        }
        let full = super::resources::resolve_under_root(root, cache, &rel, ctor)?;
        let start = std::time::Instant::now();
        let bytes = std::fs::read(&full)
            .map_err(|error| plan_error(format!("{ctor}: cannot read '{rel}': {error}")))?;
        let members = read_members(&bytes, &rel, ctor)?;
        let mut kept: Vec<(String, u32, Vec<u8>)> = Vec::new();
        for member in &members {
            let info = lua.create_table()?;
            let size = member.size.min(i64::MAX as u64) as i64;
            info.set("size", size)?;
            info.set("executable", member.executable)?;
            let content = lua.create_string(&member.content)?;
            let returned: Value = callback.call((member.name.as_str(), info, content))?;
            if returned.is_nil() {
                continue;
            }
            let Some(relpath) = returned.opt_str() else {
                return Err(plan_error(format!(
                    "{ctor}: callback must return a destination path or nil"
                )));
            };
            check_rel(ctor, &relpath)?;
            if kept.iter().any(|(kept_rel, _, _)| kept_rel == &relpath) {
                return Err(plan_error(format!(
                    "{ctor}: tree keeps '{relpath}' more than once"
                )));
            }
            let mode = if member.executable { 0o755 } else { 0o644 };
            kept.push((relpath, mode, member.content.clone()));
        }
        if kept.is_empty() {
            return Err(plan_error(format!(
                "{ctor}: tree kept no members from '{rel}': check the pick filter"
            )));
        }
        kept.sort_by(|left, right| left.0.cmp(&right.0));
        let list = lua.create_table()?;
        for (index, (relpath, mode, content)) in kept.iter().enumerate() {
            let item = lua.create_table()?;
            item.set("rel", relpath.as_str())?;
            item.set("mode", i64::from(*mode))?;
            item.set("content", lua.create_string(content)?)?;
            list.set(index + 1, item)?;
        }
        let out = lua.create_table()?;
        out.set("path", dest)?;
        out.set("members", list)?;
        set_marker(lua, &out, "tree", None)?;
        log::debug!(
            "unpack archive={rel} kept={} total={} took {}ms",
            kept.len(),
            members.len(),
            start.elapsed().as_millis()
        );
        if let Some(sender) = progress.as_ref() {
            let _ = sender.send(Event::Unpacked {
                archive: rel.clone(),
                kept: kept.len(),
                total: members.len(),
            });
        }
        Ok(out)
    }
}

/// Rejects destination paths escaping the tree folder.
fn check_rel(ctor: &str, relpath: &str) -> mlua::Result<()> {
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

/// Builds one rc path prepend entry table from dir or var plus dir.
fn rc_prepend_impl(lua: &Lua, args: MultiValue) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.prepend";
    let collected: Vec<Value> = args.into_iter().collect();
    let (name, dir, opts) = match collected.as_slice() {
        [dir_value] => (
            "PATH".to_string(),
            dir_value.clone().req_str(CTOR, "dir")?,
            Value::Nil,
        ),
        [first, second] => match second.clone().opt_str() {
            Some(dir) => (first.clone().req_str(CTOR, "var")?, dir, Value::Nil),
            None => (
                "PATH".to_string(),
                first.clone().req_str(CTOR, "dir")?,
                second.clone(),
            ),
        },
        [var_value, dir_value, opts] => (
            var_value.clone().req_str(CTOR, "var")?,
            dir_value.clone().req_str(CTOR, "dir")?,
            opts.clone(),
        ),
        _ => {
            return Err(plan_error(format!(
                "{CTOR}: 'prepend' expects (dir, opts?) or (var, dir, opts?)"
            )));
        }
    };
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::path(lua, name, dir, when)
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
    /// * `dir` - directory under prepending.
    /// * `when` - guard table holding `None` for no guard.
    ///
    /// # Returns
    ///
    /// Entry table stamped with the rc-entry marker.
    ///
    fn path(lua: &Lua, name: String, dir: String, when: Option<Table>) -> mlua::Result<Table> {
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
    /// * `argv` - command words, empty for source entries.
    /// * `source` - sourced path for source entries.
    /// * `when` - guard table holding `None` for no guard.
    ///
    /// # Returns
    ///
    /// Entry table stamped with the rc-entry marker.
    ///
    fn init(
        lua: &Lua,
        shape: &str,
        argv: Vec<String>,
        source: Option<String>,
        when: Option<Table>,
    ) -> mlua::Result<Table> {
        let inner = lua.create_table()?;
        if shape == "source" {
            inner.set("path", source.unwrap_or_default())?;
        } else {
            let argv_table = lua.create_table()?;
            for (position, item) in argv.iter().enumerate() {
                argv_table.set((position + 1) as i64, item.as_str())?;
            }
            inner.set("argv", argv_table)?;
        }
        Self::tagged(lua, shape, inner, when)
    }

    /// Wraps one op inner table plus guard into an entry table.
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
    let argv = argv_value.req_string_array(CTOR, "argv")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::init(lua, "eval", argv, None, when)
}

/// Builds one rc cmd init entry table.
fn rc_cmd_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.cmd";
    let (argv_value, opts) = args;
    let argv = argv_value.req_string_array(CTOR, "argv")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::init(lua, "cmd", argv, None, when)
}

/// Builds one rc source init entry table.
fn rc_source_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<Table> {
    const CTOR: &str = "confit.document.rc.source";
    let (path_value, opts) = args;
    let path = path_value.req_str(CTOR, "path")?;
    let when = OptsGuard::resolve(lua, CTOR, opts)?;
    RcEntries::init(lua, "source", Vec::new(), Some(path), when)
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
