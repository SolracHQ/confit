mod common;

use common::*;
use confit_core::sha::Sha;
use confit_engine::api::config::Config;
use confit_engine::api::patch::{Level, Patch};
use confit_engine::api::profile::Profile;
use confit_engine::{assemble, assembly::Assembler};
use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::document::{
    Data, Document, LinkDocument, OpaqueDocument, RcEntry, StructuredDocument, StructuredFormat,
    TextDocument, TreeDocument, TreeMember,
};
use confit_model::id::Id;
use confit_model::routes::{Route, RouteBase};
use confit_store::handles::ResourceHandle;
use mlua::Lua;
use std::collections::{BTreeMap, HashMap};

#[test]
fn duplicates_fail() {
    let lua = Lua::new();
    let profile = Profile::new(
        vec![
            Config::new(
                Id::of("first"),
                [placed_text(".bashrc", "one")].into_iter().collect(),
                vec![],
                Vec::new(),
                HashMap::new(),
            ),
            Config::new(
                Id::of("second"),
                [placed_text(".bashrc", "two")].into_iter().collect(),
                vec![],
                Vec::new(),
                HashMap::new(),
            ),
        ],
        Vec::new(),
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "duplicate destinations fail"
    );
    let lua = Lua::new();
    let profile = Profile::new(
        vec![
            Config::new(
                Id::of("same"),
                [placed_text("a", "one")].into_iter().collect(),
                vec![],
                Vec::new(),
                HashMap::new(),
            ),
            Config::new(
                Id::of("same"),
                [placed_text("b", "two")].into_iter().collect(),
                vec![],
                Vec::new(),
                HashMap::new(),
            ),
        ],
        Vec::new(),
    );
    assert!(assemble(&lua, &profile).is_err(), "repeated names fail");
    let lua = Lua::new();
    let profile = Profile::new(
        vec![Config::new(
            Id::of("tools/test"),
            [placed_text(".bashrc", "two")].into_iter().collect(),
            vec![],
            Vec::new(),
            HashMap::new(),
        )],
        [placed_text(".bashrc", "one")].into_iter().collect(),
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "duplicate destinations fail"
    );
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
            assert!(
                documents.iter().any(|document| document.path == route),
                "top document assembles"
            );
        }
        Err(error) => panic!("top documents assemble: {error}"),
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
            let displays: Vec<String> = documents
                .iter()
                .map(|document| document.path.display())
                .collect();
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
    let lua = Lua::new();
    let (_held, stores) = test_stores();
    let root = stores.resources().roots.project_root.clone();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("glyph.ttf"), b"ttf").unwrap();
    let blob = Sha::hash(b"ttf");
    let mut members = BTreeMap::new();
    members.insert(
        "sub/nested.txt".to_owned(),
        TreeMember::Full {
            handle: ResourceHandle::Cache(blob.clone()),
            mode: 0o644,
        },
    );
    members.insert(
        "top.txt".to_owned(),
        TreeMember::Full {
            handle: ResourceHandle::Cache(blob.clone()),
            mode: 0o644,
        },
    );
    let documents = vec![
        placed_text("note.txt", "hi"),
        placed_structured("config.json"),
        Document::new(
            Route::new(RouteBase::Home, "linked").unwrap(),
            Data::Link(LinkDocument {
                target: Route::new(RouteBase::Home, "origin").unwrap(),
            }),
        ),
        placed_rc(".bashrc", vec![rc_entry("SEED", "0")]),
        placed_opaque(&stores, "blob.bin"),
        Document::new(
            Route::new(RouteBase::Home, "fonts").unwrap(),
            Data::Tree(TreeDocument { members }),
        ),
    ];
    let profile = profile_of(documents, vec![]);
    match assemble(&lua, &profile) {
        Ok(found) => {
            assert_eq!(found.len(), 6);
            let by_display = |display: &str| {
                found
                    .iter()
                    .find(|document| document.path.display() == display)
                    .map(|document| &document.data)
                    .unwrap_or_else(|| panic!("{display} assembles"))
            };
            match by_display("home('note.txt')") {
                Data::Text(TextDocument {
                    content,
                    mode,
                    unmanaged,
                }) => {
                    assert_eq!(content, "hi");
                    assert_eq!(*mode, 0o644);
                    assert!(!unmanaged);
                }
                _ => panic!("text stays text"),
            }
            match by_display("home('config.json')") {
                Data::Structured(StructuredDocument { format, data }) => {
                    assert_eq!(*format, StructuredFormat::Json);
                    assert_eq!(data.get("seed").and_then(|item| item.as_u64()), Some(0));
                }
                _ => panic!("structured stays structured"),
            }
            match by_display("home('linked')") {
                Data::Link(LinkDocument { target }) => {
                    assert_eq!(target.display(), "home('origin')")
                }
                _ => panic!("link stays link"),
            }
            assert_eq!(
                rc_names(by_display("home('.bashrc')")),
                vec!["SEED".to_owned()]
            );
            match by_display("home('blob.bin')") {
                Data::Opaque(OpaqueDocument {
                    mode, unmanaged, ..
                }) => {
                    assert_eq!(*mode, 0o644);
                    assert!(unmanaged);
                }
                _ => panic!("opaque stays opaque"),
            }
            match by_display("home('fonts')") {
                Data::Tree(TreeDocument { members: found }) => {
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
        r#"return function(document) document.data.winner = "minor" end"#,
    );
    let major = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Major,
        r#"return function(document) document.data.winner = "major" end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(minor), Patch::Structured(major)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            match &documents.first().unwrap().data {
                Data::Structured(StructuredDocument { data, .. }) => {
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
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "LOW", value = "1" } } end"#,
    );
    let high = rc_patch_of(
        &lua,
        ".bashrc",
        Level::High,
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "HIGH", value = "2" } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(low), Patch::Rc(high)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            assert_eq!(
                rc_names(&documents.first().unwrap().data),
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
        r#"return function(document) document.data.winner = "first" end"#,
    );
    let second = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(document) document.data.winner = "second" end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(first), Patch::Structured(second)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            match &documents.first().unwrap().data {
                Data::Structured(StructuredDocument { data, .. }) => {
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
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "DUP", value = "1" } } end"#,
    );
    let high = rc_patch_of(
        &lua,
        ".bashrc",
        Level::High,
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "DUP", value = "2" } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(low), Patch::Rc(high)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let data = &documents.first().unwrap().data;
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
        r#"return function(document) document.data.seed = 99 end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            match &documents.first().unwrap().data {
                Data::Structured(StructuredDocument { data, .. }) => {
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
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "SEED", value = "1" } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let data = &documents.first().unwrap().data;
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
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "FIRST", value = "1" } } end"#,
    );
    let second = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "SECOND", value = "2" } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(first), Patch::Rc(second)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let document = documents.first().unwrap();
            assert_eq!(document.path.display(), "home('.bashrc')");
            assert_eq!(
                rc_names(&document.data),
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
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "MADE", value = "yes" } } end"#,
    );
    let profile = profile_of(vec![], vec![Patch::Rc(patch)]);
    assert!(assemble(&lua, &profile).is_err(), "missing rc target fails");
}

