//! Collected faces
//! Method and static shapes with stub faces.

use syn::{Pat, Type};

/// One collected method parameter.
pub(crate) struct ParamFace {
    /// The parameter label.
    pub(crate) label: String,
    /// The parameter binding.
    pub(crate) binding: Pat,
    /// The parameter shape.
    pub(crate) shape: Type,
    /// The parameter documentation.
    pub(crate) documentation: String,
}

/// One collected method.
pub(crate) struct MethodFace {
    /// The method ident.
    pub(crate) ident: syn::Ident,
    /// The method name.
    pub(crate) name: String,
    /// The method documentation.
    pub(crate) docs: String,
    /// The collected parameters.
    pub(crate) params: Vec<ParamFace>,
    /// The unwrapped return shape.
    pub(crate) returns: Option<Type>,
    /// The return documentations in order.
    pub(crate) return_docs: Vec<String>,
    /// The Lua context marker.
    pub(crate) takes_lua: bool,
    /// The fallible return marker.
    pub(crate) fallible: bool,
}

/// One collected static parameter.
pub(crate) struct StaticParam {
    /// The parameter label.
    pub(crate) label: String,
    /// The parameter binding.
    pub(crate) binding: Pat,
    /// The parameter shape.
    pub(crate) shape: Type,
    /// The parameter documentation.
    pub(crate) documentation: String,
    /// The optional marker.
    pub(crate) optional: bool,
}

/// One collected static.
pub(crate) struct StaticFace {
    /// The method ident.
    pub(crate) ident: syn::Ident,
    /// The method name.
    pub(crate) name: String,
    /// The method documentation.
    pub(crate) docs: String,
    /// The collected parameters.
    pub(crate) params: Vec<StaticParam>,
    /// The unwrapped return shape.
    pub(crate) returns: Option<Type>,
    /// The return documentations in order.
    pub(crate) return_docs: Vec<String>,
    /// The Lua context marker.
    pub(crate) takes_lua: bool,
    /// The fallible return marker.
    pub(crate) fallible: bool,
}
