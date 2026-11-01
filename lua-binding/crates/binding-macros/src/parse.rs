//! Parses `#[shape(...)]` attributes into strategy, field, and variant options.

use syn::{Attribute, Fields, Lit, Meta, Type, punctuated::Punctuated, token::Comma};

/// Strategy marker reading on every derived type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShapeStrategy {
    Table,
    Function,
    Object,
}

/// Alias strategy reading on derived enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AliasStrategy {
    StrAlias,
    Untagged,
    TypedAlias,
    Probe,
}

/// Parses variant options from a shape attribute.
#[derive(Default, Debug)]
pub(crate) struct VariantOptions {
    pub(crate) name: Option<String>,
    pub(crate) variant: Option<String>,
    pub(crate) probe: Vec<String>,
}

/// Parses shape options from every derived type.
#[derive(Default)]
pub(crate) struct ShapeOptions {
    /// The strategy marker.
    pub(crate) strategy: Option<ShapeStrategy>,
    /// The trace fill marker.
    pub(crate) traced: bool,
    /// The alias strategy.
    pub(crate) alias: Option<AliasStrategy>,
    /// The stub name override.
    pub(crate) name: Option<String>,
    /// The probe hook path overriding untagged family routing.
    pub(crate) probe: Option<syn::Path>,
    /// The validate hook path checking decoded values.
    pub(crate) validate: Option<syn::Path>,
    /// The function return shape path.
    pub(crate) returns: Option<syn::Path>,
}

/// Parses field options from a shape attribute.
#[derive(Default)]
pub(crate) struct FieldOptions {
    /// The optional marker for plain types.
    pub(crate) optional: bool,
    /// The opaque marker for pass-through display.
    pub(crate) opaque: bool,
    /// The borrowed marker for callable tables.
    pub(crate) borrowed: bool,
    /// The ignore marker for Rust-filled fields.
    pub(crate) ignore: bool,
    /// The flatten marker for nested table shapes.
    pub(crate) flatten: bool,
    /// The trace marker for Rust-filled call stacks.
    pub(crate) lua_trace: bool,
    /// The getter marker for object fields.
    pub(crate) get: bool,
    /// The setter marker for object fields.
    pub(crate) set: bool,
    /// The Lua key override.
    pub(crate) name: Option<String>,
    /// The Lua shape override.
    pub(crate) proxy: Option<syn::Path>,
    /// The decode hook path.
    pub(crate) decode_hook: Option<syn::Path>,
    /// The encode hook path.
    pub(crate) encode_hook: Option<syn::Path>,
    /// The map key decode function path.
    pub(crate) key_decode: Option<syn::Path>,
    /// The map value decode function path.
    pub(crate) value_decode: Option<syn::Path>,
    /// The absent-key default.
    pub(crate) default: Option<Option<syn::Path>>,
}

