//! Binding
//!
//! This module runs user configuration and translates its failures. Executing Lua
//! stays apart from both the stdlib surface and the planning logic.

use std::collections::BTreeMap;
use std::path::Path;

use mlua::{Table, Value};

use crate::error::{Error, Result};
use crate::framework::apply::{Area, ExecPatch, OwnerMap};
use crate::framework::{ConfigBuilder, apply};
use crate::model::state::config::ConfigContribution;
use crate::model::state::document::{Document, DocumentData, DocumentKind, StructuredFormat};

/// Evaluated profile graph: declared shells plus documents plus config contributions.
///
/// `shells` plus `configs` stay non-empty and keep profile order. `documents`
/// hold profile-declared documents in profile order. `merged` holds final
/// structured plus rc documents from live execution. Slice 2 handles conversion.
///
/// # Examples
///
/// A profile returning `{ shells = { "bash" }, configs = { web } }` evaluates to one shell and
/// the `web` contribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileGraph {
    /// Declared shell names, e.g. `["bash"]`.
    pub shells: Vec<String>,
    /// Profile-declared documents in profile order.
    pub documents: Vec<Document>,
    /// Config contributions in profile order.
    pub configs: Vec<ConfigContribution>,
    /// Final structured plus rc documents from live execution.
    pub merged: Vec<Document>,
}

/// Evaluates the profile file into a profile graph.
///
/// # Arguments
///
/// * `root` - project root for module resolution.
/// * `profile` - profile file path.
///
/// # Returns
///
/// Profile graph holding shells plus declared documents plus config contributions.
///
/// # Errors
///
/// Fails with `Error::Lua` for evaluation failures and `Error::Config` for malformed return tables.
///
/// # Examples
/// ```rust,no_run
/// use std::path::Path;
/// use confit::binding::evaluate;
///
/// let graph = match evaluate(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua"), None) {
///     Ok(graph) => graph,
///     Err(error) => panic!("fixture evaluates: {error}"),
/// };
/// assert_eq!(graph.shells.len(), 1);
/// ```
/// Evaluates the profile file into a profile graph with plugins available.
///
/// # Arguments
///
/// * `root` - project root for module resolution.
/// * `profile` - profile file path.
/// * `plugins` - external plugins folder shaped `{user}/{name}/plugin.lua`,
///   empty keeps embedded defaults only.
///
/// # Returns
///
/// Profile graph holding shells plus declared documents plus final merged data.
///
/// # Errors
///
/// Fails with `Error::Lua` for evaluation failures and `Error::Config` for malformed return tables.
pub fn evaluate(root: &Path, profile: &Path, plugins: Option<&Path>) -> Result<ProfileGraph> {
    let lua = mlua::Lua::new();
    prepend_project_path(&lua, root)?;
    crate::framework::install_confit(&lua, root, plugins)?;
    let source = std::fs::read(profile)?;
    let returned: Value = lua
        .load(&source)
        .set_name(format!("@{}", profile.display()))
        .call(())
        .map_err(wrap_mlua)?;
    let table = match returned {
        Value::Table(table) => table,
        _ => {
            return Err(Error::Config(
                "profile must return a table with 'shells' and 'configs'".to_string(),
            ));
        }
    };
    let shells = read_shells(&table)?;
    let documents = read_documents(&table)?;
    let builders = read_builders(&table)?;
    let mut configs = Vec::with_capacity(builders.len());
    let mut handles: Vec<StoredHandle> = Vec::new();
    for builder in &builders {
        configs.push(builder.contribution().clone());
        for stored in builder.handles() {
            handles.push(StoredHandle {
                target: stored.handle.target.clone(),
                format: stored.handle.format,
                callback: stored.handle.callback.clone(),
                priority: stored.handle.priority,
                owner: stored.owner.clone(),
            });
        }
    }
    let mut graph = ProfileGraph {
        shells,
        documents,
        configs,
        merged: Vec::new(),
    };
    check_declared_documents(&graph.documents, &graph.configs)?;
    let merged = execute_live(&lua, &graph, &handles).map_err(wrap_mlua)?;
    graph.merged = merged;
    Ok(graph)
}

