#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use confit_core::resource_ref::ResourceRef;
use confit_core::sha::Sha;
use confit_engine::api::config::Config;
use confit_engine::api::patch::{HookPatch, Level, Patch, RcPatch, StructuredPatch};
use confit_engine::api::profile::Profile;
use confit_engine::{assemble, assembly::Assembler};
use confit_events::EventHandles;
use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::document::{
    Data, Document, FileMode, LinkDocument, MemberKey, OpaqueDocument, RcData, RcEntry,
    StructuredDocument, StructuredFormat, TextDocument, TreeDocument, TreeMember,
};
use confit_model::hook::{Hook, HookTimeout};
use confit_model::id::Id;
use confit_model::routes::{Route, RouteBase};
use confit_store::{StoreRoots, Stores};
use lua_binding::callable::Callable;
use lua_binding::value::Decode;
use mlua::{Function, Lua, Value};
use std::collections::{BTreeMap, HashMap};

#[test]
fn assemble_rejects_duplicate_destinations() {
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
        "the assembler rejects duplicate destinations"
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
    assert!(
        assemble(&lua, &profile).is_err(),
        "the assembler rejects repeated config names"
    );
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
        "the assembler rejects duplicate destinations"
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
                "the assembler keeps the top document"
            );
        }
        Err(error) => panic!("the top document fails to assemble: {error}"),
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
        Err(error) => panic!("the distinct destinations fail to assemble: {error}"),
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
        MemberKey::new("sub/nested.txt".to_owned()).unwrap(),
        TreeMember::Full {
            handle: ResourceRef::new(blob),
            mode: FileMode::new(0o644),
        },
    );
    members.insert(
        MemberKey::new("top.txt".to_owned()).unwrap(),
        TreeMember::Full {
            handle: ResourceRef::new(blob),
            mode: FileMode::new(0o644),
        },
    );
    let documents = vec![
        placed_text("note.txt", "hi"),
        placed_structured("config.json"),
        Document::new(
            Route::new(RouteBase::Home, "linked").unwrap(),
            false,
            Data::Link(LinkDocument {
                target: Route::new(RouteBase::Home, "origin").unwrap(),
            }),
        ),
        placed_rc(".bashrc", vec![rc_entry("SEED", "0")]),
        placed_opaque(&stores, "blob.bin"),
        Document::new(
            Route::new(RouteBase::Home, "fonts").unwrap(),
            false,
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
                    .unwrap_or_else(|| panic!("expected {display}"))
            };
            match &by_display("home('note.txt')").data {
                Data::Text(TextDocument { content, mode }) => {
                    assert_eq!(content, "hi");
                    assert_eq!(mode.bits(), 0o644);
                    assert!(!by_display("home('note.txt')").unmanaged);
                }
                _ => panic!("the text document keeps its text"),
            }
            match &by_display("home('config.json')").data {
                Data::Structured(StructuredDocument { format, data }) => {
                    assert_eq!(*format, StructuredFormat::Json);
                    assert_eq!(data.get("seed").and_then(|item| item.as_u64()), Some(0));
                }
                _ => panic!("the document is structured"),
            }
            match &by_display("home('linked')").data {
                Data::Link(LinkDocument { target }) => {
                    assert_eq!(target.display(), "home('origin')")
                }
                _ => panic!("the document is a link"),
            }
            assert_eq!(
                rc_names(&by_display("home('.bashrc')").data),
                vec!["SEED".to_owned()]
            );
            match &by_display("home('blob.bin')").data {
                Data::Opaque(OpaqueDocument { mode, .. }) => {
                    assert_eq!(mode.bits(), 0o644);
                    assert!(by_display("home('blob.bin')").unmanaged);
                }
                _ => panic!("the document is opaque"),
            }
            match &by_display("home('fonts')").data {
                Data::Tree(TreeDocument { members: found }) => {
                    assert_eq!(
                        found.keys().map(MemberKey::as_str).collect::<Vec<_>>(),
                        vec!["sub/nested.txt", "top.txt"],
                        "the tree members hold flat paths in key order"
                    );
                }
                _ => panic!("the document is a tree"),
            }
        }
        Err(error) => panic!("the kinds fail to assemble: {error}"),
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
                        "the higher level applies first and the first write wins",
                    );
                }
                _ => panic!("the document is structured"),
            }
        }
        Err(error) => panic!("the ranked patches fail to merge: {error}"),
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
                "the higher levels apply first",
            );
        }
        Err(error) => panic!("the ranked rc patches fail to merge: {error}"),
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
                        "the equal levels keep declaration order and the first write wins",
                    );
                }
                _ => panic!("the document is structured"),
            }
        }
        Err(error) => panic!("the equal priorities fail to merge: {error}"),
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
                "the first entry remains and the later duplicates drop",
            );
            assert_eq!(
                rc_value(data, "DUP").as_deref(),
                Some("2"),
                "the higher level applies first and the first write wins",
            );
        }
        Err(error) => panic!("the shared keys fail to merge: {error}"),
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
                        "the base keys keep their values against later writes",
                    );
                }
                _ => panic!("the document is structured"),
            }
        }
        Err(error) => panic!("the base keys fail to merge: {error}"),
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
                "the base entries survive later writes",
            );
            assert_eq!(
                rc_value(data, "SEED").as_deref(),
                Some("0"),
                "the base keys keep their values against later writes",
            );
        }
        Err(error) => panic!("the base keys fail to merge: {error}"),
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
        Err(error) => panic!("the rc patches fail to merge: {error}"),
    }
}

