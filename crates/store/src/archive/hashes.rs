//! Recorded extract member hashes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit_driver::sha::Sha;
use confit_driver::{self as driver};
use serde::{Deserialize, Serialize};

/// Recorded member hash with size and mtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    sha: Sha,
    size: u64,
    modified: u64,
}

/// Recorded hashes keyed by archive-relative name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    files: BTreeMap<String, Entry>,
}

/// Derive the sibling manifest path for one extract folder.
fn manifest_path(dest: &Path) -> PathBuf {
    dest.with_extension("json")
}

/// Load one manifest, torn payloads read empty.
fn load(dest: &Path) -> Manifest {
    let bytes = match driver::fs::read(&manifest_path(dest)) {
        Ok(bytes) => bytes,
        Err(_) => return Manifest::default(),
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Store one manifest.
///
/// - Write failures pass silent.
fn store(dest: &Path, manifest: &Manifest) {
    let bytes = match serde_json::to_vec(manifest) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let _ = driver::fs::write(&manifest_path(dest), &bytes);
}

/// Read one file size with mtime. Unknown mtimes read absent.
fn stat_tick(path: &Path) -> Option<(u64, u64)> {
    let facts = driver::fs::metadata(path).ok()?;
    let modified = facts.modified();
    if modified == 0 {
        return None;
    }
    Some((facts.len(), modified))
}

/// Look up one recorded hash by name. Stale entries read absent.
pub(crate) fn fresh(dest: &Path, name: &str) -> Option<Sha> {
    let found = load(dest).files.remove(name)?;
    let (size, modified) = stat_tick(&dest.join(name))?;
    if found.size == size && found.modified == modified {
        Some(found.sha)
    } else {
        None
    }
}

/// Record hashes for members. Members without stamps stay out.
pub(crate) fn record(dest: &Path, entries: &BTreeMap<String, Sha>) {
    let mut manifest = load(dest);
    for (name, sha) in entries {
        match stat_tick(&dest.join(name)) {
            Some((size, modified)) => {
                manifest.files.insert(
                    name.clone(),
                    Entry {
                        sha: sha.clone(),
                        size,
                        modified,
                    },
                );
            }
            None => {
                manifest.files.remove(name);
            }
        }
    }
    store(dest, &manifest);
}

/// Record one member hash.
pub(crate) fn note(dest: &Path, name: &str, sha: &Sha) {
    let mut entries = BTreeMap::new();
    entries.insert(name.to_owned(), sha.clone());
    record(dest, &entries);
}
