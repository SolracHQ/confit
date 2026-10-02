//! Eval
//!
//! Profile loading and document assembly.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, LuaOptions, StdLib, Table, Value};
use serde_json::Value as Json;

use crate::EvalOpts;
use crate::error::{EngineError, FieldRef, Scope, wrap};
use crate::exec::{Area, ExecPatch, Executor, OwnerMap};
use crate::lua::{JsonExt, TableExt};
use crate::model::{ConfigData, StoredPatch};
use crate::path_expr::flatten_json;
use crate::require::Requirer;
use crate::surface::config::ConfigBuilder;
use crate::surface::document::Declared;
use crate::surface::document::convert::translate_entry;
use crate::surface::utils;
use confit_model::arg::Arg;
use confit_model::document::Document;
use confit_model::document::{BlobRef, Data, RcData, RcEntry, RcOp, StructuredFormat};
use confit_model::hook::{Hook, merge_hooks};
use confit_model::progress::{Event, ProgressSender};
use confit_model::routes::{Route, RouteBase};
use confit_store::Stores;
use confit_store::handles::BlobHandle;

use crate::error::Result;

/// One evaluation holding the Lua state and its context.
///
/// The state, the resolution roots, the fetcher, and the progress
/// sender travel together, so assembly methods read them from self
/// instead of threading six arguments per call.
pub(crate) struct Session {
    /// Lua state carrying the confit surface.
    pub(crate) lua: Lua,
    /// Require and resource base.
    pub(crate) root: PathBuf,
    /// External plugin folder.
    pub(crate) plugins: PathBuf,
    /// Forces remote downloads past the sidecar cache.
    pub(crate) re_fetch: bool,
    /// Progress sender, holding `None` for silence.
    pub(crate) progress: Option<ProgressSender>,
    /// Finished patch count shared across documents.
    pub(crate) patch_done: Cell<usize>,
    /// Write capabilities behind the handle surface.
    pub(crate) stores: Stores,
}

impl Session {
    /// Runs one profile file into finished documents and hooks.
    pub(crate) fn run(profile: &Path, opts: EvalOpts) -> Result<crate::Evaluation> {
        let start = std::time::Instant::now();
        let root = absolutize(&resolve_root(profile, &opts.root))?;
        let stores = match opts.stores {
            Some(stores) => stores,
            None => {
                let (sender, _) = crossbeam_channel::unbounded();
                Stores::new(confit_store::StoreRoots::standard(), sender)
            }
        };
        let lua = Lua::new_with(
            StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE,
            LuaOptions::default(),
        )
        .map_err(|error| EngineError::Unknown {
            context: "lua state".to_owned(),
            message: error.to_string(),
        })?;
        let session = Self {
            lua,
            root,
            plugins: opts.plugins,
            re_fetch: opts.re_fetch,
            progress: opts.progress,
            patch_done: Cell::new(0),
            stores,
        };
        let requirer = Requirer {
            current: session.root.clone(),
            folder: session.root.clone(),
            scope: "profile",
        }
        .make(&session.lua)
        .map_err(|error| EngineError::Unknown {
            context: "require".to_owned(),
            message: error.to_string(),
        })?;
        session
            .lua
            .globals()
            .set("require", requirer)
            .map_err(|error| EngineError::Unknown {
                context: "require".to_owned(),
                message: error.to_string(),
            })?;
        crate::surface::install(&session)?;
        let resources = session.stores.resources();
        let absolute = absolutize(profile)?;
        let handle = resources.resource(&session.root, &absolute)?;
        let source = resources.read_text(&handle)?;
        let scope = Scope::profile(profile);
        let returned: Value = session
            .lua
            .load(&source)
            .set_name(format!("@{}", profile.display()))
            .call(())
            .map_err(wrap)?;
        let table = coerce_profile_table(returned).map_err(wrap)?;
        let profile = Profile::read(&table, &scope)?;
        profile.check(&scope)?;
        let patches = profile.patches();
        let total = patches.len();
        if total > 0
            && let Some(sender) = session.progress.as_ref()
        {
            let _ = sender.send(Event::PatchesStarted { patches: total });
        }
        let mut blobs: BTreeMap<String, BlobRef> = BTreeMap::new();
        let mut out = session
            .assemble_structured(&profile, &patches, &scope)
            .map_err(wrap)?;
        let (rest, handles) = profile.text_link(&scope)?;
        blobs.extend(handles);
        out.extend(rest);
        out.extend(
            session
                .assemble_rc(&profile, &patches, &scope)
                .map_err(wrap)?,
        );
        log::debug!(
            "evaluate took {}ms for {} documents",
            start.elapsed().as_millis(),
            out.len()
        );
        let mut declared: Vec<Hook> = Vec::new();
        for config in &profile.configs {
            declared.extend(config.hooks.iter().cloned());
        }
        let hooks = merge_hooks(declared);
        validate_changed(&hooks, &out).map_err(wrap)?;
        Ok(crate::Evaluation {
            documents: out,
            blobs,
            hooks,
        })
    }
}

