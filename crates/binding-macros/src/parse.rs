//! Face parsing
//! Reads face attributes into marker selections.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Fields, Lit, Meta, Type, punctuated::Punctuated, token::Comma};

/// Kind marker reading on every derived type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerKind {
    Table,
    Userdata,
}

/// Union annotation reading on derived enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnionKind {
    String,
    Untagged,
    Tagged,
    Probe,
}

/// Variant flags parsing from one face attribute.
#[derive(Default, Debug)]
pub(crate) struct VariantFlags {
    pub(crate) rename: Option<String>,
    pub(crate) tag: Option<String>,
    pub(crate) probe: Vec<String>,
}

/// Parse the kind marker plus the union annotation.
pub(crate) fn parse_header(
    identifier: &syn::Ident,
    attrs: &[Attribute],
    is_enum: bool,
) -> Result<(ContainerKind, Option<UnionKind>), syn::Error> {
    let type_name = identifier.to_string();
    let mut kind: Option<ContainerKind> = None;
    let mut union: Option<UnionKind> = None;
    for attr in attrs {
        if !attr.path().is_ident("face") {
            continue;
        }
        let items = attr.parse_args_with(Punctuated::<Meta, Comma>::parse_terminated)?;
        for item in &items {
            if !matches!(item, Meta::Path(_)) {
                return Err(syn::Error::new_spanned(
                    item,
                    format!(
                        "{type_name}: face header takes markers alone; rename, \
                         tag, and probe read variant-level"
                    ),
                ));
            }
            let ident = item
                .path()
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            let found = match ident.as_str() {
                "table" => {
                    if kind.is_some() {
                        return Err(syn::Error::new_spanned(
                            item,
                            format!("{type_name}: single kind marker alone"),
                        ));
                    }
                    kind = Some(ContainerKind::Table);
                    continue;
                }
                "userdata" => {
                    if kind.is_some() {
                        return Err(syn::Error::new_spanned(
                            item,
                            format!("{type_name}: single kind marker alone"),
                        ));
                    }
                    kind = Some(ContainerKind::Userdata);
                    continue;
                }
                "string" => UnionKind::String,
                "untagged" => UnionKind::Untagged,
                "tagged" => UnionKind::Tagged,
                "probe" => UnionKind::Probe,
                _ => {
                    return Err(syn::Error::new_spanned(
                        item,
                        format!(
                            "{type_name}: unknown face marker '{ident}'; want \
                             table, userdata, string, untagged, tagged, or \
                             probe"
                        ),
                    ));
                }
            };
            if union.is_some() {
                return Err(syn::Error::new_spanned(
                    item,
                    format!("{type_name}: single union annotation alone"),
                ));
            }
            union = Some(found);
        }
    }
    let Some(kind) = kind else {
        return Err(syn::Error::new_spanned(
            identifier,
            format!(
                "{type_name}: Face derive wants one kind marker, add \
                 #[face(table)] or #[face(userdata)]"
            ),
        ));
    };
    if is_enum && union.is_none() {
        return Err(syn::Error::new_spanned(
            identifier,
            format!(
                "{type_name}: Face derive wants one union annotation, add \
                 #[face(string)], #[face(untagged)], #[face(tagged)], or \
                 #[face(probe)]"
            ),
        ));
    }
    if !is_enum && union.is_some() {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: struct faces take no union annotation"),
        ));
    }
    Ok((kind, union))
}

/// Parse variant flags from one variant attribute list.
pub(crate) fn parse_variant_flags(
    outer: &str,
    variant: &str,
    attrs: &[Attribute],
) -> Result<VariantFlags, syn::Error> {
    let mut flags = VariantFlags::default();
    for attr in attrs {
        if !attr.path().is_ident("face") {
            continue;
        }
        let items = attr.parse_args_with(Punctuated::<Meta, Comma>::parse_terminated)?;
        for item in items {
            match &item {
                Meta::Path(path) => {
                    let ident = path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    return Err(syn::Error::new_spanned(
                        &item,
                        format!(
                            "{outer}.{variant}: face option '{ident}' reads \
                             container-level alone"
                        ),
                    ));
                }
                Meta::NameValue(pair) => {
                    let key = pair
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let syn::Expr::Lit(lit) = &pair.value else {
                        return Err(syn::Error::new_spanned(
                            &pair.value,
                            format!(
                                "{outer}.{variant}: face option '{key}' wants \
                                 a string literal"
                            ),
                        ));
                    };
                    let Lit::Str(text) = &lit.lit else {
                        return Err(syn::Error::new_spanned(
                            &pair.value,
                            format!(
                                "{outer}.{variant}: face option '{key}' wants \
                                 a string literal"
                            ),
                        ));
                    };
                    match key.as_str() {
                        "rename" => flags.rename = Some(text.value()),
                        "tag" => flags.tag = Some(text.value()),
                        "probe" => {
                            flags.probe = text
                                .value()
                                .split(',')
                                .map(|part| part.trim().to_owned())
                                .filter(|part| !part.is_empty())
                                .collect();
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                &pair.path,
                                format!(
                                    "{outer}.{variant}: unknown face option \
                                     '{key}'; want rename, tag, or probe"
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        &item,
                        format!(
                            "{outer}.{variant}: unknown face shape; want \
                             rename, tag, or probe"
                        ),
                    ));
                }
            }
        }
    }
    Ok(flags)
}

/// Collect trimmed doc lines from one attribute list.
pub(crate) fn doc_lines(attrs: &[Attribute]) -> Vec<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(pair) = &attr.meta
            && let syn::Expr::Lit(lit) = &pair.value
            && let Lit::Str(text) = &lit.lit
        {
            lines.push(text.value().trim().to_owned());
        }
    }
    lines
}

/// Render the kind marker path for one container kind.
pub(crate) fn kind_tokens(kind: ContainerKind) -> Tokens {
    match kind {
        ContainerKind::Table => {
            quote! { ::confit_binding::face::FaceKind::Table }
        }
        ContainerKind::Userdata => {
            quote! { ::confit_binding::face::FaceKind::Userdata }
        }
    }
}

/// Collect the single payload type of one tuple variant.
pub(crate) fn payload_of(
    outer: &str,
    variant: &syn::Variant,
    strategy: &str,
) -> Result<Type, syn::Error> {
    let variant_name = variant.ident.to_string();
    match &variant.fields {
        Fields::Unnamed(fields) => {
            let mut inner = fields.unnamed.iter();
            match (inner.next(), inner.next()) {
                (Some(first), None) => Ok(first.ty.clone()),
                _ => Err(invalid_payload(outer, &variant_name, variant, strategy)),
            }
        }
        _ => Err(invalid_payload(outer, &variant_name, variant, strategy)),
    }
}

/// Build the payload shape error for one variant.
fn invalid_payload(
    outer: &str,
    variant_name: &str,
    variant: &syn::Variant,
    strategy: &str,
) -> syn::Error {
    syn::Error::new_spanned(
        &variant.ident,
        format!(
            "{outer}.{variant_name}: {strategy} faces read single-payload \
             tuple variants alone"
        ),
    )
}
