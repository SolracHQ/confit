//! Exec
//!
//! Live patch execution with first-writer wins.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::{Function, Lua, MultiValue, Table, UserData, Value};
use serde_json::Value as Json;

use crate::error::plan_error;
use crate::level::Level;
use crate::lua::{JsonExt, ValueExt, read_marker};
use crate::model::StoredPatch;
use crate::path_expr::{Segment, flatten_json, parse_path};
use crate::surface::document::convert::{entry_slot, translate_entry};
use confit_model::progress::{Event, ProgressSender};

/// Winner map from slot key to owner name.
pub(crate) type OwnerMap = BTreeMap<String, String>;

/// One patch callback with owner for live execution.
pub(crate) struct ExecPatch {
    /// Contributing config name.
    pub(crate) owner: String,
    /// Target document path or rc.
    pub(crate) target: String,
    /// Merge priority for ordering.
    pub(crate) priority: Level,
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

/// Executor running patch callbacks against live tables.
///
/// The Lua state and the progress sender travel together, so execution
/// methods read them from self.
///
pub(crate) struct Executor<'a> {
    /// Lua state carrying the live tables.
    pub(crate) lua: &'a Lua,
    /// Progress sender holding `None` for silence.
    pub(crate) progress: Option<ProgressSender>,
    /// Total patches under the run.
    pub(crate) patch_total: usize,
    /// Finished patch count shared across documents.
    pub(crate) patch_done: &'a Cell<usize>,
}

impl Executor<'_> {
    /// Sorts patches by priority desc and declaration order asc.
    ///
    /// # Arguments
    ///
    /// * `items` - patch handles in declaration order.
    ///
    /// Sorts in place into execution order.
    ///
    pub(crate) fn sort_patches(items: &mut [&StoredPatch]) {
        items.sort_by(|left, right| {
            right
                .priority
                .rank()
                .cmp(&left.priority.rank())
                .then_with(|| left.order.cmp(&right.order))
        });
    }

    /// Executes patch callbacks against one live document table.
    ///
    /// # Arguments
    ///
    /// * `doc` - live document table under mutation.
    /// * `area` - wrapper rules selecting rc or structured writes.
    /// * `seeds` - leaf owners from the declared base.
    /// * `patches` - callbacks in execution order.
    ///
    /// # Returns
    ///
    /// Unit after each callback runs and progress facts emit.
    ///
    /// # Errors
    ///
    /// Callback failures fail as Lua errors. Bad wrapper calls fail as plan errors.
    ///
    pub(crate) fn execute(
        &self,
        doc: Table,
        area: Area,
        seeds: OwnerMap,
        patches: Vec<ExecPatch>,
    ) -> mlua::Result<()> {
        let owners: Rc<RefCell<OwnerMap>> = Rc::new(RefCell::new(seeds));
        for patch in patches {
            let live = LiveDoc {
                doc: doc.clone(),
                owners: Rc::clone(&owners),
                owner: patch.owner.clone(),
            };
            let handle = match &area {
                Area::Rc => self.lua.create_userdata(RcPatch {
                    live,
                    ctx: "confit.patch.rc".to_string(),
                })?,
                Area::Structured { format } => {
                    let ctx = format!("confit.patch.structured('{}')", patch.target);
                    self.lua.create_userdata(StructuredPatch {
                        live,
                        format: format.clone(),
                        ctx,
                    })?
                }
            };
            patch.callback.call::<()>(handle)?;
            log::debug!(
                "patch applied owner={} target={} priority={:?}",
                patch.owner,
                patch.target,
                patch.priority
            );
            let done = self.patch_done.get() + 1;
            self.patch_done.set(done);
            if let Some(sender) = self.progress.as_ref() {
                let _ = sender.send(Event::PatchApplied {
                    owner: patch.owner.clone(),
                    target: patch.target.clone(),
                    done,
                    total: self.patch_total,
                });
            }
        }
        Ok(())
    }

    /// Inserts one rc entry JSON with first-writer wins.
    ///
    /// # Arguments
    ///
    /// * `doc` - live rc document holding section lists.
    /// * `json` - entry JSON under inserting.
    /// * `section` - section name holding the list.
    /// * `owner` - contributing config name.
    /// * `owners` - shared winners mutated in place.
    ///
    /// # Returns
    ///
    /// Unit after the entry lands or yields to the recorded winner.
    ///
    /// # Errors
    ///
    /// Entry slot failures fail as plan errors. Table shape failures fail as Lua errors.
    ///
    pub(crate) fn rc_insert(
        &self,
        doc: &Table,
        json: &Json,
        section: &str,
        owner: &str,
        owners: &Rc<RefCell<OwnerMap>>,
        ctx: &str,
    ) -> mlua::Result<()> {
        let slot = match entry_slot(json, section, ctx)? {
            None => {
                let lua_value = json.to_lua(self.lua, ctx)?;
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
        let lua_value = json.to_lua(self.lua, ctx)?;
        let list: Table = doc.get(section)?;
        let next = (list.raw_len() + 1) as i64;
        list.set(next, lua_value)?;
        Ok(())
    }
}

/// Live table view pairing one table with its Lua state.
///
/// The state and the table travel together, so navigation methods
/// read them from self.
///
pub(crate) struct LiveTable<'a> {
    lua: &'a Lua,
    table: Table,
    ctx: String,
}

