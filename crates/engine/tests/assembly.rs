mod common;

use common::*;
use confit_engine::api::config::Config;
use confit_engine::api::patch::{Level, Patch};
use confit_engine::api::profile::Profile;
use confit_engine::{assemble, assembly::Assembler};
use confit_model::document::{Data, Payload, StructuredFormat, TreeMember};
use confit_model::routes::{Route, RouteBase};
use mlua::Lua;
use std::collections::{BTreeMap, HashMap};

#[test]
fn duplicates_fail_across_configs() {
    let lua = Lua::new();
    let profile = Profile::new(
        vec![
            Config::new(
                "first".to_owned(),
                [placed_text(".bashrc", "one")].into_iter().collect(),
                vec![],
                Vec::new(),
                HashMap::new(),
            ),
            Config::new(
                "second".to_owned(),
                [placed_text(".bashrc", "two")].into_iter().collect(),
                vec![],
                Vec::new(),
                HashMap::new(),
            ),
        ],
        std::collections::HashMap::new(),
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("duplicate destinations pass"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the destination: {text}"
            );
        }
    }
}

#[test]
fn repeated_names_fail() {
    let lua = Lua::new();
    let profile = Profile::new(
        vec![
            Config::new(
                "same".to_owned(),
                [placed_text("a", "one")].into_iter().collect(),
                vec![],
                Vec::new(),
                HashMap::new(),
            ),
            Config::new(
                "same".to_owned(),
                [placed_text("b", "two")].into_iter().collect(),
                vec![],
                Vec::new(),
                HashMap::new(),
            ),
        ],
        std::collections::HashMap::new(),
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("repeated names pass"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains("same"), "the text shows the config: {text}");
            assert!(
                text.contains("already defined"),
                "the text shows the fault: {text}"
            );
        }
    }
}

#[test]
fn top_level_documents_assemble() {
    let lua = Lua::new();
    let profile = Profile::new(
        Vec::new(),
        [placed_text("top.txt", "hi")].into_iter().collect(),
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let route = Route::new(RouteBase::Home, "top.txt").unwrap();
            assert!(documents.contains_key(&route), "top document assembles");
        }
        Err(error) => panic!("top documents assemble: {error}"),
    }
}

#[test]
fn duplicates_fail_across_profile_and_configs() {
    let lua = Lua::new();
    let profile = Profile::new(
        vec![Config::new(
            "tools/test".to_owned(),
            [placed_text(".bashrc", "two")].into_iter().collect(),
            vec![],
            Vec::new(),
            HashMap::new(),
        )],
        [placed_text(".bashrc", "one")].into_iter().collect(),
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("duplicate destinations pass"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the destination: {text}"
            );
        }
    }
}

#[test]
fn survivors_order_by_route() {
    let lua = Lua::new();
    let profile = profile_of(
        vec![placed_text("zebra", "z"), placed_text("apple", "a")],
        vec![],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 2);
            let displays: Vec<String> = documents.keys().map(|route| route.display()).collect();
            assert_eq!(
                displays,
                vec!["home('apple')".to_owned(), "home('zebra')".to_owned()]
            );
        }
        Err(error) => panic!("distinct destinations assemble: {error}"),
    }
}

