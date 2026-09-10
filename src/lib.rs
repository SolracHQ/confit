//! Defines the pure model foundation.
//!
//! Provides data-first plan artifacts: types (`model`), order-resolved
//! merging (`merge`), plus canonical bytes and hashes (`canonical`).
//! Supplies pure data and hashing. Filesystem and process effects go
//! through `store` traits and the `plan` service.

//! Test builds allow `expect`: unwraps and expects assert success, and a
//! failure panics the test, which is the desired outcome.
#![cfg_attr(test, allow(clippy::expect_used))]

pub mod canonical;
pub mod cli;
pub mod diff;
pub mod error;
pub mod lua;
pub mod merge;
pub mod model;
pub mod plan;
pub mod store;
