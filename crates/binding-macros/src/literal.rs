//! Literal faces
//! Expand string enums into literal unions.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Fields};

use super::parse::{doc_lines, parse_variant_flags};

/// Build the derive expansion for one string enum.
pub(crate) fn expand_string_enum(
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
                format!("{outer}.{variant_name}: unit variants are expected for string faces"),
            ));
        }
        let flags = parse_variant_flags(&outer, &variant_name, &variant.attrs)?;
        if flags.tag.is_some() || !flags.probe.is_empty() {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}.{variant_name}: string faces take rename alone"),
            ));
        }
        let literal = flags.rename.unwrap_or_else(|| variant_name.to_lowercase());
        let quoted = format!("\"{literal}\"");
        let lowered = literal.to_lowercase();
        let docs = doc_lines(&variant.attrs).join(" ");
        let ident = variant.ident.clone();
        arms.push(quote! {
            ::confit_binding::face::UnionArm {
                name: stringify!(#ident),
                documentation: #docs,
                face: ::std::borrow::Cow::Borrowed(#quoted),
                fields: ::std::vec::Vec::new(),
                collect: ::confit_binding::face::no_collect,
            }
        });
        literals.push(literal);
        lowers.push(lowered);
        idents.push(ident);
    }
    if idents.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: string faces need one variant at least"),
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
                        ::confit_binding::fault::FaceFault {
                            segments: ::std::vec::Vec::new(),
                            reason: ::confit_binding::fault::FaultReason::UnknownVariant {
                                found: ::std::format!("\"{text}\""),
                                options: ::std::vec![#(#literals.to_owned()),*],
                            },
                        },
                    );
                }
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
        None => quote! {
            match text.to_lowercase().as_str() {
                #(#lowers => ::std::result::Result::Ok(Self::#idents),)*
                _ => ::std::result::Result::Err(::confit_binding::fault::FaceFault {
                    segments: ::std::vec::Vec::new(),
                    reason: ::confit_binding::fault::FaultReason::UnknownVariant {
                        found: ::std::format!("\"{text}\""),
                        options: ::std::vec![#(#literals.to_owned()),*],
                    },
                }),
            }
        },
    };
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                #display.to_owned()
            }

            fn face() -> ::confit_binding::face::FaceData {
                ::confit_binding::face::FaceData::Union {
                    documentation: #type_docs,
                    arms: ::std::vec![#(#arms),*],
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
                let text = <::std::string::String as ::confit_binding::value::Decode>::decode(
                    value,
                    lua,
                )
                .map_err(|error| {
                    let found = match error.reason {
                        ::confit_binding::fault::FaultReason::ShapeMismatch { found, .. } => found,
                        ::confit_binding::fault::FaultReason::UnknownVariant {
                            found,
                            ..
                        } => found,
                    };
                    ::confit_binding::fault::FaceFault {
                        segments: ::std::vec::Vec::new(),
                        reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                            require: #want.to_owned(),
                            found,
                        },
                    }
                })?;
                #validate_arm
            }
        }

        impl ::confit_binding::value::Encode for #name {
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
        expand_string_enum(&input.ident, data, &input.attrs, &None, &None)
            .unwrap()
            .to_string()
    }

    #[test]
    fn literal_arms_emit_methods() {
        let expanded = expand(
            r#"
            #[face(table, string)]
            pub enum Shade {
                /// Dark tone.
                Dark,
                /// Light tone.
                Light,
            }
            "#,
        );
        assert!(expanded.contains("register"));
        assert!(expanded.contains("collect_referenced"));
        assert!(expanded.contains("face"));
        assert!(expanded.contains("UnionArm"));
        assert!(expanded.contains("no_collect"));
    }
}
