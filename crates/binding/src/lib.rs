//! Binding
//! Declare Lua faces with registry and stub emission.

#![deny(missing_docs)]

pub mod callback;
pub mod encode;
pub mod error;
pub mod face;
pub mod namespace;
pub mod registry;
pub mod stub;
pub mod userdata;
pub mod value;

#[cfg(test)]
extern crate self as confit_binding;