/// Resolves the resolution base defaulting to the profile parent.
fn resolve_root(profile: &Path, root: &Path) -> PathBuf {
    if !root.as_os_str().is_empty() {
        return root.to_path_buf();
    }
    match profile.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Resolves one profile path against the working folder.
///
/// # Errors
///
/// - [`EngineError::Unknown`] for working folder failures.
fn absolutize(profile: &Path) -> Result<PathBuf> {
    if profile.is_absolute() {
        return Ok(profile.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|error| EngineError::Unknown {
        context: "working directory".to_owned(),
        message: error.to_string(),
    })?;
    Ok(cwd.join(profile))
}

/// Rejects changed gates naming documents outside the built set.
///
/// # Errors
///
/// - [`EngineError::ChangedUnknown`] for gates naming
///   documents outside the built set.
fn validate_changed(hooks: &[Hook], documents: &[Document]) -> mlua::Result<()> {
    let built: std::collections::BTreeSet<Route> = documents
        .iter()
        .map(|document| document.destination.clone())
        .collect();
    let mut paths: Vec<Route> = Vec::new();
    for hook in hooks {
        if let Some(gate) = hook.requires.as_ref() {
            gate.collect_changed(&mut paths);
        }
        if let Some(gate) = hook.when.as_ref() {
            gate.collect_changed(&mut paths);
        }
        for check in &hook.checks {
            check.collect_changed(&mut paths);
        }
    }
    for path in paths {
        if !built.contains(&path) {
            return Err(EngineError::ChangedUnknown {
                route: path.display(),
            }
            .into());
        }
    }
    Ok(())
}

/// Coerces the profile return into a table.
///
/// # Errors
///
/// - [`EngineError::Shape`] for non-table returns.
fn coerce_profile_table(returned: Value) -> mlua::Result<Table> {
    match returned {
        Value::Table(table) => Ok(table),
        Value::Function(func) => match func.call::<Value>(())? {
            Value::Table(table) => Ok(table),
            _ => Err(EngineError::Shape {
                scope: Scope::method("profile"),
                want: "must return a table with 'shells' and 'configs'",
            }
            .into()),
        },
        _ => Err(EngineError::Shape {
            scope: Scope::method("profile"),
            want: "must return a table with 'shells' and 'configs'",
        }
        .into()),
    }
}

/// Reads the profile shells field.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped shells fields.
/// - [`EngineError::Repeat`] for repeated shell names.
fn read_shells(profile: &Table, scope: &Scope) -> Result<Vec<String>> {
    let raw: Value = profile.get("shells").map_err(|_| EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name("shells"),
        want: "must be a non-empty string array",
    })?;
    let list = match raw {
        Value::Table(list) => list,
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("shells"),
                want: "must be a non-empty string array",
            });
        }
    };
    let shells = read_string_array(&list).ok_or_else(|| EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name("shells"),
        want: "must be a non-empty string array",
    })?;
    if shells.is_empty() {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("shells"),
            want: "must be a non-empty string array",
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for shell in &shells {
        if !seen.insert(shell.as_str()) {
            return Err(EngineError::Repeat {
                scope: scope.clone(),
                collection: "shells",
                item: shell.clone(),
            });
        }
    }
    Ok(shells)
}

/// Reads one dense string array from a Lua table.
fn read_string_array(table: &Table) -> Option<Vec<String>> {
    let mut indexed: Vec<(i64, String)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair.ok()?;
        match (key, value) {
            (Value::Integer(index), Value::String(text)) => {
                indexed.push((index, text.to_string_lossy()));
            }
            _ => return None,
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return None;
        }
    }
    Some(indexed.into_iter().map(|(_, item)| item).collect())
}