impl<'a> LiveTable<'a> {
    /// Builds a live view over one table.
    ///
    /// # Arguments
    ///
    /// * `lua` - state creating child tables.
    /// * `table` - live table under navigation.
    /// * `ctx` - error prefix naming the patch caller.
    ///
    /// # Returns
    ///
    /// View borrowing the state and owning the table handle.
    ///
    fn new(lua: &'a Lua, table: Table, ctx: &str) -> Self {
        Self {
            lua,
            table,
            ctx: ctx.to_string(),
        }
    }

    /// Descends into one intermediate segment over live tables.
    ///
    /// # Arguments
    ///
    /// * `current` - table value holding the segment.
    /// * `segment` - key plus optional list index.
    /// * `full` - dotted path naming the write.
    ///
    /// # Returns
    ///
    /// Child table for the segment, created when missing.
    ///
    /// # Errors
    ///
    /// Blocked shapes fail as plan errors. Index gaps fill with fresh tables.
    ///
    fn live_child(&self, current: &Value, segment: &Segment, full: &str) -> mlua::Result<Table> {
        let map = match current {
            Value::Table(map) => map.clone(),
            _ => {
                return Err(plan_error(format!(
                    "{}: cannot write '{full}': '{full}' is blocked",
                    self.ctx
                )));
            }
        };
        match segment.index {
            None => match map.get::<Value>(segment.key.as_str())? {
                Value::Nil => {
                    let child = self.lua.create_table()?;
                    map.set(segment.key.as_str(), child.clone())?;
                    Ok(child)
                }
                Value::Table(child) => Ok(child),
                _ => Err(plan_error(format!(
                    "{}: cannot write '{full}': '{full}' is blocked",
                    self.ctx
                ))),
            },
            Some(index) => {
                let list = match map.get::<Value>(segment.key.as_str())? {
                    Value::Nil => {
                        let fresh = self.lua.create_table()?;
                        map.set(segment.key.as_str(), fresh.clone())?;
                        fresh
                    }
                    Value::Table(existing) => existing,
                    _ => {
                        return Err(plan_error(format!(
                            "{}: cannot write '{full}': '{full}' is blocked",
                            self.ctx
                        )));
                    }
                };
                self.child_at_index(&list, index, full)
            }
        }
    }

