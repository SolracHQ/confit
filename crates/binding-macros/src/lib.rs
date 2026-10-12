//! Binding macros
//! Face derive with impl-block method collection.

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

/// Derive the Face trait for one table or userdata declaration.
///
/// - Every derived type carries one kind marker, table or userdata or
///   callback shape.
/// - Enums carry one union annotation, string or untagged or tagged
///   or probe.
/// - Field docs plus variant docs read `///` lines.
/// - Optional fields hold Option values with inner content.
/// - Defaulted fields fill absent keys with defaults and face `?`.
/// - Renamed fields read their Lua key.
/// - Opaque fields pass Lua functions plus handles through.
/// - Userdata faces pair with face_impl for method stubs.
/// - Callback shapes read documented params with live tables borrowed.
/// - Proxy fields face as the proxy type and decode through the convert
///   function with Lua access. Lone convert functions read the table
///   with sibling access and hold no Lua key.
/// - Proxy fields with convert and encode functions round-trip through
///   strings in both directions.
/// - Map fields read key_convert with value_convert entry functions
///   over HashMap and BTreeMap shapes and face `{[K]: V}`.
/// - Untagged enums route tables through the synthesized family router.
/// - The probe hook overrides the family router on untagged enums.
/// - Decoded values pass the validate hook.
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
/// - Runtime methods decode arguments through Decode and encode returns through Encode.
#[proc_macro_attribute]
pub fn face_impl(attribute: TokenStream, item: TokenStream) -> TokenStream {
    block_collector::expand(attribute, item)
}
