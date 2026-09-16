use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit_core::document::{Condition, Document, DocumentData, StructuredFormat};
use confit_core::error::{Error, Result};
use confit_engine::{EvalOpts, evaluate};
use serde_json::Value as Json;

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
    let outcome = evaluate(
        &profile_path,
        EvalOpts {
            root: dir.path().to_path_buf(),
            plugins: dir.path().join("plugins"),
            re_fetch: false,
            cache_dir: None,
            fetcher: None,
            progress: None,
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
fn unsafe_libraries_stay_unloaded() {
    let profile = r#"
assert(io == nil, "io loads")
assert(os == nil, "os loads")
assert(package == nil, "package loads")
assert(debug == nil, "debug loads")
return { shells = { "bash" }, configs = { confit.config("tool") } }
"#;
    run_ok(&[], profile);
}

#[test]
fn safe_libraries_stay_loaded() {
    let profile = r#"
assert(string.upper("a") == "A", "string loads")
assert(#table.pack(1, 2) == 2, "table loads")
assert(math.type(1) == "integer", "math loads")
assert(utf8.len("a") == 1, "utf8 loads")
local alive = coroutine.create(function() coroutine.yield(1) end)
assert(coroutine.resume(alive), "coroutine loads")
return { shells = { "bash" }, configs = { confit.config("tool") } }
"#;
    run_ok(&[], profile);
}

#[test]
fn profile_require_resolves_inside_root() {
    let files = [(
        "tools/extra.lua",
        r#"return { shells = { "bash" }, configs = { confit.config("tool") } }"#,
    )];
    let profile = r#"return require("tools.extra")"#;
    let documents = run_ok(&files, profile);
    assert!(documents.is_empty());
}

#[test]
fn profile_require_escape_fails_as_plan_error() {
    let profile = r#"local evil = require("..evil") return evil"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("bad module"));
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
c:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.alias("cat", "bat", {
    when = function(shell) return shell.in_path("bat") end,
  }))
end))
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
  profile = { confit.document.rc.prepend("/x/bin") },
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
        DocumentData::Text { ref content, .. } if content == "hi"
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
fn repeat_shell_fails_as_plan_error() {
    let profile = r#"return { shells = { "bash", "bash" }, configs = { confit.config("tool") } }"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("more than once"));
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
first:add_patch(confit.patch.rc(function(data)
  data:add("profile", confit.document.rc.env("SHARED", "one"))
end))
local second = confit.config("second")
second:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.env("SHARED", "two"))
end))
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
bat:add_patch(mise.activate())
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
            assert_eq!(data.profile.len(), 2);
            match &data.profile[0].op {
                confit_core::document::RcOp::Path { dir, .. } => {
                    assert!(dir.ends_with(".local/bin"), "mise bin dir first: {dir}");
                }
                other => panic!("path expected, got {other:?}"),
            }
            match &data.profile[1].op {
                confit_core::document::RcOp::Eval { argv } => {
                    assert_eq!(argv[0], "mise");
                }
                other => panic!("eval expected, got {other:?}"),
            }
            assert!(data.final_entries.is_empty());
        }
        other => panic!("rc expected, got {other:?}"),
    }
}

#[test]
fn explicit_section_opt_places_entry() {
    let profile = r#"
local tool = confit.config("tool")
tool:add_patch(confit.patch.rc(function(data)
  data:add("final", confit.document.rc.eval({ "starship", "init", "bash" }))
end))
tool:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.alias("ll", "ls -l"))
end))
return { shells = { "bash" }, configs = { tool } }
"#;
    let documents = run_ok(&[], profile);
    let rc = by_path(&documents, "~/.bashrc");
    match &rc.data {
        DocumentData::Rc(data) => {
            assert_eq!(data.config.len(), 1);
            assert_eq!(data.final_entries.len(), 1);
            match &data.final_entries[0].op {
                confit_core::document::RcOp::Eval { argv } => {
                    assert_eq!(argv[0], "starship");
                }
                other => panic!("eval expected, got {other:?}"),
            }
        }
        other => panic!("rc expected, got {other:?}"),
    }
}

