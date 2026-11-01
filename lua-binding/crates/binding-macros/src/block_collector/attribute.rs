//! Parses the Lua block target and the operator and optional markers.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::ext::IdentExt;

/// The surface an impl block exposes to Lua.
pub(crate) enum ImplTarget {
    /// Methods on an object.
    Object,
    /// Static functions under a dotted module path.
    Module(String),
    /// Static functions installed into chunk environments.
    Global(String),
}

impl ImplTarget {
    /// The attribute name for diagnostics.
    pub(crate) fn attribute(&self) -> &'static str {
        match self {
            Self::Object => "lua_methods",
            Self::Module(_) => "lua_module",
            Self::Global(_) => "lua_globals",
        }
    }
}

/// Rejects arguments on an attribute that takes none.
pub(crate) fn parse_empty(attribute: TokenStream, name: &str) -> Result<(), syn::Error> {
    if attribute.is_empty() {
        return Ok(());
    }
    let tokens = proc_macro2::TokenStream::from(attribute);
    Err(syn::Error::new_spanned(
        tokens,
        format!("`#[{name}]` takes no arguments"),
    ))
}

/// Parses one string literal argument.
pub(crate) fn parse_name(attribute: TokenStream, name: &str) -> Result<String, syn::Error> {
    let tokens = proc_macro2::TokenStream::from(attribute);
    let parsed: syn::LitStr = syn::parse2(tokens.clone()).map_err(|_| {
        syn::Error::new_spanned(tokens, format!("`#[{name}]` expects one string literal"))
    })?;
    Ok(parsed.value())
}

/// A metamethod operator on a shape.
pub(crate) enum Operator {
    /// The `__add` hook.
    Add,
    /// The `__sub` hook.
    Sub,
    /// The `__mul` hook.
    Mul,
    /// The `__eq` hook.
    Eq,
    /// The `__len` hook.
    Len,
    /// The `__tostring` hook.
    ToString,
}

impl Operator {
    /// Returns the `mlua` metamethod path.
    pub(crate) fn meta_method(&self) -> Tokens {
        match self {
            Self::Add => quote! { ::mlua::MetaMethod::Add },
            Self::Sub => quote! { ::mlua::MetaMethod::Sub },
            Self::Mul => quote! { ::mlua::MetaMethod::Mul },
            Self::Eq => quote! { ::mlua::MetaMethod::Eq },
            Self::Len => quote! { ::mlua::MetaMethod::Len },
            Self::ToString => quote! { ::mlua::MetaMethod::ToString },
        }
    }

    /// Returns the LuaLS operator name, absent for `__tostring`.
    pub(crate) fn lua_name(&self) -> Option<&'static str> {
        match self {
            Self::Add => Some("add"),
            Self::Sub => Some("sub"),
            Self::Mul => Some("mul"),
            Self::Eq => Some("eq"),
            Self::Len => Some("len"),
            Self::ToString => None,
        }
    }

    /// Returns the Lua operand count.
    pub(crate) fn operands(&self) -> usize {
        match self {
            Self::Add | Self::Sub | Self::Mul | Self::Eq => 1,
            Self::Len | Self::ToString => 0,
        }
    }
}

/// Parses the operator hook.
pub(crate) fn parse_operator(
    class_name: &str,
    method: &syn::ImplItemFn,
) -> Result<Option<Operator>, syn::Error> {
    let method_name = method.sig.ident.unraw().to_string();
    let mut hook = None;
    for attr in &method.attrs {
        if !attr.path().is_ident("operator") {
            continue;
        }
        if hook.is_some() {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                format!("{class_name}.{method_name}: use one operator marker"),
            ));
        }
        let parsed: syn::Ident = attr.parse_args().map_err(|_| {
            syn::Error::new_spanned(
                attr,
                format!(
                    "{class_name}.{method_name}: write operator hooks with a \
                     metamethod name such as '#[operator(__add)]'"
                ),
            )
        })?;
        match parsed.to_string().as_str() {
            "__add" => hook = Some(Operator::Add),
            "__sub" => hook = Some(Operator::Sub),
            "__mul" => hook = Some(Operator::Mul),
            "__eq" => hook = Some(Operator::Eq),
            "__len" => hook = Some(Operator::Len),
            "__tostring" => hook = Some(Operator::ToString),
            found => {
                return Err(syn::Error::new_spanned(
                    &method.sig.ident,
                    format!(
                        "{class_name}.{method_name}: unknown operator hook \
                         '{found}'; want __add, __sub, __mul, __eq, __len, or \
                         __tostring"
                    ),
                ));
            }
        }
    }
    Ok(hook)
}

/// Parses the optional marker.
pub(crate) fn param_optional(
    class_name: &str,
    method_name: &str,
    label: &str,
    attrs: &[syn::Attribute],
    span: &syn::Ident,
) -> Result<bool, syn::Error> {
    let mut optional = false;
    for attr in attrs {
        if !attr.path().is_ident("shape") {
            continue;
        }
        let items = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;
        for item in items {
            match item {
                syn::Meta::Path(path) => {
                    let ident = path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    if ident == "optional" {
                        optional = true;
                    } else {
                        return Err(syn::Error::new_spanned(
                            span,
                            format!(
                                "{class_name}.{method_name}.{label}: shape params take optional"
                            ),
                        ));
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        span,
                        format!("{class_name}.{method_name}.{label}: shape params take optional"),
                    ));
                }
            }
        }
    }
    Ok(optional)
}