/// Stored patch handle for live execution.
///
/// Holds target plus format plus callback plus priority plus owner.
#[derive(Debug, Clone)]
struct StoredHandle {
    /// Holds the target document key.
    target: String,
    /// Holds the structured format, empty for rc.
    format: Option<StructuredFormat>,
    /// Holds the callback receiving the wrapper.
    callback: mlua::Function,
    /// Holds the merge priority.
    priority: crate::model::state::level::Level,
    /// Holds the contributing config name.
    owner: String,
}

/// Maps an mlua failure onto the crate error.
///
/// # Arguments
///
/// * `err` - mlua failure.
///
/// # Returns
///
/// Crate error for the failure.
fn wrap_mlua(err: mlua::Error) -> Error {
    if let Some(Error::Lua(message)) = err.downcast_ref::<Error>() {
        return Error::Lua(message.clone());
    }
    if let Some(Error::Plan(message)) = err.downcast_ref::<Error>() {
        return Error::Plan(message.clone());
    }
    Error::Lua(err.to_string())
}

/// Extends Lua module resolution with the project root.
///
/// # Arguments
///
/// * `lua` - Lua state receiving the extended path.
/// * `root` - project root for resolution.
///
/// # Errors
///
/// Fails with `Error::Lua` for package path access failures.
fn prepend_project_path(lua: &mlua::Lua, root: &Path) -> Result<()> {
    let package: Table = lua.globals().get("package").map_err(wrap_mlua)?;
    let previous: String = package.get("path").map_err(wrap_mlua)?;
    let root = root.display();
    package
        .set("path", format!("{root}/?.lua;{root}/?/init.lua;{previous}"))
        .map_err(wrap_mlua)?;
    Ok(())
}

/// Reads a dense string array from a Lua table.
///
/// # Arguments
///
/// * `table` - Lua table holding the array.
/// * `what` - error message naming the field.
///
/// # Returns
///
/// String values in index order.
///
/// # Errors
///
/// Fails with `Error::Config` for gaps in index order and for entries holding values of other shapes.
fn read_string_array(table: &Table, what: &'static str) -> Result<Vec<String>> {
    let mut indexed: Vec<(i64, String)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair.map_err(|_| Error::Config(what.to_string()))?;
        let index = match key {
            Value::Integer(index) => index,
            _ => return Err(Error::Config(what.to_string())),
        };
        let item = match value {
            Value::String(text) => text.to_string_lossy(),
            _ => return Err(Error::Config(what.to_string())),
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(Error::Config(what.to_string()));
        }
    }
    Ok(indexed.into_iter().map(|(_, item)| item).collect())
}

/// Reads the profile shells field.
///
/// # Arguments
///
/// * `profile` - returned profile table.
///
/// # Returns
///
/// Declared shell names.
///
/// # Errors
///
/// Fails with `Error::Config` for absent fields and for empty arrays and for malformed entries.
fn read_shells(profile: &Table) -> Result<Vec<String>> {
    const WHAT: &str = "profile: field 'shells' must be a non-empty string array";
    let raw: Value = profile
        .get("shells")
        .map_err(|_| Error::Config(WHAT.to_string()))?;
    let list = match raw {
        Value::Table(list) => list,
        _ => return Err(Error::Config(WHAT.to_string())),
    };
    let shells = read_string_array(&list, WHAT)?;
    if shells.is_empty() {
        return Err(Error::Config(WHAT.to_string()));
    }
    Ok(shells)
}

/// Reads config builders from the profile configs field.
///
/// # Arguments
///
/// * `profile` - returned profile table.
///
/// # Returns
///
/// Builders in profile order.
///
/// # Errors
///
/// Fails with `Error::Config` for malformed entries naming the index.
fn read_builders(profile: &Table) -> Result<Vec<ConfigBuilder>> {
    const WHAT: &str = "profile: field 'configs' must be a non-empty array of configs";
    let raw: Value = profile
        .get("configs")
        .map_err(|_| Error::Config(WHAT.to_string()))?;
    let list = match raw {
        Value::Nil => return Err(Error::Config(WHAT.to_string())),
        Value::Table(list) => list,
        _ => {
            return Err(Error::Config(WHAT.to_string()));
        }
    };
    let len = list.raw_len();
    let mut out = Vec::with_capacity(len);
    for index in 1..=len {
        let item: Value = list
            .get(index)
            .map_err(|err| Error::Config(format!("profile: configs[{index}] unreadable: {err}")))?;
        match item {
            Value::UserData(handle) => match handle.borrow::<ConfigBuilder>() {
                Ok(builder) => out.push(builder.clone()),
                Err(_) => {
                    return Err(Error::Config(format!(
                        "profile: configs[{index}] must be a config (expected config userdata)"
                    )));
                }
            },
            _ => {
                return Err(Error::Config(format!(
                    "profile: configs[{index}] must be a config (expected config userdata)"
                )));
            }
        }
    }
    if out.is_empty() {
        return Err(Error::Config(WHAT.to_string()));
    }
    Ok(out)
}

