//! Bind Rust types to Lua through [`mlua`].
//!
//! The `Shape` derive is the canonical way to bind a type. The `lua_methods`,
//! `lua_module`, and `lua_globals` attributes expose `impl` blocks to Lua.
//! They build decoding, encoding, and LuaLS stubs from the Rust declaration.
//!
//! ```rust,no_run
//! use lua_binding::{Shape, lua_module};
//!
//! #[derive(Shape)]
//! #[shape(table)]
//! struct Point {
//!     x: i64,
//!     y: i64,
//! }
//!
//! #[lua_module("geometry")]
//! impl Point {
//!     /// Returns the origin.
//!     ///
//!     /// # Returns
//!     ///
//!     /// - The origin point.
//!     pub fn origin() -> Point {
//!         Point { x: 0, y: 0 }
//!     }
//! }
//!
//! let stub = Point::stub();
//! assert!(stub.text.contains("--- @class Point"));
//! ```

#![deny(missing_docs)]

pub use crate::shape::Shape;
pub use lua_binding_macros::{Shape, lua_globals, lua_methods, lua_module};

pub mod callable;
pub mod encode;
pub mod error;
pub mod generator;
pub mod module;
pub mod object;
pub mod shape;
pub mod stub;
pub mod trace;
pub mod value;

#[cfg(test)]
extern crate self as lua_binding;
