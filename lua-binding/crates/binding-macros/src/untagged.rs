//! Expands untagged enums into ordered alias shapes.

use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{Attribute, Fields, Variant, ext::IdentExt};

use super::parse::{doc_lines, parse_field_flags, parse_variant_flags, payload_of};

/// A payload table synthesized from an inline struct variant.
struct InlinePayload {
    /// The `Parent_Variant` table name.
    identifier: syn::Ident,
    /// The struct definition with Shape and Decode implementations.
    expansion: Tokens,
    /// The payload field names.
    fields: Vec<syn::Ident>,
    /// The shape field literals for the parent variant.
    field_shapes: Vec<Tokens>,
    /// The Lua keys of the keyed payload fields.
    keys: Vec<String>,
    /// The per-field Lua keys in field order.
    lua_keys: Vec<String>,
    /// The payload field types in field order.
    shapes: Vec<syn::Type>,
    /// The keyed payload field names for encoding.
    encode_idents: Vec<syn::Ident>,
}

/// An untagged variant inside its key group.
struct VariantArm {
    /// The enum variant name.
    variant: syn::Ident,
    /// The inline payload table name.
    payload: syn::Ident,
    /// The payload field names.
    fields: Vec<syn::Ident>,
}

/// A key group of untagged variants that share one field set.
struct VariantGroup {
    /// The sorted Lua keys shared by every arm.
    keys: Vec<String>,
    /// The decode arms in declaration order.
    arms: Vec<VariantArm>,
}

/// Places an inline variant in its key group.
fn place_variant(groups: &mut Vec<VariantGroup>, keys: Vec<String>, variant: VariantArm) {
    let mut ordered = keys;
    ordered.sort();
    match groups.iter_mut().find(|group| group.keys == ordered) {
        Some(group) => group.arms.push(variant),
        None => groups.push(VariantGroup {
            keys: ordered,
            arms: vec![variant],
        }),
    }
}

/// Returns whether a type is an `Option` with inner content.
fn is_option_shape(ty: &syn::Type) -> bool {
    if let syn::Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Option";
    }
    false
}

