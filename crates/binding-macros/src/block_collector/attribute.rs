//! Attribute reading
//! Namespace and optional markers from face attributes.

use proc_macro::TokenStream;
use syn::ext::IdentExt;

/// One face impl mode.
pub(crate) enum ImplMode {
    /// Userdata methods alone.
    Userdata,
    /// Static namespace under its dotted path.
    Namespace(String),
    /// Bare globals into chunk environments.
    Global,
}

/// One face metamethod hook.
pub(crate) enum MetaHook {
    /// The `__tostring` hook.
    ToString,
}

/// Read the impl mode from one face_impl attribute.
pub(crate) fn parse_namespace(attribute: TokenStream) -> Result<ImplMode, syn::Error> {
    if attribute.is_empty() {
        return Ok(ImplMode::Userdata);
    }
    let tokens = proc_macro2::TokenStream::from(attribute);
    let parsed: Result<syn::Meta, syn::Error> = syn::parse2(tokens.clone());
    let meta = parsed.map_err(|_| {
        syn::Error::new_spanned(
            tokens,
            "at most 1 argument (namespace) is expected for #[face_impl]",
        )
    })?;
    match meta {
        syn::Meta::Path(path) if path.is_ident("global") => Ok(ImplMode::Global),
        syn::Meta::NameValue(pair) if pair.path.is_ident("namespace") => match &pair.value {
            syn::Expr::Lit(lit) => match &lit.lit {
                syn::Lit::Str(text) => Ok(ImplMode::Namespace(text.value())),
                _ => Err(syn::Error::new_spanned(
                    &pair.value,
                    "at most 1 argument (namespace) is expected for #[face_impl]",
                )),
            },
            _ => Err(syn::Error::new_spanned(
                &pair.value,
                "at most 1 argument (namespace) is expected for #[face_impl]",
            )),
        },
        _ => Err(syn::Error::new_spanned(
            meta,
            "at most 1 argument (namespace) is expected for #[face_impl]",
        )),
    }
}

/// Read the face_meta hook from one method.
pub(crate) fn parse_face_meta(
    class_name: &str,
    method: &syn::ImplItemFn,
) -> Result<Option<MetaHook>, syn::Error> {
    let method_name = method.sig.ident.unraw().to_string();
    let mut hook = None;
    for attr in &method.attrs {
        if !attr.path().is_ident("face_meta") {
            continue;
        }
        if hook.is_some() {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                format!("{class_name}.{method_name}: keep a single face_meta marker alone"),
            ));
        }
        let parsed: syn::Ident = attr.parse_args().map_err(|_| {
            syn::Error::new_spanned(
                attr,
                format!(
                    "{class_name}.{method_name}: write face_meta hooks as \
                     '#[face_meta(__tostring)]'"
                ),
            )
        })?;
        match parsed.to_string().as_str() {
            "__tostring" => hook = Some(MetaHook::ToString),
            found => {
                return Err(syn::Error::new_spanned(
                    &method.sig.ident,
                    format!(
                        "{class_name}.{method_name}: unknown face_meta hook \
                         '{found}'; want __tostring"
                    ),
                ));
            }
        }
    }
    Ok(hook)
}

/// Read the optional marker from one static parameter.
pub(crate) fn param_optional(
    class_name: &str,
    method_name: &str,
    label: &str,
    attrs: &[syn::Attribute],
    span: &syn::Ident,
) -> Result<bool, syn::Error> {
    let mut optional = false;
    for attr in attrs {
        if !attr.path().is_ident("face") {
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
                                "{class_name}.{method_name}.{label}: face params take optional alone"
                            ),
                        ));
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        span,
                        format!(
                            "{class_name}.{method_name}.{label}: face params take optional alone"
                        ),
                    ));
                }
            }
        }
    }
    Ok(optional)
}
