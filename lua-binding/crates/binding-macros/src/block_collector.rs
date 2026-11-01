//! Collects methods and static functions from Lua impl blocks.

mod attribute;
mod blocks;
mod collect;
mod collected;
mod docs;
mod emit;
mod shapes;
#[cfg(test)]
mod tests;

use proc_macro::TokenStream;
use syn::{ItemImpl, parse_macro_input};

use attribute::{ImplTarget, parse_empty, parse_name};
use blocks::expand_block;

/// Expands an object method block.
pub fn expand_methods(attribute: TokenStream, item: TokenStream) -> TokenStream {
    match parse_empty(attribute, "lua_methods") {
        Ok(()) => expand(ImplTarget::Object, item),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Expands a module block.
pub fn expand_module(attribute: TokenStream, item: TokenStream) -> TokenStream {
    match parse_name(attribute, "lua_module") {
        Ok(name) => expand(ImplTarget::Module(name), item),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Expands a globals block.
pub fn expand_globals(attribute: TokenStream, item: TokenStream) -> TokenStream {
    match parse_name(attribute, "lua_globals") {
        Ok(name) => expand(ImplTarget::Global(name), item),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Expands one parsed block.
fn expand(mode: ImplTarget, item: TokenStream) -> TokenStream {
    let block = parse_macro_input!(item as ItemImpl);
    let expanded = expand_block(&block, mode);
    match expanded {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
