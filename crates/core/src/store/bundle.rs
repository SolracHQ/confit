//! Bundle
//!
//! Portable bundle archives.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::fs::Filesystem;
use crate::plan::{BUNDLE_VERSION, Bundle};
use crate::progress::{Event, ProgressSender};

use super::blobs::{check_blob_id, collect_blobs, gunzip_bytes, gzip_bytes};
use super::manifest::Manifest;
use super::slots::load_state;

/// Gzip level for the outer bundle tar.
const BUNDLE_GZIP_LEVEL: u32 = 0;

/// Bundle manifest file name inside the archive.
const BUNDLE_MANIFEST: &str = "manifest.json";

/// Bundle blob folder prefix inside the archive.
const BUNDLE_BLOBS_PREFIX: &str = "blobs/";

/// Writes one portable bundle holding the manifest plus referenced blobs.
///
/// The tar.gz archive holds `manifest.json` first, then one
/// `blobs/<sha>` gzip entry per referenced blob in sorted
/// order with no duplicates.
///
/// # Arguments
///
/// * `plan` - the live plan under exporting.
/// * `dest` - the bundle file under writing.
/// * `fs` - the backend under writing.
/// * `progress` - the sink for compression facts, holding `None` for silence.
///
/// # Returns
///
/// Unit once the bundle lands.
///
/// # Errors
///
/// Compression, archive, plus write failures surface as
/// plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Bundle;
/// use confit_core::store::bundle::write_bundle;
///
/// let outcome = write_bundle(&Bundle::empty(), std::path::Path::new("bundle.tgz"), &MemoryFs::new(), None);
/// assert!(matches!(outcome, Ok(())));
/// ```
pub fn write_bundle(
    plan: &Bundle,
    dest: &Path,
    fs: &dyn Filesystem,
    progress: Option<&ProgressSender>,
) -> Result<()> {
    let stored = Manifest::of(plan);
    let manifest = serde_json::to_vec_pretty(&stored)
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
    let encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(BUNDLE_GZIP_LEVEL));
    let mut builder = tar::Builder::new(encoder);
    append_bundle_entry(&mut builder, BUNDLE_MANIFEST, &manifest, dest)?;
    let blobs: Vec<(String, Vec<u8>)> = collect_blobs(plan)
        .into_iter()
        .map(|(sha, bytes)| (sha, bytes.to_vec()))
        .collect();
    let total = blobs.len();
    if total > 0
        && let Some(sender) = progress
    {
        let bytes: u64 = blobs.iter().map(|(_, raw)| raw.len() as u64).sum();
        let _ = sender.send(Event::CompressStarted {
            blobs: total,
            bytes,
        });
    }
    let indexed: Vec<(usize, String, Vec<u8>)> = blobs
        .into_iter()
        .enumerate()
        .map(|(index, (sha, raw))| (index, sha, raw))
        .collect();
    let compressed: Vec<Result<(String, Vec<u8>)>> = indexed
        .par_iter()
        .map(|(index, sha, raw)| {
            let raw_len = raw.len() as u64;
            gzip_bytes(raw).map(|gzipped| {
                if let Some(sender) = progress {
                    let _ = sender.send(Event::BlobCompressed {
                        done: index + 1,
                        total,
                        bytes: raw_len,
                    });
                }
                (sha.clone(), gzipped)
            })
        })
        .collect();
    for entry in compressed {
        let (sha, gzipped) = entry?;
        append_bundle_entry(
            &mut builder,
            &format!("{BUNDLE_BLOBS_PREFIX}{sha}"),
            &gzipped,
            dest,
        )?;
    }
    let encoder = builder
        .into_inner()
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
    let archive = encoder
        .finish()
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
    fs.write(dest, &archive)
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))
}

/// Appends one file entry to a bundle archive.
fn append_bundle_entry(
    builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
    name: &str,
    bytes: &[u8],
    dest: &Path,
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, name, bytes)
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))
}

