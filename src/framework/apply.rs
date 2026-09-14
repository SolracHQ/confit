//! Apply
//!
//! Live document wrapper for patch callbacks. The wrapper holds one live
//! document table plus shared owners. Callbacks run in pipeline order.
//! First writer wins per slot.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::{Function, Lua, MultiValue, Table, UserData, Value};
use serde_json::Value as Json;

use crate::model::state::level::Level;

use super::document::{entry_table_to_json, table_kind};

/// Patch input for live execution.
///
/// Carries owner plus priority plus callback. Binding sorts inputs by
/// priority desc plus owner asc, then runs them in order.
///
/// # Examples
///
/// ```rust
/// use confit::framework::apply::ExecPatch;
/// use confit::model::state::level::Level;
///
/// let owner = String::from("bat");
/// assert_eq!(owner, "bat");
/// assert!(matches!(Level::Normal, Level::Normal));
/// ```
#[derive(Debug, Clone)]
pub struct ExecPatch {
    /// Holds the contributing config name.
    pub owner: String,
    /// Holds the merge priority for ordering.
    pub priority: Level,
    /// Holds the callback receiving the wrapper.
    pub callback: Function,
}

/// Document area selecting wrapper rules.
///
/// Structured areas write dotted paths. Rc areas write closed sections.
/// Format string feeds log lines only.
///
/// # Examples
///
/// ```rust
/// use confit::framework::apply::Area;
///
/// let area = Area::Rc;
/// assert!(matches!(area, Area::Rc));
/// ```
#[derive(Debug, Clone)]
pub enum Area {
    /// Structured area holding the format name for log lines.
    Structured {
        /// Holds the format name for log lines.
        format: String,
    },
    /// Rc area holding profile plus config plus final sections.
    Rc,
}

/// Shared owners mapping canonical slot to winner name.
///
/// Structured slots key by dotted path. Rc slots key by section plus
/// name plus guard. First writer wins.
pub type OwnerMap = BTreeMap<String, String>;

/// Wrapper userdata holding one live document table.
///
/// Holds a clone of the document table plus shared owners plus area
/// plus current owner plus priority. Methods mutate the live table.
#[derive(Debug, Clone)]
struct Wrapper {
    /// Holds the live document table.
    doc: Table,
    /// Holds shared winners, mutated in place.
    owners: Rc<RefCell<OwnerMap>>,
    /// Holds the area selecting rules.
    area: Area,
    /// Holds the current patch owner.
    owner: String,
    /// Holds the current patch priority.
    priority: Level,
}

/// Executes patch callbacks against one live document table.
///
/// Builds one wrapper per patch in order sharing one owners map seeded
/// from declared bases. Each callback receives the wrapper. Mutations
/// land on the live table.
///
/// # Arguments
///
/// * `lua` - state owning the tables plus callbacks.
/// * `doc` - live document table under mutation.
/// * `area` - area selecting wrapper rules.
/// * `seeds` - initial winners from declared bases.
/// * `patches` - patches in pipeline order.
///
/// # Returns
///
/// Unit after every callback runs.
///
/// # Errors
///
/// Fails with callback failures plus wrapper plan errors.
///
/// # Examples
///
/// ```rust,no_run
/// use std::collections::BTreeMap;
/// use confit::framework::apply::{Area, execute};
/// use mlua::Lua;
///
/// let lua = Lua::new();
/// let doc = match lua.create_table() {
///     Ok(doc) => doc,
///     Err(error) => panic!("table builds: {error}"),
/// };
/// let area = Area::Structured { format: "json".into() };
/// let seeds = BTreeMap::new();
/// match execute(&lua, doc, area, seeds, Vec::new()) {
///     Ok(()) => (),
///     Err(error) => panic!("execute runs: {error}"),
/// };
/// assert!(true);
/// ```
pub fn execute(
    lua: &Lua,
    doc: Table,
    area: Area,
    seeds: OwnerMap,
    patches: Vec<ExecPatch>,
) -> mlua::Result<()> {
    let owners: Rc<RefCell<OwnerMap>> = Rc::new(RefCell::new(seeds));
    for patch in patches {
        let wrapper = Wrapper {
            doc: doc.clone(),
            owners: Rc::clone(&owners),
            area: area.clone(),
            owner: patch.owner.clone(),
            priority: patch.priority,
        };
        let handle = lua.create_userdata(wrapper)?;
        patch.callback.call::<()>(handle)?;
    }
    Ok(())
}

/// Flattens JSON into dotted leaves.
///
/// # Arguments
///
/// * `value` - subtree under walk.
/// * `prefix` - dotted location of the subtree.
/// * `out` - accumulator for leaf entries.
pub(crate) fn flatten_json(value: &Json, prefix: &str, out: &mut BTreeMap<String, Json>) {
    match value {
        Json::Object(map) => {
            for (key, item) in map {
                let child = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_json(item, &child, out);
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten_json(item, &format!("{prefix}[{index}]"), out);
            }
        }
        _ => {
            out.insert(prefix.to_string(), value.clone());
        }
    }
}