/// Builds a payload table.
///
/// # Errors
///
/// - `syn::Error` for unnamed fields and proxy shapes in payloads.
fn build_inline_payload(
    outer: &str,
    variant: &Variant,
    named: &syn::FieldsNamed,
) -> Result<InlinePayload, syn::Error> {
    let variant_name = variant.ident.to_string();
    let identifier = format_ident!("{}_{}", outer, variant.ident);
    let class_name = identifier.to_string();
    let class_docs = doc_lines(&variant.attrs).join(" ");
    let mut declarations = Vec::new();
    let mut field_shapes = Vec::new();
    let mut decoders = Vec::new();
    let mut field_idents = Vec::new();
    let mut field_names = Vec::new();
    let mut field_lua_keys = Vec::new();
    let mut field_types = Vec::new();
    let mut encode_idents = Vec::new();
    for field in &named.named {
        let Some(field_ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}.{variant_name}: named fields are expected for inline shapes"),
            ));
        };
        let field_name = field_ident.unraw().to_string();
        let flags = parse_field_flags(
            &format!("{outer}.{variant_name}"),
            &field_name,
            &field.attrs,
        )?;
        if flags.flatten {
            return Err(syn::Error::new_spanned(
                &field_ident,
                format!(
                    "{outer}.{variant_name}.{field_name}: flattened shapes need \
                     a table struct"
                ),
            ));
        }
        if flags.opaque {
            return Err(syn::Error::new_spanned(
                &field_ident,
                format!(
                    "{outer}.{variant_name}.{field_name}: inline variant fields take \
                     plain shapes with only name or default"
                ),
            ));
        }
        let field_type = &field.ty;
        if flags.optional && is_option_shape(field_type) {
            return Err(syn::Error::new_spanned(
                field_type,
                format!(
                    "{outer}.{variant_name}.{field_name}: Option fields imply \
                     optional; drop #[shape(optional)]"
                ),
            ));
        }
        if flags.optional && !is_option_shape(field_type) {
            return Err(syn::Error::new_spanned(
                field_type,
                format!(
                    "{outer}.{variant_name}.{field_name}: shape option 'optional' \
                     needs Option with inner content"
                ),
            ));
        }
        if flags.default.is_some() && is_option_shape(field_type) {
            return Err(syn::Error::new_spanned(
                field_type,
                format!(
                    "{outer}.{variant_name}.{field_name}: plain types are expected for shape option 'default'"
                ),
            ));
        }
        let lua_key = flags.name.clone().unwrap_or_else(|| field_name.clone());
        let field_docs = doc_lines(&field.attrs).join(" ");
        if flags.ignore {
            declarations.push(quote! { #[doc = #field_docs] pub #field_ident: #field_type });
            decoders.push(quote! {
                let #field_ident: #field_type =
                    <#field_type as ::std::default::Default>::default();
            });
            field_idents.push(field_ident);
            continue;
        }
        declarations.push(quote! { #[doc = #field_docs] pub #field_ident: #field_type });
        encode_idents.push(field_ident.clone());
        field_lua_keys.push(lua_key.clone());
        field_types.push(field.ty.clone());
        let shape_tokens = field_type;
        let inferred = is_option_shape(field_type);
        if flags.default.is_some() || inferred {
            field_shapes.push(quote! {
                ::lua_binding::shape::Field {
                    name: #lua_key,
                    description: #field_docs,
                    shape: ::std::borrow::Cow::Owned(
                        ::lua_binding::stub::emit_optional(
                            &<#shape_tokens as ::lua_binding::shape::Shape>::name(),
                        ),
                    ),
                    optional: true,
                    collect: <#shape_tokens as ::lua_binding::shape::Shape>::collect,
                }
            });
        } else {
            let marked = flags.optional;
            field_shapes.push(quote! {
                ::lua_binding::shape::Field {
                    name: #lua_key,
                    description: #field_docs,
                    shape: ::std::borrow::Cow::Owned(
                        <#shape_tokens as ::lua_binding::shape::Shape>::name(),
                    ),
                    optional: #marked,
                    collect: <#shape_tokens as ::lua_binding::shape::Shape>::collect,
                }
            });
        }
        if flags.optional || inferred {
            decoders.push(quote! {
                let raw_value: ::mlua::Value =
                    ::lua_binding::value::read_field(&table, #lua_key)?;
                let #field_ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    <#field_type as ::std::default::Default>::default()
                } else {
                    <#field_type as ::lua_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::lua_binding::error::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?
                };
            });
        } else if let Some(default) = flags.default.as_ref() {
            let fallback: Tokens = match default {
                Some(path) => quote! { #path() },
                None => quote! { <#field_type as ::std::default::Default>::default() },
            };
            decoders.push(quote! {
                let raw_value: ::mlua::Value =
                    ::lua_binding::value::read_field(&table, #lua_key)?;
                let #field_ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    #fallback
                } else {
                    <#field_type as ::lua_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::lua_binding::error::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?
                };
            });
        } else {
            decoders.push(quote! {
                let raw_value: ::mlua::Value =
                    ::lua_binding::value::read_field(&table, #lua_key)?;
                if matches!(raw_value, ::mlua::Value::Nil) {
                    return ::std::result::Result::Err(
                        ::lua_binding::error::DecodeError {
                            segments: ::std::vec![::lua_binding::error::Segment::Field(
                                #lua_key.to_owned(),
                            )],
                            reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                                require: <#field_type as ::lua_binding::shape::Shape>::name(),
                                found: "nil".to_owned(),
                            },
                        },
                    );
                }
                let #field_ident =
                    <#field_type as ::lua_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::lua_binding::error::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?;
            });
        }
        field_idents.push(field_ident);
        if !field_names.iter().any(|key| key == &lua_key) {
            field_names.push(lua_key);
        }
    }
    let fields = field_idents.clone();
    let mut encode_stmts = Vec::new();
    for ((ident, key), shape) in encode_idents
        .iter()
        .zip(field_lua_keys.iter())
        .zip(field_types.iter())
    {
        encode_stmts.push(quote! {
            table.set(
                #key,
                <#shape as ::lua_binding::value::Encode>::encode(
                    &self.#ident,
                    lua,
                )?,
            )?;
        });
    }
    let expansion = quote! {
        #[allow(non_camel_case_types)]
        #[doc = #class_docs]
        pub struct #identifier {
            #(#declarations),*
        }

        impl ::lua_binding::shape::Shape for #identifier {
            fn name() -> ::std::string::String {
                #class_name.to_owned()
            }

            fn shape_data() -> ::lua_binding::shape::ShapeData {
                ::lua_binding::shape::ShapeData::Table {
                    description: #class_docs,
                    fields: ::std::vec![#(#field_shapes),*],
                }
            }

            fn collect(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                let data = <Self as ::lua_binding::shape::Shape>::shape_data();
                if collector.add(<Self as ::lua_binding::shape::Shape>::name(), &data) {
                    data.collect_referenced(collector);
                }
            }
        }

        impl ::lua_binding::value::Decode for #identifier {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                Self::decode_ignored(value, lua, &[])
            }

            fn decode_ignored(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
                ignored: &[&str],
            ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                let table = ::lua_binding::value::table_value(value)?;
                ::lua_binding::value::check_keys_ignored(
                    &table,
                    &[#(#field_names),*],
                    ignored,
                )?;
                #(#decoders)*
                ::std::result::Result::Ok(Self {
                    #(#field_idents),*
                })
            }
        }

        impl ::lua_binding::value::Encode for #identifier {
            fn encode(
                &self,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<::mlua::Value, ::mlua::Error> {
                let table = lua.create_table()?;
                #(#encode_stmts)*
                ::std::result::Result::Ok(::mlua::Value::Table(table))
            }
        }
    };
    Ok(InlinePayload {
        identifier,
        expansion,
        fields,
        field_shapes,
        keys: field_names,
        lua_keys: field_lua_keys,
        shapes: field_types,
        encode_idents,
    })
}

/// Renders the validate check.
fn validate_check(outer: &str, validate: &Option<syn::Path>) -> Tokens {
    match validate {
        Some(hook) => quote! {
            #hook(&result).map_err(|error: ::mlua::Error| {
                ::lua_binding::error::DecodeError {
                    segments: ::std::vec::Vec::new(),
                    reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                        require: ::std::format!("valid {}", #outer),
                        found: error.to_string(),
                    },
                }
            })?;
        },
        None => quote! {},
    }
}

