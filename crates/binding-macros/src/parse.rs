//! Face parsing
//! Read face attributes into marker selections.

use syn::{Attribute, Fields, Lit, Meta, Type, punctuated::Punctuated, token::Comma};

/// Kind marker reading on every derived type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerKind {
    Table,
    CallbackShape,
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

/// Container header reading on every derived type.
#[derive(Default)]
pub(crate) struct ContainerHeader {
    /// The kind marker.
    pub(crate) kind: Option<ContainerKind>,
    /// The trace fill marker.
    pub(crate) traced: bool,
    /// The union annotation.
    pub(crate) union: Option<UnionKind>,
    /// The stub name override.
    pub(crate) rename: Option<String>,
    /// The probe hook path overriding untagged family routing.
    pub(crate) probe: Option<syn::Path>,
    /// The validate hook path checking decoded values.
    pub(crate) validate: Option<syn::Path>,
    /// The callback return face path.
    pub(crate) returns: Option<syn::Path>,
}

/// Field flags parsing from one face attribute.
#[derive(Default)]
pub(crate) struct FieldFlags {
    /// The optional marker for plain types.
    pub(crate) optional: bool,
    /// The opaque marker for pass-through display.
    pub(crate) opaque: bool,
    /// The live marker for borrowed callback tables.
    pub(crate) live: bool,
    /// The ignore marker for Rust-filled fields.
    pub(crate) ignore: bool,
    /// The flatten marker for nested table faces.
    pub(crate) flatten: bool,
    /// The trace marker for Rust-filled call stacks.
    pub(crate) lua_trace: bool,
    /// The Lua key override.
    pub(crate) rename: Option<String>,
    /// The Lua face override.
    pub(crate) proxy: Option<syn::Path>,
    /// The decode hook path.
    pub(crate) decode_hook: Option<syn::Path>,
    /// The encode hook path.
    pub(crate) encode_hook: Option<syn::Path>,
    /// The map key decode function path.
    pub(crate) key_convert: Option<syn::Path>,
    /// The map value decode function path.
    pub(crate) value_convert: Option<syn::Path>,
    /// The absent-key default with its optional function path.
    pub(crate) default: Option<Option<syn::Path>>,
}