#[test]
fn bad_section_opt_fails_as_plan_error() {
    let profile = r#"
local tool = confit.config("tool")
tool:add_patch(confit.patch.rc(function(data)
  data:add("nope", confit.document.rc.alias("ll", "ls -l"))
end))
return { shells = { "bash" }, configs = { tool } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("unknown section"));
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
        DocumentData::Text { ref content, .. } if content == "timeout = 5"
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

#[test]
fn merge_plugin_shallow_replaces_nested() {
    let profile = r#"
local merge = confit.plugin.solrachq.merge
local c = confit.config("c")
local merged = merge({ nested = { x = 1, y = 1 } }, { nested = { y = 2 } }, { shallow = true })
c:add_document(confit.document.structured("json", { path = "app.json", data = merged }))
return { shells = { "bash" }, configs = { c } }
"#;
    let documents = run_ok(&[], profile);
    let (_, data) = structured(&by_path(&documents, "app.json"));
    assert_eq!(data.get("nested"), Some(&serde_json::json!({"y": 2})));
}

#[test]
fn merge_plugin_list_append_joins_arrays() {
    let profile = r#"
local merge = confit.plugin.solrachq.merge
local c = confit.config("c")
local merged = merge({ items = { 1, 2 } }, { items = { 3 } }, { list_append = true })
c:add_document(confit.document.structured("json", { path = "app.json", data = merged }))
return { shells = { "bash" }, configs = { c } }
"#;
    let documents = run_ok(&[], profile);
    let (_, data) = structured(&by_path(&documents, "app.json"));
    assert_eq!(data.get("items"), Some(&serde_json::json!([1, 2, 3])));
}

#[test]
fn merge_plugin_unknown_opt_fails_as_plan_error() {
    let profile = r#"
local merge = confit.plugin.solrachq.merge
local _ = merge({}, {}, { bogus = true })
return { shells = { "bash" }, configs = { confit.config("c") } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("unknown field 'bogus'"));
}

#[test]
fn merge_plugin_direct_self_cycle_fails_as_plan_error() {
    let profile = r#"
local merge = confit.plugin.solrachq.merge
local base = { a = 1 }
base.self = base
local _ = merge(base, {})
return { shells = { "bash" }, configs = { confit.config("c") } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("merge: base holds a recursive table")
    );
}

#[test]
fn merge_plugin_nested_cycle_fails_as_plan_error() {
    let profile = r#"
local merge = confit.plugin.solrachq.merge
local a = {}
local b = { link = a }
a.link = b
local _ = merge(a, {})
return { shells = { "bash" }, configs = { confit.config("c") } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("merge: base holds a recursive table")
    );
}

#[test]
fn merge_plugin_overlay_cycle_fails_as_plan_error() {
    let profile = r#"
local merge = confit.plugin.solrachq.merge
local overlay = { a = 1 }
overlay.self = overlay
local _ = merge({}, overlay)
return { shells = { "bash" }, configs = { confit.config("c") } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("merge: overlay holds a recursive table")
    );
}

#[test]
fn merge_plugin_shared_refs_still_merge() {
    let profile = r#"
local merge = confit.plugin.solrachq.merge
local c = confit.config("c")
local shared = { x = 1, deep = { y = 2 } }
local base = { p = shared, q = shared, nested = { first = shared, second = { inner = shared } } }
local merged = merge(base, { r = 3 })
c:add_document(confit.document.structured("json", { path = "app.json", data = merged }))
return { shells = { "bash" }, configs = { c } }
"#;
    let documents = run_ok(&[], profile);
    let (_, data) = structured(&by_path(&documents, "app.json"));
    let want = serde_json::json!({ "x": 1, "deep": { "y": 2 } });
    assert_eq!(data.get("p"), Some(&want));
    assert_eq!(data.get("q"), Some(&want));
    assert_eq!(data.get("r"), Some(&serde_json::json!(3)));
    assert_eq!(
        data.get("nested"),
        Some(&serde_json::json!({
            "first": { "x": 1, "deep": { "y": 2 } },
            "second": { "inner": { "x": 1, "deep": { "y": 2 } } },
        }))
    );
}

#[test]
fn structured_cycle_fails_as_plan_error() {
    let profile = r#"
local c = confit.config("c")
local data = { a = 1 }
data.self = data
c:add_document(confit.document.structured("json", { path = "app.json", data = data }))
return { shells = { "bash" }, configs = { c } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("recursive"));
}

#[test]
fn opaque_constructor_keeps_raw_bytes_without_utf8() {
    let profile = r#"
return {
  shells = { "bash" },
  documents = {
confit.document.opaque("bin/logo", string.char(0xFF, 0x00, 0x41)),
  },
  configs = { confit.config("tool") },
}
"#;
    let documents = run_ok(&[], profile);
    let found = by_path(&documents, "bin/logo");
    match &found.data {
        DocumentData::Opaque { content, .. } => assert_eq!(content, &vec![0xFF, 0x00, 0x41]),
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn load_bytes_feeds_opaque_without_utf8() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let asset = dir.path().join("logo.bin");
    if let Err(error) = std::fs::write(&asset, [0xFF, 0x00, 0x41]) {
        panic!("binary writes: {error}");
    }
    let profile_path = dir.path().join("profile.lua");
    let profile = r#"
local raw = confit.resources.load_bytes("logo.bin")
return {
  shells = { "bash" },
  documents = { confit.document.opaque("bin/logo", raw) },
  configs = { confit.config("tool") },
}
"#;
    if let Err(error) = std::fs::write(&profile_path, profile) {
        panic!("profile writes: {error}");
    }
    let outcome = evaluate(
        &profile_path,
        EvalOpts {
            root: dir.path().to_path_buf(),
            plugins: dir.path().join("plugins"),
            re_fetch: false,
            cache_dir: None,
            fetcher: None,
            progress: None,
        },
    );
    let documents = match outcome {
        Ok(documents) => documents,
        Err(error) => panic!("profile evaluates: {error}"),
    };
    let found = by_path(&documents, "bin/logo");
    match &found.data {
        DocumentData::Opaque { content, .. } => assert_eq!(content, &vec![0xFF, 0x00, 0x41]),
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn opaque_path_collision_fails_as_plan_error() {
    let profile = r#"
return {
  shells = { "bash" },
  documents = {
confit.document.text("shared", "hi"),
confit.document.opaque("shared", string.char(0xFF, 0x00)),
  },
  configs = { confit.config("tool") },
}
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("more than once"));
}

/// Builds one memory fetcher holding a single stub.
fn stubbed(url: &str, body: &[u8]) -> std::sync::Arc<confit_engine::fetch::MemoryFetch> {
    let fake = std::sync::Arc::new(confit_engine::fetch::MemoryFetch::new());
    fake.insert(url, body);
    fake
}

/// Runs one profile with fetch inputs in an isolated root.
fn run_fetch(
    profile: &str,
    cache: &Path,
    fetcher: std::sync::Arc<confit_engine::fetch::MemoryFetch>,
    re_fetch: bool,
) -> Result<Vec<Document>> {
    let (dir, profile_path) = project(&[], profile);
    let outcome = evaluate(
        &profile_path,
        EvalOpts {
            root: dir.path().to_path_buf(),
            plugins: dir.path().join("plugins"),
            re_fetch,
            cache_dir: Some(cache.to_path_buf()),
            fetcher: Some(fetcher),
            progress: None,
        },
    );
    std::mem::drop(dir);
    outcome
}

/// Reads text content from one document path.
fn text_content(documents: &[Document], path: &str) -> String {
    let found = by_path(documents, path);
    match &found.data {
        DocumentData::Text { content, .. } => content.clone(),
        other => panic!("text expected, got {other:?}"),
    }
}

#[test]
fn fetch_text_caches_and_serves_zero_calls_on_hit() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let url = "https://example.com/version";
    let profile = r#"
local v = confit.resources.fetch_text("https://example.com/version")
return {
  shells = { "bash" },
  documents = { confit.document.text("v", v) },
  configs = { confit.config("tool") },
}
"#;
    let fake = stubbed(url, b"1.2.3");
    let first = match run_fetch(profile, cache.path(), fake.clone(), false) {
        Ok(documents) => documents,
        Err(error) => panic!("first fetch runs: {error}"),
    };
    assert_eq!(text_content(&first, "v"), "1.2.3");
    assert_eq!(fake.calls(url), 1);
    let empty = std::sync::Arc::new(confit_engine::fetch::MemoryFetch::new());
    let second = match run_fetch(profile, cache.path(), empty, false) {
        Ok(documents) => documents,
        Err(error) => panic!("cached fetch runs: {error}"),
    };
    assert_eq!(text_content(&second, "v"), "1.2.3");
}

#[test]
fn fetch_sidecar_mismatch_triggers_redownload() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let url = "https://example.com/tool.bin";
    let profile = r#"
local p = confit.resources.fetch_file("https://example.com/tool.bin")
return {
  shells = { "bash" },
  documents = { confit.document.text("note", p) },
  configs = { confit.config("tool") },
}
"#;
    let fake = stubbed(url, b"fresh");
    match run_fetch(profile, cache.path(), fake.clone(), false) {
        Ok(_) => {}
        Err(error) => panic!("first fetch runs: {error}"),
    }
    assert_eq!(fake.calls(url), 1);
    let cached = confit_engine::fetch::cache_path(cache.path(), url);
    if let Err(error) = std::fs::write(&cached, b"stale") {
        panic!("cache corrupts: {error}");
    }
    match run_fetch(profile, cache.path(), fake.clone(), false) {
        Ok(_) => {}
        Err(error) => panic!("redownload runs: {error}"),
    }
    assert_eq!(fake.calls(url), 2);
    let stored = match std::fs::read(&cached) {
        Ok(bytes) => bytes,
        Err(error) => panic!("cache reads: {error}"),
    };
    assert_eq!(stored, b"fresh");
}

#[test]
fn fetch_user_sha_mismatch_fails_as_plan_error() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let url = "https://example.com/data";
    let profile = r#"
local v = confit.resources.fetch_text("https://example.com/data", { sha256 = "0000000000000000000000000000000000000000000000000000000000000000" })
return {
  shells = { "bash" },
  documents = { confit.document.text("v", v) },
  configs = { confit.config("tool") },
}
"#;
    let fake = stubbed(url, b"data");
    match run_fetch(profile, cache.path(), fake, false) {
        Ok(_) => panic!("bad sha passes"),
        Err(error) => {
            assert!(matches!(error, Error::Plan(_)));
            assert!(error.to_string().contains("sha256"));
        }
    }
}

