//! Untagged faces
//! Expands untagged enums into ordered unions.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Fields};

use super::parse::{ContainerKind, doc_lines, kind_tokens, parse_variant_flags, payload_of};

/// Build the derive expansion for one untagged enum.
pub(crate) fn expand_untagged_enum(
    name: &syn::Ident,
    data: &syn::DataEnum,
    attrs: &[Attribute],
    kind: ContainerKind,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let type_docs = doc_lines(attrs).join(" ");
    let marker = kind_tokens(kind);
    let mut descriptors = Vec::new();
    let mut faces = Vec::new();
    let mut members = Vec::new();
    let mut attempts = Vec::new();
    let mut units = 0usize;
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: untagged faces need one variant at least"),
        ));
    }
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
                attempts.push(quote! {
                    if matches!(value, ::mlua::Value::Nil) {
                        return ::std::result::Result::Ok(Self::#ident);
                    }
                });
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
                attempts.push(quote! {
                    if let ::std::result::Result::Ok(inner) =
                        <#payload as ::confit_binding::value::Decode>::decode(
                            &value.clone(),
                            lua,
                        )
                    {
                        return ::std::result::Result::Ok(Self::#ident(inner));
                    }
                });
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    &variant.ident,
                    format!(
                        "{outer}.{variant_name}: untagged faces read unit or \
                         single-payload tuple variants alone"
                    ),
                ));
            }
        }
    }
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                stringify!(#name).to_owned()
            }

            fn face_stub() -> ::std::string::String {
                ::confit_binding::stub::emit_alias(
                    stringify!(#name),
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
                #(#attempts)*
                ::std::result::Result::Err(::confit_binding::value::Mismatch {
                    path: stringify!(#name).to_owned(),
                    want: [#(#faces),*].join(" | "),
                    found: value.type_name().to_owned(),
                })
            }
        }
    })
}
