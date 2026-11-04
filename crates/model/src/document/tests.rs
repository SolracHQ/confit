use super::{
    Data, Document, FileMode, MemberKey, OpaqueDocument, RcEntry, TextDocument, TreeDocument,
    TreeMember,
};
use confit_core::resource_ref::ResourceRef;
use confit_core::sha::Sha;
use lua_binding::value::{Decode, Encode};
use mlua::{Lua, Value};
use std::collections::BTreeMap;

use crate::routes::{Route, RouteBase};

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

/// Sets one content hash as a Lua global and returns its name.
fn publish_ref(lua: &Lua, name: &str, bytes: &[u8]) -> String {
    let global = format!("handle_{name}");
    let reference = ResourceRef::new(Sha::hash(bytes));
    lua.globals().set(global.clone(), reference).unwrap();
    global
}

fn literal(relative: &str) -> Route {
    Route::new(RouteBase::Literal, relative).unwrap()
}

/// Builds the expected manifest bytes for one tree.
fn manifest_bytes(members: &BTreeMap<MemberKey, TreeMember>) -> Vec<u8> {
    let mut out = Vec::new();
    for (relative, member) in members {
        let line = format!(
            "{:o} {} {}\n",
            member.mode().bits(),
            relative.as_str(),
            member.handle().sha()
        );
        out.extend_from_slice(line.as_bytes());
    }
    out
}

/// Returns the content hash of one document payload.
fn hash_of(data: Data) -> Sha {
    let document = Document {
        path: literal("pinned/subject"),
        data,
    };
    match document.hash() {
        Ok(found) => found,
        Err(error) => panic!("the document fails to hash: {error}"),
    }
}

/// Returns one parsed member key.
fn member_key(relative: &str) -> MemberKey {
    match MemberKey::new(relative.to_owned()) {
        Ok(found) => found,
        Err(error) => panic!("the key fails to parse: {error}"),
    }
}

#[test]
fn member_keys_reject_bad_paths() {
    for key in [
        "",
        "/top.txt",
        "sub//nested.txt",
        "sub/../nested.txt",
        "../top.txt",
        ".",
        "..",
        "a/./b",
        "back\\slash",
        "a\\b",
    ] {
        assert!(
            MemberKey::new(key.to_owned()).is_err(),
            "the key fails: {key}"
        );
    }
    let key = member_key("sub/nested.txt");
    assert_eq!(key.as_str(), "sub/nested.txt");
}

