//! Binding macros
//! Face derive with impl-block method collection.

use proc_macro::TokenStream;

mod block_collector;
mod face_derive;
mod literal;
mod parse;
mod probe;
mod structs;
mod tagged;
mod untagged;

/// Derive the Face trait for one table or userdata declaration.
///
/// - Every derived type carries one kind marker, table or userdata.
/// - Enums carry one union annotation, string or untagged or tagged
///   or probe.
/// - Field docs plus variant docs read `///` lines.
/// - Optional fields hold Option values with inner content.
/// - Opaque fields pass Lua functions plus handles through.
/// - Proxy fields face as the proxy type and decode through the convert
///   function with Lua access.
/// - Userdata faces carry Clone handles.
#[proc_macro_derive(Face, attributes(face))]
pub fn derive_face(input: TokenStream) -> TokenStream {
    face_derive::expand(input)
}

/// Collect userdata methods from one impl block.
///
/// - Every method in the block registers or the build fails.
/// - Method names read idents and docs read `///` lines.
/// - Method faces read declared bounds.
/// - The type answers the Face trait beside the block.
/// - The macro owns the UserData glue and keeps the block.
#[proc_macro_attribute]
pub fn face_impl(attribute: TokenStream, item: TokenStream) -> TokenStream {
    block_collector::expand(attribute, item)
}
