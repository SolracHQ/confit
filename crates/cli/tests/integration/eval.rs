use crate::common::*;

#[test]
fn fixture_plans_stay_deterministic() {
    for fixture in [
        "0-basic_tool",
        "1-structured_resource",
        "2-templated_resource",
    ] {
        let cache = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(error) => panic!("cache builds: {error}"),
        };
        let root = examples_root().join(fixture);
        let profile = root.join("profile.lua");
        let first = match evaluate_fetch(&profile, &root, cache.path(), fixture_fetch()) {
            Ok(documents) => match build(documents) {
                Ok(built) => built,
                Err(error) => panic!("{fixture} first bundle builds: {error}"),
            },
            Err(error) => panic!("{fixture} first evaluation runs: {error}"),
        };
        let second = match evaluate_fetch(&profile, &root, cache.path(), fixture_fetch()) {
            Ok(documents) => match build(documents) {
                Ok(built) => built,
                Err(error) => panic!("{fixture} second bundle builds: {error}"),
            },
            Err(error) => panic!("{fixture} second evaluation runs: {error}"),
        };
        assert!(
            !first.manifest.documents.is_empty(),
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
            Err(error) => panic!("bundle builds: {error}"),
        }
    };
    let first = run(&forward);
    let second = run(&swapped);
    assert_eq!(plan_value(&first), plan_value(&second));
    let shared = match serde_json::to_value(&first.manifest.documents) {
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
