use super::{
    Data, Document, OpaqueDocument, Payload, RcEntry, TextDocument, TreeDocument, TreeMember,
};
use confit_binding::value::{Decode, Encode};
use confit_driver as driver;
use confit_driver::sha::Sha;
use confit_store::{StoreRoots, Stores};
use mlua::{Lua, Value};

use crate::routes::{Route, RouteBase};

/// Build a fresh Lua state for tests.
fn test_lua() -> Lua {
    Lua::new()
}

/// Decode one flat document with the subject path beside the body.
fn document_of(lua: &Lua, body: &str) -> Document {
    let source = format!(
        r#"local body = {body}
body.path = {{ base = "home", relative = "subject" }}
return body"#
    );
    let value: Value = match lua.load(&source).call(()) {
        Ok(found) => found,
        Err(error) => panic!("body loads: {error}"),
    };
    match Document::decode(&value, lua) {
        Ok(decoded) => decoded,
        Err(error) => panic!("body decodes: {error}"),
    }
}

/// Refuse one flat document with the subject path.
fn body_refuses(lua: &Lua, body: &str) {
    let source = format!(
        r#"local body = {body}
body.path = {{ base = "home", relative = "subject" }}
return body"#
    );
    let value: Value = match lua.load(&source).call(()) {
        Ok(found) => found,
        Err(error) => panic!("body loads: {error}"),
    };
    assert!(Document::decode(&value, lua).is_err(), "foreign body fails");
}

/// Open a project with two files and Lua state holding its stores.
fn opened_lua() -> (tempfile::TempDir, Lua, Stores) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("note.txt"), b"hello").unwrap();
    std::fs::write(root.join("other.txt"), b"world").unwrap();
    let (sender, _) = crossbeam_channel::unbounded();
    let stores = Stores::new(
        StoreRoots {
            config_base: directory.path().join("config"),
            temp_base: directory.path().join("temp"),
            project_root: directory.path().join("project"),
        },
        sender,
    );
    let lua = Lua::new();
    let _ = lua.set_app_data(stores.clone());
    (directory, lua, stores)
}

/// Publish one project file as a Lua global handle.
fn handle_of(lua: &Lua, stores: &Stores, name: &str) -> String {
    let global = format!("handle_{}", name.replace('.', "_"));
    let handle = match stores.resources().resource(name) {
        Ok(found) => found,
        Err(error) => panic!("project opens: {error}"),
    };
    lua.globals().set(global.clone(), handle).unwrap();
    global
}

/// Report whether decode wrote zero blob files.
fn no_blob_files(directory: &tempfile::TempDir) -> bool {
    let cache = directory.path().join("temp").join("blobs");
    !cache.exists()
        || confit_driver::fs::read_dir(&cache)
            .unwrap_or_default()
            .is_empty()
}