    /// Reads or creates the child table at one list index.
    ///
    /// # Arguments
    ///
    /// * `list` - parent list table.
    /// * `index` - zero based position.
    /// * `full` - dotted path naming the write.
    ///
    /// # Returns
    ///
    /// Child table at the index, created when missing.
    ///
    /// # Errors
    ///
    /// Blocked leaves fail as plan errors.
    ///
    fn child_at_index(&self, list: &Table, index: usize, full: &str) -> mlua::Result<Table> {
        let lua_index = (index + 1) as i64;
        let current_len = list.raw_len() as i64;
        if lua_index > current_len + 1 {
            while (list.raw_len() as i64) < lua_index - 1 {
                let filler = self.lua.create_table()?;
                let next = (list.raw_len() + 1) as i64;
                list.set(next, filler)?;
            }
            let child = self.lua.create_table()?;
            list.set(lua_index, child.clone())?;
            return Ok(child);
        }
        if lua_index == current_len + 1 {
            let child = self.lua.create_table()?;
            list.set(lua_index, child.clone())?;
            return Ok(child);
        }
        match list.get::<Value>(lua_index)? {
            Value::Table(child) => Ok(child),
            Value::Nil => {
                let child = self.lua.create_table()?;
                list.set(lua_index, child.clone())?;
                Ok(child)
            }
            _ => Err(plan_error(format!(
                "{}: cannot write '{full}': '{full}' is blocked",
                self.ctx
            ))),
        }
    }

    /// Navigates to the parent table for a segment prefix.
    ///
    /// # Arguments
    ///
    /// * `prefix` - leading segments above the leaf.
    /// * `full` - dotted path naming the write.
    ///
    /// # Returns
    ///
    /// Parent table holding the leaf slot.
    ///
    /// # Errors
    ///
    /// Blocked shapes fail as plan errors.
    ///
    fn live_parent(&self, prefix: &[Segment], full: &str) -> mlua::Result<Table> {
        let mut current = Value::Table(self.table.clone());
        for segment in prefix {
            let child = self.live_child(&current, segment, full)?;
            current = Value::Table(child);
        }
        match current {
            Value::Table(parent) => Ok(parent),
            _ => Err(plan_error(format!(
                "{}: cannot write '{full}': '{full}' is blocked",
                self.ctx
            ))),
        }
    }