#[test]
fn fetch_re_fetch_flag_forces_download() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let url = "https://example.com/version";
    let profile = r#"
local v = confit.resources.fetch_text("https://example.com/version")
return {
  shells = { "bash" },
  documents = { confit.document.text("v", v) },
  configs = { confit.config("tool") },
}
"#;
    let fake = stubbed(url, b"1.2.3");
    match run_fetch(profile, cache.path(), fake.clone(), false) {
        Ok(_) => {}
        Err(error) => panic!("first fetch runs: {error}"),
    }
    assert_eq!(fake.calls(url), 1);
    match run_fetch(profile, cache.path(), fake.clone(), true) {
        Ok(_) => {}
        Err(error) => panic!("forced fetch runs: {error}"),
    }
    assert_eq!(fake.calls(url), 2);
}

#[test]
fn fetch_file_cache_reads_pass_the_jail() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let url = "https://example.com/tool.bin";
    let profile = r#"
local p = confit.resources.fetch_file("https://example.com/tool.bin")
local raw = confit.resources.load_bytes(p)
return {
  shells = { "bash" },
  documents = { confit.document.opaque("bin/tool", raw) },
  configs = { confit.config("tool") },
}
"#;
    let fake = stubbed(url, &[0xFF, 0x00, 0x41]);
    let documents = match run_fetch(profile, cache.path(), fake, false) {
        Ok(documents) => documents,
        Err(error) => panic!("cached read runs: {error}"),
    };
    let found = by_path(&documents, "bin/tool");
    match &found.data {
        DocumentData::Opaque { content, .. } => assert_eq!(content, &vec![0xFF, 0x00, 0x41]),
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn fetch_outside_cache_absolute_reads_still_fail() {
    let outside = match tempfile::NamedTempFile::new() {
        Ok(file) => file,
        Err(error) => panic!("outside builds: {error}"),
    };
    let path = outside.path().to_string_lossy().into_owned();
    std::mem::drop(outside);
    let profile = format!(
        r#"local _ = confit.resources.load_text("{path}") return {{ shells = {{ "bash" }}, configs = {{ confit.config("tool") }} }}"#
    );
    let error = run_err(&[], &profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("absolute"));
}
/// Builds tar.gz bytes from member triples in memory.
fn tar_gz_bytes(members: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut builder = tar::Builder::new(&mut encoder);
        for (name, bytes, mode) in members {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            if builder.append_data(&mut header, *name, *bytes).is_err() {
                panic!("archive member writes");
            }
        }
        if builder.finish().is_err() {
            panic!("archive finishes");
        }
    }
    match encoder.finish() {
        Ok(bytes) => bytes,
        Err(error) => panic!("gzip finishes: {error}"),
    }
}

