//! Render behavior test: data-to-bytes goldens with SHA assertions.
//!
//! Covers every artifact kind through `render_artifact` plus `render_rc`
//! grouping, asserting exact bytes and golden SHA-256 digests. Uses
//! memory template sources only; never touches `$HOME`.
//!
//! To regenerate after an intentional render change, run with
//! `UPDATE_GOLDEN=1` to print fresh digests, eyeball the bytes against the
//! layout documented in `src/services/render/rc.rs`, then overwrite the constants
//! below verbatim.

#![allow(clippy::expect_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;

use confit::model::state::artifact::ArtifactData;
use confit::model::state::artifact::Table;
use confit::model::state::rc::AliasEntry;
use confit::model::state::rc::EnvEntry;
use confit::model::state::rc::InitEntry;
use confit::model::state::rc::PathOp;
use confit::model::state::rc::ProfileEntry;
use confit::model::state::rc::RcData;
use confit::repository::MemoryFilesystem;
use confit::security::sha256_hex;
use confit::services::render::render_artifact;

/// Golden: `render_toml({"name": "bat"})` plus newline.
const TOML_SHA: &str = "961136fcff2451c2193d8c19f6080d4c385eb30462e640ef9a02ee0a4224a23f";
/// Golden: `render_json({"name": "bat"})` pretty, no trailing newline.
const JSON_SHA: &str = "6977ae416db389f8a6bdec4d5138ae5081a8e5513a9131c8663df31f766613ac";
/// Golden: `render_yaml({"name": "bat"})`, no trailing newline.
const YAML_SHA: &str = "743f2f5a0147b84bde1814819c773c04e2057f71984226e90b0e8dee8d3cf427";
/// Golden: file content `export X=1` with no added newline.
const FILE_SHA: &str = "7f6e51ac7d765befc387a1bd87405065e0c60bfc442f4ad18d5a37d9901b249e";
/// Golden: link target `dest` bytes.
const LINK_SHA: &str = "1d5e6a1edddf2cb59b7bbc0218e03c305de6c11485a2aa0d3bafc7466b4b8e3c";
/// Golden: inline template `hello {{ name }}!` with `name = world`.
const TEMPLATE_SHA: &str = "0fbd9f7acaf3edf65b61f048479552eac91eb22bbd133ae518a7bdd4fe8ea5dc";
/// Golden: bare rc groups in `two_tool_rc`.
const RC_SHA: &str = "af993886f9dd7fc5b24fd1ebbee90b769c1fc74160a9a52b190871e5b501e000";

/// Flat single-pair table used by the structured goldens.
fn flat_table() -> Table {
    [("name".to_string(), serde_json::json!("bat"))]
        .into_iter()
        .collect()
}

/// Two-area rc fixture: profile plus env, two aliases, two init lines.
fn two_tool_rc() -> RcData {
    RcData {
        profile: vec![ProfileEntry {
            name: "PATH".into(),
            value: "/a".into(),
            op: PathOp::Prepend,
            when: None,
            priority: 0,
        }],
        env: vec![
            EnvEntry {
                name: "A".into(),
                value: "1".into(),
                when: None,
                priority: 0,
            },
            EnvEntry {
                name: "B".into(),
                value: "x y".into(),
                when: None,
                priority: 0,
            },
        ],
        aliases: vec![
            AliasEntry {
                name: "cat".into(),
                value: "bat".into(),
                when: None,
                priority: 0,
            },
            AliasEntry {
                name: "ls".into(),
                value: "eza --icons".into(),
                when: None,
                priority: 0,
            },
        ],
        init: vec![
            InitEntry::Eval {
                argv: vec!["zoxide".into(), "init".into(), "bash".into()],
                when: None,
                priority: 0,
            },
            InitEntry::Cmd {
                argv: vec!["task".into(), "--completion".into(), "bash".into()],
                when: None,
                priority: 0,
            },
        ],
    }
}

/// Expected bytes for the two-area rc fixture.
fn expected_rc() -> &'static str {
    "export PATH=/a:\"${PATH}\"\n\
     export A=1\n\
     export B='x y'\n\
     \n\
     alias cat=bat\n\
     alias ls='eza --icons'\n\
     \n\
     eval \"$(zoxide init bash)\"\n\
     task --completion bash\n"
}

/// Prints fresh digests when `UPDATE_GOLDEN=1`, else asserts goldens.
fn check_golden(label: &str, bytes: &[u8], golden: &str) {
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        println!("GOLDEN {label}: {}", sha256_hex(bytes));
    } else {
        assert_eq!(sha256_hex(bytes), golden, "{label} bytes changed");
    }
}

#[test]
fn structured_kinds_match_golden_bytes_and_hashes() {
    let source = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    let table = flat_table();
    let root = std::path::Path::new("root");

    let toml_bytes =
        render_artifact(&ArtifactData::Toml(table.clone()), &source, root).expect("toml renders");
    assert_eq!(toml_bytes, b"name = \"bat\"\n");
    check_golden("toml", &toml_bytes, TOML_SHA);

    let json_bytes =
        render_artifact(&ArtifactData::Json(table.clone()), &source, root).expect("json renders");
    assert_eq!(json_bytes, b"{\n  \"name\": \"bat\"\n}");
    check_golden("json", &json_bytes, JSON_SHA);

    let yaml_bytes =
        render_artifact(&ArtifactData::Yaml(table), &source, root).expect("yaml renders");
    assert_eq!(yaml_bytes, b"name: bat");
    check_golden("yaml", &yaml_bytes, YAML_SHA);
}

#[test]
fn file_and_link_pass_bytes_through() {
    let source = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    let root = std::path::Path::new("root");

    let file_bytes = render_artifact(
        &ArtifactData::File {
            content: "export X=1".into(),
        },
        &source,
        root,
    )
    .expect("file renders");
    assert_eq!(file_bytes, b"export X=1");
    check_golden("file", &file_bytes, FILE_SHA);

    let link_bytes = render_artifact(
        &ArtifactData::Link {
            target: "dest".into(),
        },
        &source,
        root,
    )
    .expect("link renders");
    assert_eq!(link_bytes, b"dest");
    check_golden("link", &link_bytes, LINK_SHA);
}

#[test]
fn template_renders_inline_vars() {
    let source = MemoryFilesystem {
        files: RefCell::new(BTreeMap::new()),
        failures: RefCell::new(BTreeMap::new()),
    };
    let root = std::path::Path::new("root");
    let vars = flat_table();
    let bytes = render_artifact(
        &ArtifactData::Template {
            src: "hello {{ name }}!".into(),
            vars,
        },
        &source,
        root,
    )
    .expect("template renders");
    assert_eq!(bytes, b"hello bat!");
    check_golden("template", &bytes, TEMPLATE_SHA);
}

#[test]
fn rc_sections_with_golden_hash() {
    let data = two_tool_rc();
    let rendered = confit::services::render::rc::render_rc(&data);
    assert_eq!(rendered, expected_rc());
    check_golden("rc", rendered.as_bytes(), RC_SHA);
}

#[test]
fn sha256_hex_matches_known_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
