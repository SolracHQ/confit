//! ConfIt
//!
//! The library half of the binary. Everything reusable lives here so
//! main stays a thin shell and tests link the same code the binary runs.
//! Actions compose flows, services transform and run effects behind seams,
//! presentation renders.

//! Test builds allow `expect`: unwraps and expects assert success, and a
//! failure panics the test, which is the desired outcome.
#![cfg_attr(test, allow(clippy::expect_used))]

pub mod actions;
pub mod binding;
pub mod cli;
pub mod error;
pub mod framework;
pub mod model;
pub mod presentation;
pub mod repository;
pub mod security;
pub mod services;
