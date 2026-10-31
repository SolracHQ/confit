//! Tagged faces
//! Expand tagged enums into single-key unions.

use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{Attribute, Fields, Variant, ext::IdentExt};

use super::parse::{doc_lines, parse_field_flags, parse_variant_flags, payload_of};

/// Payload class synthesized from one inline struct variant.
struct InlinePayload {
    /// The `Parent_Variant` class name.
    identifier: syn::Ident,
    /// The struct definition with Face and Decode implementations.
    expansion: Tokens,
    /// The payload field names.
    fields: Vec<syn::Ident>,
    /// The per-field Lua keys in field order.
    lua_keys: Vec<String>,
    /// The payload field types in field order.
    shapes: Vec<syn::Type>,
}

/// Build one `Parent_Variant` payload class from one inline variant.
///
/// # Errors
///
/// - `syn::Error` for unnamed fields and fields carrying face options.
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
    let mut field_idents = Vec::new();
    let mut field_names = Vec::new();
    let mut field_lua_keys = Vec::new();
    let mut field_shapes = Vec::new();
    for field in &named.named {
        let Some(field_ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}.{variant_name}: named fields are expected for inline faces"),
            ));
        };
        let field_name = field_ident.unraw().to_string();
        let flags = parse_field_flags(
            &format!("{outer}.{variant_name}"),
            &field_name,
            &field.attrs,
        )?;
        if flags.proxy.is_some()
            || flags.decode_hook.is_some()
            || flags.encode_hook.is_some()
            || flags.opaque
            || flags.ignore
            || flags.flatten
        {
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
                    "{outer}.{variant_name}.{field_name}: plain types are expected for face option 'default'"
                ),
            ));
        }
        let lua_key = flags.rename.clone().unwrap_or_else(|| field_name.clone());
        let field_docs = doc_lines(&field.attrs).join(" ");
        declarations.push(quote! { #[doc = #field_docs] pub #field_ident: #field_type });
        field_shapes.push(field.ty.clone());
        field_lua_keys.push(lua_key.clone());
        let inferred = is_option_shape(field_type);
        if flags.default.is_some() || inferred {
            descriptors.push(quote! {
                ::confit_binding::face::FieldDescriptor {
                    name: #lua_key,
                    documentation: #field_docs,
                    face: ::std::borrow::Cow::Owned(
                        ::confit_binding::stub::emit_optional(
                            &<#field_type as ::confit_binding::face::Face>::face_name(),
                        ),
                    ),
                    optional: true,
                    collect: <#field_type as ::confit_binding::face::Face>::collect,
                }
            });
        } else {
            let marked = flags.optional;
            descriptors.push(quote! {
                ::confit_binding::face::FieldDescriptor {
                    name: #lua_key,
                    documentation: #field_docs,
                    face: ::std::borrow::Cow::Owned(
                        <#field_type as ::confit_binding::face::Face>::face_name(),
                    ),
                    optional: #marked,
                    collect: <#field_type as ::confit_binding::face::Face>::collect,
                }
            });
        }
        if flags.optional || inferred {
            decoders.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #lua_key)?;
                let #field_ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    <#field_type as ::std::default::Default>::default()
                } else {
                    <#field_type as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::confit_binding::fault::Segment::Field(#lua_key.to_owned()),
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
                    ::confit_binding::value::read_field(&table, #lua_key)?;
                let #field_ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    #fallback
                } else {
                    <#field_type as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::confit_binding::fault::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?
                };
            });
        } else {
            decoders.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #lua_key)?;
                if matches!(raw_value, ::mlua::Value::Nil) {
                    return ::std::result::Result::Err(
                        ::confit_binding::fault::FaceFault {
                            segments: ::std::vec![::confit_binding::fault::Segment::Field(
                                #lua_key.to_owned(),
                            )],
                            reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                require: <#field_type as ::confit_binding::face::Face>::face_name(),
                                found: "nil".to_owned(),
                            },
                        },
                    );
                }
                let #field_ident =
                    <#field_type as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::confit_binding::fault::Segment::Field(#lua_key.to_owned()),
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
    let encode_keys = field_lua_keys.clone();
    let encode_shapes = field_shapes.clone();
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

            fn face() -> ::confit_binding::face::FaceData {
                ::confit_binding::face::FaceData::Class {
                    documentation: #class_docs,
                    fields: ::std::vec![#(#descriptors),*],
                }
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) {
                let data = Self::face();
                if collector.register(Self::face_name(), &data) {
                    data.collect_referenced(collector);
                }
            }
        }

        impl ::confit_binding::value::Decode for #identifier {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::fault::FaceFault> {
                let table = ::confit_binding::value::table_value(value)?;
                ::confit_binding::value::check_keys(
                    &table,
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
                #(
                    table.set(
                        #encode_keys,
                        <#encode_shapes as ::confit_binding::value::Encode>::encode(
                            &self.#fields,
                            lua,
                        )?,
                    )?;
                )*
                ::std::result::Result::Ok(::mlua::Value::Table(table))
            }
        }
    };
    Ok(InlinePayload {
        identifier,
        expansion,
        fields,
        lua_keys: field_lua_keys,
        shapes: field_shapes,
    })
}

