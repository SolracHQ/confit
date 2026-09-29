//! Handles
//!
//! Unforgeable identity for trusted sources and pooled bytes.

use std::path::{Path, PathBuf};

use confit_model::document::BlobRef;
use confit_model::error::{Error, Result};
use confit_model::sha::Sha;

/// Canonical identity for a trusted source.
///
/// Sources pass birth checks once; downstream code trusts the type
/// without rechecking.
///
pub trait TrustedHandle {
    /// Reads the canonical source path.
    ///
    fn canonical(&self) -> &Path;

    /// Reads the content hash.
    ///
    fn sha(&self) -> &Sha;
}

/// Fetched artifact identity.
///
/// Cache file path plus content hash plus origin url.
/// The path holds a non-empty shape; the fetcher proves bytes.
/// The hash holds 64 hex characters.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FetchHandle {
    cache_path: PathBuf,
    sha256: Sha,
    origin: String,
}

/// Exec-rooted project file identity.
///
/// Rooted path plus content hash.
/// The path holds containment under the exec root.
/// The hash holds 64 hex characters.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceHandle {
    path: PathBuf,
    sha256: Sha,
}

/// Content-addressed bytes identity.
///
/// The content hash names payload bytes; the stored hash names
/// pool bytes. Handles seal both at birth, so pool reads never
/// re-derive identity from bytes.
/// Both hashes hold 64 hex characters.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlobHandle {
    sha256: Sha,
    stored: Sha,
}

/// Verified compressed archive identity.
///
/// Trusted source path plus source hash plus compression proof.
/// The proof exists only for sources verified as compressed archives.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArchiveHandle {
    source_path: PathBuf,
    source_sha256: Sha,
    proof: ArchiveProof,
}

/// Compression proof for an archive handle.
///
/// Only sources verified as compressed archives carry this marker.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArchiveProof {
    /// Verified compressed source.
    Compressed,
}

impl TrustedHandle for FetchHandle {
    fn canonical(&self) -> &Path {
        &self.cache_path
    }

    fn sha(&self) -> &Sha {
        &self.sha256
    }
}

impl FetchHandle {
    /// Builds a fetch handle from cache identity.
    ///
    /// # Errors
    ///
    /// Empty cache paths fail as plan errors.
    ///
    pub(crate) fn new(
        cache_path: impl Into<PathBuf>,
        sha256: Sha,
        origin: impl Into<String>,
    ) -> Result<Self> {
        let cache_path = cache_path.into();
        check_present(&cache_path, "fetch cache path")?;
        Ok(Self {
            cache_path,
            sha256,
            origin: origin.into(),
        })
    }

    /// Reads the origin url.
    ///
    pub fn origin(&self) -> &str {
        &self.origin
    }
}

impl TrustedHandle for ResourceHandle {
    fn canonical(&self) -> &Path {
        &self.path
    }

    fn sha(&self) -> &Sha {
        &self.sha256
    }
}

impl ResourceHandle {
    /// Builds a resource handle from exec-rooted identity.
    ///
    /// # Errors
    ///
    /// Paths outside the exec root fail as plan errors.
    ///
    pub(crate) fn new(exec_root: &Path, path: impl Into<PathBuf>, sha256: Sha) -> Result<Self> {
        let path = path.into();
        if !path.starts_with(exec_root) {
            return Err(Error::Plan(format!(
                "resource path '{}' escapes exec root '{}'",
                path.display(),
                exec_root.display()
            )));
        }
        Ok(Self { path, sha256 })
    }
}

impl BlobHandle {
    /// Builds a blob handle from sealed content plus stored hashes.
    ///
    pub(crate) fn new(sha256: Sha, stored: Sha) -> Result<Self> {
        Ok(Self { sha256, stored })
    }

    /// Reads the content hash.
    ///
    pub fn sha(&self) -> &Sha {
        &self.sha256
    }

    /// Reads the pool-bytes hash.
    ///
    pub fn stored(&self) -> &Sha {
        &self.stored
    }

    /// Reads the manifest ref carrying both hashes.
    ///
    pub fn to_ref(&self) -> BlobRef {
        BlobRef::new(self.sha256.clone(), self.stored.clone())
    }
}

impl TrustedHandle for ArchiveHandle {
    fn canonical(&self) -> &Path {
        &self.source_path
    }

