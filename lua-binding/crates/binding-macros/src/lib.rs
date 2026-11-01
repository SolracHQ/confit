//! Lua shapes from Rust declarations.
//!
//! The [`Shape`] derive is the canonical way to bind a type. It builds `Shape`,
//! `Decode`, and `Encode` for a struct or enum. The [`lua_methods`],
//! [`lua_module`], and [`lua_globals`] attributes expose impl blocks to Lua.
//!
//! The `#[shape(...)]` marker selects the Lua form:
//!
//! - `table` maps a struct to a Lua table with one key per field.
//! - `object` maps a struct or enum to Lua userdata with methods from
//!   [`lua_methods`].
//! - `function` maps a signature struct to a Lua function.
//! - `str_alias` maps an enum to Lua strings.
//! - `typed_alias` maps an enum to single-key tables.
//! - `untagged` routes a table by its field set.
//! - `probe` routes an enum by a marker key.
//!
//! Alias markers ride the `table` strategy: `#[shape(table, str_alias)]`.
//!
//! Field markers tune a key: `default`, `optional`, `name`, `flatten`,
//! `ignore`, `proxy`, `borrowed`, `opaque`, `lua_trace`, `key_decode`, and
//! `value_decode`.
//!
//! Doc comments on method blocks become stub text. A method with parameters
//! needs an `# Arguments` section with one `- [name] description` bullet per
//! parameter. A method with returns needs an `# Returns` section with one
//! `- description` bullet per return value, in order.

use proc_macro::TokenStream;

mod block_collector;
mod enum_object;
mod parse;
mod probe;
mod shape_derive;
mod signature;
mod str_alias;
mod structs;
mod typed_alias;
mod untagged;

/// Derives `Shape`, `Decode`, and `Encode` for a struct or enum.
///
/// The `#[shape(...)]` attribute selects the Lua form. A `table` struct maps
/// each field to one Lua key. A `str_alias` enum maps each variant to one
/// string. A `typed_alias` enum maps each variant to a single-key table. A
/// `probe` enum routes by a marker key. A `function` struct describes a Lua
/// function signature. An `untagged` enum routes a table by its field set. An
/// `object` enum holds its value behind methods from [`lua_methods`].
///
/// Field markers change a key. `default` fills an absent key from `Default`.
/// `optional` accepts an absent key. `name` sets the Lua key. `flatten` merges
/// a nested table into the outer table. `ignore` leaves a field to Rust. `proxy`
/// routes the value through decode and encode hooks. An `Option` field accepts
/// an absent key. The type carries that rule.
///
/// Object fields mark Lua access. `get` exposes the field through `__index`.
/// `get, set` also exposes it through `__newindex`. A field with neither marker
/// stays internal.
///
/// A table struct becomes a Lua table with one key per field.
///
/// ```rust,no_run
/// use lua_binding::Shape;
///
/// #[derive(Shape)]
/// #[shape(table)]
/// struct Hook {
///     argv: Vec<String>,
///     #[shape(default)]
///     timeout_secs: u64,
/// }
/// ```
///
/// A string enum becomes a Lua string.
///
/// ```rust,no_run
/// use lua_binding::Shape;
///
/// #[derive(Shape)]
/// #[shape(table, str_alias)]
/// enum Kind {
///     Requires,
///     When,
///     Check,
/// }
/// ```
///
/// An untagged enum routes a table by its field set.
///
/// ```rust,no_run
/// use lua_binding::Shape;
///
/// #[derive(Shape)]
/// #[shape(table, untagged)]
/// enum Arg {
///     Text(String),
///     Count(u64),
/// }
/// ```
///
/// An object enum holds its value behind methods.
///
/// ```rust,no_run
/// use lua_binding::{Shape, lua_methods};
///
/// #[derive(Clone, Shape)]
/// #[shape(object)]
/// enum Handle {
///     Cached(String),
///     Missing,
/// }
///
/// #[lua_methods]
/// impl Handle {
///     /// Returns the label.
///     ///
///     /// # Returns
///     ///
///     /// - The variant label.
///     fn label(&self) -> String {
///         match self {
///             Self::Cached(digest) => digest.clone(),
///             Self::Missing => "missing".to_owned(),
///         }
///     }
/// }
/// ```
///
/// Proxy fields route through hook functions. A decode hook takes the Lua
/// value and returns the Rust value. An encode hook turns the Rust value into
/// the proxy value, and the derive encodes that value.
///
/// ```rust,no_run
/// use lua_binding::error::DecodeError;
/// use lua_binding::Shape;
/// use mlua::{Lua, Value};
///
/// fn decode_timeout(lua: &Lua, value: &Value) -> Result<u64, DecodeError> {
///     Ok(60)
/// }
///
/// fn render_timeout(_lua: &Lua, timeout: &u64) -> mlua::Result<String> {
///     Ok(timeout.to_string())
/// }
///
/// #[derive(Shape)]
/// #[shape(table)]
/// struct Job {
///     #[shape(proxy = String, decode_hook = decode_timeout, encode_hook = render_timeout)]
///     timeout_secs: u64,
/// }
/// ```
#[proc_macro_derive(Shape, attributes(shape))]
pub fn derive_shape(input: TokenStream) -> TokenStream {
    shape_derive::expand(input)
}