#[test]
fn assemble_rejects_an_absent_rc_target() {
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "MADE", value = "yes" } } end"#,
    );
    let profile = profile_of(vec![], vec![Patch::Rc(patch)]);
    assert!(
        assemble(&lua, &profile).is_err(),
        "the assembler rejects a missing rc target"
    );
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
                _ => panic!("the document is structured"),
            }
        }
        Err(error) => panic!("the structured patches fail to merge: {error}"),
    }
}

#[test]
fn assemble_rejects_an_absent_structured_target() {
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
        "the assembler rejects a missing structured target"
    );
}

#[test]
fn assemble_rejects_mixed_structured_formats() {
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
        "the assembler rejects mixed structured formats"
    );
}

#[test]
fn assemble_rejects_patches_over_other_bases() {
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
        "the assembler rejects a structured patch over text"
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
        "the assembler rejects an rc patch over structured data"
    );
    let lua = Lua::new();
    let patch = rc_patch_of(
        &lua,
        "note.txt",
        Level::Normal,
        r#"return function(document) return document end"#,
    );
    let profile = profile_of(vec![placed_text("note.txt", "hi")], vec![Patch::Rc(patch)]);
    assert!(
        assemble(&lua, &profile).is_err(),
        "the assembler rejects an rc patch over text"
    );
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
        "the assembler rejects a structured patch over rc data"
    );
}

#[test]
fn assemble_rejects_a_callback_error() {
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
        "the assembler rejects a failing rc callback"
    );
}

#[test]
fn assemble_rejects_a_broken_rc_body() {
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
    assert!(
        assemble(&lua, &profile).is_err(),
        "the assembler rejects a broken rc body"
    );
}

#[test]
fn collect_hooks_keeps_declared_order() {
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
            assert_eq!(hooks[0].timeout_secs.secs(), 60);
            assert_eq!(Arg::join(&hooks[1].argv), "fc-cache");
            assert_eq!(Arg::join(&hooks[2].argv), "mise install");
            assert_eq!(hooks[2].timeout_secs.secs(), 600);
        }
        Err(error) => panic!("the hooks fail to collect: {error}"),
    }
}

#[test]
fn collect_hooks_rejects_repeated_ids() {
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
        "the hook collector rejects repeated ids"
    );
}

#[test]
fn hook_patches_mutate_working_hooks_in_order() {
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
            assert_eq!(hooks[0].id.as_str(), "mise:install");
            assert_eq!(
                hooks[0].check,
                Condition::InPath {
                    name: "extra".to_owned(),
                }
            );
        }
        Err(error) => panic!("the hook patch fails to apply: {error}"),
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
        Err(error) => panic!("the hook patches fail to append: {error}"),
    }
}

#[test]
fn collect_hooks_rejects_an_absent_target() {
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
        "the hook collector rejects a missing target"
    );
}

#[test]
fn assemble_rejects_an_absent_requirement() {
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
        "the assembler rejects a missing requirement"
    );
}

#[test]
fn assemble_accepts_a_present_requirement() {
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
        Err(error) => panic!("the present requirement fails to assemble: {error}"),
    }
}