/// Report whether one type is Option with inner content.
fn is_option_shape(ty: &syn::Type) -> bool {
    if let syn::Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Option";
    }
    false
}

/// Fold one CamelCase name into its snake case tag.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (index, letter) in name.chars().enumerate() {
        if letter.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(letter.to_ascii_lowercase());
        } else {
            out.push(letter);
        }
    }
    out
}

/// Build the derive expansion for one tagged enum.
pub(crate) fn expand_tagged_enum(
    name: &syn::Ident,
    data: &syn::DataEnum,
    attrs: &[Attribute],
    validate: &Option<syn::Path>,
    rename: &Option<String>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
    let type_docs = doc_lines(attrs).join(" ");
    let mut union_arms = Vec::new();
    let mut arms = Vec::new();
    let mut encodes = Vec::new();
    let mut tags = Vec::new();
    let mut payloads = Vec::new();
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: tagged faces need one variant at least"),
        ));
    }
    for variant in &data.variants {
        let variant_name = variant.ident.to_string();
        let ident = variant.ident.clone();
        let flags = parse_variant_flags(&outer, &variant_name, &variant.attrs)?;
        if flags.rename.is_some() || !flags.probe.is_empty() {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}.{variant_name}: tagged faces take tag alone"),
            ));
        }
        let tag = flags.tag.unwrap_or_else(|| snake_case(&variant_name));
        let docs = doc_lines(&variant.attrs).join(" ");
        if let Fields::Named(named) = &variant.fields {
            let payload = build_inline_payload(&display, variant, named)?;
            let identifier = payload.identifier.clone();
            let field_idents = payload.fields.clone();
            let field_keys = payload.lua_keys.clone();
            let field_shapes = payload.shapes.clone();
            payloads.push(payload.expansion);
            union_arms.push(quote! {
                ::confit_binding::face::UnionArm {
                    name: stringify!(#ident),
                    documentation: #docs,
                    face: ::std::borrow::Cow::Owned(::std::format!(
                        "{{ {}: {} }}",
                        #tag,
                        <#identifier as ::confit_binding::face::Face>::face_name(),
                    )),
                    fields: ::std::vec::Vec::new(),
                    collect: <#identifier as ::confit_binding::face::Face>::collect,
                }
            });
            if let Some(hook) = validate {
                arms.push(quote! {
                    #tag => {
                        let inner =
                            <#identifier as ::confit_binding::value::Decode>::decode(&inner, lua)
                                .map_err(|mut error| {
                                    error.segments.insert(
                                        0,
                                        ::confit_binding::fault::Segment::Field(#tag.to_owned()),
                                    );
                                    error
                                })?;
                        let result = Self::#ident {
                            #(#field_idents: inner.#field_idents),*
                        };
                        #hook(&result).map_err(|error: ::mlua::Error| {
                            ::confit_binding::fault::FaceFault {
                                segments: ::std::vec::Vec::new(),
                                reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                    require: ::std::format!("valid {}", #outer),
                                    found: error.to_string(),
                                },
                            }
                        })?;
                        ::std::result::Result::Ok(result)
                    },
                });
            } else {
                arms.push(quote! {
                    #tag => {
                        let inner =
                            <#identifier as ::confit_binding::value::Decode>::decode(&inner, lua)
                                .map_err(|mut error| {
                                    error.segments.insert(
                                        0,
                                        ::confit_binding::fault::Segment::Field(#tag.to_owned()),
                                    );
                                    error
                                })?;
                        ::std::result::Result::Ok(Self::#ident {
                            #(#field_idents: inner.#field_idents),*
                        })
                    },
                });
            }
            tags.push(tag.clone());
            encodes.push(quote! {
                Self::#ident { #(#field_idents),* } => {
                    let inner = lua.create_table()?;
                    #(
                        inner.set(
                            #field_keys,
                            <#field_shapes as ::confit_binding::value::Encode>::encode(
                                #field_idents,
                                lua,
                            )?,
                        )?;
                    )*
                    let table = lua.create_table()?;
                    table.set(#tag, inner)?;
                    ::std::result::Result::Ok(::mlua::Value::Table(table))
                },
            });
            continue;
        }
        let payload = payload_of(&outer, variant, "tagged")?;
        union_arms.push(quote! {
            ::confit_binding::face::UnionArm {
                name: stringify!(#ident),
                documentation: #docs,
                face: ::std::borrow::Cow::Owned(::std::format!(
                    "{{ {}: {} }}",
                    #tag,
                    <#payload as ::confit_binding::face::Face>::face_name(),
                )),
                fields: ::std::vec::Vec::new(),
                collect: <#payload as ::confit_binding::face::Face>::collect,
            }
        });
        encodes.push(quote! {
            Self::#ident(inner) => {
                let table = lua.create_table()?;
                table.set(
                    #tag,
                    <#payload as ::confit_binding::value::Encode>::encode(inner, lua)?,
                )?;
                ::std::result::Result::Ok(::mlua::Value::Table(table))
            },
        });
        if let Some(hook) = validate {
            arms.push(quote! {
                #tag => {
                    let result = Self::#ident(
                        <#payload as ::confit_binding::value::Decode>::decode(&inner, lua)
                            .map_err(|mut error| {
                                error.segments.insert(
                                    0,
                                    ::confit_binding::fault::Segment::Field(#tag.to_owned()),
                                );
                                error
                            })?,
                    );
                    #hook(&result).map_err(|error: ::mlua::Error| {
                        ::confit_binding::fault::FaceFault {
                            segments: ::std::vec::Vec::new(),
                            reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                require: ::std::format!("valid {}", #outer),
                                found: error.to_string(),
                            },
                        }
                    })?;
                    ::std::result::Result::Ok(result)
                },
            });
        } else {
            arms.push(quote! {
                #tag => ::std::result::Result::Ok(Self::#ident(
                    <#payload as ::confit_binding::value::Decode>::decode(&inner, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::confit_binding::fault::Segment::Field(#tag.to_owned()),
                            );
                            error
                        })?,
                )),
            });
        }
        tags.push(tag);
    }
    Ok(quote! {
        #(#payloads)*

        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                #display.to_owned()
            }

            fn face() -> ::confit_binding::face::FaceData {
                ::confit_binding::face::FaceData::Union {
                    documentation: #type_docs,
                    arms: ::std::vec![#(#union_arms),*],
                }
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) {
                let data = Self::face();
                if collector.register(Self::face_name(), &data) {
                    data.collect_referenced(collector);
                }
            }
        }

        impl ::confit_binding::value::Decode for #name {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::fault::FaceFault> {
                let table =
                    ::confit_binding::value::table_value(value)?;
                let (key, inner) =
                    ::confit_binding::value::single_entry(&table)?;
                match key.as_str() {
                    #(#arms)*
                    _ => ::std::result::Result::Err(::confit_binding::fault::FaceFault {
                        segments: ::std::vec::Vec::new(),
                        reason: ::confit_binding::fault::FaultReason::UnknownVariant {
                            found: ::std::format!("'{key}'"),
                            options: ::std::vec![#(#tags.to_owned()),*],
                        },
                    }),
                }
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
        expand_tagged_enum(&input.ident, data, &input.attrs, &None, rename)
            .unwrap()
            .to_string()
    }

    #[test]
    fn inline_variant_synthesizes_documented_payload_class() {
        let expanded = expand(
            r#"
            #[face(table, tagged)]
            pub enum TaggedCond {
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
        assert!(expanded.contains("TaggedCond_EnvEq"));
        assert!(expanded.contains("register"));
        assert!(expanded.contains("collect_referenced"));
        assert!(expanded.contains("Equality check"));
        assert!(expanded.contains("The variable name"));
        assert!(expanded.contains("env_eq"));
        assert!(expanded.contains("face"));
        assert!(expanded.contains("UnionArm"));
    }

    #[test]
    fn union_face_references_payload_class() {
        let expanded = expand(
            r#"
            #[face(table, tagged)]
            pub enum TaggedCond {
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
        assert!(expanded.contains("register"));
        assert!(expanded.contains("collect_referenced"));
        assert!(expanded.contains("TaggedCond_EnvEq"));
        assert!(expanded.contains("{}: {}"));
        assert!(!expanded.contains("{} = {}"));
        assert!(expanded.contains("face"));
    }

    #[test]
    fn tuple_arm_synthesizes_no_payload_class() {
        let expanded = expand(
            r#"
            #[face(table, tagged)]
            pub enum TaggedCond {
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
        assert!(!expanded.contains("TaggedCond_All"));
        assert!(expanded.contains("{}: {}"));
        assert!(!expanded.contains("{} = {}"));
    }
}
