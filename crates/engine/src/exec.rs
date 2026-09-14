//! Exec
//!
//! Live patch execution with first-writer wins.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::{Function, Lua, MultiValue, Table, UserData, Value};
use serde_json::Value as Json;

use crate::model::StoredPatch;
use crate::surface::document::entry_slot;
use crate::values::{
    Segment, flatten_json, json_to_lua, lua_to_json, parse_path, plan_error, read_marker,
};

/// Winner map from slot key to owner name.
pub(crate) type OwnerMap = BTreeMap<String, String>;

/// One patch callback with owner for live execution.
pub(crate) struct ExecPatch {
    /// Contributing config name.
    pub(crate) owner: String,
    /// Callback receiving the live wrapper.
    pub(crate) callback: Function,
}

/// Execution area selecting wrapper rules.
#[derive(Debug, Clone)]
pub(crate) enum Area {
    /// Rc sections with entry tables.
    Rc,
    /// Structured data with dotted paths.
    Structured {
        /// Format name for collision lines.
        format: String,
    },
}

/// Sorts patches by priority desc plus owner asc.
pub(crate) fn sort_patches(items: &mut [&StoredPatch]) {
    items.sort_by(|left, right| {
        right
            .priority
            .rank()
            .cmp(&left.priority.rank())
            .then_with(|| left.owner.cmp(&right.owner))
    });
}

/// Executes patch callbacks against one live document table.
pub(crate) fn execute(
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
        };
        let handle = lua.create_userdata(wrapper)?;
        patch.callback.call::<()>(handle)?;
    }
    Ok(())
}

/// Inserts one rc entry JSON with first-writer wins.
pub(crate) fn rc_insert(
    lua: &Lua,
    doc: &Table,
    json: &Json,
    section: &str,
    owner: &str,
    owners: &Rc<RefCell<OwnerMap>>,
) -> mlua::Result<()> {
    let slot = match entry_slot(json, section)? {
        None => {
            let lua_value = json_to_lua(lua, json)?;
            let list: Table = doc.get(section)?;
            let next = (list.raw_len() + 1) as i64;
            list.set(next, lua_value)?;
            return Ok(());
        }
        Some(slot) => slot,
    };
    let (name, key, label) = slot;
    {
        let mut owned = owners.borrow_mut();
        if let Some(winner) = owned.get(&key) {
            if winner != owner {
                log::warn!(
                    "collision on {label} \"{name}\": \"{owner}\" overwritten, \"{winner}\" wins"
                );
            }
            return Ok(());
        }
        owned.insert(key, owner.to_string());
    }
    let lua_value = json_to_lua(lua, json)?;
    let list: Table = doc.get(section)?;
    let next = (list.raw_len() + 1) as i64;
    list.set(next, lua_value)?;
    Ok(())
}

/// Live document wrapper handed to patch callbacks.
struct Wrapper {
    /// Live document table under mutation.
    doc: Table,
    /// Shared winners mutated in place.
    owners: Rc<RefCell<OwnerMap>>,
    /// Area selecting rules.
    area: Area,
    /// Current patch owner.
    owner: String,
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
    fn call_op(&self, lua: &Lua, append: bool, args: MultiValue) -> mlua::Result<()> {
        let collected: Vec<Value> = args.into_iter().collect();
        let (path_value, value_value) = match collected.as_slice() {
            [path, value] => (path.clone(), value.clone()),
            _ => {
                let op = if append { "append" } else { "set" };
                return Err(plan_error(format!("patch: '{op}' expects (path, value)")));
            }
        };
        let path = match path_value {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                let op = if append { "append" } else { "set" };
                return Err(plan_error(format!(
                    "patch: '{op}' field 'path' must be a string"
                )));
            }
        };
        match &self.area {
            Area::Rc => {
                if append {
                    self.rc_append(lua, &path, value_value)
                } else {
                    self.rc_set(lua, &path, value_value)
                }
            }
            Area::Structured { format } => {
                if append {
                    structured_append(
                        lua,
                        &self.doc,
                        &path,
                        value_value,
                        &self.owner,
                        &self.owners,
                    )
                } else {
                    structured_set(
                        lua,
                        &self.doc,
                        &path,
                        value_value,
                        &self.owner,
                        format,
                        &self.owners,
                    )
                }
            }
        }
    }

    /// Applies one rc set with first-writer wins.
    fn rc_set(&self, lua: &Lua, path: &str, value: Value) -> mlua::Result<()> {
        rc_op(lua, &self.doc, path, value, &self.owner, &self.owners)
    }

    /// Applies one rc append with first-writer wins.
    fn rc_append(&self, lua: &Lua, path: &str, value: Value) -> mlua::Result<()> {
        rc_op(lua, &self.doc, path, value, &self.owner, &self.owners)
    }
}