    /// Reports list status for a Lua value.
    ///
    /// # Arguments
    ///
    /// * `value` - value under checking.
    ///
    /// # Returns
    ///
    /// True for nil, empty, and dense integer keyed tables.
    ///
    fn is_live_list(&self, value: &Value) -> bool {
        match value {
            Value::Nil => true,
            Value::Table(table) => {
                if Self::new(self.lua, table.clone(), &self.ctx).table_is_empty() {
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

    /// Reports emptiness for the live table.
    ///
    /// # Returns
    ///
    /// True while iteration yields zero pairs.
    ///
    fn table_is_empty(&self) -> bool {
        for pair in self.table.pairs::<Value, Value>() {
            if pair.is_ok() {
                return false;
            }
        }
        true
    }

    /// Writes one leaf into the live table with first-writer wins.
    ///
    /// # Arguments
    ///
    /// * `patch` - structured handle holding owner, format, plus shared winners.
    /// * `segments` - parsed path with the leaf last.
    /// * `full` - dotted path naming the write.
    /// * `lua_value` - converted value under writing.
    /// * `json` - data value seeding leaf owners.
    ///
    /// # Returns
    ///
    /// Unit after the leaf lands or yields to the recorded winner.
    ///
    /// # Errors
    ///
    /// Empty paths fail as plan errors. Blocked shapes fail as plan errors.
    /// Out of bounds indexes fail as plan errors.
    ///
    fn set_live_value(
        &self,
        patch: &StructuredPatch,
        segments: &[Segment],
        full: &str,
        lua_value: Value,
        json: &Json,
    ) -> mlua::Result<()> {
        let (last, prefix) = match segments.split_last() {
            Some(pair) => pair,
            None => {
                return Err(plan_error(format!(
                    "{}: invalid path '{full}': empty path",
                    self.ctx
                )));
            }
        };
        let parent = self.live_parent(prefix, full)?;
        {
            let owned = patch.live.owners.borrow();
            if let Some(winner) = owned.get(full)
                && winner != &patch.live.owner
            {
                log::warn!(
                    "collision on {} \"{full}\": \"{}\" overwritten, \"{winner}\" wins",
                    patch.format,
                    patch.live.owner
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
                        let fresh = self.lua.create_table()?;
                        parent.set(last.key.as_str(), fresh.clone())?;
                        fresh
                    }
                    Value::Table(existing) => existing,
                    _ => {
                        return Err(plan_error(format!(
                            "{}: cannot write '{full}': '{full}' is blocked",
                            self.ctx
                        )));
                    }
                };
                let lua_index = (index + 1) as i64;
                if lua_index > list.raw_len() as i64 + 1 {
                    return Err(plan_error(format!(
                        "{}: cannot write '{full}': index out of bounds",
                        self.ctx
                    )));
                }
                list.set(lua_index, lua_value)?;
            }
        }
        {
            let mut owned = patch.live.owners.borrow_mut();
            let dotted = format!("{full}.");
            let indexed = format!("{full}[");
            owned.retain(|key, _| {
                key != full && !key.starts_with(&dotted) && !key.starts_with(&indexed)
            });
            let mut leaves = BTreeMap::new();
            flatten_json(json, full, &mut leaves);
            for leaf in leaves.keys() {
                owned.insert(leaf.clone(), patch.live.owner.clone());
            }
        }
        Ok(())
    }

    /// Appends one value to a live list.
    ///
    /// # Arguments
    ///
    /// * `live` - document state holding owners plus the patch owner.
    /// * `segments` - parsed path with the list last.
    /// * `full` - dotted path naming the write.
    /// * `lua_value` - converted value under appending.
    /// * `json` - data value seeding leaf owners.
    ///
    /// # Returns
    ///
    /// Unit after the value lands at the list tail.
    ///
    /// # Errors
    ///
    /// Empty paths fail as plan errors. Non-list leaves fail as plan errors.
    /// Out of bounds indexes fail as plan errors.
    ///
    fn append_live_value(
        &self,
        live: &LiveDoc,
        segments: &[Segment],
        full: &str,
        lua_value: Value,
        json: &Json,
    ) -> mlua::Result<()> {
        let (last, prefix) = match segments.split_last() {
            Some(pair) => pair,
            None => {
                return Err(plan_error(format!(
                    "{}: invalid path '{full}': empty path",
                    self.ctx
                )));
            }
        };
        let parent = self.live_parent(prefix, full)?;
        match last.index {
            None => {
                let list = match parent.get::<Value>(last.key.as_str())? {
                    Value::Nil => {
                        let fresh = self.lua.create_table()?;
                        parent.set(last.key.as_str(), fresh.clone())?;
                        fresh
                    }
                    Value::Table(existing) => {
                        if !self.is_live_list(&Value::Table(existing.clone())) {
                            return Err(self.non_list_error(full));
                        }
                        existing
                    }
                    _ => return Err(self.non_list_error(full)),
                };
                let next = (list.raw_len() + 1) as i64;
                list.set(next, lua_value)?;
                let mut owned = live.owners.borrow_mut();
                let mut leaves = BTreeMap::new();
                flatten_json(json, &format!("{full}[{}]", next), &mut leaves);
                for leaf in leaves.keys() {
                    owned.insert(leaf.clone(), live.owner.clone());
                }
                Ok(())
            }
            Some(index) => {
                let list = match parent.get::<Value>(last.key.as_str())? {
                    Value::Nil => {
                        let fresh = self.lua.create_table()?;
                        parent.set(last.key.as_str(), fresh.clone())?;
                        fresh
                    }
                    Value::Table(existing) => existing,
                    _ => return Err(self.non_list_error(full)),
                };
                let lua_index = (index + 1) as i64;
                if lua_index > list.raw_len() as i64 + 1 {
                    return Err(plan_error(format!(
                        "{}: cannot append '{full}': index out of bounds",
                        self.ctx
                    )));
                }
                if lua_index == list.raw_len() as i64 + 1 {
                    let inner = self.lua.create_table()?;
                    inner.set(1_i64, lua_value)?;
                    list.set(lua_index, inner)?;
                    return Ok(());
                }
                match list.get::<Value>(lua_index)? {
                    Value::Table(inner) => {
                        if !self.is_live_list(&Value::Table(inner.clone())) {
                            return Err(self.non_list_error(full));
                        }
                        let next = (inner.raw_len() + 1) as i64;
                        inner.set(next, lua_value)?;
                        Ok(())
                    }
                    Value::Nil => {
                        let inner = self.lua.create_table()?;
                        inner.set(1_i64, lua_value)?;
                        list.set(lua_index, inner)?;
                        Ok(())
                    }
                    _ => Err(self.non_list_error(full)),
                }
            }
        }
    }

    /// Builds the non-list append error for one path.
    ///
    /// # Arguments
    ///
    /// * `full` - dotted path naming the write.
    ///
    /// # Returns
    ///
    /// Plan error naming the path and the non-list leaf.
    ///
    fn non_list_error(&self, full: &str) -> mlua::Error {
        plan_error(format!(
            "{}: cannot append '{full}': '{full}' holds a non-list leaf",
            self.ctx
        ))
    }
}

/// Live document state shared by patch callback handles.
struct LiveDoc {
    /// Live document table under mutation.
    doc: Table,
    /// Shared winners mutated in place.
    owners: Rc<RefCell<OwnerMap>>,
    /// Current patch owner.
    owner: String,
}

/// Rc patch handle handed to `confit.patch.rc` callbacks.
///
/// Exposes `add` only. Structured verbs do not exist here.
struct RcPatch {
    /// Live document state under mutation.
    live: LiveDoc,
    /// Caller prefix naming the patch constructor.
    ctx: String,
}

impl UserData for RcPatch {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("add", |lua, this, args: MultiValue| {
            this.call_add(lua, args)
        });
    }
}

impl RcPatch {
    /// Runs one rc `add` call.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the live table.
    /// * `args` - section and entry values.
    ///
    /// # Returns
    ///
    /// Unit after the entry lands or yields to the recorded winner.
    ///
    /// # Errors
    ///
    /// Wrong arity fails as a plan error. Bad section or entry fails as a plan error.
    ///
    fn call_add(&self, lua: &Lua, args: MultiValue) -> mlua::Result<()> {
        let collected: Vec<Value> = args.into_iter().collect();
        let (section_value, value_value) = match collected.as_slice() {
            [section, value] => (section.clone(), value.clone()),
            _ => {
                return Err(plan_error(format!(
                    "{}: 'add' expects (section, entry)",
                    self.ctx
                )));
            }
        };
        let section = section_value.req_str(&self.ctx, "section")?;
        self.rc_op(lua, &section, value_value)
    }

    /// Applies one rc insert from a patch callback.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the live table.
    /// * `section` - section name holding the list.
    /// * `value` - entry value under inserting.
    ///
    /// # Returns
    ///
    /// Unit after the entry lands or yields to the recorded winner.
    ///
    /// # Errors
    ///
    /// Unknown sections fail as plan errors. Non-entry tables fail as plan errors.
    ///
    fn rc_op(&self, lua: &Lua, section: &str, value: Value) -> mlua::Result<()> {
        let ctx = &self.ctx;
        if !matches!(section, "profile" | "config" | "final") {
            return Err(plan_error(format!(
                "{ctx}: unknown section '{section}' (expected 'profile', 'config', or 'final')"
            )));
        }
        let table = match value {
            Value::Table(table) => table,
            _ => {
                return Err(plan_error(format!(
                    "{ctx}: field 'value' must be an rc entry table"
                )));
            }
        };
        if read_marker(&table, "__kind").as_deref() != Some("rc-entry") {
            return Err(plan_error(format!(
                "{ctx}: field 'value' must be an rc entry table"
            )));
        }
        let json = translate_entry(&table, &format!("{ctx}: field 'value'"))?;
        Executor {
            lua,
            progress: None,
            patch_total: 0,
            patch_done: &Cell::new(0),
        }
        .rc_insert(
            &self.live.doc,
            &json,
            section,
            &self.live.owner,
            &self.live.owners,
            ctx,
        )
    }
}

/// Structured patch handle handed to `confit.patch.structured` callbacks.
///
/// Exposes `set` and `append` only. The rc verb does not exist here.
struct StructuredPatch {
    /// Live document state under mutation.
    live: LiveDoc,
    /// Format name for collision lines.
    format: String,
    /// Caller prefix naming the patch constructor.
    ctx: String,
}

impl UserData for StructuredPatch {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("set", |lua, this, args: MultiValue| {
            let (path, value) = this.structured_args("set", args)?;
            this.structured_set(lua, &path, value)
        });
        methods.add_method("append", |lua, this, args: MultiValue| {
            let (path, value) = this.structured_args("append", args)?;
            this.structured_append(lua, &path, value)
        });
    }
}

impl StructuredPatch {
    /// Reads one `(path, value)` pair for a structured op.
    ///
    /// # Errors
    ///
    /// Wrong arity fails as a plan error. Bad path fails as a plan error.
    ///
    fn structured_args(&self, op: &str, args: MultiValue) -> mlua::Result<(String, Value)> {
        let collected: Vec<Value> = args.into_iter().collect();
        let (path_value, value_value) = match collected.as_slice() {
            [path, value] => (path.clone(), value.clone()),
            _ => {
                return Err(plan_error(format!(
                    "{}: '{op}' expects (path, value)",
                    self.ctx
                )));
            }
        };
        let path = path_value.req_str(&self.ctx, "path")?;
        Ok((path, value_value))
    }

