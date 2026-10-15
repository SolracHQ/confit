//! Block collector
//! Impl-block method collection for userdata classes.

mod attribute;
mod blocks;
mod collect;
mod docs;
mod emit;
mod faces;
mod shapes;
#[cfg(test)]
mod tests;

use proc_macro::TokenStream;
use syn::{ItemImpl, parse_macro_input};

use attribute::parse_namespace;
use blocks::expand_block;

/// Expand method collection for one impl block.
pub fn expand(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let namespace = match parse_namespace(attribute) {
        Ok(found) => found,
        Err(error) => return error.to_compile_error().into(),
    };
    let block = parse_macro_input!(item as ItemImpl);
    let expanded = expand_block(&block, namespace);
    match expanded {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