#[test]
fn placed_documents_convert_every_kind() {
    let _guard = confit_driver::fs::TestGuard::install();
    let lua = Lua::new();
    let (_held, stores) = test_stores();
    let root = stores.resources().roots.project_root.clone();
    confit_driver::fs::create_dir_all(&root).unwrap();
    confit_driver::fs::write(&root.join("glyph.ttf"), b"ttf").unwrap();
    let resource = stores.resources().resource("glyph.ttf").unwrap();
    let opened = stores.blobs().put(&resource).unwrap();
    let blob = opened.sha256().clone();
    let mut members = BTreeMap::new();
    members.insert(
        "sub/nested.txt".to_owned(),
        TreeMember::Full {
            handle: Payload::Resolved(blob.clone()),
            mode: 0o644,
        },
    );
    members.insert(
        "top.txt".to_owned(),
        TreeMember::Full {
            handle: Payload::Resolved(blob.clone()),
            mode: 0o644,
        },
    );
    let documents = vec![
        placed_text("note.txt", "hi"),
        placed_structured("config.json"),
        (
            Route::new(RouteBase::Home, "linked").unwrap(),
            Data::Link {
                target: Route::new(RouteBase::Home, "origin").unwrap(),
            },
        ),
        placed_rc(".bashrc", vec![rc_entry("SEED", "0")]),
        placed_opaque(&stores, "blob.bin"),
        (
            Route::new(RouteBase::Home, "fonts").unwrap(),
            Data::Tree { members },
        ),
    ];
    let profile = profile_of(documents, vec![]);
    match assemble(&lua, &profile) {
        Ok(found) => {
            assert_eq!(found.len(), 6);
            let by_display = |display: &str| {
                found
                    .iter()
                    .find(|(route, _)| route.display() == display)
                    .map(|(_, data)| data)
                    .unwrap_or_else(|| panic!("{display} assembles"))
            };
            match by_display("home('note.txt')") {
                Data::Text {
                    content,
                    mode,
                    unmanaged,
                } => {
                    assert_eq!(content, "hi");
                    assert_eq!(*mode, 0o644);
                    assert!(!unmanaged);
                }
                _ => panic!("text stays text"),
            }
            match by_display("home('config.json')") {
                Data::Structured { format, data } => {
                    assert_eq!(*format, StructuredFormat::Json);
                    assert_eq!(data.get("seed").and_then(|item| item.as_u64()), Some(0));
                }
                _ => panic!("structured stays structured"),
            }
            match by_display("home('linked')") {
                Data::Link { target } => assert_eq!(target.display(), "home('origin')"),
                _ => panic!("link stays link"),
            }
            assert_eq!(
                rc_names(by_display("home('.bashrc')")),
                vec!["SEED".to_owned()]
            );
            match by_display("home('blob.bin')") {
                Data::Opaque {
                    mode, unmanaged, ..
                } => {
                    assert_eq!(*mode, 0o644);
                    assert!(unmanaged);
                }
                _ => panic!("opaque stays opaque"),
            }
            match by_display("home('fonts')") {
                Data::Tree { members: found } => {
                    assert_eq!(
                        found.keys().map(String::as_str).collect::<Vec<_>>(),
                        vec!["sub/nested.txt", "top.txt"],
                        "tree members hold flat paths in key order"
                    );
                }
                _ => panic!("tree stays tree"),
            }
        }
        Err(error) => panic!("every kind assembles: {error}"),
    }
}

#[test]
fn structured_patches_apply_in_priority_order() {
    let lua = Lua::new();
    let minor = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Minor,
        r#"return function(_, document) document.data.winner = "minor" end"#,
    );
    let major = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Major,
        r#"return function(_, document) document.data.winner = "major" end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(minor), Patch::Structured(major)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            match documents.values().next().unwrap() {
                Data::Structured { data, .. } => {
                    assert_eq!(
                        data.get("winner").and_then(|item| item.as_str()),
                        Some("major"),
                        "higher levels apply first with first writes winning",
                    );
                }
                _ => panic!("structured patches stay structured"),
            }
        }
        Err(error) => panic!("ranked patches merge: {error}"),
    }
}

#[test]
fn rc_patches_apply_in_priority_order() {
    let lua = Lua::new();
    let low = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Low,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "LOW", value = "1" } } end"#,
    );
    let high = rc_patch_of(
        &lua,
        ".bashrc",
        Level::High,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "HIGH", value = "2" } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(low), Patch::Rc(high)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            assert_eq!(
                rc_names(documents.values().next().unwrap()),
                vec!["SEED".to_owned(), "HIGH".to_owned(), "LOW".to_owned(),],
                "higher levels apply first",
            );
        }
        Err(error) => panic!("ranked rc patches merge: {error}"),
    }
}

#[test]
fn equal_priorities_keep_declaration_order() {
    let lua = Lua::new();
    let first = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(_, document) document.data.winner = "first" end"#,
    );
    let second = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(_, document) document.data.winner = "second" end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(first), Patch::Structured(second)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            match documents.values().next().unwrap() {
                Data::Structured { data, .. } => {
                    assert_eq!(
                        data.get("winner").and_then(|item| item.as_str()),
                        Some("first"),
                        "equal levels keep declaration order with first writes winning",
                    );
                }
                _ => panic!("structured patches stay structured"),
            }
        }
        Err(error) => panic!("equal priorities merge: {error}"),
    }
}

