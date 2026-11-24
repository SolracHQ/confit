//! Holds the collected method and function shapes used during expansion.

use syn::{Pat, Type};

/// The receiver kind of a collected method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceiverKind {
    /// `&self`
    Shared,
    /// `&mut self`
    Mutable,
}

/// A method parameter collected from an impl block.
pub(crate) struct ParameterShape {
    /// The parameter label.
    pub(crate) label: String,
    /// The parameter binding.
    pub(crate) binding: Pat,
    /// The parameter shape.
    pub(crate) shape: Type,
    /// The parameter description.
    pub(crate) description: String,
}

/// A method collected from an impl block.
pub(crate) struct MethodShape {
    /// The method ident.
    pub(crate) ident: syn::Ident,
    /// The method name.
    pub(crate) name: String,
    /// The method description.
    pub(crate) docs: String,
    /// The receiver kind.
    pub(crate) receiver: ReceiverKind,
    /// The collected parameters.
    pub(crate) params: Vec<ParameterShape>,
    /// The unwrapped return shape.
    pub(crate) returns: Option<Type>,
    /// The return descriptions in order.
    pub(crate) return_docs: Vec<String>,
    /// The Lua context marker.
    pub(crate) takes_lua: bool,
    /// The fallible return marker.
    pub(crate) fallible: bool,
}

/// A static function parameter collected from an impl block.
pub(crate) struct StaticParam {
    /// The parameter label.
    pub(crate) label: String,
    /// The parameter binding.
    pub(crate) binding: Pat,
    /// The parameter shape.
    pub(crate) shape: Type,
    /// The parameter description.
    pub(crate) description: String,
    /// The optional marker.
    pub(crate) optional: bool,
}

/// A static function collected from an impl block.
pub(crate) struct FunctionShape {
    /// The method ident.
    pub(crate) ident: syn::Ident,
    /// The method name.
    pub(crate) name: String,
    /// The method description.
    pub(crate) docs: String,
    /// The collected parameters.
    pub(crate) params: Vec<StaticParam>,
    /// The unwrapped return shape.
    pub(crate) returns: Option<Type>,
    /// The return descriptions in order.
    pub(crate) return_docs: Vec<String>,
    /// The Lua context marker.
    pub(crate) takes_lua: bool,
    /// The fallible return marker.
    pub(crate) fallible: bool,
}