/// Reads config contributions from the profile configs field.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped configs fields.
/// - [`EngineError::Item`] for unreadable entries.
fn read_builders(profile: &Table, scope: &Scope) -> Result<Vec<ConfigData>> {
    let raw: Value = profile.get("configs").map_err(|_| EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name("configs"),
        want: "must be a non-empty array of configs",
    })?;
    let list = match raw {
        Value::Table(list) => list,
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("configs"),
                want: "must be a non-empty array of configs",
            });
        }
    };
    let len = list.raw_len();
    let mut out = Vec::with_capacity(len);
    for index in 1..=len {
        let item: Value = list.get(index).map_err(|error| EngineError::Item {
            scope: scope.clone(),
            collection: "configs",
            index,
            reason: error.to_string(),
        })?;
        match item {
            Value::UserData(handle) => match handle.borrow::<ConfigBuilder>() {
                Ok(builder) => out.push(builder.contribution().clone()),
                Err(_) => {
                    return Err(EngineError::Field {
                        scope: scope.clone(),
                        field: FieldRef::index("configs", index),
                        want: "must be a config (expected config userdata)",
                    });
                }
            },
            _ => {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::index("configs", index),
                    want: "must be a config (expected config userdata)",
                });
            }
        }
    }
    if out.is_empty() {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("configs"),
            want: "must be a non-empty array of configs",
        });
    }
    Ok(out)
}

/// Reads profile-declared documents with profile ownership.
///
/// # Errors
///
/// - [`EngineError::Field`] for misshaped documents fields.
/// - [`EngineError::Item`] for unreadable entries.
fn read_documents(profile: &Table, scope: &Scope) -> mlua::Result<ProfileDeclared> {
    let raw: Value = profile.get("documents").map_err(|_| EngineError::Field {
        scope: scope.clone(),
        field: FieldRef::name("documents"),
        want: "must be an array of documents",
    })?;
    let list = match raw {
        Value::Nil => return Ok(ProfileDeclared::default()),
        Value::Table(list) => list,
        _ => {
            return Err(EngineError::Field {
                scope: scope.clone(),
                field: FieldRef::name("documents"),
                want: "must be an array of documents",
            }
            .into());
        }
    };
    let len = list.raw_len();
    let mut out = ProfileDeclared::default();
    for index in 1..=len {
        let item: Value = list.get(index).map_err(|error| EngineError::Item {
            scope: scope.clone(),
            collection: "documents",
            index,
            reason: error.to_string(),
        })?;
        let table = match item {
            Value::Table(table) => table,
            _ => {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::index("documents", index),
                    want: "must be a document (expected document table)",
                }
                .into());
            }
        };
        let item_scope = scope.item("documents", index);
        out.push(
            crate::surface::document::convert::convert_document(&table, &item_scope)?,
            scope,
        )?;
    }
    Ok(out)
}

/// Profile-declared documents split by kind.
#[derive(Default)]
struct ProfileDeclared {
    /// Structured declarations in profile order.
    structured: Vec<crate::model::StructuredDecl>,
    /// Text declarations in profile order.
    texts: Vec<crate::model::TextDecl>,
    /// Link declarations in profile order.
    links: Vec<crate::model::LinkDecl>,
    /// Opaque declarations in profile order.
    opaques: Vec<crate::model::OpaqueDecl>,
    /// Tree declarations in profile order.
    trees: Vec<crate::model::TreeDecl>,
    /// Optional rc base from one rc.new table.
    rc_base: Option<Vec<crate::model::RcEntryDecl>>,
}

/// Pushes one declared document into the profile accumulator.
///
/// # Errors
///
/// - [`EngineError::Duplicate`] for repeated rc bases.
impl ProfileDeclared {
    /// Pushes one declared document into the accumulator.
    fn push(&mut self, declared: Declared, scope: &Scope) -> mlua::Result<()> {
        match declared {
            Declared::Structured(decl) => self.structured.push(decl),
            Declared::Text(decl) => self.texts.push(decl),
            Declared::Link(decl) => self.links.push(decl),
            Declared::Opaque(decl) => self.opaques.push(decl),
            Declared::Tree(decl) => self.trees.push(decl),
            Declared::Rc(entries) => {
                if self.rc_base.is_some() {
                    return Err(EngineError::Duplicate {
                        scope: scope.clone(),
                        item: "rc".to_owned(),
                        first: "profile".to_owned(),
                        second: "profile".to_owned(),
                    }
                    .into());
                }
                self.rc_base = Some(entries);
            }
        }
        Ok(())
    }
}