#[test]
fn collect_hooks_rejects_changed_conditions_over_unbuilt_routes() {
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
        Err(error) => panic!("the built changed condition fails to collect: {error}"),
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
        "the hook collector rejects a changed condition over an unbuilt route"
    );
}

#[test]
fn assemble_rejects_changed_rc_guards() {
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
    assert!(
        assemble(&lua, &profile).is_err(),
        "the assembler rejects a changed rc guard"
    );
    let lua = Lua::new();
    let smuggled = rc_patch_of(
        &lua,
        ".bashrc",
        Level::Normal,
        r#"return function(document) document.profile[#document.profile + 1] = { env = { name = "SMUGGLED", value = "1", when = { changed = { route = route("home", ".bashrc") } } } } end"#,
    );
    let profile = profile_of(
        vec![placed_rc(".bashrc", vec![rc_entry("SEED", "0")])],
        vec![Patch::Rc(smuggled)],
    );
    assert!(
        assemble(&lua, &profile).is_err(),
        "the assembler rejects a changed guard from a patch"
    );
}

#[test]
fn assemble_accepts_plain_rc_guards() {
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
            "the plain guard survives assembly",
        ),
        Err(error) => panic!("the plain rc guard fails to assemble: {error}"),
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
                            "the matched document takes the patch"
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
        Err(error) => panic!("the filter matches fail to merge: {error}"),
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
                        "the matched rc takes the patch"
                    );
                }
            }
            let order: String = lua
                .load("return table.concat(order, ',')")
                .call(())
                .unwrap();
            assert_eq!(order, "apple.sh,zebra.sh");
        }
        Err(error) => panic!("the rc filter matches fail to merge: {error}"),
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
                _ => panic!("the document is structured"),
            }
        }
        Err(error) => panic!("the route argument fails to merge: {error}"),
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
        Err(error) => panic!("the rc route argument fails to merge: {error}"),
    }
}

#[test]
fn absent_filter_matches_cover_every_document_of_the_kind() {
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
                        _ => panic!("the document is structured"),
                    }
                }
            }
        }
        Err(error) => panic!("the default filter fails to merge: {error}"),
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
                            "the refused documents skip the patch"
                        );
                        assert_eq!(
                            data.get("seed").and_then(|item| item.as_u64()),
                            Some(0),
                            "the refused documents keep their base"
                        );
                    }
                    _ => panic!("the documents are structured"),
                }
            }
        }
        Err(error) => panic!("the false filter fails to merge: {error}"),
    }
}

#[test]
fn assemble_rejects_a_filter_error() {
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
    assert!(
        assemble(&lua, &profile).is_err(),
        "the assembler rejects a failing filter"
    );
}

/// Wraps text in a document.
fn placed_text(relative: &str, text: &str) -> Document {
    Document::new(
        Route::new(RouteBase::Home, relative).unwrap(),
        false,
        Data::Text(TextDocument {
            content: text.to_owned(),
            mode: FileMode::new(0o644),
        }),
    )
}

/// Wraps profile entries in a document.
fn placed_rc(relative: &str, entries: Vec<RcEntry>) -> Document {
    Document::new(
        Route::new(RouteBase::Home, relative).unwrap(),
        false,
        Data::Rc(RcData::new(entries, Vec::new(), Vec::new())),
    )
}

/// Wraps seeded json in a document.
fn placed_structured(relative: &str) -> Document {
    Document::new(
        Route::new(RouteBase::Home, relative).unwrap(),
        false,
        Data::Structured(StructuredDocument {
            format: StructuredFormat::Json,
            data: BTreeMap::from([("seed".to_owned(), serde_json::json!(0))]),
        }),
    )
}

/// Returns isolated stores in a fresh tempdir.
fn test_stores() -> (tempfile::TempDir, Stores) {
    let dir = tempfile::tempdir().unwrap();
    let stores = Stores::new(
        StoreRoots {
            config_base: dir.path().join("config"),
            temp_base: dir.path().join("temp"),
            project_root: dir.path().join("project"),
        },
        EventHandles::silent(),
    );
    (dir, stores)
}

/// Wraps a project blob in a document.
fn placed_opaque(stores: &Stores, relative: &str) -> Document {
    let root = stores.resources().roots.project_root.clone();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("blob.bin"), b"hi").unwrap();
    Document::new(
        Route::new(RouteBase::Home, relative).unwrap(),
        true,
        Data::Opaque(OpaqueDocument {
            handle: ResourceRef::new(Sha::hash(b"hi")),
            mode: FileMode::new(0o644),
        }),
    )
}

