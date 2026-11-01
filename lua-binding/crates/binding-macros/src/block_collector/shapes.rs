//! Answers type queries used to classify signature parameters and returns.

use syn::Type;

/// Returns whether a type is the Lua context by reference.
pub(crate) fn is_lua_ref(ty: &Type) -> bool {
    if let Type::Reference(reference) = ty
        && reference.mutability.is_none()
        && let Type::Path(path) = &*reference.elem
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Lua";
    }
    false
}

/// Returns whether a type is the Lua context in any shape.
pub(crate) fn names_lua(ty: &Type) -> bool {
    match ty {
        Type::Reference(reference) => names_lua(&reference.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|last| last.ident == "Lua"),
        _ => false,
    }
}

/// Unwraps the success type of a `Result` return.
pub(crate) fn unwrap_result(ty: &Type) -> Type {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
        && last.ident == "Result"
        && let syn::PathArguments::AngleBracketed(args) = &last.arguments
        && let Some(syn::GenericArgument::Type(first)) = args.args.first()
    {
        return first.clone();
    }
    ty.clone()
}

/// Returns whether a type is a `Result`.
pub(crate) fn is_result(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Result";
    }
    false
}

/// Reports the return arity.
pub(crate) fn return_arity(returns: &Option<Type>) -> usize {
    match returns {
        None => 0,
        Some(Type::Tuple(tuple)) => tuple.elems.len(),
        Some(_) => 1,
    }
}

/// Returns whether a type is a tuple return.
pub(crate) fn is_tuple(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(_))
}

/// Returns whether a type is an `Option` with inner content.
pub(crate) fn is_option(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Option";
    }
    false
}

/// Builds the element indices.
pub(crate) fn tuple_indices(ty: &Type) -> Vec<syn::Index> {
    let Type::Tuple(tuple) = ty else {
        return Vec::new();
    };
    (0..tuple.elems.len()).map(syn::Index::from).collect()
}
