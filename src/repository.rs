//! Repository
//!
//! Single filesystem seam. The binary binds the live backend
//! here while tests bind the memory fake.

pub mod fs;
pub mod memory;
pub mod traits;

pub use fs::OsFilesystem;
pub use memory::MemoryFilesystem;
pub use traits::Filesystem;