/// Builds one profile from the documents and patches.
fn profile_of(documents: Vec<Document>, patches: Vec<Patch>) -> Profile {
    Profile::new(
        vec![Config::new(
            Id::of("tools/test"),
            documents,
            patches,
            Vec::new(),
            HashMap::new(),
        )],
        Vec::new(),
    )
}

/// Builds one env entry.
fn rc_entry(name: &str, value: &str) -> RcEntry {
    RcEntry::Env {
        name: name.to_owned(),
        value: value.to_owned(),
        when: None,
    }
}

/// Builds one structured patch with its callback.
fn structured_patch_of(
    lua: &Lua,
    relative: &str,
    format: StructuredFormat,
    priority: Level,
    source: &str,
) -> StructuredPatch {
    let apply: Function = lua.load(source).call(()).unwrap();
    StructuredPatch {
        target: Some(Route::new(RouteBase::Home, relative).unwrap()),
        filter: None,
        format,
        apply: Callable::new(apply),
        priority,
    }
}

/// Builds one rc patch with its callback.
fn rc_patch_of(lua: &Lua, relative: &str, priority: Level, source: &str) -> RcPatch {
    let apply: Function = lua.load(source).call(()).unwrap();
    RcPatch {
        target: Some(Route::new(RouteBase::Home, relative).unwrap()),
        filter: None,
        apply: Callable::new(apply),
        priority,
    }
}

/// Builds one structured filter patch.
fn structured_filter_of(
    lua: &Lua,
    filter_source: &str,
    format: StructuredFormat,
    priority: Level,
    source: &str,
) -> StructuredPatch {
    let filter: Function = lua.load(filter_source).call(()).unwrap();
    let apply: Function = lua.load(source).call(()).unwrap();
    StructuredPatch {
        target: None,
        filter: Some(Callable::new(filter)),
        format,
        apply: Callable::new(apply),
        priority,
    }
}

/// Builds one rc filter patch.
fn rc_filter_of(lua: &Lua, filter_source: &str, priority: Level, source: &str) -> RcPatch {
    let filter: Function = lua.load(filter_source).call(()).unwrap();
    let apply: Function = lua.load(source).call(()).unwrap();
    RcPatch {
        target: None,
        filter: Some(Callable::new(filter)),
        apply: Callable::new(apply),
        priority,
    }
}

/// Returns one profile env value.
fn rc_value(data: &Data, name: &str) -> Option<String> {
    match data {
        Data::Rc(found) => found.profile.iter().find_map(|entry| match entry {
            RcEntry::Env {
                name: key, value, ..
            } if key == name => Some(value.clone()),
            _ => None,
        }),
        _ => panic!("expected an rc document"),
    }
}

/// Builds one hook from argv words and a timeout.
fn hook_of(argv: &[&str], timeout_secs: u64) -> Hook {
    Hook {
        argv: argv
            .iter()
            .map(|item| Arg::Text((*item).to_owned()))
            .collect(),
        path: Vec::new(),
        id: Id::of(format!("{}:{timeout_secs}", argv.join(" "))),
        requires: Condition::default(),
        when: Condition::default(),
        check: Condition::default(),
        timeout_secs: HookTimeout::new(timeout_secs),
    }
}

/// Builds one hook with an id.
fn identified_hook_of(id: &str, argv: &[&str], timeout_secs: u64) -> Hook {
    let mut hook = hook_of(argv, timeout_secs);
    hook.id = Id::of(id);
    hook
}

/// Builds one hook patch.
fn hook_patch_of(lua: &Lua, target: &str, source: &str) -> HookPatch {
    let apply: Function = lua.load(source).call(()).unwrap();
    let table = lua.create_table().unwrap();
    table.set("target", target).unwrap();
    table.set("apply", apply).unwrap();
    match HookPatch::decode(&Value::Table(table), lua) {
        Ok(patch) => patch,
        Err(error) => panic!("the hook patch fails to decode: {error}"),
    }
}

/// Builds one profile with hooks.
fn profile_with_hooks(configs: Vec<Config>) -> Profile {
    Profile::new(configs, Vec::new())
}