/// Builds plain tar bytes from member triples in memory.
fn tar_bytes(members: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut raw: Vec<u8> = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut raw);
        for (name, bytes, mode) in members {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            if builder.append_data(&mut header, *name, *bytes).is_err() {
                panic!("archive member writes");
            }
        }
        if builder.finish().is_err() {
            panic!("archive finishes");
        }
    }
    raw
}

/// Writes archive bytes plus profile into a temp root and evaluates.
fn run_with_archive(archive_name: &str, archive: &[u8], profile: &str) -> Result<Vec<Document>> {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let archive_path = dir.path().join(archive_name);
    if let Err(error) = std::fs::write(&archive_path, archive) {
        panic!("archive writes: {error}");
    }
    let profile_path = dir.path().join("profile.lua");
    if let Err(error) = std::fs::write(&profile_path, profile) {
        panic!("profile writes: {error}");
    }
    let outcome = evaluate(
        &profile_path,
        EvalOpts {
            root: dir.path().to_path_buf(),
            plugins: dir.path().join("plugins"),
            re_fetch: false,
            cache_dir: None,
            fetcher: None,
            progress: None,
        },
    );
    std::mem::drop(dir);
    outcome
}

/// Evaluates one passing archive profile in a temp root.
fn run_archive_ok(archive_name: &str, archive: &[u8], profile: &str) -> Vec<Document> {
    match run_with_archive(archive_name, archive, profile) {
        Ok(documents) => documents,
        Err(error) => panic!("profile evaluates: {error}"),
    }
}

