//! Builders that the cli integration tests share.

#![allow(clippy::unwrap_used)]

use std::path::Path;

use confit_driver::fs;

/// Writes one file and creates the parent folders first.
pub fn seed_file(path: &Path, text: &[u8]) {
    fs::create_parent(path).unwrap();
    std::fs::write(path, text).unwrap();
}
