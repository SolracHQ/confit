use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit_core::document::{Condition, ManifestData, ManifestDocument, StructuredFormat};
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
fn run_profile(files: &[(&str, &str)], profile: &str) -> Result<Vec<ManifestDocument>> {
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
    outcome.map(|evaluation| evaluation.documents)
}

/// Evaluates one passing profile string in a temp root.
fn run_ok(files: &[(&str, &str)], profile: &str) -> Vec<ManifestDocument> {
    match run_profile(files, profile) {
        Ok(documents) => documents,
        Err(error) => panic!("profile evaluates: {error}"),
    }
}

/// Evaluates one passing profile string into its full evaluation.
fn run_eval_ok(files: &[(&str, &str)], profile: &str) -> confit_engine::Evaluation {
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
    match outcome {
        Ok(evaluation) => evaluation,
        Err(error) => panic!("profile evaluates: {error}"),
    }
}

/// Reads one hook by argv head from an evaluation.
fn hook_by_head(evaluation: &confit_engine::Evaluation, head: &str) -> confit_core::hook::Hook {
    match evaluation
        .hooks
        .iter()
        .find(|hook| hook.argv.first().is_some_and(|first| first == head))
    {
        Some(found) => found.clone(),
        None => panic!("hook '{head}' missing"),
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
fn by_path(documents: &[ManifestDocument], path: &str) -> ManifestDocument {
    match documents.iter().find(|item| item.path.as_str() == path) {
        Some(found) => found.clone(),
        None => panic!("document '{path}' missing"),
    }
}

/// Reads structured data from one document.
fn structured(document: &ManifestDocument) -> (StructuredFormat, BTreeMap<String, Json>) {
    match &document.data {
        ManifestData::Structured { format, data } => (*format, data.clone()),
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
fn declaration_order_wins_equal_priority() {
    let profile = r#"
local beta = confit.config("beta")
beta:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:set("theme", "beta")
end))
local alpha = confit.config("alpha")
alpha:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:set("theme", "alpha")
end))
return { shells = { "bash" }, configs = { %s } }
"#;
    let forward = run_ok(&[], &profile.replace("%s", "beta, alpha"));
    let (_, data) = structured(&by_path(&forward, "app.json"));
    assert_eq!(data.get("theme"), Some(&Json::String("beta".to_string())));
    let swapped = run_ok(&[], &profile.replace("%s", "alpha, beta"));
    let (_, data) = structured(&by_path(&swapped, "app.json"));
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
        ManifestData::Rc(data) => {
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
  final = { confit.document.rc.eval({ "echo", confit.runtime.SHELL }) },
}))
return { shells = { "bash", "zsh" }, configs = { c } }
"#;
    let documents = run_ok(&[], profile);
    assert_eq!(documents.len(), 2);
    let bash = by_path(&documents, "~/.bashrc");
    let zsh = by_path(&documents, "~/.zshrc");
    match &bash.data {
        ManifestData::Rc(data) => {
            assert_eq!(data.profile.len(), 1);
            assert_eq!(data.final_entries.len(), 1);
            match &data.final_entries[0].op {
                confit_core::document::RcOp::Eval { argv, .. } => {
                    assert_eq!(*argv, vec!["echo".to_string(), "bash".to_string()]);
                }
                other => panic!("eval expected, got {other:?}"),
            }
        }
        other => panic!("rc expected, got {other:?}"),
    }
    match &zsh.data {
        ManifestData::Rc(data) => match &data.final_entries[0].op {
            confit_core::document::RcOp::Eval { argv, .. } => {
                assert_eq!(*argv, vec!["echo".to_string(), "zsh".to_string()]);
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
        ManifestData::Text { ref content, .. } if content == "hi"
    ));
    let link = by_path(&documents, "~/.vimrc");
    assert!(matches!(
        link.data,
        ManifestData::Link { ref target } if target == "~/.vim/vimrc"
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
        ManifestData::Rc(data) => {
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
local installer = confit.config("plugin:solrachq/mise:install")
local bat = mise.package({
  name = "bat",
  rc_builder = function(rc)
    rc:alias("cat", "bat")
  end,
})
return { shells = { "bash" }, configs = { installer, bat } }
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
        ManifestData::Rc(data) => {
            assert_eq!(data.config.len(), 1);
            match &data.config[0].op {
                confit_core::document::RcOp::Alias { name, .. } => {
                    assert_eq!(name, "cat");
                }
                other => panic!("alias expected, got {other:?}"),
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
        ManifestData::Rc(data) => {
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
        ManifestData::Text { ref content, .. } if content == "timeout = 5"
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
    let evaluation = run_eval_ok(&[], profile);
    let found = by_path(&evaluation.documents, "bin/logo");
    match &found.data {
        ManifestData::Opaque { blob, .. } => {
            assert_eq!(evaluation.blobs.get(blob), Some(&vec![0xFF, 0x00, 0x41]))
        }
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
        Ok(evaluation) => evaluation,
        Err(error) => panic!("profile evaluates: {error}"),
    };
    let found = by_path(&documents.documents, "bin/logo");
    match &found.data {
        ManifestData::Opaque { blob, .. } => {
            assert_eq!(documents.blobs.get(blob), Some(&vec![0xFF, 0x00, 0x41]))
        }
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
) -> Result<Vec<ManifestDocument>> {
    run_fetch_eval(profile, cache, fetcher, re_fetch).map(|evaluation| evaluation.documents)
}

/// Runs one profile with fetch inputs returning its full evaluation.
fn run_fetch_eval(
    profile: &str,
    cache: &Path,
    fetcher: std::sync::Arc<confit_engine::fetch::MemoryFetch>,
    re_fetch: bool,
) -> Result<confit_engine::Evaluation> {
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
fn text_content(documents: &[ManifestDocument], path: &str) -> String {
    let found = by_path(documents, path);
    match &found.data {
        ManifestData::Text { content, .. } => content.clone(),
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
    let evaluation = match run_fetch_eval(profile, cache.path(), fake, false) {
        Ok(evaluation) => evaluation,
        Err(error) => panic!("cached read runs: {error}"),
    };
    let found = by_path(&evaluation.documents, "bin/tool");
    match &found.data {
        ManifestData::Opaque { blob, .. } => {
            assert_eq!(evaluation.blobs.get(blob), Some(&vec![0xFF, 0x00, 0x41]))
        }
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
fn run_with_archive(
    archive_name: &str,
    archive: &[u8],
    profile: &str,
) -> Result<confit_engine::Evaluation> {
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
fn run_archive_ok(archive_name: &str, archive: &[u8], profile: &str) -> Vec<ManifestDocument> {
    run_archive_eval(archive_name, archive, profile).documents
}

/// Evaluates one passing archive profile returning its full evaluation.
fn run_archive_eval(
    archive_name: &str,
    archive: &[u8],
    profile: &str,
) -> confit_engine::Evaluation {
    match run_with_archive(archive_name, archive, profile) {
        Ok(evaluation) => evaluation,
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
        ManifestData::Text { content, .. } => assert_eq!(content, "ttfdata"),
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
        ManifestData::Text { content, .. } => assert_eq!(content, "3"),
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
    let evaluation = run_archive_eval("bin.tar.gz", &archive, profile);
    let found = by_path(&evaluation.documents, "bin/logo");
    match &found.data {
        ManifestData::Opaque { blob, .. } => {
            assert_eq!(
                evaluation.blobs.get(blob),
                Some(&vec![0xFF, 0x00, 0x41, 0xFE])
            );
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
    let evaluation = match run_fetch_eval(profile, cache.path(), fake, false) {
        Ok(evaluation) => evaluation,
        Err(error) => panic!("paired fetch runs: {error}"),
    };
    let found = by_path(&evaluation.documents, "bin/logo");
    match &found.data {
        ManifestData::Opaque { blob, .. } => {
            assert_eq!(evaluation.blobs.get(blob), Some(&vec![0xFF, 0x00, 0x41]))
        }
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn tree_pick_returns_one_sorted_document() {
    let archive = tar_gz_bytes(&[
        ("pkg/zulu.ttf", b"zulu".as_slice(), 0o644),
        ("pkg/alpha.ttf", b"alpha".as_slice(), 0o644),
        ("pkg/readme.txt", b"readme".as_slice(), 0o644),
    ]);
    let profile = r#"
local fonts = confit.document.tree("fonts.tar.gz", "/fonts", function(path, info, content)
  if not path:match("%.ttf$") then
    return nil
  end
  return path:match("([^/]+)$")
end)
return { shells = { "bash" }, documents = { fonts }, configs = { confit.config("tool") } }
"#;
    let evaluation = run_archive_eval("fonts.tar.gz", &archive, profile);
    assert_eq!(evaluation.documents.len(), 1);
    let found = by_path(&evaluation.documents, "/fonts");
    match &found.data {
        ManifestData::Tree { members } => {
            assert_eq!(members.len(), 2);
            assert_eq!(members[0].relative, "alpha.ttf");
            assert_eq!(
                evaluation.blobs.get(&members[0].blob),
                Some(&b"alpha".to_vec())
            );
            assert_eq!(members[0].mode, 0o644);
            assert_eq!(members[1].relative, "zulu.ttf");
        }
        other => panic!("tree expected, got {other:?}"),
    }
}

#[test]
fn tree_executable_member_reads_the_mode() {
    let archive = tar_bytes(&[
        ("bin/run", b"run".as_slice(), 0o755),
        ("bin/data", b"data".as_slice(), 0o644),
    ]);
    let profile = r#"
local tools = confit.document.tree("tools.tar", "/out", function(path, info, content)
  return path:match("([^/]+)$")
end)
assert(tools.members[1].rel == "data", "members expose rels in order")
return { shells = { "bash" }, documents = { tools }, configs = { confit.config("tool") } }
"#;
    let evaluation = run_archive_eval("tools.tar", &archive, profile);
    let found = by_path(&evaluation.documents, "/out");
    match &found.data {
        ManifestData::Tree { members } => {
            assert_eq!(members[0].mode, 0o644);
            assert_eq!(members[1].mode, 0o755);
            assert_eq!(
                evaluation.blobs.get(&members[1].blob),
                Some(&b"run".to_vec())
            );
        }
        other => panic!("tree expected, got {other:?}"),
    }
}

#[test]
fn tree_document_return_fails_as_plan_error() {
    let archive = tar_gz_bytes(&[("a.ttf", b"a".as_slice(), 0o644)]);
    let profile = r#"
local fonts = confit.document.tree("a.tar.gz", "/fonts", function(path, info, content)
  return confit.document.opaque("/fonts/a.ttf", content)
end)
return { shells = { "bash" }, documents = { fonts }, configs = { confit.config("tool") } }
"#;
    let error = run_archive_err("a.tar.gz", &archive, profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("destination path or nil"));
}

#[test]
fn tree_dotdot_rel_fails_as_plan_error() {
    let archive = tar_gz_bytes(&[("a.ttf", b"a".as_slice(), 0o644)]);
    let profile = r#"
local fonts = confit.document.tree("a.tar.gz", "/fonts", function(path, info, content)
  return "../escape.ttf"
end)
return { shells = { "bash" }, documents = { fonts }, configs = { confit.config("tool") } }
"#;
    let error = run_archive_err("a.tar.gz", &archive, profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("under the folder"));
}

#[test]
fn tree_duplicate_rel_fails_as_plan_error() {
    let archive = tar_gz_bytes(&[
        ("x/a.ttf", b"a".as_slice(), 0o644),
        ("y/a.ttf", b"b".as_slice(), 0o644),
    ]);
    let profile = r#"
local fonts = confit.document.tree("a.tar.gz", "/fonts", function(path, info, content)
  return "same.ttf"
end)
return { shells = { "bash" }, documents = { fonts }, configs = { confit.config("tool") } }
"#;
    let error = run_archive_err("a.tar.gz", &archive, profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("more than once"));
}

#[test]
fn tree_empty_pick_fails_naming_the_filter() {
    let archive = tar_gz_bytes(&[("a.ttf", b"a".as_slice(), 0o644)]);
    let profile = r#"
local fonts = confit.document.tree("a.tar.gz", "/fonts", function(path, info, content)
  return nil
end)
return { shells = { "bash" }, documents = { fonts }, configs = { confit.config("tool") } }
"#;
    let error = run_archive_err("a.tar.gz", &archive, profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(error.to_string().contains("check the pick filter"));
}

#[test]
fn mise_packages_fold_into_one_shared_document() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local bat = mise.package({
  name = "bat",
  rc_builder = function(rc)
    rc:alias("cat", "bat")
  end,
})
local eza = mise.package({
  name = "eza",
  rc_builder = function(rc)
    rc:alias("ls", "eza")
  end,
})
return { shells = { "bash" }, configs = { installer, bat, eza } }
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
    let evaluation = run_archive_eval("fonts.zip", &archive, profile);
    assert_eq!(evaluation.documents.len(), 1);
    let found = by_path(&evaluation.documents, "fonts/JetBrainsMono-Bold.ttf");
    match &found.data {
        ManifestData::Opaque { blob, .. } => {
            assert_eq!(
                evaluation.blobs.get(blob),
                Some(&vec![0x00, 0x01, 0x00, 0x00])
            );
        }
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn shell_slot_materializes_outside_final() {
    let profile = r#"
local tool = confit.config("tool")
tool:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.eval({ "mise", "activate", confit.runtime.SHELL }))
end))
return { shells = { "bash" }, configs = { tool } }
"#;
    let documents = run_ok(&[], profile);
    let rc = by_path(&documents, "~/.bashrc");
    match &rc.data {
        ManifestData::Rc(data) => {
            assert_eq!(data.config.len(), 1);
            match &data.config[0].op {
                confit_core::document::RcOp::Eval { argv } => {
                    assert_eq!(
                        *argv,
                        vec![
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
        ManifestData::Rc(data) => {
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

#[test]
fn missing_require_fails_naming_both() {
    let profile = r#"
local bat = confit.config("bat")
bat:require("plugin:solrachq/mise:install")
return { shells = { "bash" }, configs = { bat } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains(r#"config "bat" requires "plugin:solrachq/mise:install" config"#),
        "names both: {error}"
    );
}

#[test]
fn require_hint_renders_on_its_own_line() {
    let profile = r#"
local bat = confit.config("bat")
bat:require("plugin:solrachq/mise:install", "Add mise.install() to the profile configs.")
return { shells = { "bash" }, configs = { bat } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    let message = error.to_string();
    assert!(
        message.contains(r#"config "bat" requires "plugin:solrachq/mise:install" config"#),
        "names both: {message}"
    );
    assert!(
        message.contains("\nhint: Add mise.install() to the profile configs."),
        "hint on own line: {message}"
    );
}

#[test]
fn transitive_require_checks_required_configs() {
    let profile = r#"
local bat = confit.config("bat")
bat:require("helper")
local helper = confit.config("helper")
helper:require("plugin:solrachq/mise:install")
return { shells = { "bash" }, configs = { bat, helper } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains(r#"config "helper" requires "plugin:solrachq/mise:install" config"#),
        "names transitive requirer: {error}"
    );
}

#[test]
fn self_require_cycle_resolves_fine() {
    let profile = r#"
local tool = confit.config("tool")
tool:require("tool")
return { shells = { "bash" }, configs = { tool } }
"#;
    let documents = run_ok(&[], profile);
    assert!(documents.is_empty());
}

#[test]
fn pair_require_cycle_resolves_fine() {
    let profile = r#"
local first = confit.config("first")
first:require("second")
local second = confit.config("second")
second:require("first")
return { shells = { "bash" }, configs = { first, second } }
"#;
    let documents = run_ok(&[], profile);
    assert!(documents.is_empty());
}

#[test]
fn hooks_collect_in_declaration_order() {
    let profile = r#"
local first = confit.config("first")
first:add_hook(confit.hook.run({ "first-tool" }))
local second = confit.config("second")
second:add_hook(confit.hook.run({ "second-tool" }))
return { shells = { "bash" }, configs = { first, second } }
"#;
    let evaluation = run_eval_ok(&[], profile);
    let heads: Vec<String> = evaluation
        .hooks
        .iter()
        .map(|hook| hook.argv[0].clone())
        .collect();
    assert_eq!(
        heads,
        vec!["first-tool".to_string(), "second-tool".to_string()]
    );
}

#[test]
fn hooks_merge_across_configs_with_or_gates() {
    let profile = r#"
local first = confit.config("first")
first:add_hook(confit.hook.run({ "mise", "install" }, {
  path = { "/home/tester/.local/bin" },
  when = confit.runtime.in_path("mise"),
  checks = { confit.runtime.in_path("bat") },
  timeout = "5m",
}))
local second = confit.config("second")
second:add_hook(confit.hook.run({ "mise", "install" }, {
  path = { "/home/tester/.local/bin" },
  when = function(runtime) return runtime.in_path("eza") end,
  checks = { confit.runtime.in_path("eza") },
  timeout = "10m",
}))
return { shells = { "bash" }, configs = { first, second } }
"#;
    let evaluation = run_eval_ok(&[], profile);
    assert_eq!(evaluation.hooks.len(), 1);
    let hook = hook_by_head(&evaluation, "mise");
    assert_eq!(hook.argv, vec!["mise".to_string(), "install".to_string()]);
    assert_eq!(hook.path, vec!["/home/tester/.local/bin".to_string()]);
    assert_eq!(
        hook.checks,
        vec![
            Condition::InPath {
                name: "bat".to_string()
            },
            Condition::InPath {
                name: "eza".to_string()
            },
        ]
    );
    assert_eq!(hook.timeout_secs, 600);
}

#[test]
fn hook_empty_argv_fails_as_plan_error() {
    let error = run_err(
        &[],
        r#"
local c = confit.config("c")
c:add_hook(confit.hook.run({}))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error.to_string().contains("field 'argv'"),
        "names argv: {error}"
    );
}

#[test]
fn hook_unknown_opts_field_fails_as_plan_error() {
    let error = run_err(
        &[],
        r#"
local c = confit.config("c")
c:add_hook(confit.hook.run({ "mise" }, { lane = "first" }))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error.to_string().contains("unknown field 'lane'"),
        "names the field: {error}"
    );
}

#[test]
fn hook_bad_duration_fails_as_plan_error() {
    let error = run_err(
        &[],
        r#"
local c = confit.config("c")
c:add_hook(confit.hook.run({ "mise" }, { timeout = "soon" }))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error.to_string().contains("field 'timeout'"),
        "names timeout: {error}"
    );
}

#[test]
fn hook_bad_condition_fails_as_plan_error() {
    let error = run_err(
        &[],
        r#"
local c = confit.config("c")
c:add_hook(confit.hook.run({ "mise" }, { when = { bogus = {} } }))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error.to_string().contains("field 'when'"),
        "names when: {error}"
    );
}

#[test]
fn hook_non_hook_value_fails_naming_config() {
    let error = run_err(
        &[],
        r#"
local c = confit.config("c")
c:add_hook(confit.document.text("note", "hi"))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(matches!(error, Error::Plan(_)));
    let message = error.to_string();
    assert!(message.contains("config 'c'"), "names config: {message}");
    assert!(message.contains("add_hook"), "names field: {message}");
}

#[test]
fn mise_package_table_folds_version() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local bat = mise.package({ name = "bat", version = "1.2.3" })
return { shells = { "bash" }, configs = { installer, bat } }
"#;
    let documents = run_ok(&[], profile);
    let found = by_path(&documents, "~/.config/mise/config.toml");
    let (_, data) = structured(&found);
    assert_eq!(
        data.get("tools"),
        Some(&serde_json::json!({"bat": "1.2.3"}))
    );
}

#[test]
fn mise_package_positional_fails_as_plan_error() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local bat = mise.package("bat", function(rc)
  rc:alias("cat", "bat")
end)
return { shells = { "bash" }, configs = { bat } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("mise: package opts must be a table"),
        "names the table shape: {error}"
    );
}

#[test]
fn mise_package_unknown_field_fails_as_plan_error() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local bat = mise.package({ name = "bat", callback = function(rc) end })
return { shells = { "bash" }, configs = { bat } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("mise: field 'opts' unknown field 'callback'"),
        "names the field: {error}"
    );
}

#[test]
fn mise_package_without_installer_fails_naming_both() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local bat = mise.package({ name = "bat" })
return { shells = { "bash" }, configs = { bat } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    let message = error.to_string();
    assert!(
        message.contains(r#"config "bat" requires "plugin:solrachq/mise:install" config"#),
        "names both: {message}"
    );
    assert!(
        message.contains("\nhint: Add mise.init() to the profile configs."),
        "hint on own line: {message}"
    );
}

#[test]
fn mise_package_declares_install_hook() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local bat = mise.package({ name = "bat" })
return { shells = { "bash" }, configs = { installer, bat } }
"#;
    let evaluation = run_eval_ok(&[], profile);
    assert_eq!(evaluation.hooks.len(), 1);
    let hook = hook_by_head(&evaluation, "mise");
    assert_eq!(hook.argv, vec!["mise".to_string(), "install".to_string()]);
    assert_eq!(hook.path.len(), 1);
    assert!(
        hook.path[0].ends_with(".local/bin"),
        "home binary dir: {:?}",
        hook.path
    );
    assert_eq!(
        hook.when,
        Some(Condition::InPath {
            name: "mise".to_string()
        })
    );
    assert_eq!(hook.checks.len(), 1);
    match &hook.checks[0] {
        Condition::Exists { path } => {
            assert!(path.ends_with("mise/shims/bat"), "shim path: {path}")
        }
        other => panic!("shim exists check expected, got {other:?}"),
    };
    assert_eq!(hook.timeout_secs, 600);
}

#[test]
fn mise_package_bin_proves_custom_binary() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local ripgrep = mise.package({ name = "ripgrep", bin = "rg" })
return { shells = { "bash" }, configs = { installer, ripgrep } }
"#;
    let evaluation = run_eval_ok(&[], profile);
    let hook = hook_by_head(&evaluation, "mise");
    assert_eq!(hook.checks.len(), 1);
    match &hook.checks[0] {
        Condition::Exists { path } => {
            assert!(path.ends_with("mise/shims/rg"), "shim path: {path}")
        }
        other => panic!("shim exists check expected, got {other:?}"),
    }
}

#[test]
fn mise_package_aliases_render_sorted_with_guard() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local bat = mise.package({ name = "bat", aliases = { c = "bat", cat = "bat" } })
return { shells = { "bash" }, configs = { installer, bat } }
"#;
    let documents = run_ok(&[], profile);
    let rc = by_path(&documents, "~/.bashrc");
    let ManifestData::Rc(data) = &rc.data else {
        panic!("rc expected");
    };
    let names: Vec<&str> = data
        .config
        .iter()
        .filter_map(|entry| match &entry.op {
            confit_core::document::RcOp::Alias { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["c", "cat"]);
}

#[test]
fn mise_package_hooks_merge_across_packages() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local bat = mise.package({ name = "bat" })
local eza = mise.package({ name = "eza" })
return { shells = { "bash" }, configs = { installer, bat, eza } }
"#;
    let evaluation = run_eval_ok(&[], profile);
    assert_eq!(evaluation.hooks.len(), 1);
    let hook = hook_by_head(&evaluation, "mise");
    assert_eq!(hook.argv, vec!["mise".to_string(), "install".to_string()]);
    assert_eq!(hook.checks.len(), 2);
    for (check, name) in hook.checks.iter().zip(["bat", "eza"]) {
        match check {
            Condition::Exists { path } => assert!(
                path.ends_with(&format!("mise/shims/{name}")),
                "shim path: {path}"
            ),
            other => panic!("shim exists check expected, got {other:?}"),
        }
    }
}

#[test]
fn mise_init_explicit_version_builds_installer() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let archive = tar_gz_bytes(&[("mise/bin/mise", b"mise-binary".as_slice(), 0o755)]);
    let url =
        "https://github.com/jdx/mise/releases/download/v2026.9.12/mise-v2026.9.12-linux-x64.tar.gz";
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = mise.init("2026.9.12")
local bat = mise.package({ name = "bat" })
return { shells = { "bash" }, configs = { installer, bat } }
"#;
    let evaluation = match run_fetch_eval(profile, cache.path(), stubbed(url, &archive), false) {
        Ok(evaluation) => evaluation,
        Err(error) => panic!("installer builds: {error}"),
    };
    let documents = &evaluation.documents;
    let found = documents
        .iter()
        .find(|item| item.path.as_str().ends_with(".local/bin/mise"))
        .unwrap_or_else(|| panic!("installer binary missing"));
    match &found.data {
        ManifestData::Opaque { blob, mode } => {
            assert_eq!(evaluation.blobs.get(blob), Some(&b"mise-binary".to_vec()));
            assert_eq!(mode, &Some(0o755));
        }
        other => panic!("opaque expected, got {other:?}"),
    }
    let folded = by_path(documents, "~/.config/mise/config.toml");
    let (_, data) = structured(&folded);
    assert_eq!(
        data.get("tools"),
        Some(&serde_json::json!({"bat": "latest"}))
    );
    let rc = by_path(documents, "~/.bashrc");
    let ManifestData::Rc(rc) = &rc.data else {
        panic!("rc expected");
    };
    assert_eq!(rc.profile.len(), 2);
    match &rc.profile[0].op {
        confit_core::document::RcOp::Path { dir, .. } => {
            assert!(dir.ends_with(".local/bin"), "mise bin dir first: {dir}");
        }
        other => panic!("path expected, got {other:?}"),
    }
    match &rc.profile[1].op {
        confit_core::document::RcOp::Eval { argv } => {
            assert_eq!(argv[0], "mise");
        }
        other => panic!("eval expected, got {other:?}"),
    }
}

#[test]
fn mise_init_resolves_latest_tag() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let archive = tar_gz_bytes(&[("mise/bin/mise", b"mise-binary".as_slice(), 0o755)]);
    let releases = r#"[{"tag_name": "v2026.9.10", "name": "v2026.9.10"}]"#;
    let fake = stubbed(
        "https://api.github.com/repos/jdx/mise/releases",
        releases.as_bytes(),
    );
    fake.insert(
        "https://github.com/jdx/mise/releases/download/v2026.9.10/mise-v2026.9.10-linux-x64.tar.gz",
        &archive,
    );
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = mise.init()
local bat = mise.package({ name = "bat" })
return { shells = { "bash" }, configs = { installer, bat } }
"#;
    let evaluation = match run_fetch_eval(profile, cache.path(), fake, false) {
        Ok(evaluation) => evaluation,
        Err(error) => panic!("installer resolves: {error}"),
    };
    let documents = &evaluation.documents;
    let found = documents
        .iter()
        .find(|item| item.path.as_str().ends_with(".local/bin/mise"))
        .unwrap_or_else(|| panic!("installer binary missing"));
    match &found.data {
        ManifestData::Opaque { blob, .. } => {
            assert_eq!(evaluation.blobs.get(blob), Some(&b"mise-binary".to_vec()));
        }
        other => panic!("opaque expected, got {other:?}"),
    }
}

#[test]
fn mise_init_rejects_bad_version() {
    for profile in [
        r#"
local installer = confit.plugin.solrachq.mise.init("")
return { shells = { "bash" }, configs = { installer } }
"#,
        r#"
local installer = confit.plugin.solrachq.mise.init(42)
return { shells = { "bash" }, configs = { installer } }
"#,
    ] {
        let error = run_err(&[], profile);
        assert!(matches!(error, Error::Plan(_)));
        assert!(
            error
                .to_string()
                .contains("mise: field 'version' must be a non-empty string or nil"),
            "names version: {error}"
        );
    }
}

#[test]
fn mise_init_empty_releases_feed_fails_as_plan_error() {
    let cache = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("cache builds: {error}"),
    };
    let profile = r#"
local installer = confit.plugin.solrachq.mise.init()
return { shells = { "bash" }, configs = { installer } }
"#;
    let outcome = run_fetch(
        profile,
        cache.path(),
        stubbed("https://api.github.com/repos/jdx/mise/releases", b"[]"),
        false,
    );
    let error = match outcome {
        Ok(_) => panic!("empty feed passes"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error.to_string().contains("mise: cannot resolve"),
        "names resolution: {error}"
    );
}

#[test]
fn mise_package_options_render_components_table() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local rust = mise.package({
  name = "rust",
  version = "1.83.0",
  bin = "rustc",
  options = {
    components = { "clippy", "rustfmt", "rust-src", "llvm-tools" },
  },
})
return { shells = { "bash" }, configs = { installer, rust } }
"#;
    let documents = run_ok(&[], profile);
    let found = by_path(&documents, "~/.config/mise/config.toml");
    let (_, data) = structured(&found);
    assert_eq!(
        data.get("tools"),
        Some(&serde_json::json!({
            "rust": {
                "version": "1.83.0",
                "components": ["clippy", "rustfmt", "rust-src", "llvm-tools"],
            },
        }))
    );
}

#[test]
fn mise_package_options_default_version_latest() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local rust = mise.package({
  name = "rust",
  options = {
    components = { "clippy" },
  },
})
return { shells = { "bash" }, configs = { installer, rust } }
"#;
    let documents = run_ok(&[], profile);
    let found = by_path(&documents, "~/.config/mise/config.toml");
    let (_, data) = structured(&found);
    assert_eq!(
        data.get("tools"),
        Some(&serde_json::json!({
            "rust": { "version": "latest", "components": ["clippy"] },
        }))
    );
}

#[test]
fn mise_package_options_scalar_values_land() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local installer = confit.config("plugin:solrachq/mise:install")
local rust = mise.package({
  name = "rust",
  version = "1.83.0",
  options = { profile = "minimal", jobs = 4, locked = true },
})
return { shells = { "bash" }, configs = { installer, rust } }
"#;
    let documents = run_ok(&[], profile);
    let found = by_path(&documents, "~/.config/mise/config.toml");
    let (_, data) = structured(&found);
    assert_eq!(
        data.get("tools"),
        Some(&serde_json::json!({
            "rust": {
                "version": "1.83.0",
                "profile": "minimal",
                "jobs": 4,
                "locked": true,
            },
        }))
    );
}

#[test]
fn mise_package_options_non_table_fails_as_plan_error() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local rust = mise.package({ name = "rust", options = "components" })
return { shells = { "bash" }, configs = { rust } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("mise: field 'options' must be a table"),
        "names options: {error}"
    );
}

#[test]
fn mise_package_options_map_value_fails_naming_key() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local rust = mise.package({ name = "rust", options = { components = { clippy = true } } })
return { shells = { "bash" }, configs = { rust } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("mise: field 'options.components'"),
        "names key: {error}"
    );
}

#[test]
fn mise_package_options_nested_table_fails_naming_key() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local rust = mise.package({ name = "rust", options = { components = { { "clippy" } } } })
return { shells = { "bash" }, configs = { rust } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("mise: field 'options.components'"),
        "names key: {error}"
    );
}

#[test]
fn mise_package_options_function_value_fails_naming_key() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local rust = mise.package({ name = "rust", options = { hook = function() end } })
return { shells = { "bash" }, configs = { rust } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error.to_string().contains("mise: field 'options.hook'"),
        "names key: {error}"
    );
}

#[test]
fn mise_package_options_sparse_array_fails_naming_key() {
    let profile = r#"
local mise = confit.plugin.solrachq.mise
local comps = {}
comps[1] = "clippy"
comps[3] = "rustfmt"
local rust = mise.package({ name = "rust", options = { components = comps } })
return { shells = { "bash" }, configs = { rust } }
"#;
    let error = run_err(&[], profile);
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error
            .to_string()
            .contains("mise: field 'options.components'"),
        "names key: {error}"
    );
}
