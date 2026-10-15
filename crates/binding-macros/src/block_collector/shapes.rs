//! Type shapes
//! Lua, result, plus tuple queries over signature types.

use syn::Type;

/// Report whether one type reads as the Lua context by reference.
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

/// Report whether one type contains the Lua context in any shape.
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

/// Unwrap the success type of one Result return.
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

/// Report whether one type reads as a result.
pub(crate) fn is_result(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Result";
    }
    false
}

/// Report the return arity of one unwrapped return shape.
pub(crate) fn return_arity(returns: &Option<Type>) -> usize {
    match returns {
        None => 0,
        Some(Type::Tuple(tuple)) => tuple.elems.len(),
        Some(_) => 1,
    }
}

/// Report whether one type reads as a tuple return.
pub(crate) fn is_tuple(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(_))
}

/// Build the element indices of one tuple return.
pub(crate) fn tuple_indices(ty: &Type) -> Vec<syn::Index> {
    let Type::Tuple(tuple) = ty else {
        return Vec::new();
    };
    (0..tuple.elems.len()).map(syn::Index::from).collect()
}