/// One profile holding shells, declarations, and configs.
struct Profile {
    /// Shells under rendering.
    shells: Vec<String>,
    /// Profile-declared documents.
    declared: ProfileDeclared,
    /// Config contributions in profile order.
    configs: Vec<ConfigData>,
}

/// Declared structured base holding format, data, and owner.
struct StructuredBase {
    /// Late-bound destination route.
    destination: Route,
    /// Declared output format.
    format: StructuredFormat,
    /// Declared top-level fields.
    data: BTreeMap<String, Json>,
    /// Contributing owner name.
    owner: String,
}

/// Walks one require closure over present configs.
fn walk_requires(
    name: &str,
    present: &BTreeMap<&str, &ConfigData>,
    visited: &mut std::collections::BTreeSet<String>,
) -> Result<()> {
    if !visited.insert(name.to_string()) {
        return Ok(());
    }
    let Some(config) = present.get(name) else {
        return Ok(());
    };
    for edge in &config.requires {
        let Some(_) = present.get(edge.target.as_str()) else {
            return Err(EngineError::Require {
                name: config.name.clone(),
                target: edge.target.clone(),
                hint: edge.hint.clone(),
            });
        };
        walk_requires(edge.target.as_str(), present, visited)?;
    }
    Ok(())
}

impl Profile {
    /// Reads shells, documents, and configs from a profile table.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Field`] for misshaped profile fields.
    /// - [`EngineError::Item`] for unreadable entries.
    fn read(table: &Table, scope: &Scope) -> Result<Self> {
        Ok(Self {
            shells: read_shells(table, scope)?,
            declared: read_documents(table, scope).map_err(wrap)?,
            configs: read_builders(table, scope)?,
        })
    }

    /// Rejects repeated declarations across profile and configs.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Duplicate`] for repeated declarations.
    /// - [`EngineError::Require`] for dangling require edges.
    fn check(&self, scope: &Scope) -> Result<()> {
        let mut owners: BTreeMap<String, &str> = BTreeMap::new();
        for item in &self.declared.structured {
            let display = item.destination.display();
            if let Some(first) = owners.insert(display.clone(), "profile") {
                return Err(EngineError::Duplicate {
                    scope: scope.clone(),
                    item: display,
                    first: first.to_owned(),
                    second: "profile".to_owned(),
                });
            }
        }
        for config in &self.configs {
            for item in &config.structured {
                let display = item.destination.display();
                if let Some(first) = owners.insert(display.clone(), config.name.as_str()) {
                    return Err(EngineError::Duplicate {
                        scope: scope.clone(),
                        item: display,
                        first: first.to_owned(),
                        second: config.name.clone(),
                    });
                }
            }
        }
        let mut first: Option<&str> = None;
        if self.declared.rc_base.is_some() {
            first = Some("profile");
        }
        for config in &self.configs {
            if config.rc_base.is_some() {
                match first {
                    None => first = Some(config.name.as_str()),
                    Some(owner) => {
                        return Err(EngineError::Duplicate {
                            scope: scope.clone(),
                            item: "rc".to_owned(),
                            first: owner.to_owned(),
                            second: config.name.clone(),
                        });
                    }
                }
            }
        }
        self.check_requires()?;
        Ok(())
    }

    /// Rejects require edges naming configs absent from the profile.
    fn check_requires(&self) -> Result<()> {
        let mut present: BTreeMap<&str, &ConfigData> = BTreeMap::new();
        for config in &self.configs {
            present.insert(config.name.as_str(), config);
        }
        let mut visited: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for config in &self.configs {
            walk_requires(config.name.as_str(), &present, &mut visited)?;
        }
        Ok(())
    }

