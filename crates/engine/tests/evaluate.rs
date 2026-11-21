#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use confit_engine::Engine;
use confit_events::EventHandles;
use confit_model::arg::Arg;
use confit_model::document::{Data, StructuredDocument, StructuredFormat};
use confit_model::routes::{Route, RouteBase};
use confit_store::{StoreRoots, Stores};
use std::path::{Path, PathBuf};

/// Lua profile with two text configs in zebra then apple order.
const PAIR: &str = r#"
        return {
            configs = {
                {
                    name = id("first"),
                    documents = { { path = route("home", "zebra"), text = "z" } },
                },
                {
                    name = id("second"),
                    documents = { { path = route("home", "apple"), text = "a" } },
                },
            },
        }
    "#;

/// Lua profile with repeated destination documents.
const DUPLICATE: &str = r#"
        return {
            configs = {
                { name = id("first"), documents = { { path = route("home", ".bashrc"), text = "one" } } },
                { name = id("second"), documents = { { path = route("home", ".bashrc"), text = "two" } } },
            },
        }
    "#;

/// Lua profile with one structured toml document.
const STRUCTURED: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/bat"),
                    documents = {
                        {
                            path = route("home", "config.toml"),
                            format = "toml",
                            data = { profile = { shell = "bash" } },
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with repeated config names.
const DUPLICATE_NAMES: &str = r#"
        return {
            configs = {
                { name = id("same"), documents = { { path = route("home", "a"), text = "one" } } },
                { name = id("same"), documents = { { path = route("home", "b"), text = "two" } } },
            },
        }
    "#;

/// Lua profile with two structured patches over one base.
const MERGED_PATCHES: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    documents = {
                        {
                            path = route("home", "config.json"),
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = route("home", "config.json"),
                            format = "json",
                            apply = function(d) d.data.alpha = "one" end,
                        },
                        {
                            target = route("home", "config.json"),
                            format = "json",
                            apply = function(d) d.data.beta = "two" end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one structured patch over a missing target.
const MISSING_TARGET: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    patches = {
                        {
                            target = route("home", "fresh.json"),
                            format = "json",
                            apply = function(d) d.data.made = "yes" end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one structured patch over a text base.
const TEXT_BASE: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    documents = { { path = route("home", "note.txt"), text = "hi" } },
                    patches = {
                        {
                            target = route("home", "note.txt"),
                            format = "json",
                            apply = function(d) d.data.x = 1 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one failing structured callback.
const FAILING_CALLBACK: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    documents = {
                        {
                            path = route("home", "config.json"),
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = route("home", "config.json"),
                            format = "json",
                            apply = function(d) error("patch boom") end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with two rc patches over one base.
const RC_MERGED: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/shell"),
                    documents = {
                        {
                            path = route("home", ".bashrc"),
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = route("home", ".bashrc"),
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "FIRST", value = "1" } } end,
                        },
                        {
                            target = route("home", ".bashrc"),
                            apply = function(d) d.config[#d.config + 1] = { alias = { name = "ll", expansion = "ls -l" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one rc patch over a missing target.
const RC_MISSING: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/shell"),
                    patches = {
                        {
                            target = route("home", ".bashrc"),
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one rc patch over a text base.
const RC_TEXT_BASE: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/shell"),
                    documents = { { path = route("home", "note.txt"), text = "hi" } },
                    patches = {
                        {
                            target = route("home", "note.txt"),
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one failing rc callback.
const RC_FAILING: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/shell"),
                    documents = {
                        {
                            path = route("home", ".bashrc"),
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = route("home", ".bashrc"),
                            apply = function(d) error("rc boom") end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one structured patch over an rc base.
const RC_FORMAT_GAP: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/shell"),
                    documents = {
                        {
                            path = route("home", ".bashrc"),
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = route("home", ".bashrc"),
                            format = "json",
                            apply = function(d) d.data.x = 1 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with a toml patch over a json base.
const FORMAT_GAP: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    documents = {
                        {
                            path = route("home", "config.json"),
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = route("home", "config.json"),
                            format = "toml",
                            apply = function(d) d.data.x = 1 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile reading project text through the resource bridge.
const PROJECT_TEXT: &str = r#"
        local open = require("confit.open")
        local handle = assert(open("note.txt"))
        return {
            configs = {
                {
                    name = id("tools/bat"),
                    documents = { { path = route("home", ".bashrc"), text = assert(handle:text()) } },
                },
            },
        }
    "#;

/// Lua profile with one patch adding count and extra keys.
const PATCHED: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    documents = {
                        {
                            path = route("home", "config.json"),
                            format = "json",
                            data = { count = 1 },
                        },
                    },
                    patches = {
                        {
                            target = route("home", "config.json"),
                            format = "json",
                            apply = function(d) d.data.count = 2 d.data.extra = "yes" end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile mixing structured and rc patches over working targets.
const MIXED_SPLIT: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/both"),
                    documents = {
                        {
                            path = route("home", "config.json"),
                            format = "json",
                            data = { seed = 0 },
                        },
                        {
                            path = route("home", ".bashrc"),
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = route("home", "config.json"),
                            format = "json",
                            apply = function(d) d.data.alpha = "one" end,
                        },
                        {
                            target = route("home", ".bashrc"),
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "FIRST", value = "1" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one rc patch over a structured base.
const RC_OVER_STRUCTURED: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    documents = {
                        {
                            path = route("home", "config.json"),
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = route("home", "config.json"),
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with mixed structured formats over one working target.
const MIXED_FORMATS: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    documents = {
                        {
                            path = route("home", "fresh.json"),
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = route("home", "fresh.json"),
                            format = "json",
                            apply = function(d) d.data.a = 1 end,
                        },
                        {
                            target = route("home", "fresh.json"),
                            format = "toml",
                            apply = function(d) d.data.b = 2 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with structured and rc patches over one structured base.
const MIXED_SAME_TARGET: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    documents = {
                        {
                            path = route("home", "config.json"),
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = route("home", "config.json"),
                            format = "json",
                            apply = function(d) d.data.alpha = "one" end,
                        },
                        {
                            target = route("home", "config.json"),
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one patch with a foreign key.
const FOREIGN_PATCH_KEY: &str = r#"
        return {
            configs = {
                {
                    name = id("tools/app"),
                    patches = {
                        {
                            target = route("home", "config.json"),
                            format = "json",
                            apply = function(d) return d end,
                            nope = 1,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with merged hooks over one text document.
const HOOKS_THROUGH: &str = r#"
            return {
                configs = {
                    {
                        name = id("tools/app"),
                        documents = { { path = route("home", "note.txt"), text = "hi" } },
                        hooks = {
                            { id = id("tools/mise-install"), argv = { "mise", "install" } },
                            { id = id("tools/mise-install-quick"), argv = { "mise", "install" }, timeout = "90" },
                            { id = id("tools/fc-cache"), argv = { "fc-cache" }, timeout = "10m" },
                        },
                    },
                },
            }
        "#;

/// Returns one engine over isolated stores.
fn test_engine(dir: &Path) -> Engine {
    let stores = Stores::new(
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        },
        EventHandles::silent(),
    );
    Engine::new(stores, EventHandles::silent())
}

/// Writes the profile file and returns both paths.
fn profile_with(dir: &Path, source: &str) -> (PathBuf, PathBuf) {
    let root = project_root(dir);
    let profile = root.join("profile.lua");
    std::fs::write(&profile, source.as_bytes()).unwrap();
    (root, profile)
}

/// Creates the project root.
fn project_root(dir: &Path) -> PathBuf {
    let root = dir.join("project");
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// Returns the text content.
fn text_of(data: &Data) -> &str {
    match data {
        Data::Text(found) => &found.content,
        _ => panic!("expected a text document"),
    }
}

#[test]
fn evaluate_keeps_text_in_destination_order() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), PAIR);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 2);
            let displays: Vec<String> = found
                .documents
                .iter()
                .map(|document| document.path.display())
                .collect();
            assert_eq!(
                displays,
                vec!["home('apple')".to_owned(), "home('zebra')".to_owned()]
            );
            let apple = Route::new(RouteBase::Home, "apple").unwrap();
            let zebra = Route::new(RouteBase::Home, "zebra").unwrap();
            let apple_data = &found
                .documents
                .iter()
                .find(|document| document.path == apple)
                .unwrap()
                .data;
            let zebra_data = &found
                .documents
                .iter()
                .find(|document| document.path == zebra)
                .unwrap()
                .data;
            assert_eq!(text_of(apple_data), "a");
            assert_eq!(text_of(zebra_data), "z");
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_builds_structured_data() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), STRUCTURED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let document = found.documents.first().unwrap();
            assert_eq!(document.path.display(), "home('config.toml')");
            match &document.data {
                Data::Structured(StructuredDocument { format, data }) => {
                    assert_eq!(*format, StructuredFormat::Toml);
                    match data.get("profile").and_then(|item| item.get("shell")) {
                        Some(shell) => assert_eq!(shell.as_str(), Some("bash")),
                        None => panic!("expected the shell key"),
                    }
                }
                _ => panic!("expected a structured body"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_duplicate_destinations() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), DUPLICATE);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects duplicate destinations"
    );
}

#[test]
fn evaluate_uses_the_project_root_from_stores() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let root = project_root(dir.path());
    std::fs::write(root.join("note.txt"), b"hello").unwrap();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let profile = elsewhere.join("profile.lua");
    std::fs::write(&profile, PROJECT_TEXT.as_bytes()).unwrap();
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            assert_eq!(text_of(&found.documents.first().unwrap().data), "hello");
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn project_text_reaches_the_documents() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let root = project_root(dir.path());
    std::fs::write(root.join("note.txt"), b"hello").unwrap();
    let profile = root.join("profile.lua");
    std::fs::write(&profile, PROJECT_TEXT.as_bytes()).unwrap();
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            assert_eq!(text_of(&found.documents.first().unwrap().data), "hello");
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn patches_apply_over_declared_documents() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), PATCHED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            match &found.documents.first().unwrap().data {
                Data::Structured(StructuredDocument { data, .. }) => {
                    assert_eq!(
                        data.get("count").and_then(|item| item.as_u64()),
                        Some(1),
                        "the base keys keep their values against later writes",
                    );
                    assert_eq!(
                        data.get("extra").and_then(|item| item.as_str()),
                        Some("yes"),
                        "the fresh keys take the writing patch",
                    );
                }
                _ => panic!("the patched document is structured"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_duplicate_names() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), DUPLICATE_NAMES);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects duplicate config names"
    );
}

#[test]
fn two_patches_merge_both_keys() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MERGED_PATCHES);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let document = found.documents.first().unwrap();
            assert_eq!(document.path.display(), "home('config.json')");
            match &document.data {
                Data::Structured(StructuredDocument { format, data }) => {
                    assert_eq!(*format, StructuredFormat::Json);
                    assert_eq!(
                        data.get("alpha").and_then(|item| item.as_str()),
                        Some("one")
                    );
                    assert_eq!(data.get("beta").and_then(|item| item.as_str()), Some("two"));
                    assert_eq!(data.get("seed").and_then(|item| item.as_u64()), Some(0));
                }
                _ => panic!("the merged document is structured"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_an_absent_target() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MISSING_TARGET);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a missing patch target"
    );
}

#[test]
fn evaluate_rejects_a_text_base() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), TEXT_BASE);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a patch over text"
    );
}

#[test]
fn evaluate_rejects_a_callback_error() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FAILING_CALLBACK);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a failing callback"
    );
}

#[test]
fn evaluate_rejects_a_format_gap() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FORMAT_GAP);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a format gap"
    );
}

#[test]
fn rc_two_patches_merge_entries() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_MERGED);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 1);
            let document = found.documents.first().unwrap();
            assert_eq!(document.path.display(), "home('.bashrc')");
            assert_eq!(
                rc_names(&document.data),
                vec!["SEED".to_owned(), "FIRST".to_owned()]
            );
            match &document.data {
                Data::Rc(rc) => {
                    assert_eq!(rc.config.len(), 1);
                }
                _ => panic!("the merged document is rc"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_an_absent_rc_target() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_MISSING);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a missing rc target"
    );
}

#[test]
fn evaluate_rejects_an_rc_text_base() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_TEXT_BASE);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects an rc patch over text"
    );
}

#[test]
fn evaluate_rejects_an_rc_callback_error() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_FAILING);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a failing rc callback"
    );
}

#[test]
fn evaluate_rejects_an_rc_format_gap() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_FORMAT_GAP);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects an rc format gap"
    );
}

#[test]
fn mixed_sides_merge_working_targets() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_SPLIT);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.documents.len(), 2);
            let ordered: &[_] = &found.documents;
            match &ordered[0].data {
                Data::Rc(_) => {
                    assert_eq!(ordered[0].path.display(), "home('.bashrc')")
                }
                _ => panic!("the first document is rc"),
            }
            match &ordered[1].data {
                Data::Structured(StructuredDocument { data, .. }) => {
                    assert_eq!(ordered[1].path.display(), "home('config.json')");
                    assert_eq!(
                        data.get("alpha").and_then(|item| item.as_str()),
                        Some("one")
                    );
                    assert_eq!(data.get("seed").and_then(|item| item.as_u64()), Some(0));
                }
                _ => panic!("the second document is structured"),
            }
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_rejects_an_rc_patch_over_structured() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), RC_OVER_STRUCTURED);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects an rc patch over a structured body"
    );
}