/// Reads the profile documents field.
///
/// # Arguments
///
/// * `profile` - returned profile table.
///
/// # Returns
///
/// Profile-declared documents in profile order, empty while the field stays absent.
///
/// # Errors
///
/// Fails with `Error::Config` for bad shapes naming the index.
fn read_documents(profile: &Table) -> Result<Vec<Document>> {
    const WHAT: &str = "profile: field 'documents' must be an array of documents";
    let raw: Value = profile
        .get("documents")
        .map_err(|_| Error::Config(WHAT.to_string()))?;
    let list = match raw {
        Value::Nil => return Ok(Vec::new()),
        Value::Table(list) => list,
        _ => return Err(Error::Config(WHAT.to_string())),
    };
    let len = list.raw_len();
    let mut documents = Vec::with_capacity(len);
    for index in 1..=len {
        let item: Value = list.get(index).map_err(|err| {
            Error::Config(format!("profile: documents[{index}] unreadable: {err}"))
        })?;
        match item {
            Value::Table(table) => {
                let ctx = format!("profile: documents[{index}]");
                match crate::framework::document::document_table_to_document(&table, &ctx) {
                    Ok(document) => documents.push(document),
                    Err(err) => return Err(wrap_mlua(err)),
                }
            }
            _ => {
                return Err(Error::Config(format!(
                    "profile: documents[{index}] must be a document (expected document table)"
                )));
            }
        }
    }
    Ok(documents)
}