    /// Writes one structured leaf with first-writer wins.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the live table.
    /// * `path` - dotted path naming the write.
    /// * `value` - value under writing.
    ///
    /// # Returns
    ///
    /// Unit after the leaf lands or yields to the recorded winner.
    ///
    /// # Errors
    ///
    /// Bad paths and values fail as plan errors. Blocked shapes fail as plan errors.
    ///
    fn structured_set(&self, lua: &Lua, path: &str, value: Value) -> mlua::Result<()> {
        let ctx = format!("{}: field '{path}'", self.ctx);
        let json = value.to_json(&ctx)?;
        let segments = parse_path(path, &ctx)?;
        let lua_value = json.to_lua(lua, &ctx)?;
        LiveTable::new(lua, self.live.doc.clone(), &ctx)
            .set_live_value(self, &segments, path, lua_value, &json)
    }

    /// Extends one structured list.
    ///
    /// # Arguments
    ///
    /// * `lua` - state owning the live table.
    /// * `path` - dotted path naming the list.
    /// * `value` - value under appending.
    ///
    /// # Returns
    ///
    /// Unit after the value lands at the list tail.
    ///
    /// # Errors
    ///
    /// Bad paths and values fail as plan errors. Non-list leaves fail as plan errors.
    ///
    fn structured_append(&self, lua: &Lua, path: &str, value: Value) -> mlua::Result<()> {
        let ctx = format!("{}: field '{path}'", self.ctx);
        let json = value.to_json(&ctx)?;
        let segments = parse_path(path, &ctx)?;
        let lua_value = json.to_lua(lua, &ctx)?;
        LiveTable::new(lua, self.live.doc.clone(), &ctx)
            .append_live_value(&self.live, &segments, path, lua_value, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;

    fn state() -> Lua {
        Lua::new()
    }

    fn patch(lua: &Lua, owner: &str, priority: Level, target: &str, order: usize) -> StoredPatch {
        let callback: Function = match lua.load("return function(_) end").eval() {
            Ok(callback) => callback,
            Err(error) => panic!("callback loads: {error}"),
        };
        StoredPatch {
            target: target.to_string(),
            format: None,
            callback,
            priority,
            order,
            owner: owner.to_string(),
        }
    }

    #[test]
    fn sort_orders_priority_desc_plus_declaration_order() {
        let lua = state();
        let low = patch(&lua, "aaa", Level::Low, "rc", 3);
        let major = patch(&lua, "zebra", Level::Major, "rc", 2);
        let normal_beta = patch(&lua, "beta", Level::Normal, "rc", 0);
        let normal_alpha = patch(&lua, "alpha", Level::Normal, "rc", 1);
        let mut items: Vec<&StoredPatch> = vec![&low, &normal_alpha, &major, &normal_beta];
        Executor::sort_patches(&mut items);
        let owners = items
            .iter()
            .map(|item| item.owner.as_str())
            .collect::<Vec<_>>();
        assert_eq!(owners, vec!["zebra", "beta", "alpha", "aaa"]);
        assert_eq!(items[0].priority, Level::Major);
        assert_eq!(items[3].priority, Level::Low);
    }
}
