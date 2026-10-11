//! Probe faces
//! Expands probe enums into marker-key unions.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::Attribute;

use super::parse::{ContainerKind, doc_lines, kind_tokens, parse_variant_flags, payload_of};

/// Build the derive expansion for one probe enum.
pub(crate) fn expand_probe_enum(
    name: &syn::Ident,
    data: &syn::DataEnum,
    attrs: &[Attribute],
    kind: ContainerKind,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let type_docs = doc_lines(attrs).join(" ");
    let marker = kind_tokens(kind);
    let count = data.variants.len();
    if count == 0 {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: probe faces need one variant at least"),
        ));
    }
    let mut descriptors = Vec::new();
    let mut faces = Vec::new();
    let mut members = Vec::new();
    let mut dispatch = Vec::new();
    for (index, variant) in data.variants.iter().enumerate() {
        let variant_name = variant.ident.to_string();
        let ident = variant.ident.clone();
        let flags = parse_variant_flags(&outer, &variant_name, &variant.attrs)?;
        if flags.rename.is_some() || flags.tag.is_some() {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}.{variant_name}: probe faces take probe alone"),
            ));
        }
        if flags.probe.is_empty() && index + 1 != count {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}: probe fallback reads last position alone"),
            ));
        }
        let payload = payload_of(&outer, variant, "probe")?;
        let docs = doc_lines(&variant.attrs).join(" ");
        let path = format!("{outer}.{variant_name}");
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
        let last = index + 1 == count;
        if flags.probe.is_empty() {
            dispatch.push(quote! {
                return <#payload as ::confit_binding::value::Decode>::decode(
                    &::mlua::Value::Table(table),
                    lua,
                )
                .map(Self::#ident)
                .map_err(|error| ::confit_binding::value::Mismatch {
                    path: #path.to_owned(),
                    want: error.want,
                    found: error.found,
                });
            });
        } else {
            let mut check: Tokens = {
                let first = &flags.probe[0];
                quote! { probe(#first) }
            };
            for key in flags.probe.iter().skip(1) {
                check = quote! { #check && probe(#key) };
            }
            let table_value = if last {
                quote! { &::mlua::Value::Table(table) }
            } else {
                quote! { &::mlua::Value::Table(table.clone()) }
            };
            dispatch.push(quote! {
                if #check {
                    return <#payload as ::confit_binding::value::Decode>::decode(
                        #table_value,
                        lua,
                    )
                    .map(Self::#ident);
                }
            });
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
                let table = <::mlua::Table as ::mlua::FromLua>::from_lua(
                    value.clone(),
                    lua,
                )
                .map_err(|_| {
                    ::confit_binding::value::Mismatch {
                        path: stringify!(#name).to_owned(),
                        want: "a table".to_owned(),
                        found: value.type_name().to_owned(),
                    }
                })?;
                let probe = |key: &str| -> bool {
                    table
                        .get::<::mlua::Value>(key)
                        .is_ok_and(|item| !matches!(item, ::mlua::Value::Nil))
                };
                #(#dispatch)*
                ::std::result::Result::Err(::confit_binding::value::Mismatch {
                    path: stringify!(#name).to_owned(),
                    want: [#(#faces),*].join(" | "),
                    found: "an unknown shape".to_owned(),
                })
            }
        }
    })
}