#[test]
fn structured_patches_merge_data() {
    let lua = Lua::new();
    let first = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(document) document.data.alpha = "one" end"#,
    );
    let second = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(document) document.data.beta = "two" end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(first), Patch::Structured(second)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            let document = documents.first().unwrap();
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
        r#"return function(document) document.data.made = "yes" end"#,
    );
    let profile = profile_of(vec![], vec![Patch::Structured(patch)]);
    assert!(
        assemble(&lua, &profile).is_err(),
        "missing structured target fails"
    );
}

#[test]
fn structured_mixed_formats_fail_showing_formats() {
    let lua = Lua::new();
    let first = structured_patch_of(
        &lua,
        "fresh.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(document) return document end"#,
    );
    let second = structured_patch_of(
        &lua,
        "fresh.json",
        StructuredFormat::Toml,
        Level::Normal,
        r#"return function(document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_structured("fresh.json")],
        vec![Patch::Structured(first), Patch::Structured(second)],
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "mixed structured formats fail"
    );
}

#[test]
fn patches_rejected_foreign_bases() {
    let lua = Lua::new();
    let patch = structured_patch_of(
        &lua,
        "note.txt",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_text("note.txt", "hi")],
        vec![Patch::Structured(patch)],
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "structured patch over text fails"
    );
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        "config.json",
        Level::Normal,
        r#"return function(document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Rc(patch)],
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "rc patch over structured fails"
    );
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        "note.txt",
        Level::Normal,
        r#"return function(document) return document end"#,
    );
    let profile = profile_of(vec![placed_text("note.txt", "hi")], vec![Patch::Rc(patch)]);
    assert!(assemble(&lua, &profile).is_err(), "patched text fails");
    let lua = Lua::new();
    let patch = structured_patch_of(
        &lua,
        ".bashrc",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Structured(patch)],
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "format patch over rc fails"
    );
}