#[test]
fn rc_first_write_wins_shared_keys() {
    let lua = Lua::new();
    let low = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Low,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "DUP", value = "1" } } end"#,
    );
    let high = rc_patch_of(
        &lua,
        ".bashrc",
        Level::High,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "DUP", value = "2" } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(low), Patch::Rc(high)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let data = documents.values().next().unwrap();
            assert_eq!(
                rc_names(data),
                vec!["SEED".to_owned(), "DUP".to_owned()],
                "later duplicates drop keeping the first entry",
            );
            assert_eq!(
                rc_value(data, "DUP").as_deref(),
                Some("2"),
                "higher levels apply first with first writes winning",
            );
        }
        Err(error) => panic!("shared keys merge: {error}"),
    }
}

#[test]
fn structured_base_beats_every_patch() {
    let lua = Lua::new();
    let patch = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Major,
        r#"return function(_, document) document.data.seed = 99 end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            match documents.values().next().unwrap() {
                Data::Structured { data, .. } => {
                    assert_eq!(
                        data.get("seed").and_then(|item| item.as_u64()),
                        Some(0),
                        "base keys keep their values against later writes",
                    );
                }
                _ => panic!("structured patches stay structured"),
            }
        }
        Err(error) => panic!("base keys merge: {error}"),
    }
}

#[test]
fn rc_base_beats_every_patch() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Major,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "SEED", value = "1" } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let data = documents.values().next().unwrap();
            assert_eq!(
                rc_names(data),
                vec!["SEED".to_owned()],
                "base entries survive later writes",
            );
            assert_eq!(
                rc_value(data, "SEED").as_deref(),
                Some("0"),
                "base keys keep their values against later writes",
            );
        }
        Err(error) => panic!("base keys merge: {error}"),
    }
}

#[test]
fn rc_patches_merge_entries() {
    let lua = Lua::new();
    let first = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "FIRST", value = "1" } } end"#,
    );
    let second = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "SECOND", value = "2" } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(first), Patch::Rc(second)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let (route, data) = documents.iter().next().unwrap();
            assert_eq!(route.display(), "home('.bashrc')");
            assert_eq!(
                rc_names(data),
                vec!["SEED".to_owned(), "FIRST".to_owned(), "SECOND".to_owned()]
            );
        }
        Err(error) => panic!("rc patches merge: {error}"),
    }
}

#[test]
fn rc_missing_targets_fail_showing_targets() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "MADE", value = "yes" } } end"#,
    );
    let profile = profile_of(vec![], vec![Patch::Rc(patch)]);
    match assemble(&lua, &profile) {
        Ok(_) => panic!("missing rc target passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the target: {text}"
            );
        }
    }
}

#[test]
fn structured_patches_merge_data() {
    let lua = Lua::new();
    let first = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(_, document) document.data.alpha = "one" end"#,
    );
    let second = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(_, document) document.data.beta = "two" end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(first), Patch::Structured(second)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let (route, data) = documents.iter().next().unwrap();
            assert_eq!(route.display(), "home('config.json')");
            match data {
                Data::Structured { format, data } => {
                    assert_eq!(*format, StructuredFormat::Json);
                    assert_eq!(
                        data.get("alpha").and_then(|item| item.as_str()),
                        Some("one")
                    );
                    assert_eq!(data.get("beta").and_then(|item| item.as_str()), Some("two"));
                    assert_eq!(data.get("seed").and_then(|item| item.as_u64()), Some(0));
                }
                _ => panic!("structured patches stay structured"),
            }
        }
        Err(error) => panic!("structured patches merge: {error}"),
    }
}

#[test]
fn structured_missing_targets_fail_showing_targets() {
    let lua = Lua::new();
    let patch = structured_patch_of(
        &lua,
        "fresh.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(_, document) document.data.made = "yes" end"#,
    );
    let profile = profile_of(vec![], vec![Patch::Structured(patch)]);
    match assemble(&lua, &profile) {
        Ok(_) => panic!("missing structured target passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('fresh.json')"),
                "the text shows the target: {text}"
            );
        }
    }
}

