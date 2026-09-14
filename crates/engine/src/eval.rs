//! Eval
//!
//! Profile loading plus document assembly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mlua::{Lua, Table, Value};
use serde_json::Value as Json;

use crate::EvalOpts;
use crate::exec::{self, Area, ExecPatch, OwnerMap};
use crate::model::{ConfigData, StoredPatch};
use crate::surface::config::ConfigBuilder;
use crate::surface::document::{self, Declared};
use crate::surface::text;
use crate::values::{find_plan, flatten_json, json_to_lua, plan_error, table_to_json};
use confit_core::document::Document;
use confit_core::document::{DocumentData, RcData, RcEntry, RcOp, StructuredFormat};
use confit_core::error::{Error, Result};
use confit_core::ids::DocPath;

/// Runs one profile file into finished documents.
pub(crate) fn run(profile: &Path, opts: EvalOpts) -> Result<Vec<Document>> {
    let root = resolve_root(profile, &opts.root);
    let source = std::fs::read(profile)?;
    let lua = Lua::new();
    prepend_module_path(&lua, &root).map_err(wrap)?;
    crate::surface::install(&lua, &root, opts.plugins.as_deref())?;
    let returned: Value = lua
        .load(&source)
        .set_name(format!("@{}", profile.display()))
        .call(())
        .map_err(wrap)?;
    let table = coerce_profile_table(returned).map_err(wrap)?;
    let shells = read_shells(&table)?;
    let declared = read_documents(&table).map_err(wrap)?;
    let configs = read_builders(&table)?;
    check_structured_repeats(&declared, &configs)?;
    let patches = collect_patches(&configs);
    let mut out = assemble_structured(&lua, &declared, &configs, &patches).map_err(wrap)?;
    out.extend(assemble_text_link(&declared, &configs)?);
    out.extend(assemble_rc(&lua, &shells, &declared, &configs, &patches).map_err(wrap)?);
    Ok(out)
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
        return Error::Plan(message);
    }
    Error::Plan(error.to_string())
}