/// Parse the kind marker, the union annotation and hooks.
pub(crate) fn parse_header(
    identifier: &syn::Ident,
    attrs: &[Attribute],
    is_enum: bool,
) -> Result<ContainerHeader, syn::Error> {
    let type_name = identifier.to_string();
    let mut header = ContainerHeader::default();
    for attr in attrs {
        if !attr.path().is_ident("face") {
            continue;
        }
        let items = attr.parse_args_with(Punctuated::<Meta, Comma>::parse_terminated)?;
        for item in &items {
            match item {
                Meta::Path(path) => {
                    let ident = path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let found = match ident.as_str() {
                        "table" => {
                            if header.kind.is_some() {
                                return Err(syn::Error::new_spanned(
                                    item,
                                    format!("{type_name}: single kind marker alone"),
                                ));
                            }
                            header.kind = Some(ContainerKind::Table);
                            continue;
                        }
                        "userdata" => {
                            if header.kind.is_some() {
                                return Err(syn::Error::new_spanned(
                                    item,
                                    format!("{type_name}: single kind marker alone"),
                                ));
                            }
                            header.kind = Some(ContainerKind::Userdata);
                            continue;
                        }
                        "callback_shape" => {
                            if header.kind.is_some() {
                                return Err(syn::Error::new_spanned(
                                    item,
                                    format!("{type_name}: single kind marker alone"),
                                ));
                            }
                            header.kind = Some(ContainerKind::CallbackShape);
                            continue;
                        }
                        "string" => UnionKind::String,
                        "untagged" => UnionKind::Untagged,
                        "tagged" => UnionKind::Tagged,
                        "probe" => UnionKind::Probe,
                        "traced" => {
                            header.traced = true;
                            continue;
                        }
                        "optional" | "opaque" | "default" => {
                            return Err(syn::Error::new_spanned(
                                item,
                                format!(
                                    "{type_name}: face option '{ident}' expected \
                                     field-level alone"
                                ),
                            ));
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                item,
                                format!(
                                    "{type_name}: unknown face marker '{ident}'; want \
                                     table, userdata, callback_shape, traced, string, \
                                     untagged, tagged, probe, validate, or probe \
                                     with a hook path"
                                ),
                            ));
                        }
                    };
                    if header.union.is_some() {
                        return Err(syn::Error::new_spanned(
                            item,
                            format!("{type_name}: single union annotation alone"),
                        ));
                    }
                    header.union = Some(found);
                }
                Meta::NameValue(pair) => {
                    let key = pair
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    match key.as_str() {
                        "probe" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!("{type_name}: face option 'probe' wants a hook path"),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!("{type_name}: face option 'probe' wants a hook path"),
                                ));
                            }
                            header.probe = Some(found.path.clone());
                        }
                        "validate" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: face option 'validate' wants a hook path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: face option 'validate' wants a hook path"
                                    ),
                                ));
                            }
                            header.validate = Some(found.path.clone());
                        }
                        "rename" => {
                            let syn::Expr::Lit(lit) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: face option 'rename' wants a string literal"
                                    ),
                                ));
                            };
                            let Lit::Str(text) = &lit.lit else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: face option 'rename' wants a string literal"
                                    ),
                                ));
                            };
                            header.rename = Some(text.value());
                        }
                        "returns" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!("{type_name}: face option 'returns' wants a face type"),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!("{type_name}: face option 'returns' wants a face type"),
                                ));
                            }
                            header.returns = Some(found.path.clone());
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                &pair.path,
                                format!(
                                    "{type_name}: unknown face option '{key}'; want \
                                     probe or validate with a hook path, rename \
                                     with a string literal, or returns \
                                     with a face type"
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        item,
                        format!(
                            "{type_name}: unknown face shape; want markers with \
                             probe or validate hooks, or rename with a string \
                             literal"
                        ),
                    ));
                }
            }
        }
    }
    let Some(kind) = header.kind else {
        return Err(syn::Error::new_spanned(
            identifier,
            format!(
                "{type_name}: Face derive wants one kind marker, add \
                 #[face(table)] or #[face(userdata)]"
            ),
        ));
    };
    header.kind = Some(kind);
    if header.returns.is_some() && header.kind != Some(ContainerKind::CallbackShape) {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: callback shapes alone support returns"),
        ));
    }
    if is_enum && header.union.is_none() && header.kind != Some(ContainerKind::Userdata) {
        return Err(syn::Error::new_spanned(
            identifier,
            format!(
                "{type_name}: Face derive wants one union annotation, add \
                 #[face(string)], #[face(untagged)], #[face(tagged)], or \
                 #[face(probe)]"
            ),
        ));
    }
    if !is_enum && header.union.is_some() {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: struct faces do not support union annotations"),
        ));
    }
    if !is_enum && header.probe.is_some() {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: untagged enums are expected for probe hooks"),
        ));
    }
    if header.kind == Some(ContainerKind::Userdata) && header.probe.is_some() {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: untagged enums are expected for probe hooks"),
        ));
    }
    if header.traced && (is_enum || header.kind != Some(ContainerKind::Userdata)) {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: face option 'traced' needs userdata structs alone"),
        ));
    }
    Ok(header)
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
                            "{outer}.{variant}: face option '{ident}' expected \
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

