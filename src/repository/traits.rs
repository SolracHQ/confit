//! Traits
//!
//! Filesystem seam. Actions program against this trait
//! while the binary binds the live backend and tests bind the memory fake.

use std::path::Path;

use crate::error::Result;

/// Filesystem access for plan inputs and outputs.
///
/// Reads resolve absent paths to `None`; writes publish bytes, text, or links at paths.
pub trait Filesystem {
    /// Reads raw bytes at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under read.
    ///
    /// # Returns
    ///
    /// File bytes, holding `None` for an absent path.
    ///
    /// # Errors
    ///
    /// Storage failure reading present bytes.
    fn read_bytes(&self, path: &str) -> Result<Option<Vec<u8>>>;
    /// Reads text at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under read.
    ///
    /// # Returns
    ///
    /// File text, holding `None` for an absent path.
    ///
    /// # Errors
    ///
    /// Storage failure reading present text.
    fn read_string(&self, path: &str) -> Result<Option<String>>;
    /// Publishes bytes at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under write.
    /// * `bytes` - the payload for the path.
    ///
    /// # Errors
    ///
    /// Storage failure publishing bytes.
    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<()>;
    /// Publishes text at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under write.
    /// * `text` - the text for the path.
    ///
    /// # Errors
    ///
    /// Storage failure publishing text.
    fn write_string(&self, path: &Path, text: &str) -> Result<()>;
    /// Publishes bytes atomically at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under write.
    /// * `bytes` - the payload for the path.
    ///
    /// # Errors
    ///
    /// Storage failure publishing bytes through temp file plus rename.
    fn write_bytes_tmp(&self, path: &Path, bytes: &[u8]) -> Result<()>;
    /// Reads a link target at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under read.
    ///
    /// # Returns
    ///
    /// Link target bytes, holding `None` for an absent path.
    ///
    /// # Errors
    ///
    /// Storage failure reading a present link.
    fn read_link(&self, path: &str) -> Result<Option<Vec<u8>>>;
    /// Publishes a link at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under write.
    /// * `target` - the link target for the path.
    ///
    /// # Errors
    ///
    /// Storage failure publishing the link.
    fn write_link(&self, path: &Path, target: &str) -> Result<()>;
}
