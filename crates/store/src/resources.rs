//! Resources
//!
//! Trusted-source birth and text reads.

use std::path::Path;

use confit_core::error::Result;
use confit_core::handles::ResourceHandle;

pub mod file;
pub mod memory;

/// Trusted project files behind exec-rooted handles.
///
/// Birth checks containment under the exec root once;
/// downstream code trusts the handle type without rechecking.
pub trait Resources {
    /// Births a resource handle for one project file.
    ///
    /// # Errors
    ///
    /// Escaping, missing, and unreadable files fail as plan or io errors.
    fn resource(&self, exec_root: &Path, path: &Path) -> Result<ResourceHandle>;

    /// Reads one trusted file as text.
    ///
    /// # Errors
    ///
    /// Missing files and invalid text fail as plan errors.
    fn read_text(&self, handle: &ResourceHandle) -> Result<String>;
}