    /// Collects patch handles in config declaration order with running declaration indexes.
    fn patches(&self) -> Vec<StoredPatch> {
        let mut out = Vec::new();
        let mut order = 0;
        for config in &self.configs {
            for patch in &config.patches {
                let mut stamped = patch.clone();
                stamped.order = order;
                order += 1;
                out.push(stamped);
            }
        }
        out
    }

    /// Assembles text, link, opaque, and tree documents in route order.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Duplicate`] for repeated declarations.
    fn text_link(&self, scope: &Scope) -> Result<(Vec<Document>, BTreeMap<String, BlobRef>)> {
        assemble_text_link(&self.declared, &self.configs, scope)
    }
}

impl Session {
    /// Assembles structured documents in destination order.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Mismatch`] for format mismatches.
    /// - [`EngineError::RouteDetail`] for destination builds.
    fn assemble_structured(
        &self,
        profile: &Profile,
        patches: &[StoredPatch],
        scope: &Scope,
    ) -> mlua::Result<Vec<Document>> {
        let mut bases: BTreeMap<String, StructuredBase> = BTreeMap::new();
        for item in &profile.declared.structured {
            bases.insert(
                item.destination.display(),
                StructuredBase {
                    destination: item.destination.clone(),
                    format: item.format,
                    data: item.data.clone(),
                    owner: "profile".to_string(),
                },
            );
        }
        for config in &profile.configs {
            for item in &config.structured {
                bases.insert(
                    item.destination.display(),
                    StructuredBase {
                        destination: item.destination.clone(),
                        format: item.format,
                        data: item.data.clone(),
                        owner: config.name.clone(),
                    },
                );
            }
        }
        let mut grouped: BTreeMap<String, Vec<&StoredPatch>> = BTreeMap::new();
        for patch in patches {
            if patch.target != "rc" {
                grouped.entry(patch.target.clone()).or_default().push(patch);
            }
        }
        let exec = Executor {
            lua: &self.lua,
            progress: self.progress.clone(),
            patch_total: patches.len(),
            patch_done: &self.patch_done,
        };
        let mut out: BTreeMap<String, Document> = BTreeMap::new();
        for (display, base) in &bases {
            let mut refs: Vec<&StoredPatch> = grouped.get(display).cloned().unwrap_or_default();
            Executor::sort_patches(&mut refs);
            for patch in &refs {
                if let Some(other) = patch.format
                    && other != base.format
                {
                    return Err(EngineError::Mismatch {
                        target: display.clone(),
                    }
                    .into());
                }
            }
            let document = run_structured_doc(
                &self.lua,
                &exec,
                &base.destination,
                base.format,
                Some((&base.data, base.owner.as_str())),
                &refs,
                scope,
            )?;
            out.insert(display.clone(), document);
        }
        for (display, items) in &grouped {
            if bases.contains_key(display) {
                continue;
            }
            let mut refs = items.clone();
            Executor::sort_patches(&mut refs);
            let format = created_format(display, &refs)?;
            let destination = Route::parse(display).map_err(|error| EngineError::NestScope {
                scope: scope.clone(),
                reason: error.to_string(),
            })?;
            let document =
                run_structured_doc(&self.lua, &exec, &destination, format, None, &refs, scope)?;
            out.insert(display.clone(), document);
        }
        Ok(out.into_values().collect())
    }
}

/// Runs one structured document from its base through patches.
///
/// The live table seeds from the declared base, so patch callbacks merge over owned leaves.
///
/// # Errors
///
/// Seeding, callback, and conversion failures fail as Lua errors.
fn run_structured_doc(
    lua: &Lua,
    exec: &Executor<'_>,
    destination: &Route,
    format: StructuredFormat,
    base: Option<(&BTreeMap<String, Json>, &str)>,
    refs: &[&StoredPatch],
    scope: &Scope,
) -> mlua::Result<Document> {
    let doc = lua.create_table()?;
    let mut seeds: OwnerMap = BTreeMap::new();
    if let Some((data, owner)) = base {
        for (key, value) in data {
            let seed = if owner == "profile" {
                scope.slot(FieldRef::name(key))
            } else {
                Scope::config(owner).slot(FieldRef::name(key))
            };
            doc.set(key.as_str(), value.to_lua(lua, &seed)?)?;
        }
        seed_owners(data, owner, &mut seeds);
    }
    let area = Area::Structured {
        format: format.name().to_string(),
    };
    exec.execute(doc.clone(), area, seeds, exec_list(refs))?;
    let table = live_to_map(&doc, &destination.display())?;
    Ok(finish_structured(destination, format, table))
}