#[test]
fn structured_mixed_formats_fail_showing_formats() {
    let lua = Lua::new();
    let first = structured_patch_of(
        &lua,
        "fresh.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(_, document) return document end"#,
    );
    let second = structured_patch_of(
        &lua,
        "fresh.json",
        StructuredFormat::Toml,
        Level::Normal,
        r#"return function(_, document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_structured("fresh.json")],
        vec![Patch::Structured(first), Patch::Structured(second)],
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("mixed structured formats pass"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('fresh.json')"),
                "the text shows the target: {text}"
            );
            assert!(
                text.contains("json"),
                "the text shows the first format: {text}"
            );
            assert!(
                text.contains("toml"),
                "the text shows the second format: {text}"
            );
        }
    }
}

#[test]
fn structured_patches_refuse_text_bases() {
    let lua = Lua::new();
    let patch = structured_patch_of(
        &lua,
        "note.txt",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(_, document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_text("note.txt", "hi")],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("structured patch over text passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('note.txt')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("'text'"), "the text shows the base: {text}");
            assert!(text.contains("'json'"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn rc_patches_refuse_structured_bases() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        "config.json",
        Level::Normal,
        r#"return function(_, document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("rc patch over structured passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('config.json')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("'json'"), "the text shows the base: {text}");
            assert!(text.contains("'rc'"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn rc_patches_refuse_text_bases() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        "note.txt",
        Level::Normal,
        r#"return function(_, document) return document end"#,
    );
    let profile = profile_of(vec![placed_text("note.txt", "hi")], vec![Patch::Rc(patch)]);
    match assemble(&lua, &profile) {
        Ok(_) => panic!("patched text passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('note.txt')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("'text'"), "the text shows the base: {text}");
            assert!(text.contains("'rc'"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn format_patches_gap_on_rc_bases() {
    let lua = Lua::new();
    let patch = structured_patch_of(
        &lua,
        ".bashrc",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(_, document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("format patch over rc passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the target: {text}"
            );
            assert!(text.contains("'rc'"), "the text shows the base: {text}");
            assert!(text.contains("'json'"), "the text shows the patch: {text}");
        }
    }
}

#[test]
fn rc_failing_callbacks_fail() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(_, document) error("rc boom") end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("failing rc callback passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains("rc boom"), "carries the cause: {text}");
        }
    }
}

#[test]
fn rc_broken_bodies_fail_showing_entries() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(_, document) document.profile = 1 end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("broken rc body passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("RcData.profile"),
                "the text shows the section: {text}"
            );
        }
    }
}

#[test]
fn hooks_pass_through_without_merging() {
    let lua = Lua::new();
    let profile = profile_with_hooks(vec![
        Config::new(
            "tools/first".to_owned(),
            HashMap::new(),
            Vec::new(),
            vec![
                hook_of(&["mise", "install"], 60),
                hook_of(&["fc-cache"], 60),
            ],
            HashMap::new(),
        ),
        Config::new(
            "tools/second".to_owned(),
            HashMap::new(),
            Vec::new(),
            vec![hook_of(&["mise", "install"], 600)],
            HashMap::new(),
        ),
    ]);
    let documents = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &documents) {
        Ok(hooks) => {
            assert_eq!(hooks.len(), 3);
            assert_eq!(confit_model::arg::Arg::join(&hooks[0].argv), "mise install");
            assert_eq!(hooks[0].timeout_secs, 60);
            assert_eq!(confit_model::arg::Arg::join(&hooks[1].argv), "fc-cache");
            assert_eq!(confit_model::arg::Arg::join(&hooks[2].argv), "mise install");
            assert_eq!(hooks[2].timeout_secs, 600);
        }
        Err(error) => panic!("hooks collect: {error}"),
    }
}

#[test]
fn repeated_hook_ids_fail_showing_ids() {
    let lua = Lua::new();
    let profile = profile_with_hooks(vec![
        Config::new(
            "tools/first".to_owned(),
            HashMap::new(),
            Vec::new(),
            vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
            HashMap::new(),
        ),
        Config::new(
            "tools/second".to_owned(),
            HashMap::new(),
            Vec::new(),
            vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
            HashMap::new(),
        ),
    ]);
    let documents = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &documents) {
        Ok(_) => panic!("repeated hook ids pass"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("mise:install"),
                "the text shows the id: {text}"
            );
        }
    }
}

#[test]
fn hook_patches_mutate_live_hooks_in_order() {
    let lua = Lua::new();
    let patch = hook_patch_of(
        &lua,
        "mise:install",
        r#"return function(hook) hook.check = { in_path = { name = "extra" } } end"#,
    );
    let profile = profile_with_hooks(vec![Config::new(
        "tools/test".to_owned(),
        HashMap::new(),
        vec![Patch::Hook(patch)],
        vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
        HashMap::new(),
    )]);
    let documents = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &documents) {
        Ok(hooks) => {
            assert_eq!(hooks.len(), 1);
            assert_eq!(hooks[0].id.as_deref(), Some("mise:install"));
            assert_eq!(
                hooks[0].check,
                confit_model::condition::Condition::InPath {
                    name: "extra".to_owned(),
                }
            );
        }
        Err(error) => panic!("hook patch applies: {error}"),
    }
}

#[test]
fn hook_patches_append_checks_through_all_unions() {
    let lua = Lua::new();
    let first = hook_patch_of(
        &lua,
        "mise:install",
        r#"return function(hook) hook.check = { in_path = { name = "first" } } end"#,
    );
    let second = hook_patch_of(
        &lua,
        "mise:install",
        r#"return function(hook)
            local next = { in_path = { name = "second" } }
            if hook.check.all ~= nil then
                local members = {}
                for _, item in ipairs(hook.check.all) do
                    members[#members + 1] = item
                end
                members[#members + 1] = next
                hook.check = { all = members }
            else
                hook.check = { all = { hook.check, next } }
            end
        end"#,
    );
    let profile = profile_with_hooks(vec![Config::new(
        "tools/test".to_owned(),
        HashMap::new(),
        vec![Patch::Hook(first), Patch::Hook(second)],
        vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
        HashMap::new(),
    )]);
    let documents = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &documents) {
        Ok(hooks) => {
            assert_eq!(hooks.len(), 1);
            assert_eq!(
                hooks[0].check,
                confit_model::condition::Condition::All(vec![
                    confit_model::condition::Condition::InPath {
                        name: "first".to_owned(),
                    },
                    confit_model::condition::Condition::InPath {
                        name: "second".to_owned(),
                    },
                ])
            );
        }
        Err(error) => panic!("hook patches append: {error}"),
    }
}

#[test]
fn hook_patches_over_missing_ids_fail() {
    let lua = Lua::new();
    let patch = hook_patch_of(
        &lua,
        "mise:absent",
        r#"return function(hook) return hook end"#,
    );
    let profile = profile_with_hooks(vec![Config::new(
        "tools/test".to_owned(),
        HashMap::new(),
        vec![Patch::Hook(patch)],
        vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
        HashMap::new(),
    )]);
    let documents = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &documents) {
        Ok(_) => panic!("missing hook target passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("mise:absent"),
                "the text shows the id: {text}"
            );
        }
    }
}