/// Applies one rc insert from a patch callback.
fn rc_op(
    lua: &Lua,
    doc: &Table,
    section: &str,
    value: Value,
    owner: &str,
    owners: &Rc<RefCell<OwnerMap>>,
) -> mlua::Result<()> {
    if !matches!(section, "profile" | "config" | "final") {
        return Err(plan_error(format!(
            "patch: unknown section '{section}' (expected 'profile', 'config', or 'final')"
        )));
    }
    let table = match value {
        Value::Table(table) => table,
        _ => {
            return Err(plan_error(
                "patch: field 'value' must be an rc entry table".to_string(),
            ));
        }
    };
    if read_marker(&table, "__kind").as_deref() != Some("rc-entry") {
        return Err(plan_error(
            "patch: field 'value' must be an rc entry table".to_string(),
        ));
    }
    let json = lua_to_json(Value::Table(table), "patch: field 'value'")?;
    rc_insert(lua, doc, &json, section, owner, owners)
}

/// Writes one structured leaf with first-writer wins.
#[allow(clippy::too_many_arguments)]
fn structured_set(
    lua: &Lua,
    doc: &Table,
    path: &str,
    value: Value,
    owner: &str,
    format: &str,
    owners: &Rc<RefCell<OwnerMap>>,
) -> mlua::Result<()> {
    let json = lua_to_json(value, &format!("config '{owner}': field '{path}'"))?;
    let segments = parse_path(path)?;
    let lua_value = json_to_lua(lua, &json)?;
    set_live_value(
        lua, doc, &segments, path, lua_value, &json, owner, format, owners,
    )
}

/// Extends one structured list.
fn structured_append(
    lua: &Lua,
    doc: &Table,
    path: &str,
    value: Value,
    owner: &str,
    owners: &Rc<RefCell<OwnerMap>>,
) -> mlua::Result<()> {
    let json = lua_to_json(value, &format!("config '{owner}': field '{path}'"))?;
    let segments = parse_path(path)?;
    let lua_value = json_to_lua(lua, &json)?;
    append_live_value(lua, doc, &segments, path, lua_value, &json, owner, owners)
}

/// Descends into one intermediate segment over live tables.
fn live_child(current: &Value, lua: &Lua, segment: &Segment, full: &str) -> mlua::Result<Table> {
    let map = match current {
        Value::Table(map) => map.clone(),
        _ => {
            return Err(plan_error(format!(
                "cannot write '{full}': '{full}' is blocked"
            )));
        }
    };
    match segment.index {
        None => match map.get::<Value>(segment.key.as_str())? {
            Value::Nil => {
                let child = lua.create_table()?;
                map.set(segment.key.as_str(), child.clone())?;
                Ok(child)
            }
            Value::Table(child) => Ok(child),
            _ => Err(plan_error(format!(
                "cannot write '{full}': '{full}' is blocked"
            ))),
        },
        Some(index) => {
            let list = match map.get::<Value>(segment.key.as_str())? {
                Value::Nil => {
                    let fresh = lua.create_table()?;
                    map.set(segment.key.as_str(), fresh.clone())?;
                    fresh
                }
                Value::Table(existing) => existing,
                _ => {
                    return Err(plan_error(format!(
                        "cannot write '{full}': '{full}' is blocked"
                    )));
                }
            };
            child_at_index(lua, &list, index, full)
        }
    }
}

/// Reads or creates the child table at one list index.
fn child_at_index(lua: &Lua, list: &Table, index: usize, full: &str) -> mlua::Result<Table> {
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
    match list.get::<Value>(lua_index)? {
        Value::Table(child) => Ok(child),
        Value::Nil => {
            let child = lua.create_table()?;
            list.set(lua_index, child.clone())?;
            Ok(child)
        }
        _ => Err(plan_error(format!(
            "cannot write '{full}': '{full}' is blocked"
        ))),
    }
}

/// Navigates to the parent table for a segment prefix.
fn live_parent(lua: &Lua, doc: &Table, prefix: &[Segment], full: &str) -> mlua::Result<Table> {
    let mut current = Value::Table(doc.clone());
    for segment in prefix {
        let child = live_child(&current, lua, segment, full)?;
        current = Value::Table(child);
    }
    match current {
        Value::Table(parent) => Ok(parent),
        _ => Err(plan_error(format!(
            "cannot write '{full}': '{full}' is blocked"
        ))),
    }
}

/// Reports list status for a Lua value.
fn is_live_list(value: &Value) -> bool {
    match value {
        Value::Nil => true,
        Value::Table(table) => {
            if table_is_empty(table) {
                return true;
            }
            let len = table.raw_len();
            for index in 1..=(len as i64) {
                if table.get::<Value>(index).is_err() {
                    return false;
                }
            }
            for pair in table.pairs::<Value, Value>() {
                match pair {
                    Ok((Value::Integer(_), _)) => {}
                    _ => return false,
                }
            }
            true
        }
        _ => false,
    }
}

/// Reports emptiness for a Lua table.
fn table_is_empty(table: &Table) -> bool {
    for pair in table.pairs::<Value, Value>() {
        if pair.is_ok() {
            return false;
        }
    }
    true
}

