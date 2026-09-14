use std::path::{Path, PathBuf};

use confit_core::document::{Document, DocumentData};
use confit_core::error::Error;
use confit_core::ids::{DocPath, ReadOutcome};

/// Points at the workspace examples folder from the cli crate dir.
fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Pins HOME to a shared temp folder so engine path joins stay hermetic.
///
/// Every test pins the same value, so parallel runs cannot diverge.
fn pin_home() -> PathBuf {
    let home = std::env::temp_dir().join("confit-cli-test-home");
    match std::fs::create_dir_all(&home) {
        Ok(()) => {}
        Err(error) => panic!("test home builds: {error}"),
    }
    unsafe {
        std::env::set_var("HOME", &home);
    }
    home
}

/// Evaluates one profile file with no external plugins.
fn evaluate(profile: &Path, root: &Path) -> Result<Vec<Document>, Error> {
    pin_home();
    confit_engine::evaluate(
        profile,
        confit_engine::EvalOpts {
            root: root.to_path_buf(),
            plugins: None,
        },
    )
}

/// Builds a plan off disk with an empty previous state.
fn build(documents: Vec<Document>) -> Result<confit_core::plan::BuiltPlan, Error> {
    let fs = confit_cli::fs::MemoryFs::new();
    confit_cli::actions::run_plan(documents, &confit_core::plan::State::empty(), &|path| {
        confit_cli::fs::snapshot(path, &fs)
    })
}

/// Serializes one built plan with the timestamp blanked for comparison.
fn plan_value(built: &confit_core::plan::BuiltPlan) -> serde_json::Value {
    let mut value = match serde_json::to_value(&built.plan) {
        Ok(value) => value,
        Err(error) => panic!("plan serializes: {error}"),
    };
    if let Some(created) = value.get_mut("created_at") {
        *created = serde_json::Value::String(String::new());
    }
    value
}

/// Writes one profile file into a temp root.
fn write_profile(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    match std::fs::write(&path, contents) {
        Ok(()) => {}
        Err(error) => panic!("profile writes: {error}"),
    }
    path
}

/// Reads the plan error message from a failing evaluation.
fn eval_error(profile: &str) -> String {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let path = write_profile(dir.path(), "profile.lua", profile);
    let error = match evaluate(&path, dir.path()) {
        Ok(_) => panic!("profile passes"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::Plan(_)));
    error.to_string()
}

#[test]
fn fixture_plans_stay_deterministic() {
    for fixture in [
        "0-basic_tool",
        "1-structured_resource",
        "2-templated_resource",
    ] {
        let root = examples_root().join(fixture);
        let profile = root.join("profile.lua");
        let first = match evaluate(&profile, &root) {
            Ok(documents) => match build(documents) {
                Ok(built) => built,
                Err(error) => panic!("{fixture} first plan builds: {error}"),
            },
            Err(error) => panic!("{fixture} first evaluation runs: {error}"),
        };
        let second = match evaluate(&profile, &root) {
            Ok(documents) => match build(documents) {
                Ok(built) => built,
                Err(error) => panic!("{fixture} second plan builds: {error}"),
            },
            Err(error) => panic!("{fixture} second evaluation runs: {error}"),
        };
        assert!(
            !first.plan.documents.is_empty(),
            "{fixture} holds documents"
        );
        assert_eq!(
            plan_value(&first),
            plan_value(&second),
            "{fixture} repeats exactly"
        );
    }
}

#[test]
fn registration_order_swap_yields_identical_bytes() {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => panic!("tempdir builds: {error}"),
    };
    let body = r#"
local first = confit.config("aaa")
first:add_document(confit.document.text("note-a", "alpha"))
first:add_patch(confit.patch.structured("json", "shared.json", function(data)
  data:set("slot", "aaa")
end))
local second = confit.config("zzz")
second:add_document(confit.document.text("note-z", "zeta"))
second:add_patch(confit.patch.structured("json", "shared.json", function(data)
  data:set("slot", "zzz")
end):priority(confit.priority.HIGH))
return { shells = { "bash" }, configs = { %s } }
"#;
    let forward = write_profile(
        dir.path(),
        "forward.lua",
        &body.replace("%s", "first, second"),
    );
    let swapped = write_profile(
        dir.path(),
        "swapped.lua",
        &body.replace("%s", "second, first"),
    );
    let run = |path: &Path| {
        let documents = match evaluate(path, dir.path()) {
            Ok(documents) => documents,
            Err(error) => panic!("profile evaluates: {error}"),
        };
        match build(documents) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        }
    };
    let first = run(&forward);
    let second = run(&swapped);
    assert_eq!(plan_value(&first), plan_value(&second));
    let shared = match serde_json::to_value(&first.plan.documents) {
        Ok(value) => value,
        Err(error) => panic!("documents serialize: {error}"),
    };
    let slot = shared
        .as_array()
        .and_then(|items| items.iter().find(|item| item["path"] == "shared.json"))
        .and_then(|item| item["data"]["structured"]["data"]["slot"].as_str());
    assert!(matches!(slot, Some("zzz")));
}

#[test]
fn bad_format_fails_as_plan_error() {
    let message = eval_error(
        r#"
local c = confit.config("c")
c:add_patch(confit.patch.structured("ini", "app.ini", function(_) end))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(
        message.contains("must be one of"),
        "names the format rule: {message}"
    );
}

#[test]
fn repeat_config_fails_as_plan_error() {
    let message = eval_error(
        r#"
local first = confit.config("dup")
local second = confit.config("dup")
return { shells = { "bash" }, configs = { first, second } }
"#,
    );
    assert!(
        message.contains("already defined"),
        "names the repeat: {message}"
    );
}

#[test]
fn append_on_non_list_fails_as_plan_error() {
    let message = eval_error(
        r#"
local c = confit.config("c")
c:add_document(confit.document.structured("json", { path = "app.json", data = { name = "x" } }))
c:add_patch(confit.patch.structured("json", "app.json", function(data)
  data:append("name", "y")
end))
return { shells = { "bash" }, configs = { c } }
"#,
    );
    assert!(
        message.contains("non-list"),
        "names the shape fault: {message}"
    );
}

/// Builds one text document.
fn text_doc(path: &str, content: &str) -> Document {
    Document::new(
        DocPath::new(path),
        DocumentData::Text {
            content: content.to_string(),
        },
    )
}

#[test]
fn repeated_declaration_fails_as_plan_error() {
    let error = match build(vec![text_doc("x", "first"), text_doc("x", "second")]) {
        Ok(_) => panic!("repeated declaration passes"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::Plan(_)));
    assert!(
        error.to_string().contains("declared more than once"),
        "names the repeat: {error}"
    );
}

#[test]
fn memory_snapshot_covers_present_absent_unreadable() {
    use confit_cli::fs::{Filesystem, MemoryFs, snapshot};

    let mut fs = MemoryFs::new();
    match fs.write(Path::new("present"), b"bytes") {
        Ok(()) => {}
        Err(error) => panic!("memory writes: {error}"),
    }
    fs.mark_unreadable(Path::new("denied"));
    assert!(matches!(
        snapshot(&DocPath::new("present"), &fs),
        ReadOutcome::Present(_)
    ));
    assert!(matches!(
        snapshot(&DocPath::new("missing"), &fs),
        ReadOutcome::Absent
    ));
    assert!(matches!(
        snapshot(&DocPath::new("denied"), &fs),
        ReadOutcome::Unreadable { .. }
    ));
}