/// Parse field flags from one field attribute list.
///
/// # Errors
///
/// - `syn::Error` for unknown options and lone proxy markers.
pub(crate) fn parse_field_flags(
    outer: &str,
    field: &str,
    attrs: &[Attribute],
) -> Result<FieldFlags, syn::Error> {
    let mut flags = FieldFlags::default();
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
                    match ident.as_str() {
                        "optional" => flags.optional = true,
                        "opaque" => flags.opaque = true,
                        "live" => flags.live = true,
                        "ignore" => flags.ignore = true,
                        "flatten" => flags.flatten = true,
                        "lua_trace" => flags.lua_trace = true,
                        "default" => flags.default = Some(None),
                        _ => {
                            return Err(syn::Error::new_spanned(
                                &item,
                                format!(
                                    "{outer}.{field}: unknown face option '{ident}'; \
                                     want 'optional', 'opaque', 'live', 'ignore', 'flatten', \
                                     'lua_trace', 'proxy', 'decode_hook', 'encode_hook', \
                                     'key_convert', 'value_convert', 'rename', or 'default'"
                                ),
                            ));
                        }
                    }
                }
                Meta::NameValue(pair) => {
                    let key = pair
                        .path
                        .get_ident()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    match key.as_str() {
                        "proxy" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'proxy' wants a face type"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'proxy' wants a face type"
                                    ),
                                ));
                            }
                            flags.proxy = Some(found.path.clone());
                        }
                        "decode_hook" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'decode_hook' wants a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'decode_hook' wants a function path"
                                    ),
                                ));
                            }
                            flags.decode_hook = Some(found.path.clone());
                        }
                        "encode_hook" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'encode_hook' wants a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'encode_hook' wants a function path"
                                    ),
                                ));
                            }
                            flags.encode_hook = Some(found.path.clone());
                        }
                        "key_convert" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'key_convert' wants a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'key_convert' wants a function path"
                                    ),
                                ));
                            }
                            flags.key_convert = Some(found.path.clone());
                        }
                        "value_convert" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'value_convert' wants a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'value_convert' wants a function path"
                                    ),
                                ));
                            }
                            flags.value_convert = Some(found.path.clone());
                        }
                        "rename" => {
                            let syn::Expr::Lit(lit) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'rename' wants \
                                         a string literal"
                                    ),
                                ));
                            };
                            let Lit::Str(text) = &lit.lit else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'rename' wants \
                                         a string literal"
                                    ),
                                ));
                            };
                            flags.rename = Some(text.value());
                        }
                        "default" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'default' wants \
                                         a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: face option 'default' wants \
                                         a function path"
                                    ),
                                ));
                            }
                            flags.default = Some(Some(found.path.clone()));
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                &pair.path,
                                format!(
                                    "{outer}.{field}: unknown face option '{key}'; \
                                     want 'optional', 'opaque', 'live', 'ignore', 'flatten', \
                                     'lua_trace', 'proxy', 'decode_hook', 'encode_hook', \
                                     'key_convert', 'value_convert', 'rename', or 'default'"
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        &item,
                        format!(
                            "{outer}.{field}: unknown face shape; want 'optional', \
                             'opaque', 'live', 'ignore', 'flatten', 'lua_trace', 'proxy', \
                             'decode_hook', 'encode_hook', 'key_convert', \
                             'value_convert', 'rename', or 'default'"
                        ),
                    ));
                }
            }
        }
    }
    if flags.proxy.is_some() && flags.decode_hook.is_none() {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: proxy without decode_hook is not supported"),
        ));
    }
    if flags.optional && flags.default.is_some() {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: optional with default is not supported"),
        ));
    }
    if flags.ignore
        && (flags.optional
            || flags.opaque
            || flags.live
            || flags.flatten
            || flags.proxy.is_some()
            || flags.decode_hook.is_some()
            || flags.encode_hook.is_some()
            || flags.key_convert.is_some()
            || flags.value_convert.is_some()
            || flags.rename.is_some()
            || flags.default.is_some())
    {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: face option 'ignore' stays apart from key options"),
        ));
    }
    if flags.flatten
        && (flags.optional
            || flags.opaque
            || flags.live
            || flags.ignore
            || flags.proxy.is_some()
            || flags.decode_hook.is_some()
            || flags.encode_hook.is_some()
            || flags.key_convert.is_some()
            || flags.value_convert.is_some()
            || flags.rename.is_some()
            || flags.default.is_some())
    {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: face option 'flatten' stays apart from key options"),
        ));
    }
    if flags.lua_trace
        && (flags.optional
            || flags.opaque
            || flags.live
            || flags.ignore
            || flags.flatten
            || flags.proxy.is_some()
            || flags.decode_hook.is_some()
            || flags.encode_hook.is_some()
            || flags.key_convert.is_some()
            || flags.value_convert.is_some()
            || flags.rename.is_some()
            || flags.default.is_some())
    {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: face option 'lua_trace' stays apart from key options"),
        ));
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