/// Renders JSON in canonical string form.
///
/// # Arguments
///
/// * `value` - value under rendering.
///
/// # Returns
///
/// Canonical string.
///
/// # Errors
///
/// Fails with plan errors for serialization failures.
pub(crate) fn json_text(value: &Json) -> mlua::Result<String> {
    serde_json::to_string(value)
        .map_err(|error| plan_err!("patch: value failed to render: {error}"))
}

impl UserData for Wrapper {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("set", |lua, this, args: MultiValue| {
            this.call_op(lua, false, args)
        });
        methods.add_method("append", |lua, this, args: MultiValue| {
            this.call_op(lua, true, args)
        });
    }
}

impl Wrapper {
    /// Runs one `set` or `append` call.
    ///
    /// Accepts `(path, value)` with optional leading self. Set plus
    /// append behave identically for rc, first wins.
    ///
    /// # Arguments
    ///
    /// * `self` - wrapper holding live table plus owners.
    /// * `lua` - state owning conversions.
    /// * `append` - true for append, false for set.
    /// * `args` - raw call arguments.
    ///
    /// # Returns
    ///
    /// Unit after the write.
    ///
    /// # Errors
    ///
    /// Fails with plan errors for bad shapes plus blocked writes.
    fn call_op(&self, lua: &Lua, append: bool, args: MultiValue) -> mlua::Result<()> {
        let collected: Vec<Value> = args.into_iter().collect();
        let (path_value, value_value) = match collected.as_slice() {
            [path, value] => (path.clone(), value.clone()),
            [_self, path, value] => (path.clone(), value.clone()),
            _ => {
                let op = if append { "append" } else { "set" };
                return Err(plan_err!("patch: '{op}' expects (path, value)"));
            }
        };
        let path = match path_value {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                let op = if append { "append" } else { "set" };
                return Err(plan_err!("patch: '{op}' field 'path' must be a string"));
            }
        };
        match &self.area {
            Area::Rc => self.rc_op(lua, &path, value_value),
            Area::Structured { format } => {
                if append {
                    self.structured_append(lua, &path, value_value)
                } else {
                    self.structured_set(lua, &path, value_value, format)
                }
            }
        }
    }

    /// Applies one rc insert with first-writer wins.
    ///
    /// # Arguments
    ///
    /// * `self` - wrapper holding live table plus owners.
    /// * `lua` - state owning conversions.
    /// * `path` - closed section name.
    /// * `value` - rc entry table.
    ///
    /// # Returns
    ///
    /// Unit after insert or foreign drop.
    ///
    /// # Errors
    ///
    /// Fails with plan errors for bad sections plus bad entries.
    fn rc_op(&self, lua: &Lua, path: &str, value: Value) -> mlua::Result<()> {
        if !matches!(path, "profile" | "config" | "final") {
            return Err(plan_err!(
                "patch: unknown section '{path}' (expected 'profile', 'config', or 'final')"
            ));
        }
        let table = match value {
            Value::Table(table) => table,
            _ => {
                return Err(plan_err!("patch: field 'value' must be an rc entry table"));
            }
        };
        if table_kind(&table).as_deref() != Some("rc-entry") {
            return Err(plan_err!("patch: field 'value' must be an rc entry table"));
        }
        let (area, mut json) = entry_table_to_json(&table, "patch: field 'value'")?;
        stamp_priority(&mut json, self.priority)?;
        let area_name = match area.as_str() {
            "alias" => "alias".to_string(),
            "env" => "env".to_string(),
            "profile" => "profile".to_string(),
            "init" => "init".to_string(),
            other => {
                return Err(plan_err!(
                    "patch: field 'value' unknown entry area '{other}'"
                ));
            }
        };
        if path == "final" && area_name != "init" {
            return Err(plan_err!("patch: invalid rc entry for section '{path}'"));
        }
        if path != "final" && area_name == "init" {
            return Err(plan_err!("patch: invalid rc entry for section '{path}'"));
        }
        if path == "config" && area_name != "alias" {
            return Err(plan_err!("patch: invalid rc entry for section '{path}'"));
        }
        if path == "profile" && area_name != "env" && area_name != "profile" {
            return Err(plan_err!("patch: invalid rc entry for section '{path}'"));
        }
        if path == "final" && area_name != "init" {
            return Err(plan_err!("patch: invalid rc entry for section '{path}'"));
        }
        if path != "final" && area_name == "init" {
            return Err(plan_err!("patch: invalid rc entry for section '{path}'"));
        }
        let name = entry_name(&json, &area_name)?;
        let slot = rc_live_slot(path, &json, &area_name)?;
        {
            let mut owned = self.owners.borrow_mut();
            if let Some(winner) = owned.get(&slot) {
                if winner != &self.owner && area_name != "init" {
                    log::debug!(
                        "collision on {area_name} \"{name}\": \"{}\" overwritten, \"{winner}\" wins",
                        self.owner,
                    );
                }
                return Ok(());
            }
            owned.insert(slot, self.owner.clone());
        }
        let lua_value = json_to_lua(lua, &json)?;
        let list = get_or_create_list(lua, &self.doc, path)?;
        let next = (list.raw_len() + 1) as i64;
        list.set(next, lua_value)?;
        Ok(())
    }

    /// Writes one structured leaf with first-writer wins.
    ///
    /// # Arguments
    ///
    /// * `self` - wrapper holding live table plus owners.
    /// * `lua` - state owning conversions.
    /// * `path` - dotted path.
    /// * `value` - data-only value.
    /// * `format` - format name for log lines.
    ///
    /// # Returns
    ///
    /// Unit after write or foreign drop.
    ///
    /// # Errors
    ///
    /// Fails with plan errors for bad paths plus blocked writes.
    fn structured_set(
        &self,
        lua: &Lua,
        path: &str,
        value: Value,
        format: &str,
    ) -> mlua::Result<()> {
        let json = value_to_json(value, "patch: field 'value'")?;
        let segments = parse_path(path)?;
        let lua_value = json_to_lua(lua, &json)?;
        set_live_value(
            lua,
            &self.doc,
            &segments,
            path,
            lua_value,
            &json,
            &self.owner,
            format,
            &self.owners,
        )
    }

    /// Extends one structured list.
    ///
    /// # Arguments
    ///
    /// * `self` - wrapper holding live table plus owners.
    /// * `lua` - state owning conversions.
    /// * `path` - dotted path naming the list.
    /// * `value` - data-only value.
    ///
    /// # Returns
    ///
    /// Unit after extension.
    ///
    /// # Errors
    ///
    /// Fails with plan errors for bad paths plus non-list leaves.
    fn structured_append(&self, lua: &Lua, path: &str, value: Value) -> mlua::Result<()> {
        let json = value_to_json(value, "patch: field 'value'")?;
        let segments = parse_path(path)?;
        let lua_value = json_to_lua(lua, &json)?;
        append_live_value(
            lua,
            &self.doc,
            &segments,
            path,
            lua_value,
            &json,
            &self.owner,
            &self.owners,
        )
    }
}

