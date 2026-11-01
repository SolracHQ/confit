//! Expands string enums into literal aliases.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Fields};

use super::parse::{doc_lines, parse_variant_flags};

/// Builds the string alias enum expansion.
pub(crate) fn expand_str_alias_enum(
    name: &syn::Ident,
    data: &syn::DataEnum,
    attrs: &[Attribute],
    validate: &Option<syn::Path>,
    rename: &Option<String>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
    let type_docs = doc_lines(attrs).join(" ");
    let mut arms = Vec::new();
    let mut literals = Vec::new();
    let mut lowers = Vec::new();
    let mut idents = Vec::new();
    for variant in &data.variants {
        let variant_name = variant.ident.to_string();
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}.{variant_name}: unit variants are expected for string shapes"),
            ));
        }
        let flags = parse_variant_flags(&outer, &variant_name, &variant.attrs)?;
        if flags.variant.is_some() || !flags.probe.is_empty() {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!(
                    "{outer}.{variant_name}: string shapes take name; variant and probe belong to other markers"
                ),
            ));
        }
        let literal = flags.name.unwrap_or_else(|| variant_name.to_lowercase());
        let quoted = format!("\"{literal}\"");
        let lowered = literal.to_lowercase();
        let docs = doc_lines(&variant.attrs).join(" ");
        let ident = variant.ident.clone();
        arms.push(quote! {
            ::lua_binding::shape::Variant {
                name: stringify!(#ident),
                description: #docs,
                shape: ::std::borrow::Cow::Borrowed(#quoted),
                fields: ::std::vec::Vec::new(),
                collect: ::lua_binding::shape::no_collect,
            }
        });
        literals.push(literal);
        lowers.push(lowered);
        idents.push(ident);
    }
    if idents.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: string shapes need one variant at least"),
        ));
    }
    let want = literals
        .iter()
        .map(|literal| format!("\"{literal}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let outer = name.to_string();
    let validate_arm: Tokens = match validate {
        Some(hook) => quote! {
            let result = match text.to_lowercase().as_str() {
                #(#lowers => Self::#idents,)*
                _ => {
                    return ::std::result::Result::Err(
                        ::lua_binding::error::DecodeError {
                            segments: ::std::vec::Vec::new(),
                            reason: ::lua_binding::error::DecodeTag::UnknownVariant {
                                found: ::std::format!("\"{text}\""),
                                options: ::std::vec![#(#literals.to_owned()),*],
                            },
                        },
                    );
                }
            };
            #hook(&result).map_err(|error: ::mlua::Error| {
                ::lua_binding::error::DecodeError {
                    segments: ::std::vec::Vec::new(),
                    reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                        require: ::std::format!("valid {}", #outer),
                        found: error.to_string(),
                    },
                }
            })?;
            ::std::result::Result::Ok(result)
        },
        None => quote! {
            match text.to_lowercase().as_str() {
                #(#lowers => ::std::result::Result::Ok(Self::#idents),)*
                _ => ::std::result::Result::Err(::lua_binding::error::DecodeError {
                    segments: ::std::vec::Vec::new(),
                    reason: ::lua_binding::error::DecodeTag::UnknownVariant {
                        found: ::std::format!("\"{text}\""),
                        options: ::std::vec![#(#literals.to_owned()),*],
                    },
                }),
            }
        },
    };
    Ok(quote! {
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
                let text = <::std::string::String as ::lua_binding::value::Decode>::decode(
                    value,
                    lua,
                )
                .map_err(|error| {
                    let found = match error.reason {
                        ::lua_binding::error::DecodeTag::ShapeMismatch { found, .. } => found,
                        ::lua_binding::error::DecodeTag::UnknownVariant {
                            found,
                            ..
                        } => found,
                    };
                    ::lua_binding::error::DecodeError {
                        segments: ::std::vec::Vec::new(),
                        reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                            require: #want.to_owned(),
                            found,
                        },
                    }
                })?;
                #validate_arm
            }
        }

        impl ::lua_binding::value::Encode for #name {
            fn encode(
                &self,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<::mlua::Value, ::mlua::Error> {
                let text = match self {
                    #(Self::#idents => #literals,)*
                };
                lua.create_string(text).map(::mlua::Value::String)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(source: &str) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Enum(data) = &input.data else {
            panic!("want enum input");
        };
        expand_str_alias_enum(&input.ident, data, &input.attrs, &None, &None)
            .unwrap()
            .to_string()
    }

    #[test]
    fn str_alias_variants_emit_methods() {
        let expanded = expand(
            r#"
            #[shape(table, str_alias)]
            pub enum Shade {
                /// Dark tone.
                Dark,
                /// Light tone.
                Light,
            }
            "#,
        );
        assert!(expanded.contains("add"));
        assert!(expanded.contains("collect_referenced"));
        assert!(expanded.contains("shape"));
        assert!(expanded.contains("Variant"));
        assert!(expanded.contains("no_collect"));
    }
}
