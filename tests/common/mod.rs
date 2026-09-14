//! Shared integration helpers.
//!
//! Temp project roots for evaluation. Uses tempfile only.
#![allow(clippy::expect_used)]

use std::path::PathBuf;

/// Writes files plus profile into a temp root.
///
/// # Arguments
///
/// * `files` - relative paths plus contents.
/// * `profile` - profile source written as profile.lua.
///
/// # Returns
///
/// Temp directory plus profile path.
pub fn project(files: &[(&str, &str)], profile: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, contents) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdirs");
        }
        std::fs::write(&path, contents).expect("write");
    }
    let profile_path = dir.path().join("profile.lua");
    std::fs::write(&profile_path, profile).expect("write profile");
    (dir, profile_path)
}