/// Stamps patch priority into an rc entry JSON.
///
/// # Arguments
///
/// * `entry` - entry JSON under stamping, mutated in place.
/// * `priority` - patch priority.
///
/// # Returns
///
/// Unit after stamping.
///
/// # Errors
///
/// Fails with plan errors for serialization failures.
fn stamp_priority(entry: &mut Json, priority: Level) -> mlua::Result<()> {
    let name = match priority {
        Level::Minor => "minor",
        Level::Low => "low",
        Level::Normal => "normal",
        Level::High => "high",
        Level::Major => "major",
    };
    if let Some(object) = entry.as_object_mut() {
        object.insert("priority".to_string(), Json::String(name.to_string()));
    }
    Ok(())
}

/// Derives the collision area for an rc entry.
///
/// Config means alias, final means init, profile means profile when
/// the table holds `op` else env.
///
/// # Arguments
///
/// * `section` - closed section name.
/// * `entry` - entry JSON.
///
/// # Returns
///
/// Area name for log lines.
///
/// # Errors
///
/// Fails with plan errors for entries holding no usable shape.
pub(crate) fn rc_area_name(section: &str, entry: &Json) -> mlua::Result<String> {
    if section == "config" {
        return Ok("alias".to_string());
    }
    if section == "final" {
        return Ok("init".to_string());
    }
    let object = match entry.as_object() {
        Some(object) => object,
        None => {
            return Err(plan_err!("patch: invalid rc entry for section '{section}'"));
        }
    };
    if object.contains_key("op") {
        Ok("profile".to_string())
    } else if object.contains_key("name") {
        Ok("env".to_string())
    } else if object.contains_key("eval")
        || object.contains_key("cmd")
        || object.contains_key("source")
    {
        Ok("init".to_string())
    } else {
        Err(plan_err!("patch: invalid rc entry for section '{section}'"))
    }
}

/// Reads the entry name for logs plus slots.
///
/// # Arguments
///
/// * `entry` - entry JSON.
/// * `area` - collision area.
///
/// # Returns
///
/// Entry name, empty for init entries.
///
/// # Errors
///
/// Fails with plan errors for missing names outside init.
pub(crate) fn entry_name(entry: &Json, area: &str) -> mlua::Result<String> {
    if area == "init" {
        return Ok(String::new());
    }
    match entry.get("name").and_then(Json::as_str) {
        Some(name) => Ok(name.to_string()),
        None => Err(plan_err!("patch: field 'value' entry holds no name")),
    }
}