#[test]
fn tree_decode_rejects_bad_member_keys() {
    let lua = fresh_lua();
    let note = publish_ref(&lua, "note", b"hello");
    for key in ["", "/top.txt", "sub//nested.txt", "sub/../nested.txt"] {
        let body = format!(r#"{{ members = {{ ["{key}"] = {note} }} }}"#);
        reject_body(&lua, &body);
    }
}

#[test]
fn member_keys_serialize_as_plain_strings() {
    let key = member_key("sub/nested.txt");
    let text = match serde_json::to_string(&key) {
        Ok(found) => found,
        Err(error) => panic!("the key fails to serialize: {error}"),
    };
    assert_eq!(text, "\"sub/nested.txt\"");
    match serde_json::from_str::<MemberKey>(&text) {
        Ok(back) => assert_eq!(back, key, "the key survives serde"),
        Err(error) => panic!("the key fails to decode: {error}"),
    }
}

#[test]
fn text_hash_pins_the_rendered_bytes() {
    let hash = hash_of(Data::Text(TextDocument {
        content: "pinned text\n".into(),
        mode: FileMode::new(0o644),
        unmanaged: false,
    }));
    assert_eq!(
        hash,
        Sha::new("f007767c80150f15e9cacf96dc8d24ac3b6b3094c9b727d1534b175910968f2a").unwrap(),
        "the text hash follows the rendered bytes"
    );
}

// Hash contracts pin text and tree alone. Other variants reuse model bytes.
#[test]
fn tree_hash_covers_the_manifest_bytes() {
    let first = Sha::new("aa".repeat(32)).unwrap();
    let second = Sha::new("cc".repeat(32)).unwrap();
    let mut members = BTreeMap::new();
    members.insert(
        member_key("b"),
        TreeMember::Full {
            handle: ResourceRef::new(second),
            mode: FileMode::new(0o755),
        },
    );
    members.insert(
        member_key("a"),
        TreeMember::Full {
            handle: ResourceRef::new(first),
            mode: FileMode::new(0o644),
        },
    );
    let expected = Sha::hash(&manifest_bytes(&members));
    assert_eq!(
        hash_of(Data::Tree(TreeDocument { members })),
        expected,
        "the tree hash covers the manifest bytes"
    );
}

#[test]
fn tree_hash_covers_bare_and_full_members_with_their_modes() {
    let bare = ResourceRef::new(Sha::hash(b"bare bytes"));
    let full = ResourceRef::new(Sha::hash(b"full bytes"));
    let mut members = BTreeMap::new();
    members.insert(member_key("top.txt"), TreeMember::Handle(bare));
    members.insert(
        member_key("sub/nested.txt"),
        TreeMember::Full {
            handle: full,
            mode: FileMode::new(0o755),
        },
    );
    let document = Document::new(
        Route::new(RouteBase::Home, "tree").unwrap(),
        Data::Tree(TreeDocument { members }),
    );
    match &document.data {
        Data::Tree(found) => {
            let kept = &found.members;
            assert_eq!(kept.len(), 2, "the tree keeps both members");
            let bare_member = match kept.get("top.txt") {
                Some(found) => found,
                None => panic!("expected the top.txt member"),
            };
            assert_eq!(
                bare_member.handle(),
                &bare,
                "the bare member keeps its bytes"
            );
            assert_eq!(
                bare_member.mode().bits(),
                0o644,
                "the bare member takes the default mode"
            );
            let full_member = match kept.get("sub/nested.txt") {
                Some(found) => found,
                None => panic!("expected the sub/nested.txt member"),
            };
            assert_eq!(
                full_member.handle(),
                &full,
                "the full member keeps its bytes"
            );
            assert_eq!(
                full_member.mode().bits(),
                0o755,
                "the full member keeps its mode"
            );
        }
        _ => panic!("expected a tree body"),
    }
    match document.hash() {
        Ok(hash) => assert_eq!(
            hash,
            Sha::hash(&manifest_bytes(&document_members(&document))),
            "the tree hash covers the manifest bytes"
        ),
        Err(error) => panic!("the tree fails to hash: {error}"),
    }
}

/// Returns the members of a tree document.
fn document_members(document: &Document) -> BTreeMap<MemberKey, TreeMember> {
    match &document.data {
        Data::Tree(found) => found.members.clone(),
        _ => panic!("expected a tree body"),
    }
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
    let lua = fresh_lua();
    let note = publish_ref(&lua, "note", b"hello");
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
    let lua = fresh_lua();
    let handle = ResourceRef::new(Sha::hash(b"hello"));
    let carried = *handle.sha();
    let data = Data::Opaque(OpaqueDocument {
        handle,
        mode: FileMode::new(0o644),
        unmanaged: false,
    });
    match data.encode(&lua) {
        Ok(Value::Table(table)) => {
            let _: mlua::AnyUserData = table.get("handle").unwrap();
        }
        Ok(_) => panic!("expected a table"),
        Err(error) => panic!("the handle fails to encode: {error}"),
    }
    let text = match serde_json::to_string(&data) {
        Ok(found) => found,
        Err(error) => panic!("the data fails to serialize: {error}"),
    };
    assert!(
        text.contains(&carried.hex()),
        "the data writes the carried sha"
    );
    match serde_json::from_str::<Data>(&text) {
        Ok(Data::Opaque(back)) => assert_eq!(
            back.handle,
            ResourceRef::new(carried),
            "the data round-trips through its digest"
        ),
        Ok(_) => panic!("expected an opaque body"),
        Err(error) => panic!("the data fails to decode: {error}"),
    };
    let blob = Sha::hash(b"stored");
    let stored = Data::Opaque(OpaqueDocument {
        handle: ResourceRef::new(blob),
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
    let lua = fresh_lua();
    let note = publish_ref(&lua, "note", b"hello");
    let other = publish_ref(&lua, "other", b"world");
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
            let top = match members.get("top.txt") {
                Some(found) => found,
                None => panic!("expected the top.txt member"),
            };
            assert_eq!(
                top.handle().sha(),
                &Sha::hash(b"hello"),
                "the bare member keeps the note digest"
            );
            let nested = match members.get("sub/nested.txt") {
                Some(found) => found,
                None => panic!("expected the sub/nested.txt member"),
            };
            assert_eq!(
                nested.handle().sha(),
                &Sha::hash(b"world"),
                "the full member keeps the other digest"
            );
        }
        _ => panic!("expected a tree body"),
    }
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
