//! Literal faces
//! Expand string enums into literal unions.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Fields};

use super::parse::{ContainerKind, doc_lines, kind_tokens, parse_variant_flags};

/// Build the derive expansion for one string enum.
pub(crate) fn expand_string_enum(
    name: &syn::Ident,
    data: &syn::DataEnum,
    attrs: &[Attribute],
    kind: ContainerKind,
    validate: &Option<syn::Path>,
    rename: &Option<String>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
    let type_docs = doc_lines(attrs).join(" ");
    let marker = kind_tokens(kind);
    let mut descriptors = Vec::new();
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
        descriptors.push(quote! {
            ::confit_binding::face::VariantDescriptor {
                name: stringify!(#ident),
                documentation: #docs,
                face: ::std::borrow::Cow::Borrowed(#quoted),
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
                        ::confit_binding::value::Mismatch {
                            path: stringify!(#name).to_owned(),
                            want: #want.to_owned(),
                            found: ::std::format!("string \"{text}\""),
                        },
                    );
                }
            };
            #hook(&result).map_err(|error: ::mlua::Error| {
                ::confit_binding::value::Mismatch {
                    path: #outer.to_owned(),
                    want: ::std::format!("valid {}", #outer),
                    found: error.to_string(),
                }
            })?;
            ::std::result::Result::Ok(result)
        },
        None => quote! {
            match text.to_lowercase().as_str() {
                #(#lowers => ::std::result::Result::Ok(Self::#idents),)*
                _ => ::std::result::Result::Err(::confit_binding::value::Mismatch {
                    path: stringify!(#name).to_owned(),
                    want: #want.to_owned(),
                    found: ::std::format!("string \"{text}\""),
                }),
            }
        },
    };
    Ok(quote! {
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
                collector.register(Self::face_name(), Self::face_stub());
                ::std::result::Result::Ok(())
            }
        }

        impl ::confit_binding::value::Decode for #name {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::value::Mismatch> {
                let text = <::std::string::String as ::confit_binding::value::Decode>::decode(
                    value,
                    lua,
                )
                .map_err(|error| {
                    ::confit_binding::value::Mismatch {
                        path: stringify!(#name).to_owned(),
                        want: #want.to_owned(),
                        found: error.found,
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
