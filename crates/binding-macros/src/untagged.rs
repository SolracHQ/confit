//! Untagged faces
//! Expands untagged enums into ordered unions.

use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{Attribute, Fields, Variant, ext::IdentExt};

use super::parse::{
    ContainerKind, doc_lines, kind_tokens, parse_field_flags, parse_variant_flags, payload_of,
};

/// Payload class synthesized from one inline struct variant.
struct InlinePayload {
    /// The `Parent_Variant` class name.
    identifier: syn::Ident,
    /// The struct definition with Face and Decode implementations.
    expansion: Tokens,
    /// The payload field names.
    fields: Vec<syn::Ident>,
    /// The Lua keys of the keyed payload fields.
    keys: Vec<String>,
    /// The per-field Lua keys in field order.
    lua_keys: Vec<String>,
    /// The payload field types in field order.
    shapes: Vec<syn::Type>,
    /// The per-field encode paths in field order.
    encodes: Vec<Option<syn::Path>>,
    /// The keyed payload field names for encoding.
    encode_idents: Vec<syn::Ident>,
}

/// One untagged arm inside its key family.
struct Arm {
    /// The enum variant name.
    variant: syn::Ident,
    /// The inline payload class name.
    payload: syn::Ident,
    /// The payload field names.
    fields: Vec<syn::Ident>,
}

/// One key family grouping untagged variants with equal field sets.
struct Family {
    /// The sorted Lua keys shared by every arm.
    keys: Vec<String>,
    /// The decode arms in declaration order.
    arms: Vec<Arm>,
}

/// Group one inline arm with its equal key family.
fn place_arm(families: &mut Vec<Family>, keys: Vec<String>, arm: Arm) {
    let mut ordered = keys;
    ordered.sort();
    match families.iter_mut().find(|family| family.keys == ordered) {
        Some(family) => family.arms.push(arm),
        None => families.push(Family {
            keys: ordered,
            arms: vec![arm],
        }),
    }
}

/// Report whether one type reads as Option with inner content.
fn is_option_shape(ty: &syn::Type) -> bool {
    if let syn::Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Option";
    }
    false
}