/// Extends module resolution with the project root.
fn prepend_module_path(lua: &Lua, root: &Path) -> mlua::Result<()> {
    let package: Table = lua.globals().get("package")?;
    let previous: String = package.get("path")?;
    let root = root.display();
    package.set("path", format!("{root}/?.lua;{root}/?/init.lua;{previous}"))?;
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
fn read_shells(profile: &Table) -> Result<Vec<String>> {
    const WHAT: &str = "profile: field 'shells' must be a non-empty string array";
    let raw: Value = profile
        .get("shells")
        .map_err(|_| Error::Plan(WHAT.to_string()))?;
    let list = match raw {
        Value::Table(list) => list,
        _ => return Err(Error::Plan(WHAT.to_string())),
    };
    let shells = read_string_array(&list).ok_or_else(|| Error::Plan(WHAT.to_string()))?;
    if shells.is_empty() {
        return Err(Error::Plan(WHAT.to_string()));
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
fn read_builders(profile: &Table) -> Result<Vec<ConfigData>> {
    const WHAT: &str = "profile: field 'configs' must be a non-empty array of configs";
    let raw: Value = profile
        .get("configs")
        .map_err(|_| Error::Plan(WHAT.to_string()))?;
    let list = match raw {
        Value::Table(list) => list,
        _ => return Err(Error::Plan(WHAT.to_string())),
    };
    let len = list.raw_len();
    let mut out = Vec::with_capacity(len);
    for index in 1..=len {
        let item: Value = list.get(index).map_err(|error| {
            Error::Plan(format!("profile: configs[{index}] unreadable: {error}"))
        })?;
        match item {
            Value::UserData(handle) => match handle.borrow::<ConfigBuilder>() {
                Ok(builder) => out.push(builder.contribution().clone()),
                Err(_) => {
                    return Err(Error::Plan(format!(
                        "profile: configs[{index}] must be a config (expected config userdata)"
                    )));
                }
            },
            _ => {
                return Err(Error::Plan(format!(
                    "profile: configs[{index}] must be a config (expected config userdata)"
                )));
            }
        }
    }
    if out.is_empty() {
        return Err(Error::Plan(WHAT.to_string()));
    }
    Ok(out)
}

/// Reads profile-declared documents with profile ownership.
fn read_documents(profile: &Table) -> mlua::Result<ProfileDeclared> {
    const WHAT: &str = "profile: field 'documents' must be an array of documents";
    let raw: Value = profile
        .get("documents")
        .map_err(|_| plan_error(WHAT.to_string()))?;
    let list = match raw {
        Value::Nil => return Ok(ProfileDeclared::default()),
        Value::Table(list) => list,
        _ => return Err(plan_error(WHAT.to_string())),
    };
    let len = list.raw_len();
    let mut out = ProfileDeclared::default();
    for index in 1..=len {
        let item: Value = list.get(index).map_err(|error| {
            plan_error(format!("profile: documents[{index}] unreadable: {error}"))
        })?;
        let table = match item {
            Value::Table(table) => table,
            _ => {
                return Err(plan_error(format!(
                    "profile: documents[{index}] must be a document (expected document table)"
                )));
            }
        };
        let ctx = format!("profile: documents[{index}]");
        out.push(document::convert_document(&table, &ctx)?);
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
    /// Rc entries in profile order.
    rc: Vec<crate::model::RcEntryDecl>,
}

/// Pushes one declared document into the profile accumulator.
impl ProfileDeclared {
    /// Pushes one declared document into the accumulator.
    fn push(&mut self, declared: Declared) {
        match declared {
            Declared::Structured(decl) => self.structured.push(decl),
            Declared::Text(decl) => self.texts.push(decl),
            Declared::Link(decl) => self.links.push(decl),
            Declared::RcEntries(entries) => self.rc.extend(entries),
        }
    }
}

/// Rejects repeated structured declarations across profile plus configs.
fn check_structured_repeats(declared: &ProfileDeclared, configs: &[ConfigData]) -> Result<()> {
    let mut owners: BTreeMap<&str, &str> = BTreeMap::new();
    for item in &declared.structured {
        if let Some(first) = owners.insert(item.path.as_str(), "profile") {
            return Err(Error::Plan(format!(
                "profile: document '{}' is declared more than once ('{first}' plus 'profile')",
                item.path
            )));
        }
    }
    for config in configs {
        for item in &config.structured {
            if let Some(first) = owners.insert(item.path.as_str(), config.name.as_str()) {
                return Err(Error::Plan(format!(
                    "profile: document '{}' is declared more than once ('{first}' plus '{}')",
                    item.path, config.name
                )));
            }
        }
    }
    Ok(())
}

/// Collects patch handles with owners in registration order.
fn collect_patches(configs: &[ConfigData]) -> Vec<StoredPatch> {
    let mut out = Vec::new();
    for config in configs {
        out.extend(config.patches.iter().cloned());
    }
    out
}

/// Assembles structured documents in path order.
fn assemble_structured(
    lua: &Lua,
    declared: &ProfileDeclared,
    configs: &[ConfigData],
    patches: &[StoredPatch],
) -> mlua::Result<Vec<Document>> {
    let mut bases: BTreeMap<String, (StructuredFormat, BTreeMap<String, Json>, String)> =
        BTreeMap::new();
    for item in &declared.structured {
        bases.insert(
            item.path.clone(),
            (item.format, item.data.clone(), "profile".to_string()),
        );
    }
    for config in configs {
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
    let mut out: BTreeMap<String, Document> = BTreeMap::new();
    for (path, (format, base, owner)) in &bases {
        let mut refs: Vec<&StoredPatch> = grouped.get(path).cloned().unwrap_or_default();
        crate::exec::sort_patches(&mut refs);
        for patch in &refs {
            if let Some(other) = patch.format
                && other != *format
            {
                return Err(plan_error(format!(
                    "cannot merge document at '{path}': format mismatch"
                )));
            }
        }
        let doc = lua.create_table()?;
        for (key, value) in base {
            doc.set(key.as_str(), json_to_lua(lua, value)?)?;
        }
        let mut seeds: OwnerMap = BTreeMap::new();
        seed_owners(base, owner, &mut seeds);
        let area = Area::Structured {
            format: format.name().to_string(),
        };
        exec::execute(lua, doc.clone(), area, seeds, exec_list(&refs))?;
        let table = live_to_map(&doc, path)?;
        out.insert(path.clone(), finish_structured(path, *format, table));
    }
    for (path, items) in &grouped {
        if bases.contains_key(path) {
            continue;
        }
        let mut refs = items.clone();
        crate::exec::sort_patches(&mut refs);
        let format = created_format(path, &refs)?;
        let doc = lua.create_table()?;
        let area = Area::Structured {
            format: format.name().to_string(),
        };
        exec::execute(lua, doc.clone(), area, OwnerMap::new(), exec_list(&refs))?;
        let table = live_to_map(&doc, path)?;
        out.insert(path.clone(), finish_structured(path, format, table));
    }
    Ok(out.into_values().collect())
}

/// Builds one finished structured document.
fn finish_structured(
    path: &str,
    format: StructuredFormat,
    table: BTreeMap<String, Json>,
) -> Document {
    Document::new(
        DocPath::new(path),
        DocumentData::Structured {
            format,
            data: table,
        },
    )
}

/// Converts one live table into a data map.
fn live_to_map(doc: &Table, path: &str) -> mlua::Result<BTreeMap<String, Json>> {
    match table_to_json(doc, "patch: convert")? {
        Json::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(plan_error(format!("patch for '{path}' holds no object"))),
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
    let mut format: Option<StructuredFormat> = None;
    for patch in patches {
        match (format, patch.format) {
            (None, Some(next)) => format = Some(next),
            (Some(current), Some(next)) if current != next => {
                return Err(plan_error(format!(
                    "cannot merge document at '{path}': format mismatch"
                )));
            }
            _ => {}
        }
    }
    format.ok_or_else(|| {
        plan_error(format!(
            "patch for '{path}' holds no format (structured patches name one)"
        ))
    })
}

/// Builds execution patches from sorted handles.
fn exec_list(refs: &[&StoredPatch]) -> Vec<ExecPatch> {
    refs.iter()
        .map(|item| ExecPatch {
            owner: item.owner.clone(),
            callback: item.callback.clone(),
        })
        .collect()
}

/// Assembles text plus link documents in path order.
fn assemble_text_link(declared: &ProfileDeclared, configs: &[ConfigData]) -> Result<Vec<Document>> {
    let mut grouped: BTreeMap<String, Vec<(DocumentData, String)>> = BTreeMap::new();
    for item in &declared.texts {
        grouped.entry(item.path.clone()).or_default().push((
            DocumentData::Text {
                content: item.content.clone(),
            },
            "profile".to_string(),
        ));
    }
    for item in &declared.links {
        grouped.entry(item.path.clone()).or_default().push((
            DocumentData::Link {
                target: item.target.clone(),
            },
            "profile".to_string(),
        ));
    }
    for config in configs {
        for item in &config.texts {
            grouped.entry(item.path.clone()).or_default().push((
                DocumentData::Text {
                    content: item.content.clone(),
                },
                config.name.clone(),
            ));
        }
        for item in &config.links {
            grouped.entry(item.path.clone()).or_default().push((
                DocumentData::Link {
                    target: item.target.clone(),
                },
                config.name.clone(),
            ));
        }
    }
    let mut out = Vec::with_capacity(grouped.len());
    for (path, items) in &grouped {
        let Some(((data, first), rest)) = items.split_first() else {
            continue;
        };
        if let Some((_, second)) = rest.first() {
            return Err(Error::Plan(format!(
                "profile: document '{path}' is declared more than once ('{first}' plus '{second}'): declare once, patch to modify"
            )));
        }
        out.push(Document::new(DocPath::new(path), data.clone()));
    }
    Ok(out)
}

/// Assembles one rc document per shell.
fn assemble_rc(
    lua: &Lua,
    shells: &[String],
    declared: &ProfileDeclared,
    configs: &[ConfigData],
    patches: &[StoredPatch],
) -> mlua::Result<Vec<Document>> {
    let mut handles: Vec<&StoredPatch> =
        patches.iter().filter(|item| item.target == "rc").collect();
    let empty_configs = configs.iter().all(|config| config.rc.is_empty());
    if declared.rc.is_empty() && empty_configs && handles.is_empty() {
        return Ok(Vec::new());
    }
    crate::exec::sort_patches(&mut handles);
    let doc = lua.create_table()?;
    for section in ["profile", "config", "final"] {
        doc.set(section, lua.create_table()?)?;
    }
    let owners = std::rc::Rc::new(std::cell::RefCell::new(OwnerMap::new()));
    for entry in &declared.rc {
        exec::rc_insert(lua, &doc, &entry.json, &entry.section, "profile", &owners)?;
    }
    for config in configs {
        for entry in &config.rc {
            exec::rc_insert(
                lua,
                &doc,
                &entry.json,
                &entry.section,
                &config.name,
                &owners,
            )?;
        }
    }
    let seeds = owners.borrow().clone();
    exec::execute(lua, doc.clone(), Area::Rc, seeds, exec_list(&handles))?;
    let data = convert_live_rc(&doc)?;
    let mut out = Vec::with_capacity(shells.len());
    for shell in shells {
        let mut per_shell = data.clone();
        materialize_shell(&mut per_shell.final_entries, shell)
            .map_err(|error| plan_error(error.to_string()))?;
        out.push(Document::new(
            DocPath::new(shell_path(shell)),
            DocumentData::Rc(per_shell),
        ));
    }
    Ok(out)
}

/// Converts one final live rc table into core data.
fn convert_live_rc(doc: &Table) -> mlua::Result<RcData> {
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
                    "patch: section '{section}' holds a non-list leaf"
                )));
            }
        };
        for index in 1..=table.raw_len() {
            let item: Value = table.get(index)?;
            let entry = match item {
                Value::Table(entry) => entry,
                _ => {
                    return Err(plan_error(format!(
                        "patch: section '{section}' holds a non-table entry"
                    )));
                }
            };
            let json = table_to_json(&entry, "patch: convert")?;
            document::push_live_entry(
                &mut profile,
                &mut config,
                &mut finals,
                section,
                &json,
                "rc",
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
                    *arg = render_init(arg, &facts)?;
                }
            }
            RcOp::Source { path, .. } => {
                *path = render_init(path, &facts)?;
            }
            RcOp::Env { .. } | RcOp::Path { .. } | RcOp::Alias { .. } => {}
        }
    }
    Ok(())
}

