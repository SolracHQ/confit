//! Side-effect seam behind repo traits.
//!
//! The service layer plans against [`StateStore`] and publishes through
//! [`PlanWriter`]; `memory` holds test fakes, `fs` the real backends,
//! `traits` the shared types. Plans serialize here; plan works on data
//! and `apply` renders bytes (minijinja, shell codegen).

pub mod fs;
pub mod memory;
pub mod traits;

pub use fs::{FsPlanWriter, FsStateStore};
pub use memory::{MemoryState, MemoryWriter};
pub use traits::{PlanFormat, PlanWriter, State, StateEntry, StateStore};