/// Exposes `&self` and `&mut self` methods on an object shape.
///
/// Doc comments become stub text. A method with parameters needs an
/// `# Arguments` section with one `- [name] description` bullet per parameter.
/// A method with returns needs an `# Returns` section with one `- description`
/// bullet per return value, in order.
///
/// `#[operator(__add)]`, `__sub`, `__mul`, `__eq`, and `__len` mark arithmetic,
/// equality, and length metamethods. `#[operator(__tostring)]` marks the
/// metamethod that renders the object. Stub output covers the named methods and
/// the operator lines.
///
/// ```rust,no_run
/// use lua_binding::{Shape, lua_methods};
///
/// #[derive(Clone, Shape)]
/// #[shape(object)]
/// struct Roster {
///     name: String,
/// }
///
/// #[lua_methods]
/// impl Roster {
///     /// Greets with the prefix.
///     ///
///     /// # Arguments
///     ///
///     /// - [prefix] Greeting prefix.
///     ///
///     /// # Returns
///     ///
///     /// - Greeting text.
///     fn greeting(&self, prefix: String) -> String {
///         format!("{prefix} {}", self.name)
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn lua_methods(attribute: TokenStream, item: TokenStream) -> TokenStream {
    block_collector::expand_methods(attribute, item)
}

/// Exposes static functions as a Lua module.
///
/// The argument is the dotted Lua path. The type gains `MODULE`, `FUNCTIONS`,
/// `add`, `stubs`, and `stub`. A block with one static returns its function
/// value from `add`. A block with several statics returns a read-only table.
///
/// Doc comments and `#[shape(optional)]` follow [`lua_methods`].
///
/// ```rust,no_run
/// use lua_binding::lua_module;
///
/// pub struct Fact;
///
/// #[lua_module("demo.fact")]
/// impl Fact {
///     /// Returns a host fact.
///     ///
///     /// # Arguments
///     ///
///     /// - [name] The fact name.
///     ///
///     /// # Returns
///     ///
///     /// - The host fact string.
///     pub fn fact(name: String) -> String {
///         name
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn lua_module(attribute: TokenStream, item: TokenStream) -> TokenStream {
    block_collector::expand_module(attribute, item)
}

/// Installs static functions as Lua globals.
///
/// The argument is the stub name. The type gains `GLOBALS`, `install`, `stubs`,
/// and `stub`.
///
/// Doc comments and `#[shape(optional)]` follow [`lua_methods`].
///
/// ```rust,no_run
/// use lua_binding::lua_globals;
///
/// pub struct Global;
///
/// #[lua_globals("demo")]
/// impl Global {
///     /// Builds an identity.
///     ///
///     /// # Arguments
///     ///
///     /// - [name] The identity text.
///     ///
///     /// # Returns
///     ///
///     /// - The identity.
///     pub fn id(name: String) -> String {
///         name
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn lua_globals(attribute: TokenStream, item: TokenStream) -> TokenStream {
    block_collector::expand_globals(attribute, item)
}