/// Builds one finished structured document.
fn finish_structured(
    destination: &Route,
    format: StructuredFormat,
    table: BTreeMap<String, Json>,
) -> Document {
    Document::new(
        destination.clone(),
        Data::Structured {
            format,
            data: table,
        },
    )
}

/// Converts one live table into a data map.
///
/// # Errors
///
/// - [`EngineError::NoObject`] for non-object tables.
fn live_to_map(doc: &Table, path: &str) -> mlua::Result<BTreeMap<String, Json>> {
    let scope = Scope::Call {
        method: "confit.patch.structured",
        target: path.to_owned(),
    };
    match doc.to_json(&scope.slot(FieldRef::name("convert")))? {
        Json::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(EngineError::NoObject {
            target: path.to_owned(),
        }
        .into()),
    }
}

/// Seeds leaf owners from one declared base.
fn seed_owners(base: &BTreeMap<String, Json>, owner: &str, seeds: &mut OwnerMap) {
    for (key, value) in base {
        let mut leaves = BTreeMap::new();
        flatten_json(value, key, &mut leaves);
        for leaf in leaves.keys() {
            seeds
                .entry(leaf.clone())
                .or_insert_with(|| owner.to_string());
        }
    }
}

/// Resolves the format for one patch-created document.
///
/// # Errors
///
/// - [`EngineError::Mismatch`] for format mismatches.
/// - [`EngineError::NoFormat`] for format-free patches.
fn created_format(path: &str, patches: &[&StoredPatch]) -> mlua::Result<StructuredFormat> {
    let mut format: Option<StructuredFormat> = None;
    for item in patches {
        match (format, item.format) {
            (None, Some(next)) => format = Some(next),
            (Some(current), Some(next)) if current != next => {
                return Err(EngineError::Mismatch {
                    target: path.to_owned(),
                }
                .into());
            }
            _ => {}
        }
    }
    format.ok_or_else(|| {
        EngineError::NoFormat {
            target: path.to_owned(),
        }
        .into()
    })
}

/// Builds execution patches from sorted handles.
fn exec_list(refs: &[&StoredPatch]) -> Vec<ExecPatch> {
    refs.iter()
        .map(|item| ExecPatch {
            owner: item.owner.clone(),
            target: item.target.clone(),
            priority: item.priority,
            callback: item.callback.clone(),
        })
        .collect()
}