/// Derives the live slot key for an rc entry.
///
/// Name plus guard compare via canonical JSON strings.
///
/// # Arguments
///
/// * `section` - closed section name.
/// * `entry` - entry JSON.
/// * `area` - collision area.
///
/// # Returns
///
/// Canonical slot key.
///
/// # Errors
///
/// Fails with plan errors for serialization failures.
pub(crate) fn rc_live_slot(section: &str, entry: &Json, area: &str) -> mlua::Result<String> {
    if area == "init" {
        let spec = entry
            .get("eval")
            .or_else(|| entry.get("cmd"))
            .or_else(|| entry.get("source"))
            .unwrap_or(&Json::Null);
        let spec_text = json_text(spec)?;
        let when_text = json_text(entry.get("when").unwrap_or(&Json::Null))?;
        return Ok(format!("{section}/{spec_text}/{when_text}"));
    }
    let name = entry_name(entry, area)?;
    let when_text = json_text(entry.get("when").unwrap_or(&Json::Null))?;
    Ok(format!("{section}/{name}/{when_text}"))
}

/// Fetches a section list, creating it while absent.
///
/// # Arguments
///
/// * `lua` - state owning new tables.
/// * `doc` - live document table.
/// * `section` - closed section name.
///
/// # Returns
///
/// Section list table.
///
/// # Errors
///
/// Fails with plan errors for sections holding non-table values.
fn get_or_create_list(lua: &Lua, doc: &Table, section: &str) -> mlua::Result<Table> {
    let current: Value = doc.get(section)?;
    match current {
        Value::Nil => {
            let list = lua.create_table().map_err(|error| {
                plan_err!("patch: section '{section}' failed to build: {error}")
            })?;
            doc.set(section, list.clone())?;
            Ok(list)
        }
        Value::Table(list) => Ok(list),
        _ => Err(plan_err!(
            "patch: section '{section}' holds a non-list leaf"
        )),
    }
}

/// Holds one parsed path segment.
///
/// Carries a table key plus an optional single list index.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    /// Holds the table key.
    key: String,
    /// Holds the list index, empty for plain keys.
    index: Option<usize>,
}

/// Parses a patch path into segments.
///
/// Accepts dotted keys with single indices like `a.b[0]`.
///
/// # Arguments
///
/// * `path` - document-local path.
///
/// # Returns
///
/// Segments in order.
///
/// # Errors
///
/// Fails with plan errors for empty paths plus bad segments naming the path.
fn parse_path(path: &str) -> mlua::Result<Vec<Segment>> {
    if path.is_empty() {
        return Err(plan_err!("invalid path '': empty path"));
    }
    let mut segments = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            return Err(plan_err!("invalid path '{path}': empty segment"));
        }
        let (key, index) = match part.find('[') {
            None => {
                if part.contains(']') {
                    return Err(plan_err!("invalid path '{path}': bad index"));
                }
                (part.to_string(), None)
            }
            Some(open) => {
                let key = part[..open].to_string();
                if key.is_empty() {
                    return Err(plan_err!("invalid path '{path}': empty key"));
                }
                if !part.ends_with(']') {
                    return Err(plan_err!("invalid path '{path}': bad index"));
                }
                let inner = &part[open + 1..part.len() - 1];
                if inner.is_empty() || !inner.chars().all(|item| item.is_ascii_digit()) {
                    return Err(plan_err!("invalid path '{path}': bad index"));
                }
                let index = match inner.parse::<usize>() {
                    Ok(index) => index,
                    Err(_) => {
                        return Err(plan_err!("invalid path '{path}': bad index"));
                    }
                };
                if part[open + 1..].contains('[') {
                    return Err(plan_err!("invalid path '{path}': bad index"));
                }
                (key, Some(index))
            }
        };
        segments.push(Segment { key, index });
    }
    Ok(segments)
}

/// Descends into one intermediate segment over live tables.
///
/// Creates tables plus lists as needed.
///
/// # Arguments
///
/// * `current` - value under descent.
/// * `lua` - state owning new tables.
/// * `segment` - intermediate segment.
/// * `full` - full path for errors.
///
/// # Returns
///
/// Child table holding the next level.
///
/// # Errors
///
/// Fails with plan errors for non-table intermediates naming the path.
fn live_child(current: &Value, lua: &Lua, segment: &Segment, full: &str) -> mlua::Result<Table> {
    let map = match current {
        Value::Table(map) => map.clone(),
        Value::Nil => {
            return Err(plan_err!("cannot write '{full}': '{full}' is blocked"));
        }
        _ => {
            return Err(plan_err!("cannot write '{full}': '{full}' is blocked"));
        }
    };
    match segment.index {
        None => {
            let slot: Value = map.get(segment.key.as_str())?;
            match slot {
                Value::Nil => {
                    let child = lua.create_table()?;
                    map.set(segment.key.as_str(), child.clone())?;
                    Ok(child)
                }
                Value::Table(child) => Ok(child),
                _ => Err(plan_err!("cannot write '{full}': '{full}' is blocked")),
            }
        }
        Some(index) => {
            let slot: Value = map.get(segment.key.as_str())?;
            let list = match slot {
                Value::Nil => {
                    let fresh = lua.create_table()?;
                    map.set(segment.key.as_str(), fresh.clone())?;
                    fresh
                }
                Value::Table(existing) => existing,
                _ => {
                    return Err(plan_err!("cannot write '{full}': '{full}' is blocked"));
                }
            };
            let lua_index = (index + 1) as i64;
            let current_len = list.raw_len() as i64;
            if lua_index > current_len + 1 {
                while (list.raw_len() as i64) < lua_index - 1 {
                    let filler = lua.create_table()?;
                    let next = (list.raw_len() + 1) as i64;
                    list.set(next, filler)?;
                }
                let child = lua.create_table()?;
                list.set(lua_index, child.clone())?;
                return Ok(child);
            }
            if lua_index == current_len + 1 {
                let child = lua.create_table()?;
                list.set(lua_index, child.clone())?;
                return Ok(child);
            }
            let element: Value = list.get(lua_index)?;
            match element {
                Value::Table(child) => Ok(child),
                Value::Nil => {
                    let child = lua.create_table()?;
                    list.set(lua_index, child.clone())?;
                    Ok(child)
                }
                _ => Err(plan_err!("cannot write '{full}': '{full}' is blocked")),
            }
        }
    }
}