#[test]
fn rc_failing_callbacks_fail() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(document) error("rc boom") end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "failing rc callback fails"
    );
}

#[test]
fn rc_broken_bodies_fail_showing_entries() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(document) document.profile = 1 end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    assert!(assemble(&lua, &profile).is_err(), "broken rc body fails");
}

#[test]
fn hooks_pass_through_without_merging() {
    let lua = Lua::new();
    let profile = profile_with_hooks(vec![
        Config::new(
            Id::of("tools/first"),
            Vec::new(),
            Vec::new(),
            vec![
                hook_of(&["mise", "install"], 60),
                hook_of(&["fc-cache"], 60),
            ],
            HashMap::new(),
        ),
        Config::new(
            Id::of("tools/second"),
            Vec::new(),
            Vec::new(),
            vec![hook_of(&["mise", "install"], 600)],
            HashMap::new(),
        ),
    ]);
    let documents = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &documents) {
        Ok(hooks) => {
            assert_eq!(hooks.len(), 3);
            assert_eq!(Arg::join(&hooks[0].argv), "mise install");
            assert_eq!(hooks[0].timeout_secs, 60);
            assert_eq!(Arg::join(&hooks[1].argv), "fc-cache");
            assert_eq!(Arg::join(&hooks[2].argv), "mise install");
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
            Id::of("tools/first"),
            Vec::new(),
            Vec::new(),
            vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
            HashMap::new(),
        ),
        Config::new(
            Id::of("tools/second"),
            Vec::new(),
            Vec::new(),
            vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
            HashMap::new(),
        ),
    ]);
    let documents = assemble(&lua, &profile).unwrap();
    assert!(
        Assembler::new(&lua)
            .collect_hooks(&profile, &documents)
            .is_err(),
        "repeated hook ids fail"
    );
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
        Id::of("tools/test"),
        Vec::new(),
        vec![Patch::Hook(patch)],
        vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
        HashMap::new(),
    )]);
    let documents = assemble(&lua, &profile).unwrap();
    match Assembler::new(&lua).collect_hooks(&profile, &documents) {
        Ok(hooks) => {
            assert_eq!(hooks.len(), 1);
            assert_eq!(
                hooks[0].id.as_ref().map(|id| id.as_str()),
                Some("mise:install")
            );
            assert_eq!(
                hooks[0].check,
                Condition::InPath {
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
        Id::of("tools/test"),
        Vec::new(),
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
                Condition::All(vec![
                    Condition::InPath {
                        name: "first".to_owned(),
                    },
                    Condition::InPath {
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
        Id::of("tools/test"),
        Vec::new(),
        vec![Patch::Hook(patch)],
        vec![identified_hook_of("mise:install", &["mise", "install"], 60)],
        HashMap::new(),
    )]);
    let documents = assemble(&lua, &profile).unwrap();
    assert!(
        Assembler::new(&lua)
            .collect_hooks(&profile, &documents)
            .is_err(),
        "missing hook target fails"
    );
}

#[test]
fn missing_requirements_fail_with_hints() {
    let lua = Lua::new();
    let profile = Profile::new(
        vec![Config::new(
            Id::of("tools/bat"),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            HashMap::from([(
                "plugin:_/mise:install".to_owned(),
                "add mise.init() to the profile configs".to_owned(),
            )]),
        )],
        Vec::new(),
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "missing requirement fails"
    );
}

#[test]
fn present_requirements_pass() {
    let lua = Lua::new();
    let profile = Profile::new(
        vec![
            Config::new(
                Id::of("plugin:_/mise:install"),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                HashMap::new(),
            ),
            Config::new(
                Id::of("tools/bat"),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                HashMap::from([(
                    "plugin:_/mise:install".to_owned(),
                    "add mise.init() to the profile configs".to_owned(),
                )]),
            ),
        ],
        Vec::new(),
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
    built_hook.when = Condition::Changed {
        route: present.clone(),
    };
    let documents = [placed_text("present", "hi")];
    let profile = profile_with_hooks(vec![Config::new(
        Id::of("tools/test"),
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
    foreign_hook.when = Condition::Changed { route: missing };
    let profile = profile_with_hooks(vec![Config::new(
        Id::of("tools/test"),
        Vec::new(),
        Vec::new(),
        vec![foreign_hook],
        HashMap::new(),
    )]);
    let finished = assemble(&lua, &profile).unwrap();
    assert!(
        Assembler::new(&lua)
            .collect_hooks(&profile, &finished)
            .is_err(),
        "changed condition over unbuilt route fails"
    );
}

#[test]
fn rc_changed_guards_fail() {
    let lua = Lua::new();
    let target = Route::new(RouteBase::Home, ".bashrc").unwrap();
    let entry = RcEntry::Env {
        name: "POISONED".to_owned(),
        value: "1".to_owned(),
        when: Some(Condition::Changed {
            route: target.clone(),
        }),
    };
    let profile = profile_of(vec![placed_rc(".bashrc", vec![entry])], Vec::new());
    assert!(assemble(&lua, &profile).is_err(), "changed rc guard fails");
    let lua = Lua::new();
    let smuggled = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "SMUGGLED", value = "1", when = { changed = { route = { base = "home", relative = ".bashrc" } } } } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(smuggled)],
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "patch-smuggled changed guard fails"
    );
}

#[test]
fn rc_plain_guards_pass() {
    let lua = Lua::new();
    let entry = RcEntry::Env {
        name: "GUARDED".to_owned(),
        value: "1".to_owned(),
        when: Some(Condition::InPath {
            name: "mise".to_owned(),
        }),
    };
    let profile = profile_of(vec![placed_rc(".bashrc", vec![entry])], Vec::new());
    match assemble(&lua, &profile) {
        Ok(documents) => assert_eq!(
            rc_names(&documents.first().unwrap().data),
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
        r#"return function(document) table.insert(order, document.path.relative) document.data.touched = true end"#,
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
            for document in &documents {
                match &document.data {
                    Data::Structured(StructuredDocument { data, .. })
                        if document.path.display().contains(".json") =>
                    {
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
        r#"return function(document) table.insert(order, document.path.relative) document.profile[#document.profile + 1] = { env = { name = "TOUCHED", value = "1" } } end"#,
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
            for document in &documents {
                if document.path.display().contains(".sh") {
                    assert!(
                        rc_names(&document.data).contains(&"TOUCHED".to_owned()),
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
fn structured_apply_receives_document_path() {
    let lua = Lua::new();
    let patch = structured_patch_of(
        &lua,
        "config.json",
        StructuredFormat::Json,
        Level::Normal,
        r#"return function(document) document.data.seen_base = document.path.base document.data.seen_relative = document.path.relative end"#,
    );
    let profile = profile_of(
        vec![placed_structured("config.json")],
        vec![Patch::Structured(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            match &documents.first().unwrap().data {
                Data::Structured(StructuredDocument { data, .. }) => {
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
fn rc_apply_receives_document_path() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "SEEN", value = document.path.base .. ":" .. document.path.relative } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    match assemble(&lua, &profile) {
        Ok(documents) => {
            assert_eq!(documents.len(), 1);
            assert_eq!(
                rc_value(&documents.first().unwrap().data, "SEEN").as_deref(),
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
        r#"return function(document) document.data.touched = true end"#,
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
            for document in &documents {
                if document.path.display().contains(".json") {
                    match &document.data {
                        Data::Structured(StructuredDocument { data, .. }) => {
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
        r#"return function(document) document.data.touched = true end"#,
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
            for document in &documents {
                match &document.data {
                    Data::Structured(StructuredDocument { data, .. }) => {
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
        r#"return function(document) return document end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(patch)],
    );
    assert!(assemble(&lua, &profile).is_err(), "failing filter fails");
}