    fn sha(&self) -> &Sha {
        &self.source_sha256
    }
}

impl ArchiveHandle {
    /// Builds an archive handle from verified source identity.
    ///
    /// Callers copy the path and hash from a trusted source handle
    /// after archive verification. The proof seals at birth.
    ///
    /// # Errors
    ///
    /// Empty source paths fail as plan errors.
    ///
    pub(crate) fn new(source_path: impl Into<PathBuf>, source_sha256: Sha) -> Result<Self> {
        let source_path = source_path.into();
        check_present(&source_path, "archive source path")?;
        Ok(Self {
            source_path,
            source_sha256,
            proof: ArchiveProof::Compressed,
        })
    }

    /// Reads the compression proof.
    ///
    pub fn proof(&self) -> ArchiveProof {
        self.proof
    }
}

/// Rejects empty paths.
fn check_present(path: &Path, label: &str) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(Error::Plan(format!("{label} is empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_sha() -> Sha {
        Sha::hash(b"confit handle fixture")
    }

    #[test]
    fn fetch_handle_builds_from_valid_identity() {
        let handle = FetchHandle::new(
            "/cache/starship.toml",
            fixture_sha(),
            "https://example.com/starship.toml",
        )
        .unwrap();
        assert_eq!(handle.canonical(), Path::new("/cache/starship.toml"));
        assert_eq!(handle.sha(), &fixture_sha());
        assert_eq!(handle.origin(), "https://example.com/starship.toml");
    }

    #[test]
    fn fetch_handle_rejects_malformed_sha() {
        for sha in ["abc", &"zz".repeat(32)] {
            let error = match Sha::new(sha) {
                Ok(_) => panic!("malformed sha passes"),
                Err(error) => error,
            };
            assert!(matches!(error, Error::Parse { .. }));
        }
    }

    #[test]
    fn fetch_handle_rejects_empty_path() {
        let error = match FetchHandle::new("", fixture_sha(), "https://example.com/a.toml") {
            Ok(_) => panic!("empty cache path passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
    }

    #[test]
    fn resource_handle_builds_under_exec_root() {
        let handle =
            ResourceHandle::new(Path::new("/proj"), "/proj/starship.toml", fixture_sha()).unwrap();
        assert_eq!(handle.canonical(), Path::new("/proj/starship.toml"));
        assert_eq!(handle.sha(), &fixture_sha());
    }

    #[test]
    fn resource_handle_rejects_root_escape() {
        let error = match ResourceHandle::new(Path::new("/proj"), "/etc/passwd", fixture_sha()) {
            Ok(_) => panic!("escape passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
    }

    #[test]
    fn resource_handle_rejects_malformed_sha() {
        let error = match Sha::new("abc") {
            Ok(_) => panic!("malformed sha passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Parse { .. }));
    }

    #[test]
    fn blob_handle_builds_from_valid_sha() {
        let handle = BlobHandle::new(fixture_sha(), fixture_sha()).unwrap();
        assert_eq!(handle.sha(), &fixture_sha());
    }

    #[test]
    fn blob_handle_rejects_malformed_sha() {
        for sha in ["abc", &"zz".repeat(32)] {
            let error = match Sha::new(sha) {
                Ok(_) => panic!("malformed sha passes"),
                Err(error) => error,
            };
            assert!(matches!(error, Error::Parse { .. }));
        }
    }

    #[test]
    fn blob_handle_converts_to_manifest_ref() {
        let handle = BlobHandle::new(fixture_sha(), fixture_sha()).unwrap();
        let blob = handle.to_ref();
        assert_eq!(blob.sha(), &fixture_sha());
        assert_eq!(blob.stored(), &fixture_sha());
    }

    #[test]
    fn archive_handle_builds_with_sealed_proof() {
        let handle = ArchiveHandle::new("/cache/fonts.zip", fixture_sha()).unwrap();
        assert_eq!(handle.canonical(), Path::new("/cache/fonts.zip"));
        assert_eq!(handle.sha(), &fixture_sha());
        assert_eq!(handle.proof(), ArchiveProof::Compressed);
    }

    #[test]
    fn archive_handle_rejects_malformed_sha() {
        let error = match Sha::new("abc") {
            Ok(_) => panic!("malformed sha passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Parse { .. }));
    }
}