/// Parses the strategy marker, the alias strategy and hooks.
pub(crate) fn parse_header(
    identifier: &syn::Ident,
    attrs: &[Attribute],
    is_enum: bool,
) -> Result<ShapeOptions, syn::Error> {
    let type_name = identifier.to_string();
    let mut header = ShapeOptions::default();
    for attr in attrs {
        if !attr.path().is_ident("shape") {
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
                            if header.strategy.is_some() {
                                return Err(syn::Error::new_spanned(
                                    item,
                                    format!("{type_name}: keep one kind marker"),
                                ));
                            }
                            header.strategy = Some(ShapeStrategy::Table);
                            continue;
                        }
                        "object" => {
                            if header.strategy.is_some() {
                                return Err(syn::Error::new_spanned(
                                    item,
                                    format!("{type_name}: keep one kind marker"),
                                ));
                            }
                            header.strategy = Some(ShapeStrategy::Object);
                            continue;
                        }
                        "function" => {
                            if header.strategy.is_some() {
                                return Err(syn::Error::new_spanned(
                                    item,
                                    format!("{type_name}: keep one kind marker"),
                                ));
                            }
                            header.strategy = Some(ShapeStrategy::Function);
                            continue;
                        }
                        "str_alias" => AliasStrategy::StrAlias,
                        "untagged" => AliasStrategy::Untagged,
                        "typed_alias" => AliasStrategy::TypedAlias,
                        "probe" => AliasStrategy::Probe,
                        "traced" => {
                            header.traced = true;
                            continue;
                        }
                        "optional" | "opaque" | "default" => {
                            return Err(syn::Error::new_spanned(
                                item,
                                format!("{type_name}: '{ident}' belongs on a field"),
                            ));
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                item,
                                format!(
                                    "{type_name}: unknown shape marker '{ident}'; want \
                                     a shape marker: table, object, function, traced, \
                                     str_alias, untagged, typed_alias, probe, validate, \
                                     or name"
                                ),
                            ));
                        }
                    };
                    if header.alias.is_some() {
                        return Err(syn::Error::new_spanned(
                            item,
                            format!("{type_name}: keep one alias marker"),
                        ));
                    }
                    header.alias = Some(found);
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
                                    format!("{type_name}: shape option 'probe' wants a hook path"),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!("{type_name}: shape option 'probe' wants a hook path"),
                                ));
                            }
                            header.probe = Some(found.path.clone());
                        }
                        "validate" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: shape option 'validate' wants a hook path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: shape option 'validate' wants a hook path"
                                    ),
                                ));
                            }
                            header.validate = Some(found.path.clone());
                        }
                        "name" => {
                            let syn::Expr::Lit(lit) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: shape option 'name' wants a string literal"
                                    ),
                                ));
                            };
                            let Lit::Str(text) = &lit.lit else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: shape option 'name' wants a string literal"
                                    ),
                                ));
                            };
                            header.name = Some(text.value());
                        }
                        "returns" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: shape option 'returns' wants a shape type"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{type_name}: shape option 'returns' wants a shape type"
                                    ),
                                ));
                            }
                            header.returns = Some(found.path.clone());
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                &pair.path,
                                format!(
                                    "{type_name}: unknown shape option '{key}'; want \
                                     probe or validate with a hook path, name \
                                     with a string literal, or returns \
                                     with a shape type"
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        item,
                        format!(
                            "{type_name}: unknown shape marker; want markers with \
                             probe or validate hooks, or name with a string \
                             literal"
                        ),
                    ));
                }
            }
        }
    }
    let Some(kind) = header.strategy else {
        return Err(syn::Error::new_spanned(
            identifier,
            format!(
                "{type_name}: Shape derive wants one kind marker: \
                 #[shape(table)], #[shape(object)], or #[shape(function)]"
            ),
        ));
    };
    header.strategy = Some(kind);
    if header.returns.is_some() && header.strategy != Some(ShapeStrategy::Function) {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: `returns` belongs to function shapes"),
        ));
    }
    if is_enum && header.alias.is_none() && header.strategy != Some(ShapeStrategy::Object) {
        return Err(syn::Error::new_spanned(
            identifier,
            format!(
                "{type_name}: Shape derive wants one alias marker with `table`: \
                 str_alias, untagged, typed_alias, or probe"
            ),
        ));
    }
    if !is_enum && header.alias.is_some() {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: struct shapes take no alias marker"),
        ));
    }
    if !is_enum && header.probe.is_some() {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: probe hooks belong to untagged enums"),
        ));
    }
    if header.strategy == Some(ShapeStrategy::Object) && header.probe.is_some() {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: probe hooks belong to untagged enums"),
        ));
    }
    if header.traced && (is_enum || header.strategy != Some(ShapeStrategy::Object)) {
        return Err(syn::Error::new_spanned(
            identifier,
            format!("{type_name}: `traced` belongs to object structs"),
        ));
    }
    Ok(header)
}

/// Parses variant flags.
pub(crate) fn parse_variant_flags(
    outer: &str,
    variant: &str,
    attrs: &[Attribute],
) -> Result<VariantOptions, syn::Error> {
    let mut flags = VariantOptions::default();
    for attr in attrs {
        if !attr.path().is_ident("shape") {
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
                            "{outer}.{variant}: shape option '{ident}' belongs on \
                             the container"
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
                                "{outer}.{variant}: shape option '{key}' wants \
                                 a string literal"
                            ),
                        ));
                    };
                    let Lit::Str(text) = &lit.lit else {
                        return Err(syn::Error::new_spanned(
                            &pair.value,
                            format!(
                                "{outer}.{variant}: shape option '{key}' wants \
                                 a string literal"
                            ),
                        ));
                    };
                    match key.as_str() {
                        "name" => flags.name = Some(text.value()),
                        "variant" => flags.variant = Some(text.value()),
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
                                    "{outer}.{variant}: unknown shape option \
                                     '{key}'; want name, variant, or probe"
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        &item,
                        format!(
                            "{outer}.{variant}: unknown shape marker; want \
                             name, variant, or probe"
                        ),
                    ));
                }
            }
        }
    }
    Ok(flags)
}

