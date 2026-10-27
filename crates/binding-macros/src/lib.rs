//! Lua faces from Rust declarations.
//!
//! The derive emits Decode, Encode and stubs from one marker.

use proc_macro::TokenStream;

mod block_collector;
mod callback_shape;
mod enum_userdata;
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
/// Userdata enums hold their value behind methods.
///
/// ```rust,no_run
/// use confit_binding_macros::{Face, face_impl};
///
/// #[derive(Clone, Face)]
/// #[face(userdata)]
/// enum Handle {
///     Cached(String),
///     Missing,
/// }
///
/// #[face_impl]
/// impl Handle {
///     /// Return the label.
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
/// Proxy fields route through hook functions. Decode hooks take
/// the Lua value. Encode hooks emit the proxy value and the
/// derive encodes it.
///
/// ```rust,no_run
/// use confit_binding::fault::FaceFault;
/// use confit_binding_macros::Face;
/// use mlua::{Lua, Value};
///
/// fn decode_timeout(lua: &Lua, value: &Value) -> Result<u64, FaceFault> {
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

/// Userdata methods and static namespaces from one impl block.
///
/// Plain blocks hold &self methods alone. Namespace blocks hold
/// statics alone and return NAMESPACE, VERBS, register and stubs
/// on the type itself. Statics accept
/// #[face(optional)] with missing filling Default.
/// Metamethods ride one marker with the hook name. Flagged methods
/// leave stubs and answer the hook alone.
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
/// use confit_binding_macros::{Face, face_impl};
///
/// #[derive(Clone, Face)]
/// #[face(userdata)]
/// struct Boom {
///     message: String,
/// }
///
/// #[face_impl]
/// impl Boom {
///     /// Read the message.
///     ///
///     /// # Returns
///     ///
///     /// - The message text.
///     fn message(&self) -> String {
///         self.message.clone()
///     }
///
///     /// Render the error line.
///     ///
///     /// # Returns
///     ///
///     /// - The rendered error line.
///     #[face_meta(__tostring)]
///     fn render(&self) -> String {
///         format!("boom: {}", self.message)
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