/// Reads one portable bundle into a live plan.
///
/// Blob entries verify against their names before hydrating.
/// Missing blobs fail naming the hash.
///
/// # Arguments
///
/// * `path` - the bundle file under reading.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The live plan holding binary bytes.
///
/// # Errors
///
/// Unreadable files plus bad archives plus version mismatch
/// plus missing blobs fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Bundle;
/// use confit_core::store::bundle::{read_bundle, write_bundle};
///
/// let fs = MemoryFs::new();
/// let dest = std::path::Path::new("bundle.tgz");
/// assert!(matches!(write_bundle(&Bundle::empty(), dest, &fs, None), Ok(())));
/// assert!(matches!(read_bundle(dest, &fs), Ok(plan) if plan.manifest.documents.is_empty()));
/// ```
pub fn read_bundle(path: &Path, fs: &dyn Filesystem) -> Result<Bundle> {
    let bytes = fs
        .read(path)
        .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?;
    let decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut archive = tar::Archive::new(decoder);
    let mut manifest: Option<Manifest> = None;
    let mut blobs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let entries = archive
        .entries()
        .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?;
    for entry in entries {
        let mut entry = entry
            .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?;
        let entry_path = entry
            .path()
            .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?
            .into_owned();
        if entry_path == Path::new(BUNDLE_MANIFEST) {
            if manifest.is_some() {
                return Err(Error::Plan(format!(
                    "read bundle '{}': duplicate manifest",
                    path.display()
                )));
            }
            let mut raw = Vec::new();
            entry.read_to_end(&mut raw).map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            let stored: Manifest = serde_json::from_slice(&raw).map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            if stored.version != BUNDLE_VERSION {
                return Err(Error::Plan(format!(
                    "bundle version {} reads unsupported, want {BUNDLE_VERSION}",
                    stored.version
                )));
            }
            manifest = Some(stored);
        } else if let Ok(rel) = entry_path.strip_prefix(BUNDLE_BLOBS_PREFIX) {
            let sha = match rel.to_str() {
                Some(sha) => sha.to_string(),
                None => {
                    return Err(Error::Plan(format!(
                        "read bundle '{}': bad blob entry",
                        path.display()
                    )));
                }
            };
            if check_blob_id(&sha).is_err() {
                return Err(Error::Plan(format!(
                    "read bundle '{}': bad blob entry '{sha}'",
                    path.display()
                )));
            }
            if blobs.contains_key(&sha) {
                return Err(Error::Plan(format!(
                    "read bundle '{}': duplicate blob '{sha}'",
                    path.display()
                )));
            }
            let mut gzipped = Vec::new();
            entry.read_to_end(&mut gzipped).map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            let raw = gunzip_bytes(&gzipped, &sha).map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            blobs.insert(sha, raw);
        } else {
            return Err(Error::Plan(format!(
                "read bundle '{}': unexpected entry '{}'",
                path.display(),
                entry_path.display()
            )));
        }
    }
    let Some(stored) = manifest else {
        return Err(Error::Plan(format!(
            "read bundle '{}': missing manifest",
            path.display()
        )));
    };
    for document in &stored.documents {
        for sha in document.data.blob_refs() {
            if !blobs.contains_key(sha) {
                return Err(Error::Plan(format!(
                    "read bundle '{}': missing blob '{sha}'",
                    path.display()
                )));
            }
        }
    }
    Ok(Bundle {
        manifest: stored,
        blobs,
    })
}

