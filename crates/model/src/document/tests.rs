use super::{
    Data, Document, FileMode, OpaqueDocument, RcEntry, TextDocument, TreeDocument, TreeMember,
};
use confit_core::sha::Sha;
use confit_store::handles::ResourceHandle;
use confit_store::{StoreRoots, Stores};
use lua_binding::value::{Decode, Encode};
use mlua::{Lua, Value};

use crate::routes::{Route, RouteBase};
use confit_driver::fs;

/// Returns a fresh Lua state.
fn fresh_lua() -> Lua {
    Lua::new()
}

/// Returns the document that the body decodes into.
fn document_from(lua: &Lua, body: &str) -> Document {
    publish_subject_path(lua);
    let source = format!(
        r#"local body = {body}
body.path = subject_path
return body"#
    );
    let value: Value = match lua.load(&source).call(()) {
        Ok(found) => found,
        Err(error) => panic!("the body fails to load: {error}"),
    };
    match Document::decode(&value, lua) {
        Ok(decoded) => decoded,
        Err(error) => panic!("the body fails to decode: {error}"),
    }
}

/// Asserts that the document decoder rejects a body.
fn reject_body(lua: &Lua, body: &str) {
    publish_subject_path(lua);
    let source = format!(
        r#"local body = {body}
body.path = subject_path
return body"#
    );
    let value: Value = match lua.load(&source).call(()) {
        Ok(found) => found,
        Err(error) => panic!("the body fails to load: {error}"),
    };
    assert!(
        Document::decode(&value, lua).is_err(),
        "the document decoder rejects the body"
    );
}

/// Sets the subject route as a Lua global.
fn publish_subject_path(lua: &Lua) {
    let route = Route::new(RouteBase::Home, "subject").unwrap();
    let encoded = route.encode(lua).unwrap();
    lua.globals().set("subject_path", encoded).unwrap();
}