/// Writes one leaf into a live table with first-writer wins.
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
        None => return Err(plan_error(format!("invalid path '{full}': empty path"))),
    };
    let parent = live_parent(lua, doc, prefix, full)?;
    {
        let owned = owners.borrow();
        if let Some(winner) = owned.get(full)
            && winner != owner
        {
            log::warn!(
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
            let list = match parent.get::<Value>(last.key.as_str())? {
                Value::Nil => {
                    let fresh = lua.create_table()?;
                    parent.set(last.key.as_str(), fresh.clone())?;
                    fresh
                }
                Value::Table(existing) => existing,
                _ => {
                    return Err(plan_error(format!(
                        "cannot write '{full}': '{full}' is blocked"
                    )));
                }
            };
            let lua_index = (index + 1) as i64;
            if lua_index > list.raw_len() as i64 + 1 {
                return Err(plan_error(format!(
                    "cannot write '{full}': index out of bounds"
                )));
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
        None => return Err(plan_error(format!("invalid path '{full}': empty path"))),
    };
    let parent = live_parent(lua, doc, prefix, full)?;
    match last.index {
        None => {
            let list = match parent.get::<Value>(last.key.as_str())? {
                Value::Nil => {
                    let fresh = lua.create_table()?;
                    parent.set(last.key.as_str(), fresh.clone())?;
                    fresh
                }
                Value::Table(existing) => {
                    if !is_live_list(&Value::Table(existing.clone())) {
                        return Err(non_list_error(full));
                    }
                    existing
                }
                _ => return Err(non_list_error(full)),
            };
            let next = (list.raw_len() + 1) as i64;
            list.set(next, lua_value)?;
            let mut owned = owners.borrow_mut();
            let mut leaves = BTreeMap::new();
            flatten_json(json, &format!("{full}[{}]", next - 1), &mut leaves);
            for leaf in leaves.keys() {
                owned.insert(leaf.clone(), owner.to_string());
            }
            Ok(())
        }
        Some(index) => {
            let list = match parent.get::<Value>(last.key.as_str())? {
                Value::Nil => {
                    let fresh = lua.create_table()?;
                    parent.set(last.key.as_str(), fresh.clone())?;
                    fresh
                }
                Value::Table(existing) => existing,
                _ => return Err(non_list_error(full)),
            };
            let lua_index = (index + 1) as i64;
            if lua_index > list.raw_len() as i64 + 1 {
                return Err(plan_error(format!(
                    "cannot append '{full}': index out of bounds"
                )));
            }
            if lua_index == list.raw_len() as i64 + 1 {
                let inner = lua.create_table()?;
                inner.set(1_i64, lua_value)?;
                list.set(lua_index, inner)?;
                return Ok(());
            }
            match list.get::<Value>(lua_index)? {
                Value::Table(inner) => {
                    if !is_live_list(&Value::Table(inner.clone())) {
                        return Err(non_list_error(full));
                    }
                    let next = (inner.raw_len() + 1) as i64;
                    inner.set(next, lua_value)?;
                    Ok(())
                }
                Value::Nil => {
                    let inner = lua.create_table()?;
                    inner.set(1_i64, lua_value)?;
                    list.set(lua_index, inner)?;
                    Ok(())
                }
                _ => Err(non_list_error(full)),
            }
        }
    }
}

/// Builds the non-list append error for one path.
fn non_list_error(full: &str) -> mlua::Error {
    plan_error(format!(
        "cannot append '{full}': '{full}' holds a non-list leaf"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;

    fn state() -> Lua {
        Lua::new()
    }

    fn patch(lua: &Lua, owner: &str, priority: Level, target: &str) -> StoredPatch {
        let callback: Function = match lua.load("return function(_) end").eval() {
            Ok(callback) => callback,
            Err(error) => panic!("callback loads: {error}"),
        };
        StoredPatch {
            target: target.to_string(),
            format: None,
            callback,
            priority,
            owner: owner.to_string(),
        }
    }

    #[test]
    fn sort_orders_priority_desc_plus_owner_asc() {
        let lua = state();
        let low = patch(&lua, "zebra", Level::Low, "rc");
        let major = patch(&lua, "zebra", Level::Major, "rc");
        let normal_beta = patch(&lua, "beta", Level::Normal, "rc");
        let normal_alpha = patch(&lua, "alpha", Level::Normal, "rc");
        let mut items: Vec<&StoredPatch> = vec![&low, &normal_beta, &major, &normal_alpha];
        sort_patches(&mut items);
        let owners = items
            .iter()
            .map(|item| item.owner.as_str())
            .collect::<Vec<_>>();
        assert_eq!(owners, vec!["zebra", "alpha", "beta", "zebra"]);
        assert_eq!(items[0].priority, Level::Major);
        assert_eq!(items[3].priority, Level::Low);
    }
}
