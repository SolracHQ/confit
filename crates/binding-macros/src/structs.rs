//! Struct faces
//! Expands struct declarations into faces.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Meta, Type, ext::IdentExt, punctuated::Punctuated, token::Comma};

use super::parse::doc_lines;

/// Field flags parsing from one face attribute.
#[derive(Default, Debug)]
struct FieldFlags {
    optional: bool,
    opaque: bool,
}

/// Parse field flags from one field attribute list.
fn parse_field_flags(
    outer: &str,
    field: &str,
    attrs: &[Attribute],
) -> Result<FieldFlags, syn::Error> {
    let mut flags = FieldFlags::default();
    for attr in attrs {
        if !attr.path().is_ident("face") {
            continue;
        }
        let items = attr.parse_args_with(Punctuated::<Meta, Comma>::parse_terminated)?;
        for item in items {
            if !matches!(item, Meta::Path(_)) {
                return Err(syn::Error::new_spanned(
                    &item,
                    format!("{outer}.{field}: face field takes flags alone"),
                ));
            }
            let ident = item
                .path()
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            match ident.as_str() {
                "optional" => flags.optional = true,
                "opaque" => flags.opaque = true,
                _ => {
                    return Err(syn::Error::new_spanned(
                        &item,
                        format!(
                            "{outer}.{field}: unknown face option '{ident}'; \
                             want 'optional' or 'opaque'"
                        ),
                    ));
                }
            }
        }
    }
    Ok(flags)
}

/// Report whether one type reads as Option with inner content.
fn is_option(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Option";
    }
    false
}

/// Build the derive expansion for one table struct.
pub(crate) fn expand_struct_table(
    name: &syn::Ident,
    data: &syn::DataStruct,
    attrs: &[Attribute],
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let fields = match &data.fields {
        syn::Fields::Named(named) => &named.named,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                format!("{outer}: Face derive reads structs with named fields alone"),
            ));
        }
    };
    let struct_docs = doc_lines(attrs).join(" ");
    let mut descriptors = Vec::new();
    let mut decodes = Vec::new();
    let mut members = Vec::new();
    let mut idents = Vec::new();
    let mut keys = Vec::new();
    for field in fields {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}: Face derive reads structs with named fields alone"),
            ));
        };
        let field_name = ident.unraw().to_string();
        let flags = parse_field_flags(&outer, &field_name, &field.attrs)?;
        let ty = &field.ty;
        if flags.optional && !is_option(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "{outer}.{field_name}: face option 'optional' needs \
                     Option with inner content; wrap the field type or drop \
                     the option"
                ),
            ));
        }
        let optional = flags.optional;
        if !optional && is_option(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "{outer}.{field_name}: Option fields take \
                     #[face(optional)] alone"
                ),
            ));
        }
        let docs = doc_lines(&field.attrs).join(" ");
        let path = format!("{outer}.{field_name}");
        let flagged = if flags.opaque {
            format!("{path} (opaque)")
        } else {
            path.clone()
        };
        descriptors.push(quote! {
            ::confit_binding::face::FieldDescriptor {
                name: #field_name,
                documentation: #docs,
                face: ::std::borrow::Cow::Owned(
                    <#ty as ::confit_binding::face::Face>::face_name(),
                ),
                optional: #optional,
            }
        });
        if flags.optional {
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #outer, #field_name)?;
                let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    <#ty as ::std::default::Default>::default()
                } else {
                    <#ty as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: #flagged.to_owned(),
                            want: error.want,
                            found: error.found,
                        })?
                };
            });
        } else {
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #outer, #field_name)?;
                if matches!(raw_value, ::mlua::Value::Nil) {
                    return ::std::result::Result::Err(
                        ::confit_binding::value::Mismatch {
                            path: #path.to_owned(),
                            want: <#ty as ::confit_binding::face::Face>::face_name(),
                            found: "nil".to_owned(),
                        },
                    );
                }
                let #ident =
                    <#ty as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: #flagged.to_owned(),
                            want: error.want,
                            found: error.found,
                        })?;
            });
        }
        members.push(quote! {
            <#ty as ::confit_binding::face::Face>::collect(collector)?;
        });
        idents.push(ident);
        keys.push(field_name);
    }
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                stringify!(#name).to_owned()
            }

            fn face_stub() -> ::std::string::String {
                ::confit_binding::stub::emit_class(
                    stringify!(#name),
                    #struct_docs,
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

        impl ::confit_binding::value::Decode for #name {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::value::Mismatch> {
                let table = ::confit_binding::value::table_value(value, stringify!(#name))?;
                ::confit_binding::value::check_keys(
                    &table,
                    stringify!(#name),
                    &[#(#keys),*],
                )?;
                #(#decodes)*
                ::std::result::Result::Ok(Self {
                    #(#idents),*
                })
            }
        }
    })
}

/// Build the derive expansion for one userdata struct.
pub(crate) fn expand_struct_userdata(
    name: &syn::Ident,
    data: &syn::DataStruct,
    attrs: &[Attribute],
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let fields = match &data.fields {
        syn::Fields::Named(named) => &named.named,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                format!("{outer}: Face derive reads structs with named fields alone"),
            ));
        }
    };
    let struct_docs = doc_lines(attrs).join(" ");
    let mut members = Vec::new();
    for field in fields {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}: Face derive reads structs with named fields alone"),
            ));
        };
        let field_name = ident.unraw().to_string();
        if field.attrs.iter().any(|attr| attr.path().is_ident("face")) {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!("{outer}.{field_name}: face field options read table structs alone"),
            ));
        }
        let ty = &field.ty;
        members.push(quote! {
            <#ty as ::confit_binding::face::Face>::collect(collector)?;
        });
    }
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                stringify!(#name).to_owned()
            }

            fn face_stub() -> ::std::string::String {
                ::confit_binding::stub::emit_class(stringify!(#name), #struct_docs, &[])
            }

            fn kind() -> ::confit_binding::face::FaceKind {
                ::confit_binding::face::FaceKind::Userdata
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
                _lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::value::Mismatch> {
                let want =
                    ::std::format!("a {} handle", stringify!(#name));
                match value.as_userdata() {
                    ::std::option::Option::Some(found) => {
                        found.borrow::<Self>().map(|held| held.clone()).map_err(|_| {
                            ::confit_binding::value::Mismatch {
                                path: stringify!(#name).to_owned(),
                                want,
                                found: "foreign userdata".to_owned(),
                            }
                        })
                    }
                    ::std::option::Option::None => {
                        ::std::result::Result::Err(::confit_binding::value::Mismatch {
                            path: stringify!(#name).to_owned(),
                            want,
                            found: value.type_name().to_owned(),
                        })
                    }
                }
            }
        }
    })
}