/// Parses field flags.
///
/// # Errors
///
/// - `syn::Error` for unknown options and lone proxy markers.
pub(crate) fn parse_field_flags(
    outer: &str,
    field: &str,
    attrs: &[Attribute],
) -> Result<FieldOptions, syn::Error> {
    let mut flags = FieldOptions::default();
    for attr in attrs {
        if !attr.path().is_ident("shape") {
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
                        "borrowed" => flags.borrowed = true,
                        "ignore" => flags.ignore = true,
                        "flatten" => flags.flatten = true,
                        "lua_trace" => flags.lua_trace = true,
                        "get" => flags.get = true,
                        "set" => flags.set = true,
                        "default" => flags.default = Some(None),
                        _ => {
                            return Err(syn::Error::new_spanned(
                                &item,
                                format!(
                                    "{outer}.{field}: unknown shape option '{ident}'; \
                                     want 'optional', 'opaque', 'borrowed', 'ignore', 'flatten', \
                                     'lua_trace', 'get', 'set', 'proxy', 'decode_hook', \
                                     'encode_hook', 'key_decode', 'value_decode', 'name', or \
                                     'default'"
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
                                        "{outer}.{field}: shape option 'proxy' wants a shape type"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'proxy' wants a shape type"
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
                                        "{outer}.{field}: shape option 'decode_hook' wants a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'decode_hook' wants a function path"
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
                                        "{outer}.{field}: shape option 'encode_hook' wants a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'encode_hook' wants a function path"
                                    ),
                                ));
                            }
                            flags.encode_hook = Some(found.path.clone());
                        }
                        "key_decode" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'key_decode' wants a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'key_decode' wants a function path"
                                    ),
                                ));
                            }
                            flags.key_decode = Some(found.path.clone());
                        }
                        "value_decode" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'value_decode' wants a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'value_decode' wants a function path"
                                    ),
                                ));
                            }
                            flags.value_decode = Some(found.path.clone());
                        }
                        "name" => {
                            let syn::Expr::Lit(lit) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'name' wants \
                                         a string literal"
                                    ),
                                ));
                            };
                            let Lit::Str(text) = &lit.lit else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'name' wants \
                                         a string literal"
                                    ),
                                ));
                            };
                            flags.name = Some(text.value());
                        }
                        "default" => {
                            let syn::Expr::Path(found) = &pair.value else {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'default' wants \
                                         a function path"
                                    ),
                                ));
                            };
                            if found.qself.is_some() {
                                return Err(syn::Error::new_spanned(
                                    &pair.value,
                                    format!(
                                        "{outer}.{field}: shape option 'default' wants \
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
                                    "{outer}.{field}: unknown shape option '{key}'; \
                                     want 'optional', 'opaque', 'borrowed', 'ignore', 'flatten', \
                                     'lua_trace', 'get', 'set', 'proxy', 'decode_hook', \
                                     'encode_hook', 'key_decode', 'value_decode', 'name', or \
                                     'default'"
                                ),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        &item,
                        format!(
                            "{outer}.{field}: unknown shape marker; want 'optional', \
                             'opaque', 'borrowed', 'ignore', 'flatten', 'lua_trace', 'get', \
                             'set', 'proxy', 'decode_hook', 'encode_hook', 'key_decode', \
                             'value_decode', 'name', or 'default'"
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
    if flags.set && !flags.get {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: shape option 'set' needs 'get'"),
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
            || flags.borrowed
            || flags.flatten
            || flags.proxy.is_some()
            || flags.decode_hook.is_some()
            || flags.encode_hook.is_some()
            || flags.key_decode.is_some()
            || flags.value_decode.is_some()
            || flags.name.is_some()
            || flags.default.is_some())
    {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: shape option 'ignore' stays apart from key options"),
        ));
    }
    if flags.flatten
        && (flags.optional
            || flags.opaque
            || flags.borrowed
            || flags.ignore
            || flags.proxy.is_some()
            || flags.decode_hook.is_some()
            || flags.encode_hook.is_some()
            || flags.key_decode.is_some()
            || flags.value_decode.is_some()
            || flags.name.is_some()
            || flags.default.is_some())
    {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: shape option 'flatten' stays apart from key options"),
        ));
    }
    if flags.lua_trace
        && (flags.optional
            || flags.opaque
            || flags.borrowed
            || flags.ignore
            || flags.flatten
            || flags.proxy.is_some()
            || flags.decode_hook.is_some()
            || flags.encode_hook.is_some()
            || flags.key_decode.is_some()
            || flags.value_decode.is_some()
            || flags.name.is_some()
            || flags.default.is_some())
    {
        return Err(syn::Error::new_spanned(
            attrs.first(),
            format!("{outer}.{field}: shape option 'lua_trace' stays apart from key options"),
        ));
    }
    Ok(flags)
}

/// Collects trimmed doc lines.
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

/// Collects the payload type.
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

/// Builds the payload shape error.
fn invalid_payload(
    outer: &str,
    variant_name: &str,
    variant: &syn::Variant,
    strategy: &str,
) -> syn::Error {
    syn::Error::new_spanned(
        &variant.ident,
        format!(
            "{outer}.{variant_name}: {strategy} shapes take tuple variants \
             with one payload"
        ),
    )
}