/// Evaluates one failing archive profile in a temp root.
fn run_archive_err(archive_name: &str, archive: &[u8], profile: &str) -> Error {
    match run_with_archive(archive_name, archive, profile) {
        Ok(_) => panic!("profile passes"),
        Err(error) => error,
    }
}

#[test]
fn compressed_extension_pick_keeps_matching_members() {
    let archive = tar_gz_bytes(&[
        ("fonts/Regular.ttf", b"ttfdata".as_slice(), 0o644),
        ("fonts/readme.txt", b"readme".as_slice(), 0o644),
    ]);
    let profile = r#"
local kept = confit.document.compressed("fonts.tar.gz", function(path, info, content)
  if path:find("%.ttf$") then
    return confit.document.text("/fonts/" .. path, content)
  end
end)
return { shells = { "bash" }, documents = kept, configs = { confit.config("tool") } }
"#;
    let documents = run_archive_ok("fonts.tar.gz", &archive, profile);
    assert_eq!(documents.len(), 1);
    let found = by_path(&documents, "/fonts/fonts/Regular.ttf");
    match &found.data {
        DocumentData::Text { content, .. } => assert_eq!(content, "ttfdata"),
        other => panic!("text expected, got {other:?}"),
    }
}

#[test]
fn compressed_executable_pick_reads_the_mode() {
    let archive = tar_bytes(&[
        ("bin/run", b"run".as_slice(), 0o755),
        ("bin/readme", b"read".as_slice(), 0o644),
    ]);
    let profile = r#"
local kept = confit.document.compressed("tools.tar", function(path, info, content)
  if info.executable then
    return confit.document.text("/out/" .. path, tostring(info.size))
  end
end)
return { shells = { "bash" }, documents = kept, configs = { confit.config("tool") } }
"#;
    let documents = run_archive_ok("tools.tar", &archive, profile);
    assert_eq!(documents.len(), 1);
    let found = by_path(&documents, "/out/bin/run");
    match &found.data {
        DocumentData::Text { content, .. } => assert_eq!(content, "3"),
        other => panic!("text expected, got {other:?}"),
    }
}