#[test]
fn missing_requirements_fail_with_hints() {
    let lua = Lua::new();
    let profile = Profile::new(
        vec![Config::new(
            "tools/bat".to_owned(),
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            HashMap::from([(
                "plugin:_/mise:install".to_owned(),
                "add mise.init() to the profile configs".to_owned(),
            )]),
        )],
        HashMap::new(),
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("missing requirement passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("tools/bat"),
                "the text shows the requirer: {text}"
            );
            assert!(
                text.contains("plugin:_/mise:install"),
                "the text shows the target: {text}"
            );
            assert!(
                text.contains("add mise.init() to the profile configs"),
                "the text shows the hint: {text}"
            );
        }
    }
}

#[test]
fn present_requirements_pass() {
    let lua = Lua::new();
    let profile = Profile::new(
        vec![
            Config::new(
                "plugin:_/mise:install".to_owned(),
                HashMap::new(),
                Vec::new(),
                Vec::new(),
                HashMap::new(),
            ),
            Config::new(
                "tools/bat".to_owned(),
                HashMap::new(),
                Vec::new(),
                Vec::new(),
                HashMap::from([(
                    "plugin:_/mise:install".to_owned(),
                    "add mise.init() to the profile configs".to_owned(),
                )]),
            ),
        ],
        HashMap::new(),
    );
    match assemble(&lua, &profile) {
        Ok(documents) => assert!(documents.is_empty()),
        Err(error) => panic!("present requirement fails: {error}"),
    }
}