/// Navigates to the parent table for a segment prefix.
///
/// # Arguments
///
/// * `lua` - state owning new tables.
/// * `doc` - live document table.
/// * `prefix` - intermediate segments.
/// * `full` - full path for errors.
///
/// # Returns
///
/// Parent table holding the leaf.
///
/// # Errors
///
/// Fails with plan errors for blocked intermediates naming the path.
fn live_parent(lua: &Lua, doc: &Table, prefix: &[Segment], full: &str) -> mlua::Result<Table> {
    let mut current = Value::Table(doc.clone());
    for segment in prefix {
        let child = live_child(&current, lua, segment, full)?;
        current = Value::Table(child);
    }
    match current {
        Value::Table(parent) => Ok(parent),
        _ => Err(plan_err!("cannot write '{full}': '{full}' is blocked")),
    }
}

/// Reports list status for a Lua value.
///
/// # Arguments
///
/// * `value` - value under inspection.
///
/// # Returns
///
/// True for nil plus empty tables plus dense arrays.
fn is_live_list(value: &Value) -> bool {
    match value {
        Value::Nil => true,
        Value::Table(table) => {
            let len = table.raw_len();
            if len == 0 {
                return table_is_empty(table);
            }
            for index in 1..=(len as i64) {
                let item: Result<Value, _> = table.get(index);
                if item.is_err() {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

/// Reports emptiness for a Lua table.
///
/// # Arguments
///
/// * `table` - table under inspection.
///
/// # Returns
///
/// True while the table holds no pairs.
fn table_is_empty(table: &Table) -> bool {
    for pair in table.pairs::<Value, Value>() {
        if pair.is_ok() {
            return false;
        }
    }
    true
}

/// Writes one leaf into a live table with first-writer wins.
///
/// Foreign overwrites keep first plus one collision line.
///
/// # Arguments
///
/// * `doc` - live table under write.
/// * `segments` - parsed path.
/// * `full` - full path for errors plus winner keys plus logs.
/// * `lua_value` - Lua value landing on the path.
/// * `json` - JSON value for leaf expansion.
/// * `owner` - contributing config name.
/// * `format` - format name for log lines.
/// * `owners` - winners under update.
///
/// # Returns
///
/// Unit after write or foreign drop.
///
/// # Errors
///
/// Fails with plan errors for blocked writes naming the path.
#[allow(clippy::too_many_arguments)]
fn set_live_value(
    lua: &Lua,
    doc: &Table,
    segments: &[Segment],
    full: &str,
    lua_value: Value,
    json: &Json,
    owner: &str,
    format: &str,
    owners: &Rc<RefCell<OwnerMap>>,
) -> mlua::Result<()> {
    let (last, prefix) = match segments.split_last() {
        Some(pair) => pair,
        None => {
            return Err(plan_err!("invalid path '{full}': empty path"));
        }
    };
    let parent = live_parent(lua, doc, prefix, full)?;
    {
        let owned = owners.borrow();
        if let Some(winner) = owned.get(full)
            && winner != owner
        {
            log::debug!(
                "collision on {format} \"{full}\": \"{owner}\" overwritten, \"{winner}\" wins"
            );
            return Ok(());
        }
    }
    match last.index {
        None => {
            parent.set(last.key.as_str(), lua_value)?;
        }
        Some(index) => {
            let entry: Value = parent.get(last.key.as_str())?;
            let list = match entry {
                Value::Nil => {
                    let fresh = lua.create_table()?;
                    parent.set(last.key.as_str(), fresh.clone())?;
                    fresh
                }
                Value::Table(existing) => existing,
                _ => {
                    return Err(plan_err!("cannot write '{full}': '{full}' is blocked"));
                }
            };
            let lua_index = (index + 1) as i64;
            let current_len = list.raw_len() as i64;
            if lua_index > current_len + 1 {
                return Err(plan_err!("cannot write '{full}': index out of bounds"));
            }
            list.set(lua_index, lua_value)?;
        }
    }
    {
        let mut owned = owners.borrow_mut();
        let dotted = format!("{full}.");
        let indexed = format!("{full}[");
        owned.retain(|key, _| {
            key != full && !key.starts_with(&dotted) && !key.starts_with(&indexed)
        });
        let mut leaves = BTreeMap::new();
        flatten_json(json, full, &mut leaves);
        for leaf in leaves.keys() {
            owned.insert(leaf.clone(), owner.to_string());
        }
    }
    Ok(())
}

/// Appends one value to a live list.
///
/// Missing leaves create the list. Non-list leaves fail.
///
/// # Arguments
///
/// * `doc` - live table under extension.
/// * `segments` - parsed path.
/// * `full` - full path for errors plus winner keys.
/// * `lua_value` - Lua value appended to the list.
/// * `json` - JSON value for leaf expansion.
/// * `owner` - contributing config name.
/// * `owners` - winners under update.
///
/// # Returns
///
/// Unit after extension.
///
/// # Errors
///
/// Fails with plan errors for blocked paths plus non-list leaves.
#[allow(clippy::too_many_arguments)]
fn append_live_value(
    lua: &Lua,
    doc: &Table,
    segments: &[Segment],
    full: &str,
    lua_value: Value,
    json: &Json,
    owner: &str,
    owners: &Rc<RefCell<OwnerMap>>,
) -> mlua::Result<()> {
    let (last, prefix) = match segments.split_last() {
        Some(pair) => pair,
        None => {
            return Err(plan_err!("invalid path '{full}': empty path"));
        }
    };
    let parent = live_parent(lua, doc, prefix, full)?;
    match last.index {
        None => {
            let entry: Value = parent.get(last.key.as_str())?;
            let list = match entry {
                Value::Nil => {
                    let fresh = lua.create_table()?;
                    parent.set(last.key.as_str(), fresh.clone())?;
                    fresh
                }
                Value::Table(existing) => match is_live_list(&Value::Table(existing.clone())) {
                    true => existing,
                    false => {
                        return Err(plan_err!(
                            "cannot append '{full}': '{full}' holds a non-list leaf"
                        ));
                    }
                },
                _ => {
                    return Err(plan_err!(
                        "cannot append '{full}': '{full}' holds a non-list leaf"
                    ));
                }
            };
            let next = (list.raw_len() + 1) as i64;
            list.set(next, lua_value)?;
            let mut owned = owners.borrow_mut();
            let leaf = format!("{full}[{}]", next - 1);
            let mut leaves = BTreeMap::new();
            flatten_json(json, &leaf, &mut leaves);
            for leaf_path in leaves.keys() {
                owned.insert(leaf_path.clone(), owner.to_string());
            }
            Ok(())
        }
        Some(index) => {
            let entry: Value = parent.get(last.key.as_str())?;
            let list = match entry {
                Value::Nil => {
                    let fresh = lua.create_table()?;
                    parent.set(last.key.as_str(), fresh.clone())?;
                    fresh
                }
                Value::Table(existing) => existing,
                _ => {
                    return Err(plan_err!(
                        "cannot append '{full}': '{full}' holds a non-list leaf"
                    ));
                }
            };
            let lua_index = (index + 1) as i64;
            let current_len = list.raw_len() as i64;
            if lua_index > current_len + 1 {
                return Err(plan_err!("cannot append '{full}': index out of bounds"));
            }
            if lua_index == current_len + 1 {
                let inner = lua.create_table()?;
                inner.set(1_i64, lua_value)?;
                list.set(lua_index, inner)?;
                let mut owned = owners.borrow_mut();
                let leaf = format!("{full}[0]");
                let mut leaves = BTreeMap::new();
                flatten_json(json, &leaf, &mut leaves);
                for leaf_path in leaves.keys() {
                    owned.insert(leaf_path.clone(), owner.to_string());
                }
                return Ok(());
            }
            let slot: Value = list.get(lua_index)?;
            match slot {
                Value::Table(inner) => {
                    if !is_live_list(&Value::Table(inner.clone())) {
                        return Err(plan_err!(
                            "cannot append '{full}': '{full}' holds a non-list leaf"
                        ));
                    }
                    let next = (inner.raw_len() + 1) as i64;
                    inner.set(next, lua_value)?;
                    let mut owned = owners.borrow_mut();
                    let leaf = format!("{full}[{}]", next - 1);
                    let mut leaves = BTreeMap::new();
                    flatten_json(json, &leaf, &mut leaves);
                    for leaf_path in leaves.keys() {
                        owned.insert(leaf_path.clone(), owner.to_string());
                    }
                    Ok(())
                }
                Value::Nil => {
                    let inner = lua.create_table()?;
                    inner.set(1_i64, lua_value)?;
                    list.set(lua_index, inner)?;
                    Ok(())
                }
                _ => Err(plan_err!(
                    "cannot append '{full}': '{full}' holds a non-list leaf"
                )),
            }
        }
    }
}

/// Converts a Lua value into JSON as a plan error.
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
/// Fails with plan errors for executable shapes.
fn value_to_json(value: Value, ctx: &str) -> mlua::Result<Json> {
    match value {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(flag) => Ok(Json::Bool(flag)),
        Value::Integer(number) => Ok(Json::Number(number.into())),
        Value::Number(number) => match serde_json::Number::from_f64(number) {
            Some(parsed) => Ok(Json::Number(parsed)),
            None => Err(plan_err!("{ctx} must be a finite number")),
        },
        Value::String(text) => Ok(Json::String(text.to_string_lossy())),
        Value::Table(table) => table_to_json_value(&table, ctx),
        Value::Function(_) => Err(plan_err!("{ctx} must be data-only (function not allowed)")),
        Value::UserData(_) => Err(plan_err!("{ctx} must be data-only (userdata not allowed)")),
        Value::LightUserData(_) => Err(plan_err!("{ctx} must be data-only (userdata not allowed)")),
        Value::Thread(_) => Err(plan_err!("{ctx} must be data-only (thread not allowed)")),
        Value::Error(_) => Err(plan_err!("{ctx} must be data-only")),
        Value::Other(_) => Err(plan_err!("{ctx} must be data-only")),
    }
}

/// Converts a Lua table into JSON as a plan error.
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
/// Fails with plan errors for bad keys plus executable values.
fn table_to_json_value(table: &Table, ctx: &str) -> mlua::Result<Json> {
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
                items.push(value_to_json(value.clone(), &child)?);
            }
            return Ok(Json::Array(items));
        }
    }
    let mut map = serde_json::Map::new();
    for (key, value) in &entries {
        let name = match key {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(plan_err!("{ctx} must be a table with string keys"));
            }
        };
        let child = format!("{ctx}.{name}");
        map.insert(name, value_to_json(value.clone(), &child)?);
    }
    Ok(Json::Object(map))
}

/// Converts a live table into JSON for binding.
///
/// # Arguments
///
/// * `table` - live table.
/// * `ctx` - field path for errors.
///
/// # Returns
///
/// JSON holding table data.
///
/// # Errors
///
/// Fails with plan errors for executable values.
pub(crate) fn live_to_json(table: &Table, ctx: &str) -> mlua::Result<Json> {
    table_to_json_value(table, ctx)
}

/// Converts JSON into a Lua value.
///
/// Objects become tables with string keys, arrays become dense tables
/// with 1-based indices, scalars map directly.
///
/// # Arguments
///
/// * `lua` - state owning new tables plus strings.
/// * `json` - value under conversion.
///
/// # Returns
///
/// Lua value holding the data.
///
/// # Errors
///
/// Fails with plan errors for non-finite numbers plus string failures.
pub(crate) fn json_to_lua(lua: &Lua, json: &Json) -> mlua::Result<Value> {
    match json {
        Json::Null => Ok(Value::Nil),
        Json::Bool(flag) => Ok(Value::Boolean(*flag)),
        Json::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(Value::Integer(integer))
            } else if let Some(float) = number.as_f64() {
                Ok(Value::Number(float))
            } else {
                Err(plan_err!("patch: value must be a finite number"))
            }
        }
        Json::String(text) => {
            let created = lua.create_string(text.as_str())?;
            Ok(Value::String(created))
        }
        Json::Array(items) => {
            let table = lua.create_table()?;
            for (position, item) in items.iter().enumerate() {
                let child = json_to_lua(lua, item)?;
                table.set((position + 1) as i64, child)?;
            }
            Ok(Value::Table(table))
        }
        Json::Object(map) => {
            let table = lua.create_table()?;
            for (key, item) in map {
                let child = json_to_lua(lua, item)?;
                table.set(key.as_str(), child)?;
            }
            Ok(Value::Table(table))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::Error;

    /// Builds a Lua state for wrapper tests.
    ///
    /// # Returns
    ///
    /// Lua state.
    fn setup() -> Lua {
        Lua::new()
    }

    /// Reports plan domain status for a Lua failure.
    ///
    /// # Arguments
    ///
    /// * `err` - Lua failure.
    ///
    /// # Returns
    ///
    /// True for plan domain failures.
    fn is_plan_error(err: &mlua::Error) -> bool {
        if let Some(domain) = err.downcast_ref::<Error>() {
            return matches!(domain, Error::Plan(_));
        }
        format!("{err}").contains("plan error:")
    }

    /// Runs ordered callbacks against one structured table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state.
    /// * `doc` - live table.
    /// * `owner` - patch owner.
    /// * `source` - callback source.
    ///
    /// # Returns
    ///
    /// Unit after execution.
    fn run_one(lua: &Lua, doc: &Table, owner: &str, source: &str) {
        let callback: Function = match lua.load(source).eval() {
            Ok(callback) => callback,
            Err(error) => panic!("callback loads: {error}"),
        };
        let patches = vec![ExecPatch {
            owner: owner.to_string(),
            priority: Level::Normal,
            callback,
        }];
        let area = Area::Structured {
            format: "json".to_string(),
        };
        match execute(lua, doc.clone(), area, BTreeMap::new(), patches) {
            Ok(()) => (),
            Err(error) => panic!("execute runs: {error}"),
        };
    }

    #[test]
    fn op_order_is_preserved() {
        let lua = setup();
        let doc = lua.create_table().expect("table");
        let callback: Function = lua
            .load(r#"return function(d) d:append("items", 1) d:append("items", 2) d:append("items", 3) end"#)
            .eval()
            .expect("callback");
        let patches = vec![ExecPatch {
            owner: "demo".to_string(),
            priority: Level::Normal,
            callback,
        }];
        let area = Area::Structured {
            format: "json".to_string(),
        };
        execute(&lua, doc.clone(), area, BTreeMap::new(), patches).expect("execute");
        let items: Table = doc.get("items").expect("items");
        assert_eq!(items.raw_len(), 3);
    }

    #[test]
    fn set_over_foreign_keeps_first_writer() {
        let lua = setup();
        let doc = lua.create_table().expect("table");
        run_one(
            &lua,
            &doc,
            "zzz",
            r#"return function(d) d:set("a.b", "base") end"#,
        );
        let owners: Rc<RefCell<OwnerMap>> = Rc::new(RefCell::new(BTreeMap::new()));
        owners
            .borrow_mut()
            .insert("a.b".to_string(), "zzz".to_string());
        let wrapper = Wrapper {
            doc: doc.clone(),
            owners,
            area: Area::Structured {
                format: "json".to_string(),
            },
            owner: "aaa".to_string(),
            priority: Level::Normal,
        };
        let handle = lua.create_userdata(wrapper).expect("wrapper");
        let callback: Function = lua
            .load(r#"return function(d) d:set("a.b", "challenger") end"#)
            .eval()
            .expect("callback");
        callback.call::<()>(handle).expect("call");
        let nested: Table = doc.get("a").expect("a");
        let value: String = nested.get("b").expect("b");
        assert_eq!(value, "base");
    }

    #[test]
    fn append_on_nonlist_errors_naming_path() {
        let lua = setup();
        let doc = lua.create_table().expect("table");
        run_one(
            &lua,
            &doc,
            "tool",
            r#"return function(d) d:set("user.theme", "catppuccin") end"#,
        );
        let callback: Function = lua
            .load(r#"return function(d) d:append("user.theme", "x") end"#)
            .eval()
            .expect("callback");
        let patches = vec![ExecPatch {
            owner: "tool".to_string(),
            priority: Level::Normal,
            callback,
        }];
        let area = Area::Structured {
            format: "json".to_string(),
        };
        let err = execute(&lua, doc, area, BTreeMap::new(), patches).expect_err("must fail");
        assert!(is_plan_error(&err), "plan domain: {err}");
        assert!(format!("{err}").contains("user.theme"), "names path: {err}");
    }

    #[test]
    fn missing_intermediate_errors_naming_path() {
        let lua = setup();
        let doc = lua.create_table().expect("table");
        run_one(
            &lua,
            &doc,
            "tool",
            r#"return function(d) d:set("a", 1) end"#,
        );
        let callback: Function = lua
            .load(r#"return function(d) d:set("a.b", 2) end"#)
            .eval()
            .expect("callback");
        let patches = vec![ExecPatch {
            owner: "tool".to_string(),
            priority: Level::Normal,
            callback,
        }];
        let area = Area::Structured {
            format: "json".to_string(),
        };
        let err = execute(&lua, doc, area, BTreeMap::new(), patches).expect_err("must fail");
        assert!(is_plan_error(&err), "plan domain: {err}");
        assert!(format!("{err}").contains("a.b"), "names path: {err}");
    }

    #[test]
    fn indexed_set_writes_single_element() {
        let lua = setup();
        let doc = lua.create_table().expect("table");
        run_one(
            &lua,
            &doc,
            "tool",
            r#"return function(d) d:append("items", 1) end"#,
        );
        run_one(
            &lua,
            &doc,
            "tool",
            r#"return function(d) d:set("items[0]", 9) end"#,
        );
        let items: Table = doc.get("items").expect("items");
        let first: i64 = items.get(1).expect("first");
        assert_eq!(first, 9);
    }

    #[test]
    fn proxy_mistakes_fail_as_plan_errors() {
        let lua = setup();
        for (field, source) in [
            ("path", r#"return function(d) d:set(42, "x") end"#),
            (
                "value",
                r#"return function(d) d:set("a", function() end) end"#,
            ),
        ] {
            let doc = lua.create_table().expect("table");
            let callback: Function = lua.load(source).eval().expect("callback");
            let patches = vec![ExecPatch {
                owner: "tool".to_string(),
                priority: Level::Normal,
                callback,
            }];
            let area = Area::Structured {
                format: "json".to_string(),
            };
            let err = execute(&lua, doc, area, BTreeMap::new(), patches).expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain: {source}: {err}");
            assert!(
                format!("{err}").contains(field),
                "names {field}: {source}: {err}"
            );
        }
    }
}