#[test]
fn compressed_nil_callback_skips_members() {
    let archive = tar_gz_bytes(&[
        ("a.txt", b"a".as_slice(), 0o644),
        ("b.txt", b"b".as_slice(), 0o644),
    ]);
    let profile = r#"
local kept = confit.document.compressed("empty.tar.gz", function(path, info, content)
end)
assert(#kept == 0, "kept stays empty")
return { shells = { "bash" }, documents = kept, configs = { confit.config("tool") } }
"#;
    let documents = run_archive_ok("empty.tar.gz", &archive, profile);
    assert!(documents.is_empty());
}

#[test]
fn compressed_binary_member_wraps_opaque_without_utf8() {
    let raw: &[u8] = &[0xFF, 0x00, 0x41, 0xFE];
    let archive = tar_gz_bytes(&[("bin/logo", raw, 0o644)]);
    let profile = r#"
local kept = confit.document.compressed("bin.tar.gz", function(path, info, content)
  return confit.document.opaque("bin/logo", content)
end)
return { shells = { "bash" }, documents = kept, configs = { confit.config("tool") } }
"#;
    let documents = run_archive_ok("bin.tar.gz", &archive, profile);
    let found = by_path(&documents, "bin/logo");
    match &found.data {
        DocumentData::Opaque { content, .. } => {
            assert_eq!(content, &vec![0xFF, 0x00, 0x41, 0xFE]);
        }
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn compressed_corrupt_archive_fails_as_plan_error() {
    let bad = vec![0xFFu8; 1024];
    let profile = r#"
local kept = confit.document.compressed("bad.tar.gz", function(path, info, content)
  return confit.document.text("out", content)
end)
return { shells = { "bash" }, documents = kept, configs = { confit.config("tool") } }
"#;
    let error = run_archive_err("bad.tar.gz", &bad, profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("cannot unpack"));
}

#[test]
fn compressed_repeat_path_fails_as_plan_error() {
    let archive = tar_gz_bytes(&[("shared.txt", b"data".as_slice(), 0o644)]);
    let profile = r#"
local kept = confit.document.compressed("a.tar.gz", function(path, info, content)
  return confit.document.text("shared", content)
end)
return {
  shells = { "bash" },
  documents = { confit.document.text("shared", "hi"), kept[1] },
  configs = { confit.config("tool") },
}
"#;
    let error = run_archive_err("a.tar.gz", &archive, profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("more than once"));
}

#[test]
fn compressed_pairs_with_fetch_file_cache_path() {
    let archive = tar_gz_bytes(&[("bin/logo", &[0xFF, 0x00, 0x41], 0o644)]);
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let url = "https://example.com/fonts.tar.gz";
    let profile = r#"
local p = confit.resources.fetch_file("https://example.com/fonts.tar.gz")
local kept = confit.document.compressed(p, function(path, info, content)
  return confit.document.opaque("bin/logo", content)
end)
return { shells = { "bash" }, documents = kept, configs = { confit.config("tool") } }
"#;
    let fake = stubbed(url, &archive);
    let documents = match run_fetch(profile, cache.path(), fake, false) {
        Ok(documents) => documents,
        Err(error) => panic!("paired fetch runs: {error}"),
    };
    let found = by_path(&documents, "bin/logo");
    match &found.data {
        DocumentData::Opaque { content, .. } => assert_eq!(content, &vec![0xFF, 0x00, 0x41]),
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn mise_packages_fold_into_one_shared_document() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local bat = mise.package("bat", function(rc)
  rc:alias("cat", "bat")
end)
local eza = mise.package("eza", function(rc)
  rc:alias("ls", "eza")
end)
return { shells = { "bash" }, configs = { bat, eza } }
"#;
    let documents = run_ok(&[], profile);
    let found = by_path(&documents, "~/.config/mise/config.toml");
    let (_, data) = structured(&found);
    let tools = data
        .get("tools")
        .and_then(|tools| tools.as_object())
        .unwrap_or_else(|| panic!("tools table missing"));
    assert!(tools.contains_key("bat"));
    assert!(tools.contains_key("eza"));
}

/// Builds one zip archive in memory from members.
fn zip_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut raw: Vec<u8> = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut raw);
        let mut writer = zip::ZipWriter::new(cursor);
        for (name, bytes) in members {
            let options = zip::write::SimpleFileOptions::default();
            if writer.start_file(*name, options).is_err() {
                panic!("zip member starts");
            }
            if std::io::Write::write_all(&mut writer, bytes).is_err() {
                panic!("zip member writes");
            }
        }
        if writer.finish().is_err() {
            panic!("zip finishes");
        }
    }
    raw
}

#[test]
fn compressed_zip_pick_keeps_matching_members() {
    let archive = zip_bytes(&[
        ("JetBrainsMono-Bold.ttf", &[0x00, 0x01, 0x00, 0x00]),
        ("README.md", b"docs"),
    ]);
    let profile = r#"
local kept = confit.document.compressed("fonts.zip", function(path, info, content)
  if path:match("%.ttf$") then
    return confit.document.opaque("fonts/" .. path, content)
  end
end)
return { shells = { "bash" }, documents = kept, configs = { confit.config("tool") } }
"#;
    let documents = run_archive_ok("fonts.zip", &archive, profile);
    assert_eq!(documents.len(), 1);
    let found = by_path(&documents, "fonts/JetBrainsMono-Bold.ttf");
    match &found.data {
        DocumentData::Opaque { content, .. } => {
            assert_eq!(content, &vec![0x00, 0x01, 0x00, 0x00]);
        }
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn shell_slot_materializes_outside_final() {
    let profile = r#"
local tool = confit.config("tool")
tool:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.eval({ "mise", "activate", confit.shell.SHELL }))
end))
return { shells = { "bash" }, configs = { tool } }
"#;
    let documents = run_ok(&[], profile);
    let rc = by_path(&documents, "~/.bashrc");
    match &rc.data {
        DocumentData::Rc(data) => {
            assert_eq!(data.config.len(), 1);
            match &data.config[0].op {
                confit_core::document::RcOp::Eval { argv } => {
                    assert_eq!(
                        argv,
                        &vec![
                            "mise".to_string(),
                            "activate".to_string(),
                            "bash".to_string()
                        ]
                    );
                }
                other => panic!("eval expected, got {other:?}"),
            }
        }
        other => panic!("rc expected, got {other:?}"),
    }
}

