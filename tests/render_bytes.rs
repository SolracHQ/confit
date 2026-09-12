//! Render behavior test: data-to-bytes goldens with SHA assertions.
//!
//! Covers every artifact kind through `render_artifact` plus `render_rc`
//! grouping, asserting exact bytes and golden SHA-256 digests. Uses
//! memory template sources only; never touches `$HOME`.
//!
//! To regenerate after an intentional render change, run with
//! `UPDATE_GOLDEN=1` to print fresh digests, eyeball the bytes against the
//! layout documented in `src/render/rc.rs`, then overwrite the constants
//! below verbatim.

#![allow(clippy::expect_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;

use confit::model::state::artifact::ArtifactData;
use confit::model::state::artifact::BlameSet;
use confit::model::state::artifact::Table;
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
/// Golden: two-tool rc fixture in `two_tool_rc`.
const RC_SHA: &str = "be5445783efb9c18cfe12751d66c895df56da92b521082c440cfb7e74f61fe8e";

/// Flat single-pair table used by the structured goldens.
fn flat_table() -> Table {
    [("name".to_string(), serde_json::json!("bat"))]
        .into_iter()
        .collect()
}

/// Two-tool rc fixture: profile plus env, two alias owners, two init owners.
fn two_tool_rc() -> (RcData, BlameSet) {
    let data = RcData {
        profile: vec![ProfileEntry {
            name: "PATH".into(),
            value: "/a".into(),
            op: PathOp::Prepend,
            when: None,
        }],
        env: vec![
            EnvEntry {
                name: "A".into(),
                value: "1".into(),
                when: None,
            },
            EnvEntry {
                name: "B".into(),
                value: "x y".into(),
                when: None,
            },
        ],
        aliases: [
            ("cat".to_string(), "bat".to_string()),
            ("ls".to_string(), "eza --icons".to_string()),
        ]
        .into_iter()
        .collect(),
        init: vec![
            InitEntry::Eval {
                argv: vec!["zoxide".into(), "init".into(), "bash".into()],
            },
            InitEntry::Cmd {
                argv: vec!["task".into(), "--completion".into(), "bash".into()],
            },
        ],
    };
    let blame = BlameSet {
        aliases: [
            ("cat".to_string(), "a-tool".to_string()),
            ("ls".to_string(), "l-tool".to_string()),
        ]
        .into_iter()
        .collect(),
        env: vec!["e-tool".into(), "e-tool".into()],
        profile: vec!["p-tool".into()],
        init: vec!["i-tool".into(), "i-tool".into()],
        toml: BTreeMap::new(),
    };
    (data, blame)
}

/// Expected bytes for the two-tool rc fixture.
fn expected_rc() -> &'static str {
    "# >>> confit:p-tool\n\
     export PATH=/a:\"${PATH}\"\n\
     # <<< confit\n\
     # >>> confit:e-tool\n\
     export A=1\n\
     export B='x y'\n\
     # <<< confit\n\
     \n\
     # >>> confit:a-tool\n\
     alias cat=bat\n\
     # <<< confit\n\
     # >>> confit:l-tool\n\
     alias ls='eza --icons'\n\
     # <<< confit\n\
     \n\
     # >>> confit:i-tool\n\
     eval \"$(zoxide init bash)\"\n\
     task --completion bash\n\
     # <<< confit\n"
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

    let toml_bytes = render_artifact(
        &ArtifactData::Toml(table.clone()),
        &BlameSet::default(),
        &source,
        root,
    )
    .expect("toml renders");
    assert_eq!(toml_bytes, b"name = \"bat\"\n");
    check_golden("toml", &toml_bytes, TOML_SHA);

    let json_bytes = render_artifact(
        &ArtifactData::Json(table.clone()),
        &BlameSet::default(),
        &source,
        root,
    )
    .expect("json renders");
    assert_eq!(json_bytes, b"{\n  \"name\": \"bat\"\n}");
    check_golden("json", &json_bytes, JSON_SHA);

    let yaml_bytes = render_artifact(
        &ArtifactData::Yaml(table),
        &BlameSet::default(),
        &source,
        root,
    )
    .expect("yaml renders");
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
        &BlameSet::default(),
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
        &BlameSet::default(),
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
        &BlameSet::default(),
        &source,
        root,
    )
    .expect("template renders");
    assert_eq!(bytes, b"hello bat!");
    check_golden("template", &bytes, TEMPLATE_SHA);
}

#[test]
fn rc_groups_by_tool_with_golden_hash() {
    let (data, blame) = two_tool_rc();
    let rendered = confit::services::render::rc::render_rc(&data, &blame);
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