/// Build one `Parent_Variant` payload class from one inline variant.
///
/// # Errors
///
/// - `syn::Error` for unnamed fields and proxy faces in payloads.
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
    let mut descriptors = Vec::new();
    let mut decoders = Vec::new();
    let mut members = Vec::new();
    let mut field_idents = Vec::new();
    let mut field_names = Vec::new();
    let mut field_lua_keys = Vec::new();
    let mut field_shapes = Vec::new();
    let mut field_encodes: Vec<Option<syn::Path>> = Vec::new();
    let mut encode_idents = Vec::new();
    for field in &named.named {
        let Some(field_ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}.{variant_name}: inline faces read named fields alone"),
            ));
        };
        let field_name = field_ident.unraw().to_string();
        let flags = parse_field_flags(
            &format!("{outer}.{variant_name}"),
            &field_name,
            &field.attrs,
        )?;
        if flags.opaque || flags.key_convert.is_some() || flags.value_convert.is_some() {
            return Err(syn::Error::new_spanned(
                &field_ident,
                format!(
                    "{outer}.{variant_name}.{field_name}: inline variant fields take \
                     plain faces with rename or default alone"
                ),
            ));
        }
        let field_type = &field.ty;
        if flags.optional && is_option_shape(field_type) {
            return Err(syn::Error::new_spanned(
                field_type,
                format!(
                    "{outer}.{variant_name}.{field_name}: Option fields read \
                     optional alone; drop #[face(optional)]"
                ),
            ));
        }
        if flags.optional && !is_option_shape(field_type) {
            return Err(syn::Error::new_spanned(
                field_type,
                format!(
                    "{outer}.{variant_name}.{field_name}: face option 'optional' \
                     needs Option with inner content"
                ),
            ));
        }
        if flags.default.is_some() && is_option_shape(field_type) {
            return Err(syn::Error::new_spanned(
                field_type,
                format!(
                    "{outer}.{variant_name}.{field_name}: face option 'default' \
                     reads plain types alone"
                ),
            ));
        }
        let lua_key = flags.rename.clone().unwrap_or_else(|| field_name.clone());
        let field_docs = doc_lines(&field.attrs).join(" ");
        let path = format!("{outer}.{variant_name}.{lua_key}");
        if flags.ignore {
            declarations.push(quote! { #[doc = #field_docs] pub #field_ident: #field_type });
            decoders.push(quote! {
                let #field_ident: #field_type =
                    <#field_type as ::std::default::Default>::default();
            });
            field_idents.push(field_ident);
            continue;
        }
        if flags.convert.is_some() && flags.proxy.is_none() {
            if flags.optional || flags.default.is_some() || flags.rename.is_some() {
                return Err(syn::Error::new_spanned(
                    &field_ident,
                    format!(
                        "{outer}.{variant_name}.{field_name}: sibling convert reads the table \
                         alone with no key options"
                    ),
                ));
            }
            let sibling = format!("{outer}.{variant_name}.{field_name}");
            let convert = flags.convert.as_ref();
            declarations.push(quote! { #[doc = #field_docs] pub #field_ident: #field_type });
            decoders.push(quote! {
                let #field_ident: #field_type = #convert(lua, &table).map_err(|error| {
                    ::confit_binding::value::Mismatch {
                        path: #sibling.to_owned(),
                        want: error.want,
                        found: error.found,
                    }
                })?;
            });
            members.push(quote! {
                <#field_type as ::confit_binding::face::Face>::collect(collector)?;
            });
            field_idents.push(field_ident);
            continue;
        }
        declarations.push(quote! { #[doc = #field_docs] pub #field_ident: #field_type });
        encode_idents.push(field_ident.clone());
        field_lua_keys.push(lua_key.clone());
        field_shapes.push(field.ty.clone());
        field_encodes.push(flags.encode.clone());
        let face_tokens: Tokens = match flags.proxy.as_ref() {
            Some(proxy) => quote! { #proxy },
            None => quote! { #field_type },
        };
        let inferred = is_option_shape(field_type);
        if flags.default.is_some() || inferred {
            descriptors.push(quote! {
                ::confit_binding::face::FieldDescriptor {
                    name: #lua_key,
                    documentation: #field_docs,
                    face: ::std::borrow::Cow::Owned(
                        ::confit_binding::stub::emit_optional(
                            &<#face_tokens as ::confit_binding::face::Face>::face_name(),
                        ),
                    ),
                    optional: true,
                }
            });
        } else {
            let marked = flags.optional;
            descriptors.push(quote! {
                ::confit_binding::face::FieldDescriptor {
                    name: #lua_key,
                    documentation: #field_docs,
                    face: ::std::borrow::Cow::Owned(
                        <#face_tokens as ::confit_binding::face::Face>::face_name(),
                    ),
                    optional: #marked,
                }
            });
        }
        if let Some(convert) = flags.convert.as_ref() {
            if flags.optional || inferred {
                decoders.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                    let #field_ident = if matches!(raw_value, ::mlua::Value::Nil) {
                        <#field_type as ::std::default::Default>::default()
                    } else {
                        ::confit_binding::value::decode_proxy(
                            lua,
                            &raw_value,
                            #path,
                            #convert,
                        )?
                    };
                });
            } else if let Some(default) = flags.default.as_ref() {
                let fallback: Tokens = match default {
                    Some(path) => quote! { #path() },
                    None => quote! { <#field_type as ::std::default::Default>::default() },
                };
                decoders.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                    let #field_ident = if matches!(raw_value, ::mlua::Value::Nil) {
                        #fallback
                    } else {
                        ::confit_binding::value::decode_proxy(
                            lua,
                            &raw_value,
                            #path,
                            #convert,
                        )?
                    };
                });
            } else {
                decoders.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                    if matches!(raw_value, ::mlua::Value::Nil) {
                        return ::std::result::Result::Err(
                            ::confit_binding::value::Mismatch {
                                path: #path.to_owned(),
                                want: <#face_tokens as ::confit_binding::face::Face>::face_name(),
                                found: "nil".to_owned(),
                            },
                        );
                    }
                    let #field_ident = ::confit_binding::value::decode_proxy(
                        lua,
                        &raw_value,
                        #path,
                        #convert,
                    )?;
                });
            }
        } else if flags.optional || inferred {
            decoders.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                let #field_ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    <#field_type as ::std::default::Default>::default()
                } else {
                    <#field_type as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #path, error.path),
                            want: error.want,
                            found: error.found,
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
                    ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                let #field_ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    #fallback
                } else {
                    <#field_type as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #path, error.path),
                            want: error.want,
                            found: error.found,
                        })?
                };
            });
        } else {
            decoders.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                if matches!(raw_value, ::mlua::Value::Nil) {
                    return ::std::result::Result::Err(
                        ::confit_binding::value::Mismatch {
                            path: #path.to_owned(),
                            want: <#field_type as ::confit_binding::face::Face>::face_name(),
                            found: "nil".to_owned(),
                        },
                    );
                }
                let #field_ident =
                    <#field_type as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #path, error.path),
                            want: error.want,
                            found: error.found,
                        })?;
            });
        }
        members.push(quote! {
            <#face_tokens as ::confit_binding::face::Face>::collect(collector)?;
        });
        field_idents.push(field_ident);
        if !field_names.iter().any(|key| key == &lua_key) {
            field_names.push(lua_key);
        }
    }
    let fields = field_idents.clone();
    let mut encode_stmts = Vec::new();
    for (((ident, key), shape), encode) in encode_idents
        .iter()
        .zip(field_lua_keys.iter())
        .zip(field_shapes.iter())
        .zip(field_encodes.iter())
    {
        match encode {
            Some(path) => encode_stmts.push(quote! {
                table.set(
                    #key,
                    lua.create_string(#path(self.#ident.clone()))?,
                )?;
            }),
            None => encode_stmts.push(quote! {
                table.set(
                    #key,
                    <#shape as ::confit_binding::value::Encode>::encode(
                        &self.#ident,
                        lua,
                    )?,
                )?;
            }),
        }
    }
    let expansion = quote! {
        #[allow(non_camel_case_types)]
        #[doc = #class_docs]
        pub struct #identifier {
            #(#declarations),*
        }

        impl ::confit_binding::face::Face for #identifier {
            fn face_name() -> ::std::string::String {
                #class_name.to_owned()
            }

            fn face_stub() -> ::std::string::String {
                ::confit_binding::stub::emit_class(
                    #class_name,
                    #class_docs,
                    &[#(#descriptors),*],
                )
            }

            fn kind() -> ::confit_binding::face::FaceKind {
                ::confit_binding::face::FaceKind::Table
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) -> ::confit_binding::error::Result<()> {
                if collector.register(Self::face_name(), Self::face_stub()) {
                    #(#members)*
                }
                ::std::result::Result::Ok(())
            }
        }

        impl ::confit_binding::value::Decode for #identifier {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::value::Mismatch> {
                let table = ::confit_binding::value::table_value(value, #class_name)?;
                ::confit_binding::value::check_keys(
                    &table,
                    #class_name,
                    &[#(#field_names),*],
                )?;
                #(#decoders)*
                ::std::result::Result::Ok(Self {
                    #(#field_idents),*
                })
            }
        }

        impl ::confit_binding::value::Encode for #identifier {
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
        keys: field_names,
        lua_keys: field_lua_keys,
        shapes: field_shapes,
        encodes: field_encodes,
        encode_idents,
    })
}