#[test]
fn changed_conditions_over_unbuilt_routes_fail() {
    let lua = Lua::new();
    let present = Route::new(RouteBase::Home, "present").unwrap();
    let missing = Route::new(RouteBase::Home, "missing").unwrap();
    let mut built_hook = hook_of(&["mise"], 60);
    built_hook.when = confit_model::condition::Condition::Changed {
        route: present.clone(),
    };
    let documents = [placed_text("present", "hi")];
    let profile = profile_with_hooks(vec![Config::new(
        "tools/test".to_owned(),
        documents.into_iter().collect(),
        Vec::new(),
        vec![built_hook],
        HashMap::new(),
    )]);
    let finished = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &finished) {
        Ok(hooks) => assert_eq!(hooks.len(), 1),
        Err(error) => panic!("built changed condition passes: {error}"),
    }
    let mut foreign_hook = hook_of(&["mise"], 60);
    foreign_hook.when = confit_model::condition::Condition::Changed { route: missing };
    let profile = profile_with_hooks(vec![Config::new(
        "tools/test".to_owned(),
        HashMap::new(),
        Vec::new(),
        vec![foreign_hook],
        HashMap::new(),
    )]);
    let finished = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &finished) {
        Ok(_) => panic!("changed condition over unbuilt route passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('missing')"),
                "the text shows the route: {text}"
            );
        }
    }
}

#[test]
fn rc_changed_guards_in_base_fail() {
    let lua = Lua::new();
    let target = Route::new(RouteBase::Home, ".bashrc").unwrap();
    let entry = confit_model::document::RcEntry::Env {
        name: "POISONED".to_owned(),
        value: "1".to_owned(),
        when: Some(confit_model::condition::Condition::Changed {
            route: target.clone(),
        }),
    };
    let profile = profile_of(vec![placed_rc(".bashrc", vec![entry])], Vec::new());
    match assemble(&lua, &profile) {
        Ok(_) => panic!("changed rc guard passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the document: {text}"
            );
            assert!(
                text.contains("hooks only"),
                "the text shows the refusal: {text}"
            );
        }
    }
}

#[test]
fn rc_changed_guards_from_patches_fail() {
    let lua = Lua::new();
    let smuggled = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(_, document) document.profile[#document.profile + 1] = { env = { name = "SMUGGLED", value = "1", when = { changed = { route = { base = "home", relative = ".bashrc" } } } } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(smuggled)],
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("patch-smuggled changed guard passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("home('.bashrc')"),
                "the text shows the document: {text}"
            );
            assert!(
                text.contains("hooks only"),
                "the text shows the refusal: {text}"
            );
        }
    }
}

#[test]
fn rc_plain_guards_pass() {
    let lua = Lua::new();
    let entry = confit_model::document::RcEntry::Env {
        name: "GUARDED".to_owned(),
        value: "1".to_owned(),
        when: Some(confit_model::condition::Condition::InPath {
            name: "mise".to_owned(),
        }),
    };
    let profile = profile_of(vec![placed_rc(".bashrc", vec![entry])], Vec::new());
    match assemble(&lua, &profile) {
        Ok(documents) => assert_eq!(
            rc_names(documents.values().next().unwrap()),
            vec!["GUARDED".to_owned()],
            "guards without changed pass through",
        ),
        Err(error) => panic!("plain rc guard fails: {error}"),
    }
}

#[test]
fn structured_filters_reach_every_match_in_order() {
    let lua = Lua::new();
    lua.globals()
        .set("order", lua.create_table().unwrap())
        .unwrap();
    let patch = structured_filter_of(
        &lua,
        r#"return function(route) return string.find(route.relative, "json") ~= nil end"#,
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(route, document) table.insert(order, route.relative) document.data.touched = true end"#,
    );
    let profile = profile_of(
        vec![
            placed_structured("zebra.json"),
            placed_structured("apple.json"),
            placed_rc(".bashrc", vec![rc_entry("SEED", "0")]),
        ],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 3);
            for (route, body) in &documents {
                match body {
                    Data::Structured { data, .. } if route.display().contains(".json") => {
                        assert_eq!(
                            data.get("touched").and_then(|item| item.as_bool()),
                            Some(true),
                            "every matched document takes the patch"
                        );
                    }
                    _ => continue,
                }
            }
            let order: String = lua
                .load("return table.concat(order, ',')")
                .call(())
                .unwrap();
            assert_eq!(order, "apple.json,zebra.json");
        }
        Err(error) => panic!("filter fan-out merges: {error}"),
    }
}

