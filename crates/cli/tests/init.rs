//! Init scaffolds over temp dirs.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};

use common::seed_file;
use confit_cli::actions::init::{InitOutcome, InitRunner};
use confit_cli::cli::InitArgs;
use confit_cli::error::CliError;

/// Scaffold one temp dir with default flags.
fn scaffold(dir: &Path) -> PathBuf {
    let root = dir.join("site");
    let args = InitArgs {
        dir: Some(root.clone()),
        stubs: false,
        plugins: false,
        lsp: false,
    };
    let mut outcomes = InitRunner { args: &args }.execute().unwrap();
    match outcomes.pop() {
        Some(InitOutcome::Scaffold(report)) => {
            assert!(report.written > 0, "the scaffold writes files");
            root
        }
        Some(_) => panic!("plain init scaffolds"),
        None => panic!("plain init reports one outcome"),
    }
}

#[test]
fn init_writes_scaffold_with_stubs_and_plugins() {
    let dir = tempfile::tempdir().unwrap();
    let root = scaffold(dir.path());
    for path in [root.join("main.lua"), root.join("project.toml")] {
        assert!(path.exists(), "the scaffold holds {}", path.display());
    }
    let stubs = confit_driver::fs::read_dir(&root.join("stubs")).unwrap();
    assert!(!stubs.is_empty(), "the scaffold holds stubs");
    let plugins = confit_driver::fs::read_dir(&root.join("plugins")).unwrap();
    assert!(!plugins.is_empty(), "the scaffold holds plugins");
}

#[test]
fn init_refuses_present_scaffold() {
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
        Err(CliError::Exists { .. }) => {}
        Err(other) => panic!("present scaffold flattens: {other}"),
        Ok(_) => panic!("present scaffold passes"),
    }
}

#[test]
fn init_flags_rewrite_stubs_plugins_and_lsp() {
    let dir = tempfile::tempdir().unwrap();
    let root = scaffold(dir.path());
    let stubs = confit_driver::fs::read_dir(&root.join("stubs")).unwrap();
    seed_file(&stubs[0], b"garbage");
    let args = InitArgs {
        dir: Some(root.clone()),
        stubs: true,
        plugins: true,
        lsp: true,
    };
    let outcomes = InitRunner { args: &args }.execute().unwrap();
    assert_eq!(outcomes.len(), 3, "every flag reports one outcome");
    let bytes = std::fs::read(&stubs[0]).unwrap();
    assert_ne!(bytes, b"garbage", "the stubs flag restores files");
    assert!(
        root.join(".luarc.json").exists(),
        "the lsp flag writes the editor config"
    );
}