/// Render the validate check for one decoded candidate.
fn validate_check(outer: &str, validate: &Option<syn::Path>) -> Tokens {
    match validate {
        Some(hook) => quote! {
            #hook(&result).map_err(|error: ::mlua::Error| {
                ::confit_binding::value::Mismatch {
                    path: #outer.to_owned(),
                    want: ::std::format!("valid {}", #outer),
                    found: error.to_string(),
                }
            })?;
        },
        None => quote! {},
    }
}

/// Render the decode dispatch for one key family.
fn family_dispatch(index: usize, family: &Family, check: &Tokens) -> Tokens {
    let mut arms = Vec::new();
    for arm in &family.arms {
        let variant = &arm.variant;
        let payload = &arm.payload;
        let fields = &arm.fields;
        arms.push(quote! {
            match <#payload as ::confit_binding::value::Decode>::decode(
                &::mlua::Value::Table(table.clone()),
                lua,
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
                ::confit_binding::value::Mismatch,
            > = ::std::option::Option::None;
            #(#arms)*
            if let ::std::option::Option::Some(error) = last_error {
                return ::std::result::Result::Err(error);
            }
        }
    }
}

/// Render the default decode body routing tables through key families.
///
/// - Stranger tables keep the tuple-arm mismatch past its front door.
/// - Ambiguous tables and bare values keep the union mismatch.
fn default_body(
    name: &syn::Ident,
    families: &[Family],
    fallback: &[Tokens],
    discarded: &[Tokens],
    faces: &[Tokens],
    check: &Tokens,
) -> Tokens {
    let mut sets = Vec::new();
    let mut known: Vec<String> = Vec::new();
    let mut dispatch = Vec::new();
    for (index, family) in families.iter().enumerate() {
        let keys = &family.keys;
        sets.push(quote! { &[#(#keys),*] as &[&str] });
        for key in keys {
            if !known.iter().any(|held| held == key) {
                known.push(key.clone());
            }
        }
        dispatch.push(family_dispatch(index, family, check));
    }
    let mappings: Tokens = if sets.is_empty() {
        quote! { &[] as &[&[&str]] }
    } else {
        quote! { &[#(#sets),*] }
    };
    let stranger_want: Tokens = if known.is_empty() {
        quote! { [#(#faces),*].join(" | ") }
    } else {
        quote! { ::std::format!("one of {}", [#(#known),*].join(", ")) }
    };
    let routed: Tokens = if fallback.is_empty() {
        quote! {
            return ::std::result::Result::Err(match outcome {
                ::confit_binding::value::FamilyRoute::Stranger(unknown)
                    if !unknown.is_empty() =>
                {
                    ::confit_binding::value::Mismatch {
                        path: stringify!(#name).to_owned(),
                        want: #stranger_want,
                        found: ::std::format!(
                            "unknown field '{}'",
                            unknown.join("', '")
                        ),
                    }
                }
                _ => ::confit_binding::value::Mismatch {
                    path: stringify!(#name).to_owned(),
                    want: [#(#faces),*].join(" | "),
                    found: value.type_name().to_owned(),
                },
            });
        }
    } else {
        quote! {
            let mut fallback_error: ::std::option::Option<
                ::confit_binding::value::Mismatch,
            > = ::std::option::Option::None;
            #(#fallback)*
            if let ::confit_binding::value::FamilyRoute::Stranger(_) = &outcome {
                if let ::std::option::Option::Some(error) = fallback_error {
                    return ::std::result::Result::Err(
                        ::confit_binding::value::Mismatch {
                            path: ::std::format!(
                                "{}: {}",
                                stringify!(#name),
                                error.path
                            ),
                            want: error.want,
                            found: error.found,
                        },
                    );
                }
            }
            return ::std::result::Result::Err(match outcome {
                ::confit_binding::value::FamilyRoute::Stranger(unknown)
                    if !unknown.is_empty() =>
                {
                    ::confit_binding::value::Mismatch {
                        path: stringify!(#name).to_owned(),
                        want: #stranger_want,
                        found: ::std::format!(
                            "unknown field '{}'",
                            unknown.join("', '")
                        ),
                    }
                }
                _ => ::confit_binding::value::Mismatch {
                    path: stringify!(#name).to_owned(),
                    want: [#(#faces),*].join(" | "),
                    found: value.type_name().to_owned(),
                },
            });
        }
    };
    quote! {
        if let ::mlua::Value::Table(table) = value.clone() {
            let keys =
                ::confit_binding::value::table_keys(&table, stringify!(#name))?;
            match ::confit_binding::value::route_family(&keys, #mappings) {
                ::confit_binding::value::FamilyRoute::Match(_index) => {
                    #(#dispatch)*
                    return ::std::result::Result::Err(
                        ::confit_binding::value::Mismatch {
                            path: stringify!(#name).to_owned(),
                            want: [#(#faces),*].join(" | "),
                            found: "an unknown shape".to_owned(),
                        },
                    );
                }
                outcome => {
                    #routed
                }
            }
        }
        #(#discarded)*
        ::std::result::Result::Err(::confit_binding::value::Mismatch {
            path: stringify!(#name).to_owned(),
            want: [#(#faces),*].join(" | "),
            found: value.type_name().to_owned(),
        })
    }
}

/// Build the derive expansion for one untagged enum.
pub(crate) fn expand_untagged_enum(
    name: &syn::Ident,
    data: &syn::DataEnum,
    attrs: &[Attribute],
    kind: ContainerKind,
    probe: &Option<syn::Path>,
    validate: &Option<syn::Path>,
    rename: &Option<String>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
    let type_docs = doc_lines(attrs).join(" ");
    let marker = kind_tokens(kind);
    let mut descriptors = Vec::new();
    let mut faces = Vec::new();
    let mut members = Vec::new();
    let mut encodes = Vec::new();
    let mut families: Vec<Family> = Vec::new();
    let mut fallback = Vec::new();
    let mut discarded = Vec::new();
    let mut nil_attempt: Option<Tokens> = None;
    let mut payloads = Vec::new();
    let mut units = 0usize;
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: untagged faces need one variant at least"),
        ));
    }
    let check = validate_check(&outer, validate);
    for variant in &data.variants {
        let variant_name = variant.ident.to_string();
        let ident = variant.ident.clone();
        let flags = parse_variant_flags(&outer, &variant_name, &variant.attrs)?;
        if flags.rename.is_some() || flags.tag.is_some() || !flags.probe.is_empty() {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}.{variant_name}: untagged faces take no variant options"),
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
                            "{outer}.{variant_name}: untagged faces keep one unit \
                             variant at most"
                        ),
                    ));
                }
                descriptors.push(quote! {
                    ::confit_binding::face::VariantDescriptor {
                        name: stringify!(#ident),
                        documentation: #docs,
                        face: ::std::borrow::Cow::Borrowed("nil"),
                    }
                });
                faces.push(quote! { "nil".to_owned() });
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
                for (((ident, key), shape), encode) in payload
                    .encode_idents
                    .iter()
                    .zip(payload.lua_keys.iter())
                    .zip(payload.shapes.iter())
                    .zip(payload.encodes.iter())
                {
                    match encode {
                        Some(path) => arm_stmts.push(quote! {
                            table.set(
                                #key,
                                lua.create_string(#path(#ident.clone()))?,
                            )?;
                        }),
                        None => arm_stmts.push(quote! {
                            table.set(
                                #key,
                                <#shape as ::confit_binding::value::Encode>::encode(
                                    #ident,
                                    lua,
                                )?,
                            )?;
                        }),
                    }
                }
                payloads.push(payload.expansion);
                descriptors.push(quote! {
                    ::confit_binding::face::VariantDescriptor {
                        name: stringify!(#ident),
                        documentation: #docs,
                        face: ::std::borrow::Cow::Owned(
                            <#identifier as ::confit_binding::face::Face>::face_name(),
                        ),
                    }
                });
                faces.push(quote! {
                    <#identifier as ::confit_binding::face::Face>::face_name()
                });
                members.push(quote! {
                    <#identifier as ::confit_binding::face::Face>::collect(collector)?;
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
                place_arm(
                    &mut families,
                    payload.keys.clone(),
                    Arm {
                        variant: ident,
                        payload: identifier,
                        fields: field_idents,
                    },
                );
            }
            Fields::Unnamed(_) => {
                let payload = payload_of(&outer, variant, "untagged")?;
                descriptors.push(quote! {
                    ::confit_binding::face::VariantDescriptor {
                        name: stringify!(#ident),
                        documentation: #docs,
                        face: ::std::borrow::Cow::Owned(
                            <#payload as ::confit_binding::face::Face>::face_name(),
                        ),
                    }
                });
                faces.push(quote! {
                    <#payload as ::confit_binding::face::Face>::face_name()
                });
                members.push(quote! {
                    <#payload as ::confit_binding::face::Face>::collect(collector)?;
                });
                encodes.push(quote! {
                    Self::#ident(inner) => {
                        <#payload as ::confit_binding::value::Encode>::encode(inner, lua)
                    },
                });
                fallback.push(quote! {
                    match <#payload as ::confit_binding::value::Decode>::decode(
                        &value.clone(),
                        lua,
                    ) {
                        ::std::result::Result::Ok(inner) => {
                            let result = Self::#ident(inner);
                            #check
                            return ::std::result::Result::Ok(result);
                        }
                        ::std::result::Result::Err(error) => {
                            if error.path
                                != <#payload as ::confit_binding::face::Face>::face_name()
                            {
                                fallback_error = ::std::option::Option::Some(error);
                            }
                        }
                    }
                });
                discarded.push(quote! {
                    if let ::std::result::Result::Ok(inner) =
                        <#payload as ::confit_binding::value::Decode>::decode(
                            &value.clone(),
                            lua,
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
                let table = ::confit_binding::value::table_value(value, stringify!(#name))?;
                let result = #hook(lua, &table)?;
                #check
                ::std::result::Result::Ok(result)
            }
        }
        None => {
            let nil = nil_attempt.clone();
            let routed = default_body(name, &families, &fallback, &discarded, &faces, &check);
            quote! {
                #nil
                #routed
            }
        }
    };
    Ok(quote! {
        #(#payloads)*

        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                #display.to_owned()
            }

            fn face_stub() -> ::std::string::String {
                ::confit_binding::stub::emit_alias(
                    #display,
                    #type_docs,
                    &[#(#descriptors),*],
                )
            }

            fn kind() -> ::confit_binding::face::FaceKind {
                #marker
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) -> ::confit_binding::error::Result<()> {
                if collector.register(Self::face_name(), Self::face_stub()) {
                    #(#members)*
                }
                ::std::result::Result::Ok(())
            }
        }

        impl ::confit_binding::value::Decode for #name {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::value::Mismatch> {
                #decode_body
            }
        }

        impl ::confit_binding::value::Encode for #name {
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
        expand_untagged_enum(
            &input.ident,
            data,
            &input.attrs,
            ContainerKind::Table,
            &None,
            &None,
            rename,
        )
        .unwrap()
        .to_string()
    }

    #[test]
    fn inline_variant_synthesizes_documented_payload_class() {
        let expanded = expand(
            r#"
            #[face(table, untagged)]
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
        assert!(expanded.contains("emit_class"));
        assert!(expanded.contains("Equality check"));
        assert!(expanded.contains("The variable name"));
    }

    #[test]
    fn union_face_references_payload_class() {
        let expanded = expand(
            r#"
            #[face(table, untagged)]
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
        assert!(expanded.contains("emit_alias"));
        assert!(expanded.contains("Condition_EnvEq"));
    }

    #[test]
    fn tuple_arm_synthesizes_no_payload_class() {
        let expanded = expand(
            r#"
            #[face(table, untagged)]
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

    #[test]
    fn missing_docs_still_synthesize_payload_class() {
        let expanded = expand(
            r#"
            #[face(table, untagged)]
            pub enum NoDocs {
                Bare { key: String },
                Many(Vec<String>),
            }
            "#,
        );
        assert!(expanded.contains("NoDocs_Bare"));
        assert!(!expanded.contains("NoDocs_Many"));
    }

    #[test]
    fn container_rename_propagates_to_payload_class() {
        let expanded = expand_named(
            r#"
            #[face(table, untagged, rename = "Document")]
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
            &Some("Document".to_owned()),
        );
        assert!(expanded.contains("Document_Text"));
        assert!(!expanded.contains("Data_Text"));
    }
}