/// Renders one init string with the shell facts.
fn render_init(text: &str, facts: &BTreeMap<String, Json>) -> Result<String> {
    text::render(text, facts, "render init: ").map_err(wrap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::document::{Condition, DocumentData};

    /// Writes files plus profile into a temp root.
    fn project(files: &[(&str, &str)], profile: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(error) => panic!("tempdir builds: {error}"),
        };
        for (name, contents) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent()
                && let Err(error) = std::fs::create_dir_all(parent)
            {
                panic!("mkdirs build: {error}");
            }
            if let Err(error) = std::fs::write(&path, contents) {
                panic!("file writes: {error}");
            }
        }
        let profile_path = dir.path().join("profile.lua");
        if let Err(error) = std::fs::write(&profile_path, profile) {
            panic!("profile writes: {error}");
        }
        (dir, profile_path)
    }

    /// Evaluates one profile string in a temp root.
    fn run_profile(files: &[(&str, &str)], profile: &str) -> Result<Vec<Document>> {
        let (dir, profile_path) = project(files, profile);
        let outcome = crate::evaluate(
            &profile_path,
            EvalOpts {
                root: dir.path().to_path_buf(),
                plugins: None,
            },
        );
        std::mem::drop(dir);
        outcome
    }

    /// Evaluates one passing profile string in a temp root.
    fn run_ok(files: &[(&str, &str)], profile: &str) -> Vec<Document> {
        match run_profile(files, profile) {
            Ok(documents) => documents,
            Err(error) => panic!("profile evaluates: {error}"),
        }
    }

    /// Evaluates one failing profile string in a temp root.
    fn run_err(files: &[(&str, &str)], profile: &str) -> Error {
        match run_profile(files, profile) {
            Ok(_) => panic!("profile passes"),
            Err(error) => error,
        }
    }

    /// Reads one document by path from a result.
    fn by_path(documents: &[Document], path: &str) -> Document {
        match documents.iter().find(|item| item.path.as_str() == path) {
            Some(found) => found.clone(),
            None => panic!("document '{path}' missing"),
        }
    }

    /// Reads structured data from one document.
    fn structured(document: &Document) -> (StructuredFormat, BTreeMap<String, Json>) {
        match &document.data {
            DocumentData::Structured { format, data } => (*format, data.clone()),
            other => panic!("structured expected, got {other:?}"),
        }
    }

    #[test]
    fn append_order_shows_priority_scatter() {
        let profile = r#"
local alpha = confit.config("alpha")
alpha:add_document(confit.document.structured("json", { path = "app.json", data = { items = {} } }))
alpha:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:append("items", "normal")
end))
local zebra = confit.config("zebra")
zebra:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:append("items", "minor")
end):priority(confit.priority.MINOR))
zebra:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:append("items", "major")
end):priority(confit.priority.MAJOR))
return { shells = { "bash" }, configs = { alpha, zebra } }
"#;
        let documents = run_ok(&[], profile);
        let (_, data) = structured(&by_path(&documents, "app.json"));
        assert_eq!(
            data.get("items"),
            Some(&Json::Array(vec![
                Json::String("major".to_string()),
                Json::String("normal".to_string()),
                Json::String("minor".to_string()),
            ]))
        );
    }

    #[test]
    fn tie_break_falls_to_owner_name() {
        let profile = r#"
local beta = confit.config("beta")
beta:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:set("theme", "beta")
end))
local alpha = confit.config("alpha")
alpha:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:set("theme", "alpha")
end))
return { shells = { "bash" }, configs = { beta, alpha } }
"#;
        let documents = run_ok(&[], profile);
        let (_, data) = structured(&by_path(&documents, "app.json"));
        assert_eq!(data.get("theme"), Some(&Json::String("alpha".to_string())));
    }

    #[test]
    fn registration_order_swap_stays_invariant() {
        let config = r#"
local first = confit.config("aaa")
first:add_document(confit.document.structured("json", { path = "a.json", data = { keep = "a" } }))
first:add_patch(confit.patch.structured("json", "shared.json", function(data)
  data:set("slot", "aaa")
end))
local second = confit.config("zzz")
second:add_document(confit.document.structured("json", { path = "z.json", data = { keep = "z" } }))
second:add_patch(confit.patch.structured("json", "shared.json", function(data)
  data:set("slot", "zzz")
end):priority(confit.priority.HIGH))
return { shells = { "bash" }, configs = { %s } }
"#;
        let forward = run_ok(&[], &config.replace("%s", "first, second"));
        let swapped = run_ok(&[], &config.replace("%s", "second, first"));
        assert_eq!(forward, swapped);
        let (_, data) = structured(&by_path(&forward, "shared.json"));
        assert_eq!(data.get("slot"), Some(&Json::String("zzz".to_string())));
    }

    #[test]
    fn undeclared_patch_creates_document() {
        let profile = r#"
local c = confit.config("c")
c:add_patch(confit.patch.structured("toml", "fresh.toml", function(data)
  data:set("user.theme", "catppuccin")
end))
return { shells = { "bash" }, configs = { c } }
"#;
        let documents = run_ok(&[], profile);
        let found = by_path(&documents, "fresh.toml");
        let (format, data) = structured(&found);
        assert_eq!(format, StructuredFormat::Toml);
        assert_eq!(
            data.get("user"),
            Some(&serde_json::json!({"theme": "catppuccin"}))
        );
    }

    #[test]
    fn format_mismatch_fails_as_plan_error() {
        let profile = r#"
local c = confit.config("c")
c:add_document(confit.document.structured("toml", { path = "app.toml", data = {} }))
c:add_patch(confit.patch.structured("json", "app.toml", function(data)
  data:set("a", 1)
end))
return { shells = { "bash" }, configs = { c } }
"#;
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        assert!(error.to_string().contains("format mismatch"));
    }

    #[test]
    fn bad_format_fails_as_plan_error() {
        let profile = r#"
local c = confit.config("c")
c:add_patch(confit.patch.structured("ini", "app.ini", function(_) end))
return { shells = { "bash" }, configs = { c } }
"#;
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        assert!(error.to_string().contains("format"));
    }

    #[test]
    fn unknown_rc_section_fails_as_plan_error() {
        let profile = r#"
local c = confit.config("c")
c:add_document(confit.document.rc.new({ confg = {} }))
return { shells = { "bash" }, configs = { c } }
"#;
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        assert!(error.to_string().contains("unknown rc section"));
    }

    #[test]
    fn repeat_config_fails_as_plan_error() {
        let profile = r#"
local first = confit.config("dup")
local second = confit.config("dup")
return { shells = { "bash" }, configs = { first, second } }
"#;
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        assert!(error.to_string().contains("already defined"));
    }

    #[test]
    fn append_on_non_list_fails_as_plan_error() {
        let profile = r#"
local c = confit.config("c")
c:add_document(confit.document.structured("json", { path = "app.json", data = { name = "x" } }))
c:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:append("name", "y")
end))
return { shells = { "bash" }, configs = { c } }
"#;
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        assert!(error.to_string().contains("non-list"));
    }

    #[test]
    fn jailed_escape_fails_as_plan_error() {
        let profile = r#"return confit.resources.load_toml("../evil.toml")"#;
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        assert!(error.to_string().contains("escapes"));
        let absolute = r#"return confit.resources.load_text("/abs.txt")"#;
        let error = run_err(&[], absolute);
        assert!(error.to_string().contains("absolute"));
    }

    #[test]
    fn function_in_data_names_config_and_field() {
        let profile = r#"
local bat = confit.config("bat")
bat:add_document(confit.document.structured("json", {
  path = "app.json",
  data = { ok = 1, bad = function() end },
}))
return { shells = { "bash" }, configs = { bat } }
"#;
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        let message = error.to_string();
        assert!(message.contains("bat"), "names config: {message}");
        assert!(message.contains("bad"), "names field: {message}");
    }

    #[test]
    fn when_builder_fn_guards_entry() {
        let profile = r#"
local c = confit.config("c")
c:add_document(confit.document.rc.alias("cat", "bat", {
  when = function(shell) return shell.in_path("bat") end,
}))
return { shells = { "bash" }, configs = { c } }
"#;
        let documents = run_ok(&[], profile);
        let found = by_path(&documents, "~/.bashrc");
        match &found.data {
            DocumentData::Rc(data) => {
                assert_eq!(data.config.len(), 1);
                match &data.config[0].op {
                    confit_core::document::RcOp::Alias { name, expansion } => {
                        assert_eq!(name, "cat");
                        assert_eq!(expansion, "bat");
                    }
                    other => panic!("alias expected, got {other:?}"),
                }
                assert_eq!(
                    data.config[0].when,
                    Some(Condition::InPath {
                        name: "bat".to_string()
                    })
                );
            }
            other => panic!("rc expected, got {other:?}"),
        }
    }

    #[test]
    fn shell_expansion_substitutes_shell_slot() {
        let profile = r#"
local c = confit.config("c")
c:add_document(confit.document.rc.new({
  profile = { confit.document.rc.path_entry("/x/bin") },
  final = { confit.document.rc.eval({ "echo", confit.shell.SHELL }) },
}))
return { shells = { "bash", "zsh" }, configs = { c } }
"#;
        let documents = run_ok(&[], profile);
        assert_eq!(documents.len(), 2);
        let bash = by_path(&documents, "~/.bashrc");
        let zsh = by_path(&documents, "~/.zshrc");
        match &bash.data {
            DocumentData::Rc(data) => {
                assert_eq!(data.profile.len(), 1);
                assert_eq!(data.final_entries.len(), 1);
                match &data.final_entries[0].op {
                    confit_core::document::RcOp::Eval { argv, .. } => {
                        assert_eq!(argv, &vec!["echo".to_string(), "bash".to_string()]);
                    }
                    other => panic!("eval expected, got {other:?}"),
                }
            }
            other => panic!("rc expected, got {other:?}"),
        }
        match &zsh.data {
            DocumentData::Rc(data) => match &data.final_entries[0].op {
                confit_core::document::RcOp::Eval { argv, .. } => {
                    assert_eq!(argv, &vec!["echo".to_string(), "zsh".to_string()]);
                }
                other => panic!("eval expected, got {other:?}"),
            },
            other => panic!("rc expected, got {other:?}"),
        }
    }

    #[test]
    fn text_and_link_pass_through_with_owner() {
        let profile = r#"
local rc = confit.document.rc.new({})
return {
  shells = { "bash" },
  documents = {
    confit.document.text("/etc/motd", "hi"),
    confit.document.link("~/.vimrc", "~/.vim/vimrc"),
    rc,
  },
  configs = { confit.config("tool") },
}
"#;
        let documents = run_ok(&[], profile);
        let text = by_path(&documents, "/etc/motd");
        assert!(matches!(
            text.data,
            DocumentData::Text { ref content } if content == "hi"
        ));
        let link = by_path(&documents, "~/.vimrc");
        assert!(matches!(
            link.data,
            DocumentData::Link { ref target } if target == "~/.vim/vimrc"
        ));
    }

    #[test]
    fn profile_function_return_evaluates() {
        let profile = r#"
return function()
  return { shells = { "bash" }, configs = { confit.config("tool") } }
end
"#;
        let documents = run_ok(&[], profile);
        assert!(documents.is_empty());
    }

    #[test]
    fn unknown_opts_field_fails_as_plan_error() {
        let profile = r#"return confit.document.rc.eval({ "x" }, { lane = "first" })"#;
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        assert!(error.to_string().contains("unknown field 'lane'"));
    }

    #[test]
    fn cross_section_same_name_collision_drops_second() {
        let profile = r#"
local first = confit.config("first")
first:add_document(confit.document.rc.new({
  profile = { confit.document.rc.env("SHARED", "one") },
}))
local second = confit.config("second")
second:add_document(confit.document.rc.new({
  config = { confit.document.rc.env("SHARED", "two") },
}))
return { shells = { "bash" }, configs = { first, second } }
"#;
        let documents = run_ok(&[], profile);
        let found = by_path(&documents, "~/.bashrc");
        match &found.data {
            DocumentData::Rc(data) => {
                assert_eq!(data.profile.len(), 1);
                assert!(data.config.is_empty());
                match &data.profile[0].op {
                    confit_core::document::RcOp::Env { name, value } => {
                        assert_eq!(name, "SHARED");
                        assert_eq!(value, "one");
                    }
                    other => panic!("env expected, got {other:?}"),
                }
            }
            other => panic!("rc expected, got {other:?}"),
        }
    }

    #[test]
    fn embedded_mise_plugin_builds_package() {
        let profile = r#"
local mise = confit.plugin.solrachq.mise
local bat = mise.package("bat", function(rc)
  rc:alias("cat", "bat")
end)
bat:add_document(mise.activate())
return { shells = { "bash" }, configs = { bat } }
"#;
        let documents = run_ok(&[], profile);
        let mise = by_path(&documents, "~/.config/mise/config.toml");
        let (_, data) = structured(&mise);
        assert_eq!(
            data.get("tools"),
            Some(&serde_json::json!({"bat": "latest"}))
        );
        let rc = by_path(&documents, "~/.bashrc");
        match &rc.data {
            DocumentData::Rc(data) => {
                assert_eq!(data.config.len(), 1);
                match &data.config[0].op {
                    confit_core::document::RcOp::Alias { name, .. } => {
                        assert_eq!(name, "cat");
                    }
                    other => panic!("alias expected, got {other:?}"),
                }
                assert_eq!(data.final_entries.len(), 1);
            }
            other => panic!("rc expected, got {other:?}"),
        }
    }

    #[test]
    fn template_plugin_renders_text_document() {
        let files = &[("resources/starship.toml.j2", "timeout = {{ timeout }}\n")];
        let profile = r#"
local template = confit.plugin.solrachq.template
local c = confit.config("starship")
c:add_document(template("starship.toml", { src = "resources/starship.toml.j2", vars = { timeout = 5 } }))
return { shells = { "bash" }, configs = { c } }
"#;
        let documents = run_ok(files, profile);
        let found = by_path(&documents, "starship.toml");
        assert!(matches!(
            found.data,
            DocumentData::Text { ref content } if content == "timeout = 5"
        ));
    }

    #[test]
    fn merge_plugin_deep_merges_tables() {
        let profile = r#"
local merge = confit.plugin.solrachq.merge
local c = confit.config("c")
local merged = merge({ a = 1, nested = { x = 1 } }, { nested = { y = 2 } })
c:add_document(confit.document.structured("json", { path = "app.json", data = merged }))
return { shells = { "bash" }, configs = { c } }
"#;
        let documents = run_ok(&[], profile);
        let (_, data) = structured(&by_path(&documents, "app.json"));
        assert_eq!(
            data.get("nested"),
            Some(&serde_json::json!({"x": 1, "y": 2}))
        );
    }
}
