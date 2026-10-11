//! Tagged faces
//! Expands tagged enums into single-key unions.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::Attribute;

use super::parse::{ContainerKind, doc_lines, kind_tokens, parse_variant_flags, payload_of};

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
    kind: ContainerKind,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let type_docs = doc_lines(attrs).join(" ");
    let marker = kind_tokens(kind);
    let mut descriptors = Vec::new();
    let mut members = Vec::new();
    let mut arms = Vec::new();
    let mut tags = Vec::new();
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
        let payload = payload_of(&outer, variant, "tagged")?;
        let docs = doc_lines(&variant.attrs).join(" ");
        let path = format!("{outer}.{tag}");
        descriptors.push(quote! {
            ::confit_binding::face::VariantDescriptor {
                name: stringify!(#ident),
                documentation: #docs,
                face: ::std::borrow::Cow::Owned(::std::format!(
                    "{{ {} = {} }}",
                    #tag,
                    <#payload as ::confit_binding::face::Face>::face_name(),
                )),
            }
        });
        members.push(quote! {
            <#payload as ::confit_binding::face::Face>::collect(collector)?;
        });
        arms.push(quote! {
            #tag => ::std::result::Result::Ok(Self::#ident(
                <#payload as ::confit_binding::value::Decode>::decode(&inner, lua)
                    .map_err(|error| ::confit_binding::value::Mismatch {
                        path: #path.to_owned(),
                        want: error.want,
                        found: error.found,
                    })?,
            )),
        });
        tags.push(tag);
    }
    let want = format!("one of {}", tags.join(", "));
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
                let table =
                    ::confit_binding::value::table_value(value, stringify!(#name))?;
                let (key, inner) =
                    ::confit_binding::value::single_entry(stringify!(#name), &table)?;
                match key.as_str() {
                    #(#arms)*
                    _ => ::std::result::Result::Err(::confit_binding::value::Mismatch {
                        path: stringify!(#name).to_owned(),
                        want: #want.to_owned(),
                        found: ::std::format!("unknown tag '{key}'"),
                    }),
                }
            }
        }
    })
}
