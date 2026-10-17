//! Attribute reading
//! Namespace and optional markers from face attributes.

use proc_macro::TokenStream;

/// Read the namespace from one face_impl attribute.
pub(crate) fn parse_namespace(attribute: TokenStream) -> Result<Option<String>, syn::Error> {
    if attribute.is_empty() {
        return Ok(None);
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
        syn::Meta::NameValue(pair) if pair.path.is_ident("namespace") => match &pair.value {
            syn::Expr::Lit(lit) => match &lit.lit {
                syn::Lit::Str(text) => Ok(Some(text.value())),
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