/// Renders the decode dispatch.
fn group_dispatch(index: usize, group: &VariantGroup, check: &Tokens) -> Tokens {
    let mut arms = Vec::new();
    for arm in &group.arms {
        let variant = &arm.variant;
        let payload = &arm.payload;
        let fields = &arm.fields;
        arms.push(quote! {
            match <#payload as ::lua_binding::value::Decode>::decode_ignored(
                &::mlua::Value::Table(table.clone()),
                lua,
                ignored,
            ) {
                ::std::result::Result::Ok(inner) => {
                    let result = Self::#variant {
                        #(#fields: inner.#fields),*
                    };
                    #check
                    return ::std::result::Result::Ok(result);
                }
                ::std::result::Result::Err(error) => {
                    last_error = ::std::option::Option::Some(error);
                }
            }
        });
    }
    quote! {
        if _index == #index {
            let mut last_error: ::std::option::Option<
                ::lua_binding::error::DecodeError,
            > = ::std::option::Option::None;
            #(#arms)*
            if let ::std::option::Option::Some(error) = last_error {
                return ::std::result::Result::Err(error);
            }
        }
    }
}

/// Renders the default decode body routing tables through key groups.
///
/// - Unknown-key tables keep the tuple-variant mismatch past its front door.
/// - Ambiguous tables and bare values keep the alias mismatch.
fn default_body(
    groups: &[VariantGroup],
    fallback: &[Tokens],
    discarded: &[Tokens],
    shapes: &[Tokens],
    check: &Tokens,
) -> Tokens {
    let mut sets = Vec::new();
    let mut known: Vec<String> = Vec::new();
    let mut dispatch = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let keys = &group.keys;
        sets.push(quote! { &[#(#keys),*] as &[&str] });
        for key in keys {
            if !known.iter().any(|held| held == key) {
                known.push(key.clone());
            }
        }
        dispatch.push(group_dispatch(index, group, check));
    }
    let mappings: Tokens = if sets.is_empty() {
        quote! { &[] as &[&[&str]] }
    } else {
        quote! { &[#(#sets),*] }
    };
    let stranger_require: Tokens = if known.is_empty() {
        quote! { [#(#shapes),*].join(" | ") }
    } else {
        quote! { ::std::format!("one of {}", [#(#known),*].join(", ")) }
    };
    let routed: Tokens = if fallback.is_empty() {
        quote! {
            return ::std::result::Result::Err(match outcome {
                ::lua_binding::value::VariantRoute::Unknown(unknown)
                    if !unknown.is_empty() =>
                {
                    ::lua_binding::error::DecodeError {
                        segments: ::std::vec::Vec::new(),
                        reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                            require: #stranger_require,
                            found: ::std::format!(
                                "unknown field '{}'",
                                unknown.join("', '")
                            ),
                        },
                    }
                }
                _ => ::lua_binding::error::DecodeError {
                    segments: ::std::vec::Vec::new(),
                    reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                        require: [#(#shapes),*].join(" | "),
                        found: value.type_name().to_owned(),
                    },
                },
            });
        }
    } else {
        quote! {
            let mut fallback_error: ::std::option::Option<
                ::lua_binding::error::DecodeError,
            > = ::std::option::Option::None;
            #(#fallback)*
            if let ::lua_binding::value::VariantRoute::Unknown(_) = &outcome {
                if let ::std::option::Option::Some(error) = fallback_error {
                    return ::std::result::Result::Err(error);
                }
            }
            return ::std::result::Result::Err(match outcome {
                ::lua_binding::value::VariantRoute::Unknown(unknown)
                    if !unknown.is_empty() =>
                {
                    ::lua_binding::error::DecodeError {
                        segments: ::std::vec::Vec::new(),
                        reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                            require: #stranger_require,
                            found: ::std::format!(
                                "unknown field '{}'",
                                unknown.join("', '")
                            ),
                        },
                    }
                }
                _ => ::lua_binding::error::DecodeError {
                    segments: ::std::vec::Vec::new(),
                    reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                        require: [#(#shapes),*].join(" | "),
                        found: value.type_name().to_owned(),
                    },
                },
            });
        }
    };
    quote! {
        if let ::mlua::Value::Table(table) = value.clone() {
            let keys =
                ::lua_binding::value::table_keys(&table)?;
            let keys: ::std::vec::Vec<::std::string::String> = keys
                .into_iter()
                .filter(|key| !ignored.contains(&key.as_str()))
                .collect();
            match ::lua_binding::value::route_variant(&keys, #mappings) {
                ::lua_binding::value::VariantRoute::Match(_index) => {
                    #(#dispatch)*
                    return ::std::result::Result::Err(
                        ::lua_binding::error::DecodeError {
                            segments: ::std::vec::Vec::new(),
                            reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                                require: [#(#shapes),*].join(" | "),
                                found: "an unknown shape".to_owned(),
                            },
                        },
                    );
                }
                outcome => {
                    #routed
                }
            }
        }
        #(#discarded)*
        ::std::result::Result::Err(::lua_binding::error::DecodeError {
            segments: ::std::vec::Vec::new(),
            reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                require: [#(#shapes),*].join(" | "),
                found: value.type_name().to_owned(),
            },
        })
    }
}

/// Builds the untagged enum expansion.
pub(crate) fn expand_untagged_enum(
    name: &syn::Ident,
    data: &syn::DataEnum,
    attrs: &[Attribute],
    probe: &Option<syn::Path>,
    validate: &Option<syn::Path>,
    rename: &Option<String>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
    let type_docs = doc_lines(attrs).join(" ");
    let mut arms = Vec::new();
    let mut shapes = Vec::new();
    let mut encodes = Vec::new();
    let mut groups: Vec<VariantGroup> = Vec::new();
    let mut fallback = Vec::new();
    let mut discarded = Vec::new();
    let mut nil_attempt: Option<Tokens> = None;
    let mut payloads = Vec::new();
    let mut units = 0usize;
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: untagged shapes need one variant at least"),
        ));
    }
    let check = validate_check(&outer, validate);
    for variant in &data.variants {
        let variant_name = variant.ident.to_string();
        let ident = variant.ident.clone();
        let flags = parse_variant_flags(&outer, &variant_name, &variant.attrs)?;
        if flags.name.is_some() || flags.variant.is_some() || !flags.probe.is_empty() {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}.{variant_name}: untagged shapes do not support variant options"),
            ));
        }
        let docs = doc_lines(&variant.attrs).join(" ");
        match &variant.fields {
            Fields::Unit => {
                units += 1;
                if units > 1 {
                    return Err(syn::Error::new_spanned(
                        &variant.ident,
                        format!(
                            "{outer}.{variant_name}: untagged shapes keep one unit \
                             variant at most"
                        ),
                    ));
                }
                arms.push(quote! {
                    ::lua_binding::shape::Variant {
                        name: stringify!(#ident),
                        description: #docs,
                        shape: ::std::borrow::Cow::Borrowed("nil"),
                        fields: ::std::vec::Vec::new(),
                        collect: ::lua_binding::shape::no_collect,
                    }
                });
                shapes.push(quote! { "nil".to_owned() });
                encodes.push(quote! {
                    Self::#ident => ::std::result::Result::Ok(::mlua::Value::Nil),
                });
                nil_attempt = Some(quote! {
                    if matches!(value, ::mlua::Value::Nil) {
                        let result = Self::#ident;
                        #check
                        return ::std::result::Result::Ok(result);
                    }
                });
            }
            Fields::Named(named) => {
                let payload = build_inline_payload(&display, variant, named)?;
                let identifier = payload.identifier.clone();
                let field_idents = payload.fields.clone();
                let encode_idents = payload.encode_idents.clone();
                let mut arm_stmts = Vec::new();
                for ((ident, key), shape) in payload
                    .encode_idents
                    .iter()
                    .zip(payload.lua_keys.iter())
                    .zip(payload.shapes.iter())
                {
                    arm_stmts.push(quote! {
                        table.set(
                            #key,
                            <#shape as ::lua_binding::value::Encode>::encode(
                                #ident,
                                lua,
                            )?,
                        )?;
                    });
                }
                payloads.push(payload.expansion);
                let arm_fields = payload.field_shapes.clone();
                arms.push(quote! {
                    ::lua_binding::shape::Variant {
                        name: stringify!(#ident),
                        description: #docs,
                        shape: ::std::borrow::Cow::Owned(
                            <#identifier as ::lua_binding::shape::Shape>::name(),
                        ),
                        fields: ::std::vec![#(#arm_fields),*],
                        collect: <#identifier as ::lua_binding::shape::Shape>::collect,
                    }
                });
                shapes.push(quote! {
                    <#identifier as ::lua_binding::shape::Shape>::name()
                });
                let pattern: Tokens = if encode_idents.is_empty() {
                    quote! { .. }
                } else if encode_idents.len() == field_idents.len() {
                    quote! { #(#encode_idents),* }
                } else {
                    quote! { #(#encode_idents),*, .. }
                };
                encodes.push(quote! {
                    Self::#ident { #pattern } => {
                        let table = lua.create_table()?;
                        #(#arm_stmts)*
                        ::std::result::Result::Ok(::mlua::Value::Table(table))
                    },
                });
                place_variant(
                    &mut groups,
                    payload.keys.clone(),
                    VariantArm {
                        variant: ident,
                        payload: identifier,
                        fields: field_idents,
                    },
                );
            }
            Fields::Unnamed(_) => {
                let payload = payload_of(&outer, variant, "untagged")?;
                arms.push(quote! {
                    ::lua_binding::shape::Variant {
                        name: stringify!(#ident),
                        description: #docs,
                        shape: ::std::borrow::Cow::Owned(
                            <#payload as ::lua_binding::shape::Shape>::name(),
                        ),
                        fields: match <#payload as ::lua_binding::shape::Shape>::shape_data() {
                            ::lua_binding::shape::ShapeData::Table { fields, .. } => fields,
                            _ => ::std::vec::Vec::new(),
                        },
                        collect: <#payload as ::lua_binding::shape::Shape>::collect,
                    }
                });
                shapes.push(quote! {
                    <#payload as ::lua_binding::shape::Shape>::name()
                });
                encodes.push(quote! {
                    Self::#ident(inner) => {
                        <#payload as ::lua_binding::value::Encode>::encode(inner, lua)
                    },
                });
                fallback.push(quote! {
                    match <#payload as ::lua_binding::value::Decode>::decode_ignored(
                        &value.clone(),
                        lua,
                        ignored,
                    ) {
                        ::std::result::Result::Ok(inner) => {
                            let result = Self::#ident(inner);
                            #check
                            return ::std::result::Result::Ok(result);
                        }
                        ::std::result::Result::Err(error) => {
                            if !error.segments.is_empty() {
                                fallback_error = ::std::option::Option::Some(error);
                            }
                        }
                    }
                });
                discarded.push(quote! {
                    if let ::std::result::Result::Ok(inner) =
                        <#payload as ::lua_binding::value::Decode>::decode_ignored(
                            &value.clone(),
                            lua,
                            ignored,
                        )
                    {
                        let result = Self::#ident(inner);
                        #check
                        return ::std::result::Result::Ok(result);
                    }
                });
            }
        }
    }
    let decode_body: Tokens = match probe {
        Some(hook) => {
            let nil = nil_attempt.clone();
            quote! {
                #nil
                let _ = ignored;
                let table = ::lua_binding::value::table_value(value)?;
                let result = #hook(lua, &table)?;
                #check
                ::std::result::Result::Ok(result)
            }
        }
        None => {
            let nil = nil_attempt.clone();
            let routed = default_body(&groups, &fallback, &discarded, &shapes, &check);
            quote! {
                #nil
                #routed
            }
        }
    };
    Ok(quote! {
        #(#payloads)*

        impl ::lua_binding::shape::Shape for #name {
            fn name() -> ::std::string::String {
                #display.to_owned()
            }

            fn shape_data() -> ::lua_binding::shape::ShapeData {
                ::lua_binding::shape::ShapeData::Alias {
                    description: #type_docs,
                    variants: ::std::vec![#(#arms),*],
                }
            }

            fn collect(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                let data = <Self as ::lua_binding::shape::Shape>::shape_data();
                if collector.add(<Self as ::lua_binding::shape::Shape>::name(), &data) {
                    data.collect_referenced(collector);
                }
            }
        }

        impl ::lua_binding::value::Decode for #name {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                Self::decode_ignored(value, lua, &[])
            }

            fn decode_ignored(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
                ignored: &[&str],
            ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                #decode_body
            }
        }

        impl ::lua_binding::value::Encode for #name {
            fn encode(
                &self,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<::mlua::Value, ::mlua::Error> {
                match self {
                    #(#encodes)*
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(source: &str) -> String {
        expand_named(source, &None)
    }

    fn expand_named(source: &str, rename: &Option<String>) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Enum(data) = &input.data else {
            panic!("want enum input");
        };
        expand_untagged_enum(&input.ident, data, &input.attrs, &None, &None, rename)
            .unwrap()
            .to_string()
    }

    #[test]
    fn untagged_inline_variant_synthesizes_described_payload_table() {
        let expanded = expand(
            r#"
            #[shape(table, untagged)]
            pub enum Condition {
                /// Equality check.
                EnvEq {
                    /// The variable name.
                    key: String,
                },
                /// Conjunction.
                All(Vec<String>),
            }
            "#,
        );
        assert!(expanded.contains("Condition_EnvEq"));
        assert!(expanded.contains("add"));
        assert!(expanded.contains("collect_referenced"));
        assert!(expanded.contains("Equality check"));
        assert!(expanded.contains("The variable name"));
        assert!(expanded.contains("shape"));
        assert!(expanded.contains("Variant"));
    }

    #[test]
    fn untagged_alias_shape_references_payload_table() {
        let expanded = expand(
            r#"
            #[shape(table, untagged)]
            pub enum Condition {
                /// Equality check.
                EnvEq {
                    /// The variable name.
                    key: String,
                },
                /// Conjunction.
                All(Vec<String>),
            }
            "#,
        );
        assert!(expanded.contains("add"));
        assert!(expanded.contains("collect_referenced"));
        assert!(expanded.contains("Condition_EnvEq"));
        assert!(expanded.contains("shape"));
    }

    #[test]
    fn untagged_tuple_variant_synthesizes_no_payload_table() {
        let expanded = expand(
            r#"
            #[shape(table, untagged)]
            pub enum Condition {
                /// Equality check.
                EnvEq {
                    /// The variable name.
                    key: String,
                },
                /// Conjunction.
                All(Vec<String>),
            }
            "#,
        );
        assert!(!expanded.contains("Condition_All"));
    }

    // Rename contract lives here. Tagged reuses this shape.
    #[test]
    fn untagged_container_name_propagates_to_payload_table() {
        let expanded = expand_named(
            r#"
            #[shape(table, untagged, name = "Process")]
            pub enum Data {
                /// Plain text payload.
                Text {
                    /// The exact file text.
                    content: String,
                },
                /// Structured payload.
                Shaped(ShapedPayload),
            }
            "#,
            &Some("Process".to_owned()),
        );
        assert!(expanded.contains("Process_Text"));
        assert!(!expanded.contains("Data_Text"));
    }

    #[test]
    fn flattened_inline_fields_fail_the_build() {
        let input: syn::DeriveInput = syn::parse_str(
            r#"
            #[shape(table, untagged)]
            pub enum Wrapper {
                Inner {
                    path: String,
                    #[shape(flatten)]
                    body: Inner,
                },
            }
            "#,
        )
        .unwrap();
        let syn::Data::Enum(data) = &input.data else {
            panic!("want enum input");
        };
        assert!(
            expand_untagged_enum(&input.ident, data, &input.attrs, &None, &None, &None).is_err(),
            "foreign shape fails the build"
        );
    }
}