/// Rejects repeated structured declarations across profile plus configs.
///
/// Text plus link plus rc duplicates flow onward for later handling.
///
/// # Arguments
///
/// * `documents` - profile-declared documents.
/// * `configs` - config contributions in profile order.
///
/// # Returns
///
/// Unit for unique structured declarations.
///
/// # Errors
///
/// Fails with `Error::Config` for repeated paths, naming the path plus both owners.
fn check_declared_documents(documents: &[Document], configs: &[ConfigContribution]) -> Result<()> {
    let mut owners: BTreeMap<&str, (&str, DocumentKind)> = BTreeMap::new();
    for document in documents {
        if matches!(
            document.kind,
            DocumentKind::Text | DocumentKind::Link | DocumentKind::Rc
        ) {
            continue;
        }
        match owners.insert(document.path.as_str(), ("profile", document.kind)) {
            None => {}
            Some((first, _)) => {
                return Err(Error::Config(format!(
                    "profile: document '{}' is declared more than once ('{first}' plus 'profile')",
                    document.path
                )));
            }
        }
    }
    for config in configs {
        for pending in &config.documents {
            if matches!(
                pending.kind,
                DocumentKind::Text | DocumentKind::Link | DocumentKind::Rc
            ) {
                continue;
            }
            match owners.insert(pending.path.as_str(), (config.name.as_str(), pending.kind)) {
                None => {}
                Some((first, _)) => {
                    return Err(Error::Config(format!(
                        "profile: document '{}' is declared more than once ('{first}' plus '{}')",
                        pending.path, config.name
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Executes live patches per document key on the same state.
///
/// Sorts patches per key by priority desc plus owner asc, runs each
/// callback against a wrapper holding the live table, converts final
/// tables once. No mlua values escape.
///
/// # Arguments
///
/// * `lua` - state owning live tables plus callbacks.
/// * `graph` - declared shells plus documents plus configs.
/// * `handles` - patch handles with owners.
///
/// # Returns
///
/// Final structured plus rc documents.
///
/// # Errors
///
/// Fails with plan errors for bad paths plus format mismatches.
fn execute_live(
    lua: &mlua::Lua,
    graph: &ProfileGraph,
    handles: &[StoredHandle],
) -> mlua::Result<Vec<Document>> {
    let mut merged = Vec::new();
    merged.extend(execute_structured(lua, graph, handles)?);
    if let Some(rc) = execute_rc(lua, graph, handles)? {
        merged.push(rc);
    }
    Ok(merged)
}

/// Groups handles by target document key.
///
/// # Arguments
///
/// * `handles` - patch handles.
///
/// # Returns
///
/// Handles grouped by target.
fn group_handles(handles: &[StoredHandle]) -> BTreeMap<String, Vec<&StoredHandle>> {
    let mut grouped: BTreeMap<String, Vec<&StoredHandle>> = BTreeMap::new();
    for handle in handles {
        grouped
            .entry(handle.target.clone())
            .or_default()
            .push(handle);
    }
    grouped
}

/// Sorts handles by priority desc plus owner asc.
///
/// # Arguments
///
/// * `items` - handles under sort, mutated in place.
fn sort_handles(items: &mut [&StoredHandle]) {
    items.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.owner.cmp(&right.owner))
    });
}

/// Executes structured patches per path.
///
/// Declared bases seed live tables, sorted patches run live, final
/// tables convert once. Patch-only paths start empty with agreed format.
///
/// # Arguments
///
/// * `lua` - state owning live tables.
/// * `graph` - declared documents plus configs.
/// * `handles` - patch handles.
///
/// # Returns
///
/// Final structured documents in path order.
///
/// # Errors
///
/// Fails with merge errors for format mismatches plus plan errors for bad paths.
fn execute_structured(
    lua: &mlua::Lua,
    graph: &ProfileGraph,
    handles: &[StoredHandle],
) -> mlua::Result<Vec<Document>> {
    let mut declared: BTreeMap<
        String,
        (
            StructuredFormat,
            BTreeMap<String, serde_json::Value>,
            String,
        ),
    > = BTreeMap::new();
    for document in &graph.documents {
        if let DocumentData::Structured { format, data } = &document.data {
            declared.insert(
                document.path.clone(),
                (*format, data.clone(), "profile".to_string()),
            );
        }
    }
    for config in &graph.configs {
        for document in &config.documents {
            if let DocumentData::Structured { format, data } = &document.data {
                declared.insert(
                    document.path.clone(),
                    (*format, data.clone(), config.name.clone()),
                );
            }
        }
    }
    let grouped = group_handles(handles);
    let mut structured_handles: BTreeMap<String, Vec<&StoredHandle>> = BTreeMap::new();
    for (target, items) in grouped {
        if target == "rc" {
            continue;
        }
        structured_handles.insert(target, items);
    }
    let mut out: BTreeMap<String, Document> = BTreeMap::new();
    for (path, (format, base, owner)) in &declared {
        let patches = structured_handles.get(path);
        let mut refs: Vec<&StoredHandle> = patches.cloned().unwrap_or_default();
        sort_handles(&mut refs);
        for patch in &refs {
            if let Some(other) = patch.format
                && other != *format
            {
                return Err(crate::plan_err!(
                    "cannot merge document at '{path}': format mismatch"
                ));
            }
        }
        let doc = lua.create_table()?;
        insert_json_object(lua, &doc, base)?;
        let mut seeds: OwnerMap = BTreeMap::new();
        seed_structured_owners(base, owner, &mut seeds);
        let exec: Vec<ExecPatch> = refs
            .iter()
            .map(|item| ExecPatch {
                owner: item.owner.clone(),
                priority: item.priority,
                callback: item.callback.clone(),
            })
            .collect();
        let area = Area::Structured {
            format: format.name().to_string(),
        };
        apply::execute(lua, doc.clone(), area, seeds, exec)?;
        let json = apply::live_to_json(&doc, "patch: convert")?;
        let table: BTreeMap<String, serde_json::Value> = match json {
            serde_json::Value::Object(map) => map.into_iter().collect(),
            _ => {
                return Err(crate::plan_err!("patch for '{path}' holds no object"));
            }
        };
        out.insert(
            path.clone(),
            Document {
                kind: DocumentKind::Structured,
                path: path.clone(),
                data: DocumentData::Structured {
                    format: *format,
                    data: table,
                },
                data_hash: String::new(),
            },
        );
    }
    for (path, items) in &structured_handles {
        if declared.contains_key(path) {
            continue;
        }
        let mut refs = items.to_vec();
        sort_handles(&mut refs);
        let format = created_format(path, &refs)?;
        let doc = lua.create_table()?;
        let seeds: OwnerMap = BTreeMap::new();
        let exec: Vec<ExecPatch> = refs
            .iter()
            .map(|item| ExecPatch {
                owner: item.owner.clone(),
                priority: item.priority,
                callback: item.callback.clone(),
            })
            .collect();
        let area = Area::Structured {
            format: format.name().to_string(),
        };
        apply::execute(lua, doc.clone(), area, seeds, exec)?;
        let json = apply::live_to_json(&doc, "patch: convert")?;
        let table: BTreeMap<String, serde_json::Value> = match json {
            serde_json::Value::Object(map) => map.into_iter().collect(),
            _ => {
                return Err(crate::plan_err!("patch for '{path}' holds no object"));
            }
        };
        out.insert(
            path.clone(),
            Document {
                kind: DocumentKind::Structured,
                path: path.clone(),
                data: DocumentData::Structured {
                    format,
                    data: table,
                },
                data_hash: String::new(),
            },
        );
    }
    Ok(out.into_values().collect())
}

/// Seeds structured owners from a declared base.
///
/// # Arguments
///
/// * `base` - declared table.
/// * `owner` - declaring owner.
/// * `seeds` - winners under seeding, mutated in place.
fn seed_structured_owners(
    base: &BTreeMap<String, serde_json::Value>,
    owner: &str,
    seeds: &mut OwnerMap,
) {
    for (key, value) in base {
        let mut leaves = BTreeMap::new();
        apply::flatten_json(value, key, &mut leaves);
        for leaf in leaves.keys() {
            seeds
                .entry(leaf.clone())
                .or_insert_with(|| owner.to_string());
        }
    }
}

/// Resolves the format for a patch-created document.
///
/// Every patch for the path must agree on one format.
///
/// # Arguments
///
/// * `path` - document path.
/// * `patches` - patches targeting the path.
///
/// # Returns
///
/// The agreed format.
///
/// # Errors
///
/// Fails with merge errors for conflicting formats plus plan errors for missing formats.
fn created_format(path: &str, patches: &[&StoredHandle]) -> mlua::Result<StructuredFormat> {
    let mut format: Option<StructuredFormat> = None;
    for patch in patches {
        match (format, patch.format) {
            (None, Some(next)) => format = Some(next),
            (Some(current), Some(next)) if current != next => {
                return Err(crate::plan_err!(
                    "cannot merge document at '{path}': format mismatch"
                ));
            }
            _ => {}
        }
    }
    match format {
        Some(format) => Ok(format),
        None => Err(crate::plan_err!(
            "patch for '{path}' holds no format (structured patches name one)"
        )),
    }
}

/// Inserts a JSON object into a live Lua table.
///
/// # Arguments
///
/// * `lua` - state owning conversions.
/// * `doc` - live table under fill, mutated in place.
/// * `base` - declared object.
///
/// # Returns
///
/// Unit after fill.
///
/// # Errors
///
/// Fails with plan errors for conversion failures.
fn insert_json_object(
    lua: &mlua::Lua,
    doc: &Table,
    base: &BTreeMap<String, serde_json::Value>,
) -> mlua::Result<()> {
    for (key, value) in base {
        let lua_value = crate::framework::apply::json_to_lua(lua, value)?;
        doc.set(key.as_str(), lua_value)?;
    }
    Ok(())
}

/// Executes rc patches over one live rc table.
///
/// Declared rc entries merge first with first-writer wins, sorted
/// patches run live, final sections convert once.
///
/// # Arguments
///
/// * `lua` - state owning live tables.
/// * `graph` - declared documents plus configs.
/// * `handles` - patch handles.
///
/// # Returns
///
/// Final rc document with path `rc`, empty while no rc content exists.
///
/// # Errors
///
/// Fails with plan errors for bad sections plus bad entries.
fn execute_rc(
    lua: &mlua::Lua,
    graph: &ProfileGraph,
    handles: &[StoredHandle],
) -> mlua::Result<Option<Document>> {
    let mut base_entries: Vec<(serde_json::Value, String, String)> = Vec::new();
    collect_rc_declared(graph, &mut base_entries)?;
    let mut rc_handles: Vec<&StoredHandle> =
        handles.iter().filter(|item| item.target == "rc").collect();
    if base_entries.is_empty() && rc_handles.is_empty() {
        return Ok(None);
    }
    sort_handles(&mut rc_handles);
    let doc = lua.create_table()?;
    for section in ["profile", "config", "final"] {
        doc.set(section, lua.create_table()?)?;
    }
    let mut seeds: OwnerMap = BTreeMap::new();
    let mut owners: BTreeMap<String, String> = BTreeMap::new();
    for (json, section, owner) in &base_entries {
        insert_declared_rc(lua, &doc, json, section, owner, &mut seeds, &mut owners)?;
    }
    let exec: Vec<ExecPatch> = rc_handles
        .iter()
        .map(|item| ExecPatch {
            owner: item.owner.clone(),
            priority: item.priority,
            callback: item.callback.clone(),
        })
        .collect();
    apply::execute(lua, doc.clone(), Area::Rc, seeds, exec)?;
    let data = convert_live_rc(&doc)?;
    Ok(Some(Document {
        kind: DocumentKind::Rc,
        path: "rc".to_string(),
        data: DocumentData::Rc(data),
        data_hash: String::new(),
    }))
}

/// Collects declared rc entries in profile plus config order.
///
/// # Arguments
///
/// * `graph` - declared documents plus configs.
/// * `out` - accumulator holding entry JSON plus section plus owner.
///
/// # Returns
///
/// Unit after collection.
///
/// # Errors
///
/// Fails with plan errors for serialization failures.
fn collect_rc_declared(
    graph: &ProfileGraph,
    out: &mut Vec<(serde_json::Value, String, String)>,
) -> mlua::Result<()> {
    for document in &graph.documents {
        if let DocumentData::Rc(data) = &document.data {
            push_rc_data(data, "profile", out)?;
        }
    }
    for config in &graph.configs {
        for document in &config.documents {
            if let DocumentData::Rc(data) = &document.data {
                push_rc_data(data, &config.name, out)?;
            }
        }
    }
    Ok(())
}

/// Pushes one RcData into the declared accumulator.
///
/// Entry payload selects the section: aliases land in `config`, env
/// plus profile entries land in `profile`, init specs land in `final`.
///
/// # Arguments
///
/// * `data` - declared rc data.
/// * `owner` - declaring owner.
/// * `out` - accumulator under extension.
///
/// # Returns
///
/// Unit after push.
///
/// # Errors
///
/// Fails with plan errors for serialization failures.
fn push_rc_data(
    data: &crate::model::state::rc::RcData,
    owner: &str,
    out: &mut Vec<(serde_json::Value, String, String)>,
) -> mlua::Result<()> {
    for entry in &data.aliases {
        let json = serde_json::to_value(entry)
            .map_err(|error| crate::plan_err!("patch: entry failed to convert: {error}"))?;
        out.push((json, "config".to_string(), owner.to_string()));
    }
    for entry in &data.env {
        let json = serde_json::to_value(entry)
            .map_err(|error| crate::plan_err!("patch: entry failed to convert: {error}"))?;
        out.push((json, "profile".to_string(), owner.to_string()));
    }
    for entry in &data.profile {
        let json = serde_json::to_value(entry)
            .map_err(|error| crate::plan_err!("patch: entry failed to convert: {error}"))?;
        out.push((json, "profile".to_string(), owner.to_string()));
    }
    for entry in &data.init {
        let json = serde_json::to_value(entry)
            .map_err(|error| crate::plan_err!("patch: entry failed to convert: {error}"))?;
        out.push((json, "final".to_string(), owner.to_string()));
    }
    Ok(())
}

/// Inserts one declared rc entry with first-writer wins.
///
/// Later same-slot entries drop. Foreign drops log one line, init
/// drops stay silent.
///
/// # Arguments
///
/// * `lua` - state owning conversions.
/// * `doc` - live rc table, mutated in place.
/// * `json` - entry JSON with priority.
/// * `section` - closed section name.
/// * `owner` - declaring owner.
/// * `seeds` - winners under update, mutated in place.
/// * `seen` - slot owners under update, mutated in place.
///
/// # Returns
///
/// Unit after insert or drop.
///
/// # Errors
///
/// Fails with plan errors for conversion failures.
#[allow(clippy::too_many_arguments)]
fn insert_declared_rc(
    lua: &mlua::Lua,
    doc: &Table,
    json: &serde_json::Value,
    section: &str,
    owner: &str,
    seeds: &mut OwnerMap,
    seen: &mut BTreeMap<String, String>,
) -> mlua::Result<()> {
    let area = apply::rc_area_name(section, json)?;
    let slot = apply::rc_live_slot(section, json, &area)?;
    if let Some(winner) = seen.get(&slot) {
        if winner != owner && area != "init" {
            let name = apply::entry_name(json, &area).unwrap_or_default();
            log::debug!(
                "collision on {area} \"{name}\": \"{owner}\" overwritten, \"{winner}\" wins"
            );
        }
        return Ok(());
    }
    seen.insert(slot.clone(), owner.to_string());
    seeds.insert(slot, owner.to_string());
    let lua_value = crate::framework::apply::json_to_lua(lua, json)?;
    let list: Table = doc.get(section)?;
    let next = (list.raw_len() + 1) as i64;
    list.set(next, lua_value)?;
    Ok(())
}

/// Converts a final live rc table into RcData.
///
/// Sections hold entry tables. Set plus append behave identically,
/// first wins already enforced live.
///
/// # Arguments
///
/// * `doc` - final live rc table.
///
/// # Returns
///
/// RcData holding parsed entries.
///
/// # Errors
///
/// Fails with plan errors for bad entry shapes.
fn convert_live_rc(doc: &Table) -> mlua::Result<crate::model::state::rc::RcData> {
    let mut data = crate::model::state::rc::RcData::default();
    for section in ["profile", "config", "final"] {
        let list: Value = doc.get(section)?;
        let table = match list {
            Value::Nil => continue,
            Value::Table(table) => table,
            _ => {
                return Err(crate::plan_err!(
                    "patch: section '{section}' holds a non-list leaf"
                ));
            }
        };
        let len = table.raw_len();
        for index in 1..=len {
            let item: Value = table.get(index)?;
            let entry_table = match item {
                Value::Table(entry) => entry,
                _ => {
                    return Err(crate::plan_err!(
                        "patch: section '{section}' holds a non-table entry"
                    ));
                }
            };
            let json = apply::live_to_json(&entry_table, "patch: convert")?;
            push_live_entry(&mut data, section, &json)?;
        }
    }
    Ok(data)
}

/// Pushes one live rc entry JSON into RcData.
///
/// Section plus shape select the area: config means alias, final
/// means init, profile means profile when the table holds `op` else env.
///
/// # Arguments
///
/// * `data` - RcData under extension, mutated in place.
/// * `section` - closed section name.
/// * `json` - entry JSON.
///
/// # Returns
///
/// Unit after push.
///
/// # Errors
///
/// Fails with plan errors for shape mismatches naming the section.
fn push_live_entry(
    data: &mut crate::model::state::rc::RcData,
    section: &str,
    json: &serde_json::Value,
) -> mlua::Result<()> {
    if section == "config" {
        let entry: crate::model::state::rc::AliasEntry = serde_json::from_value(json.clone())
            .map_err(|error| {
                crate::plan_err!("invalid rc entry for section '{section}': {error}")
            })?;
        data.aliases.push(entry);
        return Ok(());
    }
    if section == "final" {
        let entry: crate::model::state::rc::InitEntry = serde_json::from_value(json.clone())
            .map_err(|error| {
                crate::plan_err!("invalid rc entry for section '{section}': {error}")
            })?;
        data.init.push(entry);
        return Ok(());
    }
    if json.get("op").is_some() {
        let entry: crate::model::state::rc::ProfileEntry = serde_json::from_value(json.clone())
            .map_err(|error| {
                crate::plan_err!("invalid rc entry for section '{section}': {error}")
            })?;
        data.profile.push(entry);
        return Ok(());
    }
    let entry: crate::model::state::rc::EnvEntry = serde_json::from_value(json.clone())
        .map_err(|error| crate::plan_err!("invalid rc entry for section '{section}': {error}"))?;
    data.env.push(entry);
    Ok(())
}
