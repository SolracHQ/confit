//! Eval
//!
//! Profile loading and document assembly.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mlua::{Lua, LuaOptions, StdLib, Table, Value};
use serde_json::Value as Json;
use sha2::{Digest as _, Sha256};

use crate::EvalOpts;
use crate::error::{find_plan, plan, plan_error};
use crate::exec::{Area, ExecPatch, Executor, OwnerMap};
use crate::fetch::Fetch;
use crate::lua::{JsonExt, TableExt};
use crate::model::{ConfigData, StoredPatch};
use crate::path_expr::flatten_json;
use crate::require::Requirer;
use crate::surface::config::ConfigBuilder;
use crate::surface::document::Declared;
use crate::surface::utils;
use confit_core::document::ManifestDocument;
use confit_core::document::{
    ManifestData, ManifestMember, RcData, RcEntry, RcOp, StructuredFormat,
};
use confit_core::error::{Error, Result};
use confit_core::hook::{Hook, merge_hooks};
use confit_core::ids::DocPath;
use confit_core::progress::{Event, ProgressSender};
use confit_core::store::blobs::BlobRef;

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
    /// Fetch sidecar cache folder.
    pub(crate) cache: PathBuf,
    /// Archive extract folder.
    pub(crate) extract: PathBuf,
    /// Network source, HTTP by default.
    pub(crate) fetcher: Arc<dyn Fetch>,
    /// External plugin folder.
    pub(crate) plugins: PathBuf,
    /// Forces remote downloads past the sidecar cache.
    pub(crate) re_fetch: bool,
    /// Progress sender, holding `None` for silence.
    pub(crate) progress: Option<ProgressSender>,
    /// Finished patch count shared across documents.
    pub(crate) patch_done: Cell<usize>,
}

