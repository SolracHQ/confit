//! Lua faces from Rust declarations.
//!
//! The derive emits Decode plus Encode plus stubs from one marker.

use proc_macro::TokenStream;

mod block_collector;
mod callback_shape;
mod face_derive;
mod literal;
mod parse;
mod probe;
mod structs;
mod tagged;
mod untagged;

/// Rust struct as Lua table.
///
/// Absent defaulted keys fill in. Option fields stay absent without
/// the marker. Docs feed stubs.
///
/// ```rust,no_run
/// use confit_binding_macros::Face;
///
/// #[derive(Face)]
/// #[face(table)]
/// struct Hook {
///     argv: Vec<String>,
///     #[face(default)]
///     timeout_secs: u64,
/// }
/// ```
///
/// String enums face as bare strings.
///
/// ```rust,no_run
/// use confit_binding_macros::Face;
///
/// #[derive(Face)]
/// #[face(table, string)]
/// enum Kind {
///     Requires,
///     When,
///     Check,
/// }
/// ```
///
/// Untagged enums route tables by shape.
///
/// ```rust,no_run
/// use confit_binding_macros::Face;
///
/// #[derive(Face)]
/// #[face(table, untagged)]
/// enum Arg {
///     Text(String),
///     Count(u64),
/// }
/// ```
///
/// Proxy fields route through hook functions. The decode hook reads
/// the Lua value. The encode hook returns the proxy value and the
/// derive encodes it.
///
/// ```rust,no_run
/// use confit_binding::value::Mismatch;
/// use confit_binding_macros::Face;
/// use mlua::{Lua, Value};
///
/// fn decode_timeout(lua: &Lua, value: &Value) -> Result<u64, Mismatch> {
///     Ok(60)
/// }
///
/// fn render_timeout(_lua: &Lua, timeout: &u64) -> mlua::Result<String> {
///     Ok(timeout.to_string())
/// }
///
/// #[derive(Face)]
/// #[face(table)]
/// struct Job {
///     #[face(proxy = String, decode_hook = decode_timeout, encode_hook = render_timeout)]
///     timeout_secs: u64,
/// }
/// ```
#[proc_macro_derive(Face, attributes(face))]
pub fn derive_face(input: TokenStream) -> TokenStream {
    face_derive::expand(input)
}

/// Userdata methods plus static namespaces from one impl block.
///
/// Blocks without a namespace hold &self methods alone. Blocks with
/// a namespace hold statics alone and answer NAMESPACE plus VERBS
/// plus register plus stubs on the type itself. Statics accept
/// #[face(optional)] with missing filling Default.
///
/// ```rust,no_run
/// use confit_binding_macros::{Face, face_impl};
///
/// #[derive(Clone, Face)]
/// #[face(userdata)]
/// struct Roster {
///     name: String,
/// }
///
/// #[face_impl]
/// impl Roster {
///     /// Greet with the prefix.
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
///
/// ```rust,no_run
/// use confit_binding_macros::face_impl;
///
/// pub struct Fact;
///
/// #[face_impl(namespace = "confit.fact")]
/// impl Fact {
///     /// Read one host fact.
///     ///
///     /// # Arguments
///     ///
///     /// - [name] The fact name under reading.
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
pub fn face_impl(attribute: TokenStream, item: TokenStream) -> TokenStream {
    block_collector::expand(attribute, item)
}
