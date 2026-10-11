//! Handles
//! Unforgeable identity for trusted sources and pooled bytes.

use std::path::{Path, PathBuf};

use confit_driver::sha::Sha;
use serde::{Deserialize, Serialize};

use crate::archive::error::ArchiveError;
use crate::resources::error::ResourceError;

/// Handle to a project resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceHandle {
    path: PathBuf,
    sha256: Sha,
}

/// Handle to pooled content bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlobHandle {
    sha256: Sha,
    stored: Sha,
}

/// Content-addressed bytes ref for manifests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobRef {
    /// Content hash for payload bytes.
    pub sha256: Sha,
    /// Pool-bytes hash for gzip bytes.
    pub stored: Sha,
}

/// Handle to a verified compressed archive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArchiveHandle {
    source_path: PathBuf,
    source_sha256: Sha,
    proof: ArchiveProof,
}

/// Compression proof for an archive handle.
///
/// Only sources verified as compressed archives carry this marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArchiveProof {
    /// Verified compressed source.
    Compressed,
}

impl ResourceHandle {
    /// Return the canonical source path.
    pub fn canonical(&self) -> &Path {
        &self.path
    }

    /// Return the content hash.
    pub fn sha(&self) -> &Sha {
        &self.sha256
    }

    /// Build a resource handle from exec-rooted identity.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Escape`] for paths outside the exec root.
    pub(crate) fn new(
        exec_root: &Path,
        path: impl Into<PathBuf>,
        sha256: Sha,
    ) -> crate::resources::error::Result<Self> {
        let path = path.into();
        if !path.starts_with(exec_root) {
            return Err(ResourceError::Escape { path });
        }
        Ok(Self { path, sha256 })
    }
}

impl BlobHandle {
    /// Build a blob handle from verified content and stored hashes.
    pub(crate) fn new(sha256: Sha, stored: Sha) -> Self {
        Self { sha256, stored }
    }

    /// Return the content hash.
    pub fn sha(&self) -> &Sha {
        &self.sha256
    }

    /// Return the pool-bytes hash.
    pub fn stored(&self) -> &Sha {
        &self.stored
    }

    /// Build the manifest ref with both hashes.
    pub fn to_ref(&self) -> BlobRef {
        BlobRef::new(self.sha256.clone(), self.stored.clone())
    }
}

impl BlobRef {
    /// Build one blob ref from verified content and stored hashes.
    pub fn new(sha256: Sha, stored: Sha) -> Self {
        Self { sha256, stored }
    }
}

impl ArchiveHandle {
    /// Return the canonical source path.
    pub fn canonical(&self) -> &Path {
        &self.source_path
    }

    /// Return the content hash.
    pub fn sha(&self) -> &Sha {
        &self.source_sha256
    }

    /// Build an archive handle from verified source identity.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::NotArchive`] for empty source paths.
    pub(crate) fn new(
        source_path: impl Into<PathBuf>,
        source_sha256: Sha,
    ) -> crate::archive::error::Result<Self> {
        let source_path = source_path.into();
        if source_path.as_os_str().is_empty() {
            return Err(ArchiveError::NotArchive { path: source_path });
        }
        Ok(Self {
            source_path,
            source_sha256,
            proof: ArchiveProof::Compressed,
        })
    }

    /// Return the compression proof.
    pub fn proof(&self) -> ArchiveProof {
        self.proof
    }
}

/// Handle to a stored state file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateHandle {
    path: PathBuf,
}

impl StateHandle {
    /// Return the state file path.
    pub fn canonical(&self) -> &Path {
        &self.path
    }

    /// Build a state handle from its file path.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::sha::ShaError;

    fn fixture_sha() -> Sha {
        Sha::hash(b"confit handle fixture")
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
        assert!(matches!(
            error,
            crate::resources::error::ResourceError::Escape { .. }
        ));
    }

    #[test]
    fn resource_handle_rejects_malformed_sha() {
        let error = match Sha::new("abc") {
            Ok(_) => panic!("malformed sha passes"),
            Err(error) => error,
        };
        assert!(matches!(error, ShaError::Parse { .. }));
    }

    #[test]
    fn blob_handle_builds_from_valid_sha() {
        let handle = BlobHandle::new(fixture_sha(), fixture_sha());
        assert_eq!(handle.sha(), &fixture_sha());
    }

    #[test]
    fn blob_handle_rejects_malformed_sha() {
        for sha in ["abc", &"zz".repeat(32)] {
            let error = match Sha::new(sha) {
                Ok(_) => panic!("malformed sha passes"),
                Err(error) => error,
            };
            assert!(matches!(error, ShaError::Parse { .. }));
        }
    }

    #[test]
    fn blob_handle_converts_to_manifest_ref() {
        let handle = BlobHandle::new(fixture_sha(), fixture_sha());
        let blob = handle.to_ref();
        assert_eq!(blob.sha256, fixture_sha());
        assert_eq!(blob.stored, fixture_sha());
    }

    #[test]
    fn archive_handle_builds_with_verified_proof() {
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
        assert!(matches!(error, ShaError::Parse { .. }));
    }
}