impl Session {
    /// Runs one profile file into finished documents and hooks.
    pub(crate) fn run(profile: &Path, opts: EvalOpts) -> Result<crate::Evaluation> {
        let start = std::time::Instant::now();
        let root = resolve_root(profile, &opts.root);
        let cache = crate::fetch::resolve_cache_dir(opts.cache_dir.as_deref())?;
        let fetcher = match opts.fetcher {
            Some(source) => source,
            None => Arc::new(crate::fetch::HttpFetch),
        };
        let lua = Lua::new_with(
            StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE,
            LuaOptions::default(),
        )
        .map_err(|error| plan(format!("lua state: {error}")))?;
        let session = Self {
            lua,
            root,
            cache,
            extract: crate::surface::document::archive::extract_root(),
            fetcher,
            plugins: opts.plugins,
            re_fetch: opts.re_fetch,
            progress: opts.progress,
            patch_done: Cell::new(0),
        };
        let requirer = Requirer {
            current: session.root.clone(),
            folder: session.root.clone(),
            scope: "profile",
        }
        .make(&session.lua)
        .map_err(|error| plan(format!("require: {error}")))?;
        session
            .lua
            .globals()
            .set("require", requirer)
            .map_err(|error| plan(format!("require: {error}")))?;
        crate::surface::install(&session)?;
        let source = std::fs::read(profile)?;
        let profile_ctx = format!("profile '{}'", profile.display());
        let returned: Value = session
            .lua
            .load(&source)
            .set_name(format!("@{}", profile.display()))
            .call(())
            .map_err(wrap)?;
        let table = coerce_profile_table(returned).map_err(wrap)?;
        let profile = Profile::read(&table, &profile_ctx)?;
        profile.check(&profile_ctx)?;
        let patches = profile.patches();
        let total = patches.len();
        if total > 0
            && let Some(sender) = session.progress.as_ref()
        {
            let _ = sender.send(Event::PatchesStarted { patches: total });
        }
        let mut blobs: BTreeMap<String, BlobRef> = BTreeMap::new();
        let mut out = session
            .assemble_structured(&profile, &patches, &profile_ctx)
            .map_err(wrap)?;
        out.extend(profile.text_link(&profile_ctx, &mut blobs)?);
        out.extend(
            session
                .assemble_rc(&profile, &patches, &profile_ctx)
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

/// Maps one Lua failure onto the core error.
fn wrap(error: mlua::Error) -> Error {
    if let Some(message) = find_plan(&error) {
        return plan(message);
    }
    plan(error.to_string())
}

/// Rejects changed gates naming documents outside the built set.
fn validate_changed(hooks: &[Hook], documents: &[ManifestDocument]) -> mlua::Result<()> {
    const CTOR: &str = "confit.runtime.changed";
    let built: std::collections::BTreeSet<&str> = documents
        .iter()
        .map(|document| document.path.as_str())
        .collect();
    let mut paths = Vec::new();
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
        if !built.contains(path.as_str()) {
            return Err(crate::error::plan_error(format!(
                "{CTOR}: unknown document '{path}'"
            )));
        }
    }
    Ok(())
}

/// Coerces the profile return into a table.
fn coerce_profile_table(returned: Value) -> mlua::Result<Table> {
    const WHAT: &str = "profile must return a table with 'shells' and 'configs'";
    match returned {
        Value::Table(table) => Ok(table),
        Value::Function(func) => match func.call::<Value>(())? {
            Value::Table(table) => Ok(table),
            _ => Err(plan_error(WHAT.to_string())),
        },
        _ => Err(plan_error(WHAT.to_string())),
    }
}

/// Reads the profile shells field.
fn read_shells(profile: &Table, ctx: &str) -> Result<Vec<String>> {
    let raw: Value = profile.get("shells").map_err(|_| {
        plan(format!(
            "{ctx}: field 'shells' must be a non-empty string array"
        ))
    })?;
    let list = match raw {
        Value::Table(list) => list,
        _ => {
            return Err(plan(format!(
                "{ctx}: field 'shells' must be a non-empty string array"
            )));
        }
    };
    let shells = read_string_array(&list).ok_or_else(|| {
        plan(format!(
            "{ctx}: field 'shells' must be a non-empty string array"
        ))
    })?;
    if shells.is_empty() {
        return Err(plan(format!(
            "{ctx}: field 'shells' must be a non-empty string array"
        )));
    }
    let mut seen = std::collections::BTreeSet::new();
    for shell in &shells {
        if !seen.insert(shell.as_str()) {
            return Err(plan(format!(
                "{ctx}: field 'shells' declares '{shell}' more than once"
            )));
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
fn read_builders(profile: &Table, ctx: &str) -> Result<Vec<ConfigData>> {
    let raw: Value = profile.get("configs").map_err(|_| {
        plan(format!(
            "{ctx}: field 'configs' must be a non-empty array of configs"
        ))
    })?;
    let list = match raw {
        Value::Table(list) => list,
        _ => {
            return Err(plan(format!(
                "{ctx}: field 'configs' must be a non-empty array of configs"
            )));
        }
    };
    let len = list.raw_len();
    let mut out = Vec::with_capacity(len);
    for index in 1..=len {
        let item: Value = list
            .get(index)
            .map_err(|error| plan(format!("{ctx}: configs[{index}] unreadable: {error}")))?;
        match item {
            Value::UserData(handle) => match handle.borrow::<ConfigBuilder>() {
                Ok(builder) => out.push(builder.contribution().clone()),
                Err(_) => {
                    return Err(plan(format!(
                        "{ctx}: configs[{index}] must be a config (expected config userdata)"
                    )));
                }
            },
            _ => {
                return Err(plan(format!(
                    "{ctx}: configs[{index}] must be a config (expected config userdata)"
                )));
            }
        }
    }
    if out.is_empty() {
        return Err(plan(format!(
            "{ctx}: field 'configs' must be a non-empty array of configs"
        )));
    }
    Ok(out)
}

/// Reads profile-declared documents with profile ownership.
fn read_documents(profile: &Table, ctx: &str) -> mlua::Result<ProfileDeclared> {
    let raw: Value = profile.get("documents").map_err(|_| {
        plan_error(format!(
            "{ctx}: field 'documents' must be an array of documents"
        ))
    })?;
    let list = match raw {
        Value::Nil => return Ok(ProfileDeclared::default()),
        Value::Table(list) => list,
        _ => {
            return Err(plan_error(format!(
                "{ctx}: field 'documents' must be an array of documents"
            )));
        }
    };
    let len = list.raw_len();
    let mut out = ProfileDeclared::default();
    for index in 1..=len {
        let item: Value = list.get(index).map_err(|error| {
            plan_error(format!("{ctx}: documents[{index}] unreadable: {error}"))
        })?;
        let table = match item {
            Value::Table(table) => table,
            _ => {
                return Err(plan_error(format!(
                    "{ctx}: documents[{index}] must be a document (expected document table)"
                )));
            }
        };
        let item_ctx = format!("{ctx}: documents[{index}]");
        out.push(
            crate::surface::document::convert::convert_document(&table, &item_ctx)?,
            ctx,
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
impl ProfileDeclared {
    /// Pushes one declared document into the accumulator.
    fn push(&mut self, declared: Declared, ctx: &str) -> mlua::Result<()> {
        match declared {
            Declared::Structured(decl) => self.structured.push(decl),
            Declared::Text(decl) => self.texts.push(decl),
            Declared::Link(decl) => self.links.push(decl),
            Declared::Opaque(decl) => self.opaques.push(decl),
            Declared::Tree(decl) => self.trees.push(decl),
            Declared::Rc(entries) => {
                if self.rc_base.is_some() {
                    return Err(crate::error::plan_error(format!(
                        "{ctx}: document 'rc' is declared more than once ('profile' plus 'profile')"
                    )));
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
            let mut message = format!(
                "config \"{}\" requires \"{}\" config",
                config.name, edge.target
            );
            if let Some(hint) = edge.hint.as_ref() {
                message.push_str(&format!("\nhint: {hint}"));
            }
            return Err(plan(message));
        };
        walk_requires(edge.target.as_str(), present, visited)?;
    }
    Ok(())
}

impl Profile {
    /// Reads shells, documents, and configs from a profile table.
    fn read(table: &Table, ctx: &str) -> Result<Self> {
        Ok(Self {
            shells: read_shells(table, ctx)?,
            declared: read_documents(table, ctx).map_err(wrap)?,
            configs: read_builders(table, ctx)?,
        })
    }

    /// Rejects repeated declarations across profile and configs.
    fn check(&self, ctx: &str) -> Result<()> {
        let mut owners: BTreeMap<&str, &str> = BTreeMap::new();
        for item in &self.declared.structured {
            if let Some(first) = owners.insert(item.path.as_str(), "profile") {
                return Err(plan(format!(
                    "{ctx}: document '{}' is declared more than once ('{first}' plus 'profile')",
                    item.path
                )));
            }
        }
        for config in &self.configs {
            for item in &config.structured {
                if let Some(first) = owners.insert(item.path.as_str(), config.name.as_str()) {
                    return Err(plan(format!(
                        "{ctx}: document '{}' is declared more than once ('{first}' plus '{}')",
                        item.path, config.name
                    )));
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
                        return Err(plan(format!(
                            "{ctx}: document 'rc' is declared more than once ('{owner}' plus '{}')",
                            config.name
                        )));
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

    /// Assembles text, link, and opaque documents in path order.
    fn text_link(
        &self,
        ctx: &str,
        blobs: &mut BTreeMap<String, BlobRef>,
    ) -> Result<Vec<ManifestDocument>> {
        assemble_text_link(&self.declared, &self.configs, ctx, blobs)
    }
}

impl Session {
    /// Assembles structured documents in path order.
    fn assemble_structured(
        &self,
        profile: &Profile,
        patches: &[StoredPatch],
        ctx: &str,
    ) -> mlua::Result<Vec<ManifestDocument>> {
        let mut bases: BTreeMap<String, (StructuredFormat, BTreeMap<String, Json>, String)> =
            BTreeMap::new();
        for item in &profile.declared.structured {
            bases.insert(
                item.path.clone(),
                (item.format, item.data.clone(), "profile".to_string()),
            );
        }
        for config in &profile.configs {
            for item in &config.structured {
                bases.insert(
                    item.path.clone(),
                    (item.format, item.data.clone(), config.name.clone()),
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
        let mut out: BTreeMap<String, ManifestDocument> = BTreeMap::new();
        for (path, (format, base, owner)) in &bases {
            let mut refs: Vec<&StoredPatch> = grouped.get(path).cloned().unwrap_or_default();
            Executor::sort_patches(&mut refs);
            for patch in &refs {
                if let Some(other) = patch.format
                    && other != *format
                {
                    let patch_ctx = format!("confit.patch.structured('{path}')");
                    return Err(plan_error(format!(
                        "{patch_ctx}: cannot merge document at '{path}': format mismatch"
                    )));
                }
            }
            let doc = self.lua.create_table()?;
            for (key, value) in base {
                let seed_ctx = if owner == "profile" {
                    format!("{ctx}: field '{key}'")
                } else {
                    format!("config '{owner}': field '{key}'")
                };
                doc.set(key.as_str(), value.to_lua(&self.lua, &seed_ctx)?)?;
            }
            let mut seeds: OwnerMap = BTreeMap::new();
            seed_owners(base, owner, &mut seeds);
            let area = Area::Structured {
                format: format.name().to_string(),
            };
            exec.execute(doc.clone(), area, seeds, exec_list(&refs))?;
            let table = live_to_map(&doc, path)?;
            out.insert(path.clone(), finish_structured(path, *format, table));
        }
        for (path, items) in &grouped {
            if bases.contains_key(path) {
                continue;
            }
            let mut refs = items.clone();
            Executor::sort_patches(&mut refs);
            let format = created_format(path, &refs)?;
            let doc = self.lua.create_table()?;
            let area = Area::Structured {
                format: format.name().to_string(),
            };
            exec.execute(doc.clone(), area, OwnerMap::new(), exec_list(&refs))?;
            let table = live_to_map(&doc, path)?;
            out.insert(path.clone(), finish_structured(path, format, table));
        }
        Ok(out.into_values().collect())
    }
}

/// Builds one finished structured document.
fn finish_structured(
    path: &str,
    format: StructuredFormat,
    table: BTreeMap<String, Json>,
) -> ManifestDocument {
    ManifestDocument::new(
        DocPath::new(path),
        ManifestData::Structured {
            format,
            data: table,
        },
    )
}

/// Converts one live table into a data map.
fn live_to_map(doc: &Table, path: &str) -> mlua::Result<BTreeMap<String, Json>> {
    let patch = format!("confit.patch.structured('{path}')");
    match doc.to_json(&format!("{patch}: convert"))? {
        Json::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(plan_error(format!(
            "{patch}: patch for '{path}' holds no object"
        ))),
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
fn created_format(path: &str, patches: &[&StoredPatch]) -> mlua::Result<StructuredFormat> {
    let patch_ctx = format!("confit.patch.structured('{path}')");
    let mut format: Option<StructuredFormat> = None;
    for item in patches {
        match (format, item.format) {
            (None, Some(next)) => format = Some(next),
            (Some(current), Some(next)) if current != next => {
                return Err(plan_error(format!(
                    "{patch_ctx}: cannot merge document at '{path}': format mismatch"
                )));
            }
            _ => {}
        }
    }
    format.ok_or_else(|| {
        plan_error(format!(
            "{patch_ctx}: patch for '{path}' holds no format (structured patches name one)"
        ))
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

/// Assembles text, link, and opaque documents in path order.
fn assemble_text_link(
    declared: &ProfileDeclared,
    configs: &[ConfigData],
    ctx: &str,
    blobs: &mut BTreeMap<String, BlobRef>,
) -> Result<Vec<ManifestDocument>> {
    let mut grouped: BTreeMap<String, Vec<(ManifestData, String)>> = BTreeMap::new();
    for item in &declared.texts {
        grouped.entry(item.path.clone()).or_default().push((
            ManifestData::Text {
                content: item.content.clone(),
                mode: item.mode,
                unmanaged: item.unmanaged,
            },
            "profile".to_string(),
        ));
    }
    for item in &declared.links {
        grouped.entry(item.path.clone()).or_default().push((
            ManifestData::Link {
                target: item.target.clone(),
            },
            "profile".to_string(),
        ));
    }
    for item in &declared.opaques {
        grouped.entry(item.path.clone()).or_default().push((
            resolve_opaque(&item.source, item.mode, item.unmanaged, blobs)?,
            "profile".to_string(),
        ));
    }
    for item in &declared.trees {
        grouped
            .entry(item.path.clone())
            .or_default()
            .push((resolve_tree(&item.members, blobs)?, "profile".to_string()));
    }
    for config in configs {
        for item in &config.texts {
            grouped.entry(item.path.clone()).or_default().push((
                ManifestData::Text {
                    content: item.content.clone(),
                    mode: item.mode,
                    unmanaged: item.unmanaged,
                },
                config.name.clone(),
            ));
        }
        for item in &config.links {
            grouped.entry(item.path.clone()).or_default().push((
                ManifestData::Link {
                    target: item.target.clone(),
                },
                config.name.clone(),
            ));
        }
        for item in &config.opaques {
            grouped.entry(item.path.clone()).or_default().push((
                resolve_opaque(&item.source, item.mode, item.unmanaged, blobs)?,
                config.name.clone(),
            ));
        }
        for item in &config.trees {
            grouped
                .entry(item.path.clone())
                .or_default()
                .push((resolve_tree(&item.members, blobs)?, config.name.clone()));
        }
    }
    let mut out = Vec::with_capacity(grouped.len());
    for (path, items) in &grouped {
        let Some(((data, first), rest)) = items.split_first() else {
            continue;
        };
        if let Some((_, second)) = rest.first() {
            return Err(plan(format!(
                "{ctx}: document '{path}' is declared more than once ('{first}' plus '{second}'): declare once, patch to modify"
            )));
        }
        out.push(ManifestDocument::new(DocPath::new(path), data.clone()));
    }
    Ok(out)
}

/// Chunk size for streaming source files into the content hash.
const HASH_CHUNK: usize = 8 * 1024;

/// Builds one opaque payload streaming its source file.
///
/// The unmanaged flag rides beside the blob, outside the data
/// hash, so toggling it with identical bytes shows no update line.
///
/// # Errors
///
/// Missing and unreadable sources fail as io errors.
fn resolve_opaque(
    source: &Path,
    mode: Option<u32>,
    unmanaged: bool,
    blobs: &mut BTreeMap<String, BlobRef>,
) -> Result<ManifestData> {
    let (blob, size) = hash_source(source)?;
    blobs.entry(blob.clone()).or_insert_with(|| BlobRef {
        sha: blob.clone(),
        size,
        path: source.to_path_buf(),
    });
    Ok(ManifestData::Opaque {
        blob,
        size,
        mode,
        unmanaged,
    })
}

/// Streams one source file into its content hash and metadata size.
///
/// # Errors
///
/// Missing and unreadable sources fail as io errors.
fn hash_source(source: &Path) -> Result<(String, u64)> {
    let mut file = std::fs::File::open(source)?;
    let size = file.metadata()?.len();
    let mut hash = Sha256::new();
    let mut chunk = [0u8; HASH_CHUNK];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        hash.update(&chunk[..read]);
    }
    let blob = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((blob, size))
}

/// Builds one tree payload streaming member files.
///
/// Member hashes cover the extracted files, uniform with
/// opaque source handling.
///
/// # Errors
///
/// Missing and unreadable member files fail as io errors.
fn resolve_tree(
    members: &[crate::model::TreeMemberDecl],
    blobs: &mut BTreeMap<String, BlobRef>,
) -> Result<ManifestData> {
    let mut out = Vec::with_capacity(members.len());
    for member in members {
        let (blob, size) = hash_source(&member.source)?;
        blobs.entry(blob.clone()).or_insert_with(|| BlobRef {
            sha: blob.clone(),
            size,
            path: member.source.clone(),
        });
        out.push(ManifestMember {
            relative: member.rel.clone(),
            blob,
            size,
            mode: member.mode,
        });
    }
    Ok(ManifestData::Tree { members: out })
}

impl Session {
    /// Assembles one rc document per shell.
    fn assemble_rc(
        &self,
        profile: &Profile,
        patches: &[StoredPatch],
        ctx: &str,
    ) -> mlua::Result<Vec<ManifestDocument>> {
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
                    return Err(crate::error::plan_error(format!(
                        "{ctx}: document 'rc' is declared more than once ('{first}' plus '{}')",
                        config.name
                    )));
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
            let base_ctx = if owner == "profile" {
                ctx.to_string()
            } else {
                format!("config '{owner}'")
            };
            for entry in entries {
                exec.rc_insert(&doc, &entry.json, &entry.section, owner, &owners, &base_ctx)?;
            }
        }
        let seeds = owners.borrow().clone();
        exec.execute(doc.clone(), Area::Rc, seeds, exec_list(&handles))?;
        let data = convert_live_rc(&doc)?;
        let mut out = Vec::with_capacity(profile.shells.len());
        for shell in &profile.shells {
            let mut per_shell = data.clone();
            materialize_shell(&mut per_shell.profile, shell)
                .map_err(|error| plan_error(error.to_string()))?;
            materialize_shell(&mut per_shell.config, shell)
                .map_err(|error| plan_error(error.to_string()))?;
            materialize_shell(&mut per_shell.final_entries, shell)
                .map_err(|error| plan_error(error.to_string()))?;
            out.push(ManifestDocument::new(
                DocPath::new(shell_path(shell)),
                ManifestData::Rc(per_shell),
            ));
        }
        Ok(out)
    }
}

/// Converts one final live rc table into core data.
fn convert_live_rc(doc: &Table) -> mlua::Result<RcData> {
    const PATCH: &str = "confit.patch.rc";
    let mut profile: Vec<RcEntry> = Vec::new();
    let mut config: Vec<RcEntry> = Vec::new();
    let mut finals: Vec<RcEntry> = Vec::new();
    for section in ["profile", "config", "final"] {
        let list: Value = doc.get(section)?;
        let table = match list {
            Value::Nil => continue,
            Value::Table(table) => table,
            _ => {
                return Err(plan_error(format!(
                    "{PATCH}: section '{section}' holds a non-list leaf"
                )));
            }
        };
        for index in 1..=table.raw_len() {
            let item: Value = table.get(index)?;
            let entry = match item {
                Value::Table(entry) => entry,
                _ => {
                    return Err(plan_error(format!(
                        "{PATCH}: section '{section}' holds a non-table entry"
                    )));
                }
            };
            let json = entry.to_json(&format!("{PATCH}: convert"))?;
            crate::surface::document::convert::push_live_entry(
                &mut profile,
                &mut config,
                &mut finals,
                section,
                &json,
                PATCH,
            )?;
        }
    }
    Ok(RcData {
        profile,
        config,
        final_entries: finals,
    })
}

/// Derives the rc path for one shell name.
fn shell_path(shell: &str) -> String {
    match shell {
        "bash" => "~/.bashrc".to_string(),
        "zsh" => "~/.zshrc".to_string(),
        other => format!("~/.{other}rc"),
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
                    *arg = render_init(arg, &facts, shell)?;
                }
            }
            RcOp::Source { path, .. } => {
                *path = render_init(path, &facts, shell)?;
            }
            RcOp::Env { .. } | RcOp::Path { .. } | RcOp::Alias { .. } => {}
        }
    }
    Ok(())
}

/// Renders one init string with the shell facts.
fn render_init(text: &str, facts: &BTreeMap<String, Json>, shell: &str) -> Result<String> {
    let prefix = format!("confit.document.rc '{}': ", shell_path(shell));
    utils::render(text, facts, &prefix).map_err(wrap)
}