/// Assembles text, link, opaque, and tree documents in route order.
///
/// # Errors
///
/// - [`EngineError::Duplicate`] for repeated declarations.
/// - [`EngineError::NestScope`] for destination builds.
fn assemble_text_link(
    declared: &ProfileDeclared,
    configs: &[ConfigData],
    scope: &Scope,
) -> Result<(Vec<Document>, BTreeMap<String, BlobRef>)> {
    let mut grouped: BTreeMap<String, Vec<(Data, String)>> = BTreeMap::new();
    let mut blobs: BTreeMap<String, BlobRef> = BTreeMap::new();
    for item in &declared.texts {
        grouped
            .entry(item.destination.display())
            .or_default()
            .push((
                Data::Text {
                    content: item.content.clone(),
                    mode: item.mode,
                    unmanaged: item.unmanaged,
                },
                "profile".to_string(),
            ));
    }
    for item in &declared.links {
        grouped
            .entry(item.destination.display())
            .or_default()
            .push((
                Data::Link {
                    target: item.target.clone(),
                },
                "profile".to_string(),
            ));
    }
    for item in &declared.opaques {
        collect_blob(&item.blob, &mut blobs);
        grouped
            .entry(item.destination.display())
            .or_default()
            .push((
                Data::Opaque {
                    blob: item.blob.to_ref(),
                    size: item.size,
                    mode: item.mode,
                    unmanaged: item.unmanaged,
                },
                "profile".to_string(),
            ));
    }
    for item in &declared.trees {
        let members = collect_tree(&item.members, &mut blobs);
        grouped
            .entry(item.destination.display())
            .or_default()
            .push((Data::Tree { members }, "profile".to_string()));
    }
    for config in configs {
        for item in &config.texts {
            grouped
                .entry(item.destination.display())
                .or_default()
                .push((
                    Data::Text {
                        content: item.content.clone(),
                        mode: item.mode,
                        unmanaged: item.unmanaged,
                    },
                    config.name.clone(),
                ));
        }
        for item in &config.links {
            grouped
                .entry(item.destination.display())
                .or_default()
                .push((
                    Data::Link {
                        target: item.target.clone(),
                    },
                    config.name.clone(),
                ));
        }
        for item in &config.opaques {
            collect_blob(&item.blob, &mut blobs);
            grouped
                .entry(item.destination.display())
                .or_default()
                .push((
                    Data::Opaque {
                        blob: item.blob.to_ref(),
                        size: item.size,
                        mode: item.mode,
                        unmanaged: item.unmanaged,
                    },
                    config.name.clone(),
                ));
        }
        for item in &config.trees {
            let members = collect_tree(&item.members, &mut blobs);
            grouped
                .entry(item.destination.display())
                .or_default()
                .push((Data::Tree { members }, config.name.clone()));
        }
    }
    let mut out = Vec::with_capacity(grouped.len());
    for (display, items) in &grouped {
        let Some(((data, first), rest)) = items.split_first() else {
            continue;
        };
        if let Some((_, second)) = rest.first() {
            return Err(EngineError::Duplicate {
                scope: scope.clone(),
                item: display.clone(),
                first: first.clone(),
                second: second.clone(),
            });
        }
        let destination = Route::parse(display).map_err(|error| EngineError::NestScope {
            scope: scope.clone(),
            reason: error.to_string(),
        })?;
        out.push(Document::new(destination, data.clone()));
    }
    Ok((out, blobs))
}

/// Collects one blob handle under its content hash.
fn collect_blob(handle: &BlobHandle, blobs: &mut BTreeMap<String, BlobRef>) {
    blobs
        .entry(handle.sha().hex())
        .or_insert_with(|| handle.to_ref());
}

/// Collects tree members into manifest order with blob refs.
fn collect_tree(
    members: &[crate::model::TreeMemberDecl],
    blobs: &mut BTreeMap<String, BlobRef>,
) -> Vec<confit_model::document::ManifestMember> {
    let mut out = Vec::with_capacity(members.len());
    for member in members {
        collect_blob(&member.blob, blobs);
        out.push(confit_model::document::ManifestMember {
            relative: member.rel.clone(),
            blob: member.blob.to_ref(),
            mode: member.mode,
        });
    }
    out
}

impl Session {
    /// Assembles one rc document per shell.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Duplicate`] for repeated rc bases.
    fn assemble_rc(
        &self,
        profile: &Profile,
        patches: &[StoredPatch],
        scope: &Scope,
    ) -> mlua::Result<Vec<Document>> {
        let mut handles: Vec<&StoredPatch> =
            patches.iter().filter(|item| item.target == "rc").collect();
        let mut base: Option<(&str, &Vec<crate::model::RcEntryDecl>)> = None;
        if let Some(entries) = profile.declared.rc_base.as_ref() {
            base = Some(("profile", entries));
        }
        for config in &profile.configs {
            if let Some(entries) = config.rc_base.as_ref() {
                if base.is_some() {
                    let (first, _) = base.unwrap_or(("profile", entries));
                    return Err(EngineError::Duplicate {
                        scope: scope.clone(),
                        item: "rc".to_owned(),
                        first: first.to_owned(),
                        second: config.name.clone(),
                    }
                    .into());
                }
                base = Some((config.name.as_str(), entries));
            }
        }
        if base.is_none() && handles.is_empty() {
            return Ok(Vec::new());
        }
        let exec = Executor {
            lua: &self.lua,
            progress: self.progress.clone(),
            patch_total: patches.len(),
            patch_done: &self.patch_done,
        };
        Executor::sort_patches(&mut handles);
        let doc = self.lua.create_table()?;
        for section in ["profile", "config", "final"] {
            doc.set(section, self.lua.create_table()?)?;
        }
        let owners = std::rc::Rc::new(std::cell::RefCell::new(OwnerMap::new()));
        if let Some((owner, entries)) = base {
            let base_scope = if owner == "profile" {
                scope.clone()
            } else {
                Scope::config(owner)
            };
            for entry in entries {
                exec.rc_insert(
                    &doc,
                    &entry.json,
                    &entry.section,
                    owner,
                    &owners,
                    &base_scope,
                )?;
            }
        }
        let seeds = owners.borrow().clone();
        exec.execute(doc.clone(), Area::Rc, seeds, exec_list(&handles))?;
        let data = convert_live_rc(&doc)?;
        let mut out = Vec::with_capacity(profile.shells.len());
        for shell in &profile.shells {
            let mut per_shell = data.clone();
            materialize_shell(&mut per_shell.profile, shell)?;
            materialize_shell(&mut per_shell.config, shell)?;
            materialize_shell(&mut per_shell.final_entries, shell)?;
            out.push(Document::new(
                shell_route(shell).map_err(EngineError::from)?,
                Data::Rc(per_shell),
            ));
        }
        Ok(out)
    }
}