#[test]
fn evaluate_rejects_mixed_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_FORMATS);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects mixed formats"
    );
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), MIXED_SAME_TARGET);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects a mixed kind target"
    );
}

#[test]
fn evaluate_rejects_an_unknown_patch_key() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), FOREIGN_PATCH_KEY);
    assert!(
        engine.evaluate(&profile).is_err(),
        "the engine rejects an unknown patch key"
    );
}

#[test]
fn evaluation_carries_hooks_through() {
    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let (_, profile) = profile_with(dir.path(), HOOKS_THROUGH);
    match engine.evaluate(&profile) {
        Ok(found) => {
            assert_eq!(found.hooks.len(), 3);
            assert_eq!(Arg::join(&found.hooks[0].argv), "mise install");
            assert_eq!(found.hooks[0].timeout_secs.secs(), 600);
            assert_eq!(Arg::join(&found.hooks[1].argv), "mise install");
            assert_eq!(found.hooks[1].timeout_secs.secs(), 90);
            assert_eq!(Arg::join(&found.hooks[2].argv), "fc-cache");
            assert_eq!(found.hooks[2].timeout_secs.secs(), 600);
        }
        Err(error) => panic!("the profile fails to evaluate: {error}"),
    }
}

#[test]
fn evaluate_reports_an_invalid_route_base() {
    use confit_core::error::{ErrorCause, EvalError};

    let dir = tempfile::tempdir().unwrap();
    let engine = test_engine(dir.path());
    let source = r#"
        return {
            documents = {
                { path = route("void", "note.txt"), text = "hi" },
            },
        }
    "#;
    let (_, profile) = profile_with(dir.path(), source);
    match engine.evaluate(&profile) {
        Ok(_) => panic!("the engine accepts a bad route base"),
        Err(error) => {
            assert!(
                matches!(error.cause(), ErrorCause::EvalError(EvalError::LuaFailed)),
                "the cause shows the failure: {error}"
            );
            assert!(
                error.to_string().contains("is not a valid variant"),
                "the line shows the shape failure: {error}"
            );
        }
    }
}