#[test]
fn rc_filters_reach_every_match_in_order() {
    let lua = Lua::new();
    lua.globals()
        .set("order", lua.create_table().unwrap())
        .unwrap();
    let patch = rc_filter_of(
        &lua,
        r#"return function(route) return string.find(route.relative, "sh") ~= nil end"#,
        Level::Normal,
        r#"return function(route, document) table.insert(order, route.relative) document.profile[#document.profile + 1] = { env = { name = "TOUCHED", value = "1" } } end"#,
    );
    let profile = profile_of(
        vec![
            placed_rc("zebra.sh", vec![rc_entry("SEED", "0")]),
            placed_rc("apple.sh", vec![rc_entry("SEED", "0")]),
            placed_structured("config.json"),
        ],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 3);
            for (route, body) in &documents {
                if route.display().contains(".sh") {
                    assert!(
                        rc_names(body).contains(&"TOUCHED".to_owned()),
                        "every matched rc takes the patch"
                    );
                }
            }
            let order: String = lua
                .load("return table.concat(order, ',')")
                .call(())
                .unwrap();
            assert_eq!(order, "apple.sh,zebra.sh");
        }
        Err(error) => panic!("rc fan-out merges: {error}"),
    }
}

#[test]
fn structured_apply_receives_route_first() {
    let lua = Lua::new();
    let patch = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(route, document) document.data.seen_base = route.base document.data.seen_relative = route.relative end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            match documents.values().next().unwrap() {
                Data::Structured { data, .. } => {
                    assert_eq!(
                        data.get("seen_base").and_then(|item| item.as_str()),
                        Some("home")
                    );
                    assert_eq!(
                        data.get("seen_relative").and_then(|item| item.as_str()),
                        Some("config.json")
                    );
                }
                _ => panic!("route patch stays structured"),
            }
        }
        Err(error) => panic!("route arg merges: {error}"),
    }
}

#[test]
fn rc_apply_receives_route_first() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(route, document) document.profile[#document.profile + 1] = { env = { name = "SEEN", value = route.base .. ":" .. route.relative } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            assert_eq!(
                rc_value(documents.values().next().unwrap(), "SEEN").as_deref(),
                Some("home:.bashrc")
            );
        }
        Err(error) => panic!("rc route arg merges: {error}"),
    }
}

#[test]
fn absent_filter_matches_cover_all_of_kind() {
    let lua = Lua::new();
    let patch = structured_filter_of(
        &lua,
        r#"return function(route) return true end"#,
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(route, document) document.data.touched = true end"#,
    );
    let profile = profile_of(
        vec![
            placed_structured("zebra.json"),
            placed_structured("apple.json"),
            placed_rc(".bashrc", vec![rc_entry("SEED", "0")]),
        ],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 3);
            for (route, body) in &documents {
                if route.display().contains(".json") {
                    match body {
                        Data::Structured { data, .. } => {
                            assert_eq!(
                                data.get("touched").and_then(|item| item.as_bool()),
                                Some(true),
                                "default match covers every document of the kind"
                            );
                        }
                        _ => panic!("default filter stays structured"),
                    }
                }
            }
        }
        Err(error) => panic!("default filter merges: {error}"),
    }
}

#[test]
fn false_filters_leave_documents_untouched() {
    let lua = Lua::new();
    let patch = structured_filter_of(
        &lua,
        r#"return function(route) return false end"#,
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(route, document) document.data.touched = true end"#,
    );
    let profile = profile_of(
        vec![
            placed_structured("zebra.json"),
            placed_structured("apple.json"),
        ],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 2);
            for body in documents.values() {
                match body {
                    Data::Structured { data, .. } => {
                        assert!(
                            data.get("touched").is_none(),
                            "refused documents skip the patch"
                        );
                        assert_eq!(
                            data.get("seed").and_then(|item| item.as_u64()),
                            Some(0),
                            "refused documents keep their base"
                        );
                    }
                    _ => panic!("untouched documents stay structured"),
                }
            }
        }
        Err(error) => panic!("false filter merges: {error}"),
    }
}

#[test]
fn failing_rc_filters_fail() {
    let lua = Lua::new();
    let patch = rc_filter_of(
        &lua,
        r#"return function(route) error("filter boom") end"#,
        Level::Normal,
        r#"return function(route, document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(_) => panic!("failing filter passes"),
        Err(error) => {
            let text = error.to_string();
            assert!(text.contains("filter boom"), "carries the cause: {text}");
        }
    }
}