/// Converts one final live rc table into core data.
///
/// # Errors
///
/// - [`EngineError::SectionLeaf`] for misshaped sections.
fn convert_live_rc(doc: &Table) -> mlua::Result<RcData> {
    const PATCH: &str = "confit.patch.rc";
    let scope = Scope::method(PATCH);
    let mut profile: Vec<RcEntry> = Vec::new();
    let mut config: Vec<RcEntry> = Vec::new();
    let mut finals: Vec<RcEntry> = Vec::new();
    for section in ["profile", "config", "final"] {
        let list: Value = doc.get(section)?;
        let table = match list {
            Value::Nil => continue,
            Value::Table(table) => table,
            _ => {
                return Err(EngineError::SectionLeaf {
                    scope: scope.clone(),
                    section: section.to_owned(),
                    want: "holds a non-list leaf",
                }
                .into());
            }
        };
        for index in 1..=table.raw_len() {
            let item: Value = table.get(index)?;
            let entry = match item {
                Value::Table(entry) => entry,
                _ => {
                    return Err(EngineError::SectionLeaf {
                        scope: scope.clone(),
                        section: section.to_owned(),
                        want: "holds a non-table entry",
                    }
                    .into());
                }
            };
            let json = translate_entry(&entry, &scope.slot(FieldRef::name("convert")))?;
            crate::surface::document::convert::push_live_entry(
                &mut profile,
                &mut config,
                &mut finals,
                section,
                &json,
                &scope,
            )?;
        }
    }
    Ok(RcData {
        profile,
        config,
        final_entries: finals,
    })
}

/// Derives the rc destination route for one shell name.
///
/// # Errors
///
/// - [`confit_model::error::Error::Parse`] for bad shell names.
fn shell_route(shell: &str) -> confit_model::error::Result<Route> {
    let relative = match shell {
        "bash" => ".bashrc".to_string(),
        "zsh" => ".zshrc".to_string(),
        other => format!(".{other}rc"),
    };
    Route::new(RouteBase::Home, relative)
}

/// Derives the rc path display for one shell name.
fn shell_path(shell: &str) -> String {
    match shell_route(shell) {
        Ok(route) => route.display(),
        Err(_) => format!("home:.{shell}rc"),
    }
}

/// Materializes init entries for one shell name.
fn materialize_shell(entries: &mut [RcEntry], shell: &str) -> Result<()> {
    let mut facts = BTreeMap::new();
    facts.insert("shell".to_string(), Json::String(shell.to_string()));
    for entry in entries {
        match &mut entry.op {
            RcOp::Eval { argv, .. } | RcOp::Cmd { argv, .. } => {
                for arg in argv {
                    let Arg::Text(text) = arg else {
                        continue;
                    };
                    *text = render_init(text, &facts, shell)?;
                }
            }
            RcOp::Source { .. } | RcOp::Env { .. } | RcOp::Path { .. } | RcOp::Alias { .. } => {}
        }
    }
    Ok(())
}

/// Renders one init string with the shell facts.
///
/// # Errors
///
/// - [`EngineError::NestScope`] for template failures.
fn render_init(text: &str, facts: &BTreeMap<String, Json>, shell: &str) -> Result<String> {
    let scope = Scope::Call {
        method: "confit.document.rc",
        target: shell_path(shell),
    };
    utils::render(text, facts, &scope).map_err(wrap)
}