#[test]
fn dual_rc_base_fails_as_plan_error() {
    let profile = r#"
local first = confit.config("first")
first:add_document(confit.document.rc.new({
  config = { confit.document.rc.alias("ll", "ls -l") },
}))
local second = confit.config("second")
second:add_document(confit.document.rc.new({
  config = { confit.document.rc.alias("la", "ls -a") },
}))
return { shells = { "bash" }, configs = { first, second } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("more than once"));
}

#[test]
fn rc_priority_major_wins_same_name_conflict() {
    let profile = r#"
local low = confit.config("aaa")
low:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.alias("ll", "ls-low"))
end))
local high = confit.config("zzz")
high:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.alias("ll", "ls-high"))
end):priority(confit.priority.MAJOR))
return { shells = { "bash" }, configs = { low, high } }
"#;
    let documents = run_ok(&[], profile);
    let rc = by_path(&documents, "~/.bashrc");
    match &rc.data {
        DocumentData::Rc(data) => {
            assert_eq!(data.config.len(), 1);
            match &data.config[0].op {
                confit_core::document::RcOp::Alias { name, expansion } => {
                    assert_eq!(name, "ll");
                    assert_eq!(expansion, "ls-high");
                }
                other => panic!("alias expected, got {other:?}"),
            }
        }
        other => panic!("rc expected, got {other:?}"),
    }
}