#[test]
fn document_flat_round_trips_path_beside_text() {
    let lua = test_lua();
    let source = Document::new(
        Route::new(RouteBase::Home, "subject").unwrap(),
        Data::Text(TextDocument {
            content: "hi".to_owned(),
            mode: 0o644,
            unmanaged: false,
        }),
    );
    let encoded = source.encode(&lua).unwrap();
    let Value::Table(table) = encoded else {
        panic!("document encodes a table");
    };
    let path: mlua::Table = table.get("path").unwrap();
    let base: String = path.get("base").unwrap();
    let relative: String = path.get("relative").unwrap();
    assert_eq!(base.as_str(), "home");
    assert_eq!(relative.as_str(), "subject");
    let content: String = table.get("text").unwrap();
    assert_eq!(content.as_str(), "hi");
    let back = Document::decode(&Value::Table(table), &lua).unwrap();
    assert_eq!(back, source);
    let flat = document_of(&lua, r#"{ text = "hi", mode = "644" }"#);
    assert_eq!(flat, source);
}

#[test]
fn mixed_markers_fail() {
    let lua = test_lua();
    #[cfg(unix)]
    {
        body_refuses(
            &lua,
            r#"{ text = "hi", target = { base = "home", relative = "linked" } }"#,
        );
    }
    #[cfg(not(unix))]
    {
        body_refuses(&lua, r#"{ text = "hi", nope = 1 }"#);
    }
    body_refuses(&lua, r#"{ nope = 1 }"#);
    let value: Value = lua.load("return { text = \"hi\" }").call(()).unwrap();
    assert!(
        Document::decode(&value, &lua).is_err(),
        "pathless body fails"
    );
}

#[test]
fn refusal_cases_fail() {
    let (_directory, lua, stores) = opened_lua();
    let note = handle_of(&lua, &stores, "note.txt");
    body_refuses(&lua, r#"{ text = "hi", bogus = 1 }"#);
    body_refuses(&lua, r#"{ text = "hi", mode = 644 }"#);
    body_refuses(&lua, r#"{ text = "hi", unmanaged = 1 }"#);
    body_refuses(&lua, r#"{ handle = "note.txt" }"#);
    body_refuses(&lua, r#"{ members = { ["top.txt"] = "hi" } }"#);
    body_refuses(
        &lua,
        &format!(r#"{{ members = {{ ["top.txt"] = {{ handle = {note} }} }} }}"#),
    );
    body_refuses(&lua, r#"{ profile = { { frobnicate = { name = "x" } } } }"#);
}

#[test]
fn mode_strings_decode_and_encode() {
    let lua = test_lua();
    let octal = document_of(&lua, r#"{ text = "hi", mode = "644", unmanaged = true }"#);
    match octal.data {
        Data::Text(TextDocument {
            mode, unmanaged, ..
        }) => {
            assert_eq!(mode, 0o644);
            assert!(unmanaged);
        }
        _ => panic!("text body decodes"),
    }
    let symbolic = document_of(&lua, r#"{ text = "hi", mode = "rwxr-xr-x" }"#);
    match symbolic.data {
        Data::Text(TextDocument {
            mode, unmanaged, ..
        }) => {
            assert_eq!(mode, 0o755);
            assert!(!unmanaged);
        }
        _ => panic!("symbolic mode decodes"),
    }
    body_refuses(&lua, r#"{ text = "hi", mode = "zzz" }"#);
    let source = Data::Text(TextDocument {
        content: "hi".to_owned(),
        mode: 0o755,
        unmanaged: false,
    });
    let encoded = source.encode(&lua).unwrap();
    let Value::Table(table) = encoded else {
        panic!("text encodes a table");
    };
    let rendered: String = table.get("mode").unwrap();
    assert_eq!(rendered.as_str(), "755");
    let back = Data::decode(&Value::Table(table), &lua).unwrap();
    assert_eq!(back, source);
}

#[test]
fn resolve_compresses_handles_into_the_pool() {
    let (directory, _lua, stores) = opened_lua();
    let handle = match stores.resources().resource("note.txt") {
        Ok(found) => found,
        Err(error) => panic!("project opens: {error}"),
    };
    let payload = Payload::Handle(handle);
    let first = match payload.resolve(&stores) {
        Ok(found) => found,
        Err(error) => panic!("handle resolves: {error}"),
    };
    assert_eq!(first, Sha::hash(b"hello"), "resolve keeps the content hash");
    let cached = directory.path().join("temp").join("blobs");
    driver::fs::set_mode(&cached, Some(0o555)).unwrap();
    match payload.resolve(&stores) {
        Ok(again) => assert_eq!(again, first, "repeat resolve reuses the file"),
        Err(error) => panic!("present resolve skips writes: {error}"),
    }
    driver::fs::set_mode(&cached, Some(0o755)).unwrap();
}

#[test]
fn sealed_arms_refuse_encode_and_serialize() {
    let (_directory, lua, stores) = opened_lua();
    let handle = match stores.resources().resource("note.txt") {
        Ok(found) => found,
        Err(error) => panic!("project opens: {error}"),
    };
    let live = Data::Opaque(OpaqueDocument {
        handle: Payload::Handle(handle),
        mode: 0o644,
        unmanaged: false,
    });
    match live.encode(&lua) {
        Ok(Value::Table(table)) => {
            let _: mlua::AnyUserData = table.get("handle").unwrap();
        }
        Ok(_) => panic!("handles encode a table"),
        Err(error) => panic!("handles encode: {error}"),
    }
    assert!(
        serde_json::to_string(&live).is_err(),
        "unsealed documents refuse serialize"
    );
    let blob = Sha::hash(b"sealed");
    let sealed = Data::Opaque(OpaqueDocument {
        handle: Payload::Resolved(blob),
        mode: 0o644,
        unmanaged: false,
    });
    assert!(
        sealed.encode(&lua).is_err(),
        "sealed payloads refuse encode"
    );
    let text = match serde_json::to_string(&sealed) {
        Ok(found) => found,
        Err(error) => panic!("sealed serializes: {error}"),
    };
    match serde_json::from_str::<Data>(&text) {
        Ok(back) => assert_eq!(back, sealed, "sealed survives serde"),
        Err(error) => panic!("sealed decodes: {error}"),
    }
}

#[test]
fn tree_mixed_members_keep_modes_without_writes() {
    let (directory, lua, stores) = opened_lua();
    let note = handle_of(&lua, &stores, "note.txt");
    let other = handle_of(&lua, &stores, "other.txt");
    let body = format!(
        r#"{{
        members = {{
            ["top.txt"] = {note},
            ["sub/nested.txt"] = {{ handle = {other}, mode = "755" }},
        }},
    }}"#
    );
    let document = document_of(&lua, &body);
    match document.data {
        Data::Tree(TreeDocument { members }) => {
            assert_eq!(members.len(), 2, "mixed body keeps both members");
            let bare = match members.get("top.txt") {
                Some(found) => found,
                None => panic!("bare member keeps its key"),
            };
            assert!(
                matches!(bare, TreeMember::Handle(_)),
                "bare entry decodes to Handle"
            );
            assert_eq!(bare.mode(), 0o644, "bare entry takes the default mode");
            let full = match members.get("sub/nested.txt") {
                Some(found) => found,
                None => panic!("full member keeps its key"),
            };
            assert!(
                matches!(full, TreeMember::Full { .. }),
                "table entry decodes to Full"
            );
            assert_eq!(full.mode(), 0o755, "table entry keeps its mode");
            assert!(
                members
                    .values()
                    .all(|member| matches!(member.blob(), Payload::Handle(_))),
                "every member holds a handle"
            );
        }
        _ => panic!("tree body decodes"),
    }
    assert!(no_blob_files(&directory), "decode writes zero blob files");
}

#[test]
fn rc_decodes_every_op() {
    let lua = test_lua();
    let body = r#"{
        profile = {
            { env = { name = "EDITOR", value = "hx", when = { env_eq = { key = "TERM", value = "xterm" } } } },
            { path = { name = "PATH", dir = { base = "home", relative = "bin" } } },
        },
        config = {
            { alias = { name = "ll", expansion = "ls -l" } },
            { eval = { argv = { "mise", "activate" } } },
        },
        final = {
            { cmd = { argv = { "echo", "hi" } } },
            { source = { path = { base = "home", relative = "extra.sh" } } },
        },
    }"#;
    let documents = document_of(&lua, body);
    match documents.data {
        Data::Rc(found) => {
            let profile = &found.profile;
            let config = &found.config;
            let final_entries = &found.r#final;
            assert_eq!(profile.len(), 2);
            assert_eq!(config.len(), 2);
            assert_eq!(final_entries.len(), 2);
            match &profile[0] {
                RcEntry::Env { name, value, .. } => {
                    assert_eq!(name, "EDITOR");
                    assert_eq!(value, "hx");
                }
                _ => panic!("profile holds the env entry"),
            }
            assert!(profile[0].when().is_some(), "guards stay on the entry");
            match &profile[1] {
                RcEntry::Path { name, dir, .. } => {
                    assert_eq!(name, "PATH");
                    assert_eq!(dir.display(), "home('bin')");
                }
                _ => panic!("profile holds the path entry"),
            }
            match &config[0] {
                RcEntry::Alias {
                    name, expansion, ..
                } => {
                    assert_eq!(name, "ll");
                    assert_eq!(expansion, "ls -l");
                }
                _ => panic!("config holds the alias entry"),
            }
            match &config[1] {
                RcEntry::Eval { argv, .. } => assert_eq!(argv.len(), 2),
                _ => panic!("config holds the eval entry"),
            }
            match &final_entries[0] {
                RcEntry::Cmd { argv, .. } => assert_eq!(argv.len(), 2),
                _ => panic!("final holds the cmd entry"),
            }
            match &final_entries[1] {
                RcEntry::Source { path, .. } => {
                    assert_eq!(path.display(), "home('extra.sh')");
                }
                _ => panic!("final holds the source entry"),
            }
        }
        _ => panic!("rc body decodes"),
    }
}

#[test]
fn rc_unknown_ops_fail() {
    let lua = test_lua();
    for body in [
        r#"{ profile = { { frobnicate = { name = "x" } } } }"#,
        r#"{ profile = { { env = { name = "A", value = "B", bogus = 1 } } } }"#,
        r#"{ profile = { { env = { name = "A" } } } }"#,
    ] {
        body_refuses(&lua, body);
    }
}