/// Returns a temp project with two files and a Lua state holding its stores.
fn opened_project_lua() -> (tempfile::TempDir, Lua, Stores) {
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

/// Sets one project file as a Lua global and returns its name.
fn publish_handle(lua: &Lua, stores: &Stores, name: &str) -> String {
    let global = format!("handle_{}", name.replace('.', "_"));
    let handle = match stores.resources().resource(name) {
        Ok(found) => found,
        Err(error) => panic!("the project fails to open: {error}"),
    };
    lua.globals().set(global.clone(), handle).unwrap();
    global
}

/// Returns whether the pool is empty.
fn pool_is_empty(directory: &tempfile::TempDir) -> bool {
    let pool = directory.path().join("config").join("blobs");
    !pool.exists() || fs::read_dir(&pool).unwrap_or_default().is_empty()
}

#[test]
fn document_flat_round_trips_path_beside_text() {
    let lua = fresh_lua();
    let source = Document::new(
        Route::new(RouteBase::Home, "subject").unwrap(),
        Data::Text(TextDocument {
            content: "hi".to_owned(),
            mode: FileMode::new(0o644),
            unmanaged: false,
        }),
    );
    let encoded = source.encode(&lua).unwrap();
    let Value::Table(table) = encoded else {
        panic!("expected a table");
    };
    let path: mlua::AnyUserData = table.get("path").unwrap();
    let route = path.borrow::<Route>().unwrap();
    assert_eq!(route.base(), RouteBase::Home);
    assert_eq!(route.relative(), "subject");
    let content: String = table.get("text").unwrap();
    assert_eq!(content.as_str(), "hi");
    let back = Document::decode(&Value::Table(table), &lua).unwrap();
    assert_eq!(back, source);
    let flat = document_from(&lua, r#"{ text = "hi", mode = "644" }"#);
    assert_eq!(flat, source);
}

#[test]
fn document_decode_rejects_mixed_markers() {
    let lua = fresh_lua();
    reject_body(
        &lua,
        r#"{ text = "hi", target = { base = "home", relative = "linked" } }"#,
    );
    reject_body(&lua, r#"{ nope = 1 }"#);
    let value: Value = lua.load("return { text = \"hi\" }").call(()).unwrap();
    assert!(
        Document::decode(&value, &lua).is_err(),
        "the document decoder rejects a body with no path"
    );
}

#[test]
fn document_decode_rejects_malformed_fields() {
    let (_directory, lua, stores) = opened_project_lua();
    let note = publish_handle(&lua, &stores, "note.txt");
    reject_body(&lua, r#"{ text = "hi", bogus = 1 }"#);
    reject_body(&lua, r#"{ text = "hi", mode = 644 }"#);
    reject_body(&lua, r#"{ text = "hi", unmanaged = 1 }"#);
    reject_body(&lua, r#"{ handle = "note.txt" }"#);
    reject_body(&lua, r#"{ members = { ["top.txt"] = "hi" } }"#);
    reject_body(
        &lua,
        &format!(r#"{{ members = {{ ["top.txt"] = {{ handle = {note} }} }} }}"#),
    );
    reject_body(&lua, r#"{ profile = { { frobnicate = { name = "x" } } } }"#);
}

#[test]
fn mode_strings_decode_and_encode() {
    let lua = fresh_lua();
    let octal = document_from(&lua, r#"{ text = "hi", mode = "644", unmanaged = true }"#);
    match octal.data {
        Data::Text(TextDocument {
            mode, unmanaged, ..
        }) => {
            assert_eq!(mode.bits(), 0o644);
            assert!(unmanaged);
        }
        _ => panic!("expected a text body"),
    }
    let symbolic = document_from(&lua, r#"{ text = "hi", mode = "rwxr-xr-x" }"#);
    match symbolic.data {
        Data::Text(TextDocument {
            mode, unmanaged, ..
        }) => {
            assert_eq!(mode.bits(), 0o755);
            assert!(!unmanaged);
        }
        _ => panic!("expected a text body"),
    }
    reject_body(&lua, r#"{ text = "hi", mode = "zzz" }"#);
    let source = Data::Text(TextDocument {
        content: "hi".to_owned(),
        mode: FileMode::new(0o755),
        unmanaged: false,
    });
    let encoded = source.encode(&lua).unwrap();
    let Value::Table(table) = encoded else {
        panic!("expected a table");
    };
    let rendered: String = table.get("mode").unwrap();
    assert_eq!(rendered.as_str(), "755");
    let back = Data::decode(&Value::Table(table), &lua).unwrap();
    assert_eq!(back, source);
}

#[test]
fn handles_serialize_as_bare_sha() {
    let (_directory, lua, stores) = opened_project_lua();
    let handle = match stores.resources().resource("note.txt") {
        Ok(found) => found,
        Err(error) => panic!("the project fails to open: {error}"),
    };
    let carried = handle.sha().clone();
    let live = Data::Opaque(OpaqueDocument {
        handle,
        mode: FileMode::new(0o644),
        unmanaged: false,
    });
    match live.encode(&lua) {
        Ok(Value::Table(table)) => {
            let _: mlua::AnyUserData = table.get("handle").unwrap();
        }
        Ok(_) => panic!("expected a table"),
        Err(error) => panic!("the handle fails to encode: {error}"),
    }
    let text = match serde_json::to_string(&live) {
        Ok(found) => found,
        Err(error) => panic!("the live data fails to serialize: {error}"),
    };
    assert!(
        text.contains(&carried.hex()),
        "the live data writes the carried sha"
    );
    match serde_json::from_str::<Data>(&text) {
        Ok(Data::Opaque(back)) => assert_eq!(
            back.handle,
            ResourceHandle::Cache(carried),
            "the live data round-trips through the cache alone"
        ),
        Ok(_) => panic!("expected an opaque body"),
        Err(error) => panic!("the live data fails to decode: {error}"),
    };
    let blob = Sha::hash(b"stored");
    let stored = Data::Opaque(OpaqueDocument {
        handle: ResourceHandle::Cache(blob.clone()),
        mode: FileMode::new(0o644),
        unmanaged: false,
    });
    let text = match serde_json::to_string(&stored) {
        Ok(found) => found,
        Err(error) => panic!("the stored data fails to serialize: {error}"),
    };
    assert!(
        text.contains(&blob.hex()),
        "the stored data writes the bare sha"
    );
    match serde_json::from_str::<Data>(&text) {
        Ok(back) => assert_eq!(back, stored, "the stored data survives serde"),
        Err(error) => panic!("the stored data fails to decode: {error}"),
    }
}

#[test]
fn tree_mixed_members_keep_their_modes() {
    let (directory, lua, stores) = opened_project_lua();
    let note = publish_handle(&lua, &stores, "note.txt");
    let other = publish_handle(&lua, &stores, "other.txt");
    let body = format!(
        r#"{{
        members = {{
            ["top.txt"] = {note},
            ["sub/nested.txt"] = {{ handle = {other}, mode = "755" }},
        }},
    }}"#
    );
    let document = document_from(&lua, &body);
    match document.data {
        Data::Tree(TreeDocument { members }) => {
            assert_eq!(members.len(), 2, "the tree document keeps both members");
            let bare = match members.get("top.txt") {
                Some(found) => found,
                None => panic!("expected the top.txt member"),
            };
            assert!(
                matches!(bare, TreeMember::Handle(_)),
                "the bare entry decodes as a bare handle"
            );
            assert_eq!(
                bare.mode().bits(),
                0o644,
                "the bare entry takes the default mode"
            );
            let full = match members.get("sub/nested.txt") {
                Some(found) => found,
                None => panic!("expected the sub/nested.txt member"),
            };
            assert!(
                matches!(full, TreeMember::Full { .. }),
                "the table entry decodes as a full member"
            );
            assert_eq!(full.mode().bits(), 0o755, "the full member keeps its mode");
            assert!(
                members
                    .values()
                    .all(|member| matches!(member.handle(), ResourceHandle::Project(..))),
                "every member holds a project handle"
            );
        }
        _ => panic!("expected a tree body"),
    }
    assert!(pool_is_empty(&directory), "the pool is empty after decode");
}

#[test]
fn rc_decodes_every_op() {
    let lua = fresh_lua();
    let bin = Route::new(RouteBase::Home, "bin").unwrap();
    lua.globals()
        .set("bin_dir", bin.encode(&lua).unwrap())
        .unwrap();
    let extra = Route::new(RouteBase::Home, "extra.sh").unwrap();
    lua.globals()
        .set("extra_path", extra.encode(&lua).unwrap())
        .unwrap();
    let body = r#"{
        profile = {
            { env = { name = "EDITOR", value = "hx", when = { env_eq = { key = "TERM", value = "xterm" } } } },
            { path = { name = "PATH", dir = bin_dir } },
        },
        config = {
            { alias = { name = "ll", expansion = "ls -l" } },
            { eval = { argv = { "mise", "activate" } } },
        },
        final = {
            { cmd = { argv = { "echo", "hi" } } },
            { source = { path = extra_path } },
        },
    }"#;
    let documents = document_from(&lua, body);
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
                _ => panic!("expected an env entry"),
            }
            assert!(profile[0].when().is_some(), "the env entry keeps its guard");
            match &profile[1] {
                RcEntry::Path { name, dir, .. } => {
                    assert_eq!(name, "PATH");
                    assert_eq!(dir.display(), "home('bin')");
                }
                _ => panic!("expected a path entry"),
            }
            match &config[0] {
                RcEntry::Alias {
                    name, expansion, ..
                } => {
                    assert_eq!(name, "ll");
                    assert_eq!(expansion, "ls -l");
                }
                _ => panic!("expected an alias entry"),
            }
            match &config[1] {
                RcEntry::Eval { argv, .. } => assert_eq!(argv.len(), 2),
                _ => panic!("expected an eval entry"),
            }
            match &final_entries[0] {
                RcEntry::Cmd { argv, .. } => assert_eq!(argv.len(), 2),
                _ => panic!("expected a cmd entry"),
            }
            match &final_entries[1] {
                RcEntry::Source { path, .. } => {
                    assert_eq!(path.display(), "home('extra.sh')");
                }
                _ => panic!("expected a source entry"),
            }
        }
        _ => panic!("expected an rc body"),
    }
}

#[test]
fn rc_decode_rejects_unknown_ops() {
    let lua = fresh_lua();
    for body in [
        r#"{ profile = { { frobnicate = { name = "x" } } } }"#,
        r#"{ profile = { { env = { name = "A", value = "B", bogus = 1 } } } }"#,
        r#"{ profile = { { env = { name = "A" } } } }"#,
    ] {
        reject_body(&lua, body);
    }
}
