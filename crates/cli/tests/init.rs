//! Init scaffolds over temporary folders.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use confit_cli::actions::init::InitRunner;
use confit_cli::cli::InitArgs;
use confit_driver::fs;

/// Scaffolds the site folder with the default flags and returns the root.
fn scaffold(dir: &Path) -> PathBuf {
    let root = dir.join("site");
    let args = InitArgs {
        dir: Some(root.clone()),
        stubs: false,
        plugins: false,
        lsp: false,
    };
    let items = InitRunner { args: &args }.execute().unwrap();
    assert_eq!(items.len(), 1, "the scaffold reports one item");
    let item = &items[0];
    assert_eq!(item.stage, "init", "the scaffold reports the init stage");
    assert_eq!(
        item.path,
        root.join("main.lua").display().to_string(),
        "the scaffold reports the profile path"
    );
    assert!(item.written > 0, "the scaffold writes files");
    root
}

#[test]
fn init_writes_the_scaffold_with_stubs_and_plugins() {
    let dir = tempfile::tempdir().unwrap();
    let root = scaffold(dir.path());
    for path in [root.join("main.lua"), root.join("project.toml")] {
        assert!(path.exists(), "the scaffold holds {}", path.display());
    }
    let stubs = fs::read_dir(&root.join("stubs")).unwrap();
    assert!(!stubs.is_empty(), "the scaffold holds stubs");
    let plugins = fs::read_dir(&root.join("plugins")).unwrap();
    assert!(!plugins.is_empty(), "the scaffold holds plugins");
}

#[test]
fn init_scaffold_writes_the_editor_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = scaffold(dir.path());
    assert!(
        root.join(".luarc.json").exists(),
        "the scaffold holds the editor config"
    );
}

#[test]
fn init_rejects_a_present_scaffold() {
    let dir = tempfile::tempdir().unwrap();
    let root = scaffold(dir.path());
    let args = InitArgs {
        dir: Some(root),
        stubs: false,
        plugins: false,
        lsp: false,
    };
    let outcome = InitRunner { args: &args }.execute();
    match outcome {
        Err(error) if error.summary().contains("Cannot overwrite existing") => {}
        Err(other) => panic!("the init fails to reject the present scaffold: {other}"),
        Ok(_) => panic!("expected a refused scaffold"),
    }
}

#[test]
fn init_flags_rewrite_stubs_plugins_and_lsp() {
    let dir = tempfile::tempdir().unwrap();
    let root = scaffold(dir.path());
    let stubs = fs::read_dir(&root.join("stubs")).unwrap();
    common::seed_file(&stubs[0], b"garbage");
    let args = InitArgs {
        dir: Some(root.clone()),
        stubs: true,
        plugins: true,
        lsp: true,
    };
    let items = InitRunner { args: &args }.execute().unwrap();
    assert_eq!(items.len(), 3, "every flag reports one item");
    assert_eq!(items[0].stage, "stubs", "the first item reports stubs");
    assert_eq!(items[1].stage, "plugins", "the second item reports plugins");
    assert_eq!(items[2].stage, "lsp", "the third item reports lsp");
    assert!(
        items[0].path.contains("stubs"),
        "the stubs item reports the stubs path"
    );
    assert!(
        items[1].path.contains("plugins"),
        "the plugins item reports the plugins path"
    );
    assert!(
        items[2].path.ends_with(".luarc.json"),
        "the lsp item reports the editor config"
    );
    for item in &items {
        assert!(item.written > 0, "every flag writes files");
    }
    let bytes = std::fs::read(&stubs[0]).unwrap();
    assert_ne!(bytes, b"garbage", "the stubs flag restores files");
    assert!(
        root.join(".luarc.json").exists(),
        "the lsp flag writes the editor config"
    );
}