/// Loads one plan input holding either a bundle or a manifest.
///
/// Bundle files carry the `.cb` extension and hydrate from
/// the archive alone. Every other path reads as a slot
/// manifest first, then retries as a bundle, so renamed
/// bundles still load.
///
/// # Arguments
///
/// * `path` - the resolved plan file under reading.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The live plan holding binary bytes.
///
/// # Errors
///
/// Unreadable files plus bad payloads fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Bundle;
/// use confit_core::store::bundle::{load_bundle_input, write_bundle};
///
/// let fs = MemoryFs::new();
/// let dest = std::path::Path::new("bundle.cb");
/// assert!(matches!(write_bundle(&Bundle::empty(), dest, &fs, None), Ok(())));
/// assert!(matches!(load_bundle_input(dest, &fs), Ok(plan) if plan.manifest.documents.is_empty()));
/// ```
pub fn load_bundle_input(path: &Path, fs: &dyn Filesystem) -> Result<Bundle> {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
    {
        return read_bundle(path, fs);
    }
    match load_state(Some(path), fs) {
        Ok(plan) => Ok(plan),
        Err(first) => match read_bundle(path, fs) {
            Ok(plan) => Ok(plan),
            Err(_) => Err(first),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Bundle;
    use crate::store::blobs::resolve_blobs_dir;
    use crate::store::blobs::tests::{mixed_plan, pool_blobs};
    use crate::store::slots::{load_state, write_manifest};

    #[test]
    fn bundle_roundtrip_restores_plan_with_referenced_blobs_only() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.tgz");
        match write_bundle(&built, dest, &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let restored = match read_bundle(dest, &fs) {
            Ok(restored) => restored,
            Err(error) => panic!("bundle reads: {error}"),
        };
        assert_eq!(restored, built);
        let bytes = match fs.read(dest) {
            Ok(bytes) => bytes,
            Err(error) => panic!("bundle reads: {error}"),
        };
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        let entries = match archive.entries() {
            Ok(entries) => entries,
            Err(error) => panic!("bundle lists: {error}"),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => panic!("entry reads: {error}"),
            };
            let path = match entry.path() {
                Ok(path) => path.into_owned(),
                Err(error) => panic!("entry names: {error}"),
            };
            names.push(path.to_string_lossy().into_owned());
        }
        names.sort();
        let mut want = vec!["manifest.json".to_string()];
        for raw in [vec![0xFF, 0x00, 0x41], vec![1, 2, 3], vec![4, 5, 6]] {
            want.push(format!("blobs/{}", crate::plan::sha256_hex(&raw)));
        }
        want.sort();
        assert_eq!(names, want);
    }

    #[test]
    fn pool_plus_bundle_roundtrip_multi_blob() {
        use crate::document::{ManifestData, ManifestDocument, ManifestMember};
        use crate::fs::MemoryFs;
        use crate::ids::DocPath;

        let compressible = vec![0x41; 2048];
        let mut noisy = Vec::with_capacity(1024);
        let mut state: u64 = 0x1234_5678_9abc_def1;
        for _ in 0..1024 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            noisy.push((state >> 33) as u8);
        }
        let empty: Vec<u8> = Vec::new();
        let compressible_blob = crate::plan::sha256_hex(&compressible);
        let noisy_blob = crate::plan::sha256_hex(&noisy);
        let empty_blob = crate::plan::sha256_hex(&empty);
        let documents = vec![
            ManifestDocument::new(
                DocPath::new("packed"),
                ManifestData::Opaque {
                    blob: compressible_blob.clone(),
                    mode: None,
                },
            ),
            ManifestDocument::new(
                DocPath::new("noisy"),
                ManifestData::Opaque {
                    blob: noisy_blob.clone(),
                    mode: None,
                },
            ),
            ManifestDocument::new(
                DocPath::new("fonts"),
                ManifestData::Tree {
                    members: vec![
                        ManifestMember {
                            relative: "empty.ttf".into(),
                            blob: empty_blob.clone(),
                            mode: 0o644,
                        },
                        ManifestMember {
                            relative: "copy.ttf".into(),
                            blob: compressible_blob.clone(),
                            mode: 0o644,
                        },
                    ],
                },
            ),
        ];
        let mut built = match Bundle::build(documents, Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        built
            .blobs
            .insert(compressible_blob.clone(), compressible.clone());
        built.blobs.insert(noisy_blob.clone(), noisy.clone());
        built.blobs.insert(empty_blob.clone(), empty.clone());
        let fs = MemoryFs::new();
        let dest = std::path::Path::new("plan.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
        }
        let pool = match resolve_blobs_dir() {
            Ok(pool) => pool,
            Err(error) => panic!("blobs resolve: {error}"),
        };
        for sha in built.blobs.keys() {
            assert!(fs.exists(&pool.join(sha)), "pool holds {sha}");
        }
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("plan loads: {error}"),
        };
        assert_eq!(loaded, built);
        let bundle = std::path::Path::new("bundle.tgz");
        match write_bundle(&built, bundle, &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let restored = match read_bundle(bundle, &fs) {
            Ok(restored) => restored,
            Err(error) => panic!("bundle reads: {error}"),
        };
        assert_eq!(restored.manifest, built.manifest);
        assert_eq!(restored.blobs, built.blobs);
        assert_eq!(restored, built);
    }

    #[test]
    fn bundle_skips_present_pool_blobs_and_stays_sorted() {
        use crate::fs::MemoryFs;
        use std::io::Read as _;

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("plan.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("first plan writes: {error}"),
        }
        let first = pool_blobs(&fs);
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("second plan writes: {error}"),
        }
        let second = pool_blobs(&fs);
        assert_eq!(first, second);
        assert_eq!(second.len(), built.blobs.len());
        let bundle = std::path::Path::new("bundle.tgz");
        match write_bundle(&built, bundle, &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let bytes = match fs.read(bundle) {
            Ok(bytes) => bytes,
            Err(error) => panic!("bundle reads: {error}"),
        };
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        let entries = match archive.entries() {
            Ok(entries) => entries,
            Err(error) => panic!("bundle lists: {error}"),
        };
        let mut names = Vec::new();
        for entry in entries {
            let mut entry = match entry {
                Ok(entry) => entry,
                Err(error) => panic!("entry reads: {error}"),
            };
            let path = match entry.path() {
                Ok(path) => path.into_owned(),
                Err(error) => panic!("entry names: {error}"),
            };
            let mut raw = Vec::new();
            match entry.read_to_end(&mut raw) {
                Ok(_) => {}
                Err(error) => panic!("entry drains: {error}"),
            }
            names.push(path.to_string_lossy().into_owned());
        }
        let first_name = match names.first() {
            Some(first_name) => first_name.clone(),
            None => panic!("bundle holds entries"),
        };
        assert_eq!(first_name, "manifest.json");
        let rest = names[1..].to_vec();
        let mut want: Vec<String> = built
            .blobs
            .keys()
            .map(|sha| format!("blobs/{sha}"))
            .collect();
        want.sort();
        assert_eq!(rest, want);
        let mut seen = std::collections::BTreeSet::new();
        for name in &rest {
            assert!(seen.insert(name.clone()), "duplicate entry {name}");
        }
    }
}
