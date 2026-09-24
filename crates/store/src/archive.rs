//! Archive
//!
//! Member listing and extraction for compressed archives.

use confit_core::error::Result;
use confit_core::handles::{ArchiveHandle, ResourceHandle, TrustedHandle};

pub mod file;
pub mod memory;

/// Compressed archive member listing and extraction.
///
/// Member names read archive-relative with forward slashes.
pub trait ArchiveStore {
    /// Seals one trusted source as a verified archive.
    ///
    /// The proof seals at birth; downstream code trusts the type.
    ///
    /// # Errors
    ///
    /// Non-archives fail as plan errors naming the source.
    fn archive(&self, source: &dyn TrustedHandle) -> Result<ArchiveHandle>;

    /// Lists member names without reading content.
    ///
    /// # Errors
    ///
    /// Unreadable archives fail as plan errors.
    fn members(&self, archive: &ArchiveHandle) -> Result<Vec<String>>;

    /// Unpacks one archive once into the spill folder.
    ///
    /// Present folders skip, so repeated unpacks share bytes.
    /// Members return as born resource handles hashed at spill time.
    ///
    /// # Errors
    ///
    /// Unreadable archives and write failures fail as plan errors.
    fn extract(&self, archive: &ArchiveHandle) -> Result<Vec<ResourceHandle>>;

    /// Opens one member stream from the unpack spill.
    ///
    /// # Errors
    ///
    /// Unknown members fail as plan errors naming the member.
    fn open_decompressed(&self, member: &ResourceHandle) -> Result<Box<dyn std::io::Read>>;

    /// Picks one archive member by its archive-relative name.
    ///
    /// Names read archive-relative with forward slashes.
    ///
    /// # Errors
    ///
    /// Unknown members fail as plan errors naming the member.
    fn extract_member(&self, archive: &ArchiveHandle, name: &str) -> Result<ResourceHandle>;

    /// Reads unix permission bits for one member handle.
    ///
    /// # Errors
    ///
    /// Missing members fail as plan errors naming the member.
    fn mode(&self, member: &ResourceHandle) -> Result<u32>;
}
