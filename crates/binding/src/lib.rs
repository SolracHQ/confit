//! Binding
//! Declares Lua faces with registry and stub emission.

#![deny(missing_docs)]

pub mod error;
pub mod face;
pub mod registry;
pub mod stub;
pub mod userdata;
pub mod value;
pub mod verb;

#[cfg(test)]
extern crate self as confit_binding;
