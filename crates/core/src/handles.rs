//! Handles
//!
//! Unforgeable identity for trusted sources plus late-bound routes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Hex character count for a SHA-256 digest.
const SHA_HEX_LEN: usize = 64;

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
    fn sha(&self) -> &str;
}

/// Fetched artifact identity.
///
/// Cache file path plus content hash plus origin url.
/// The path holds a non-empty shape; the fetcher proves bytes.
/// The hash holds 64 hex characters.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FetchHandle {
    cache_path: PathBuf,
    sha256: String,
    origin: String,
}

/// Exec-rooted project file identity.
///
/// Rooted path plus content hash.
/// The path holds containment under the exec root.
/// The hash holds 64 hex characters.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceHandle {
    path: PathBuf,
    sha256: String,
}

/// Content-addressed bytes identity.
///
/// The content hash names payload bytes; the stored hash names
/// pool bytes. Handles seal both at birth, so pool reads never
/// re-derive identity from bytes.
/// Both hashes hold 64 hex characters.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct BlobHandle {
    sha256: String,
    stored: String,
}

/// Verified compressed archive identity.
///
/// Trusted source path plus source hash plus compression proof.
/// The proof exists only for sources verified as compressed archives.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArchiveHandle {
    source_path: PathBuf,
    source_sha256: String,
    proof: ArchiveProof,
}

/// Compression proof for an archive handle.
///
/// Only sources verified as compressed archives carry this marker.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArchiveProof {
    /// Verified compressed source.
    Compressed,
}

/// Destination base for a route.
///
/// Home, config, data, and cache resolve on the applying host.
/// Literal carries a host-specific path verbatim.
/// Nothing resolves here; resolution is apply-time business.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteBase {
    /// User home folder.
    Home,
    /// Platform config folder.
    Config,
    /// Platform data folder.
    Data,
    /// Platform cache folder.
    Cache,
    /// Host-specific literal path.
    Literal,
}

/// Late-bound destination route.
///
/// Base plus relative path.
/// The path holds a non-empty shape only.
/// Parents, permissions, and platform validity resolve at apply time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Route {
    base: RouteBase,
    relative: PathBuf,
}

impl TrustedHandle for FetchHandle {
    fn canonical(&self) -> &Path {
        &self.cache_path
    }

    fn sha(&self) -> &str {
        &self.sha256
    }
}

impl FetchHandle {
    /// Builds a fetch handle from cache identity.
    ///
    /// # Errors
    ///
    /// Empty cache paths and malformed hashes fail as plan errors.
    ///
    pub fn new(
        cache_path: impl Into<PathBuf>,
        sha256: impl Into<String>,
        origin: impl Into<String>,
    ) -> Result<Self> {
        let cache_path = cache_path.into();
        let sha256 = sha256.into();
        check_present(&cache_path, "fetch cache path")?;
        check_sha_hex(&sha256)?;
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

    fn sha(&self) -> &str {
        &self.sha256
    }
}

impl ResourceHandle {
    /// Builds a resource handle from exec-rooted identity.
    ///
    /// # Errors
    ///
    /// Paths outside the exec root and malformed hashes fail
    /// as plan errors.
    ///
    pub fn new(
        exec_root: &Path,
        path: impl Into<PathBuf>,
        sha256: impl Into<String>,
    ) -> Result<Self> {
        let path = path.into();
        let sha256 = sha256.into();
        if !path.starts_with(exec_root) {
            return Err(Error::Plan(format!(
                "resource path '{}' escapes exec root '{}'",
                path.display(),
                exec_root.display()
            )));
        }
        check_sha_hex(&sha256)?;
        Ok(Self { path, sha256 })
    }
}

impl BlobHandle {
    /// Builds a blob handle from content plus stored hashes.
    ///
    /// # Errors
    ///
    /// Malformed hashes fail as plan errors.
    ///
    pub fn new(sha256: impl Into<String>, stored: impl Into<String>) -> Result<Self> {
        let sha256 = sha256.into();
        let stored = stored.into();
        check_sha_hex(&sha256)?;
        check_sha_hex(&stored)?;
        Ok(Self { sha256, stored })
    }

    /// Reads the content hash.
    ///
    pub fn sha(&self) -> &str {
        &self.sha256
    }

    /// Reads the pool-bytes hash.
    ///
    pub fn stored(&self) -> &str {
        &self.stored
    }
}

impl<'de> Deserialize<'de> for BlobHandle {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawBlobHandle {
            sha256: String,
            stored: String,
        }

        let raw = RawBlobHandle::deserialize(deserializer)?;
        Self::new(raw.sha256, raw.stored).map_err(serde::de::Error::custom)
    }
}

impl TrustedHandle for ArchiveHandle {
    fn canonical(&self) -> &Path {
        &self.source_path
    }

    fn sha(&self) -> &str {
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
    /// Empty source paths and malformed hashes fail as plan errors.
    ///
    pub fn new(source_path: impl Into<PathBuf>, source_sha256: impl Into<String>) -> Result<Self> {
        let source_path = source_path.into();
        let source_sha256 = source_sha256.into();
        check_present(&source_path, "archive source path")?;
        check_sha_hex(&source_sha256)?;
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

impl RouteBase {
    /// Reads the lowercase base name.
    ///
    /// # Returns
    ///
    /// The base name for route display and log lines.
    ///
    pub fn name(&self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Config => "config",
            Self::Data => "data",
            Self::Cache => "cache",
            Self::Literal => "literal",
        }
    }
}

impl Route {
    /// Builds a destination route from a base and a path.
    ///
    /// # Errors
    ///
    /// Empty paths fail as plan errors. Validity beyond non-empty
    /// stays apply-time business.
    ///
    pub fn new(base: RouteBase, relative: impl Into<PathBuf>) -> Result<Self> {
        let relative = relative.into();
        check_present(&relative, "route path")?;
        Ok(Self { base, relative })
    }

    /// Joins one member segment onto the route.
    ///
    /// Empty segments keep the route unchanged, so joined
    /// member paths never fail.
    ///
    pub fn join(&self, segment: &str) -> Self {
        if segment.is_empty() {
            return self.clone();
        }
        Self {
            base: self.base,
            relative: self.relative.join(segment),
        }
    }

    /// Renders the portable route display.
    ///
    /// # Returns
    ///
    /// The `base:relative` text for plan keys and drift lines.
    ///
    pub fn display(&self) -> String {
        format!("{}:{}", self.base.name(), self.relative.display())
    }

    /// Parses one portable route display.
    ///
    /// # Arguments
    ///
    /// * `text` - the `base:relative` text under parsing.
    ///
    /// # Returns
    ///
    /// The route for known bases with a non-empty relative path.
    ///
    /// # Errors
    ///
    /// Unknown bases, missing separators, and empty paths
    /// fail as plan errors.
    ///
    pub fn parse(text: &str) -> Result<Self> {
        let Some((base_name, relative)) = text.split_once(':') else {
            return Err(Error::Plan(format!(
                "invalid route '{text}': want 'base:relative' like 'home:.bashrc'"
            )));
        };
        let base = match base_name {
            "home" => RouteBase::Home,
            "config" => RouteBase::Config,
            "data" => RouteBase::Data,
            "cache" => RouteBase::Cache,
            "literal" => RouteBase::Literal,
            _ => {
                return Err(Error::Plan(format!(
                    "invalid route '{text}': unknown base '{base_name}'"
                )));
            }
        };
        Self::new(base, relative).map_err(|_| {
            Error::Plan(format!(
                "invalid route '{text}': want 'base:relative' like 'home:.bashrc'"
            ))
        })
    }

    /// Reads the destination base.
    ///
    pub fn base(&self) -> RouteBase {
        self.base
    }

    /// Reads the relative path.
    ///
    pub fn relative(&self) -> &Path {
        &self.relative
    }
}

/// Rejects malformed content hashes.
fn check_sha_hex(sha: &str) -> Result<()> {
    if sha.len() == SHA_HEX_LEN && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }
    Err(Error::Plan(format!(
        "invalid sha256 '{sha}': want 64 hex characters"
    )))
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
    use crate::ids::sha256_hex;

    fn fixture_sha() -> String {
        sha256_hex(b"confit handle fixture")
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
        assert_eq!(handle.sha(), fixture_sha());
        assert_eq!(handle.origin(), "https://example.com/starship.toml");
    }

    #[test]
    fn fetch_handle_rejects_malformed_sha() {
        for sha in ["abc", &"zz".repeat(32)] {
            let error = match FetchHandle::new("/cache/a.toml", sha, "https://example.com/a.toml") {
                Ok(_) => panic!("malformed sha passes"),
                Err(error) => error,
            };
            assert!(matches!(error, Error::Plan(_)));
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
        assert_eq!(handle.sha(), fixture_sha());
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
        let error = match ResourceHandle::new(Path::new("/proj"), "/proj/a.toml", "abc") {
            Ok(_) => panic!("malformed sha passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
    }

    #[test]
    fn blob_handle_builds_from_valid_sha() {
        let handle = BlobHandle::new(fixture_sha(), fixture_sha()).unwrap();
        assert_eq!(handle.sha(), fixture_sha());
    }

    #[test]
    fn blob_handle_rejects_malformed_sha() {
        for sha in ["abc", &"zz".repeat(32)] {
            let error = match BlobHandle::new(sha, fixture_sha()) {
                Ok(_) => panic!("malformed sha passes"),
                Err(error) => error,
            };
            assert!(matches!(error, Error::Plan(_)));
        }
    }

    #[test]
    fn archive_handle_builds_with_sealed_proof() {
        let handle = ArchiveHandle::new("/cache/fonts.zip", fixture_sha()).unwrap();
        assert_eq!(handle.canonical(), Path::new("/cache/fonts.zip"));
        assert_eq!(handle.sha(), fixture_sha());
        assert_eq!(handle.proof(), ArchiveProof::Compressed);
    }

    #[test]
    fn archive_handle_rejects_malformed_sha() {
        let error = match ArchiveHandle::new("/cache/fonts.zip", "abc") {
            Ok(_) => panic!("malformed sha passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
    }

    #[test]
    fn route_builds_for_every_base() {
        for base in [
            RouteBase::Home,
            RouteBase::Config,
            RouteBase::Data,
            RouteBase::Cache,
            RouteBase::Literal,
        ] {
            let route = Route::new(base, "starship/starship.toml").unwrap();
            assert_eq!(route.base(), base);
            assert_eq!(route.relative(), Path::new("starship/starship.toml"));
        }
    }

    #[test]
    fn route_rejects_empty_path() {
        let error = match Route::new(RouteBase::Home, "") {
            Ok(_) => panic!("empty route passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
    }
}
