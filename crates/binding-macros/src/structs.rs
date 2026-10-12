//! Struct faces
//! Expands struct declarations into faces.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Type, ext::IdentExt};

use super::parse::{doc_lines, parse_field_flags};

/// Report whether one type reads as Option with inner content.
fn is_option(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Option";
    }
    false
}

/// Render the default expression for one absent Lua key.
fn default_expr(ty: &Type, default: &Option<syn::Path>) -> Tokens {
    match default {
        Some(path) => quote! { #path() },
        None => quote! { <#ty as ::std::default::Default>::default() },
    }
}

/// Render the validate epilogue for one decoded struct value.
fn validate_epilogue(outer: &str, validate: &Option<syn::Path>) -> Tokens {
    match validate {
        Some(hook) => quote! {
            #hook(&result).map_err(|error: ::mlua::Error| {
                ::confit_binding::value::Mismatch {
                    path: #outer.to_owned(),
                    want: ::std::format!("valid {}", #outer),
                    found: error.to_string(),
                }
            })?;
            ::std::result::Result::Ok(result)
        },
        None => quote! { ::std::result::Result::Ok(result) },
    }
}

/// Collect the key with value types of one map field.
///
/// # Errors
///
/// - `syn::Error` for types outside the HashMap and BTreeMap shapes.
fn map_parts(
    outer: &str,
    field: &str,
    ty: &Type,
    span: &Option<syn::Ident>,
) -> Result<(Type, Type), syn::Error> {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
        && (last.ident == "HashMap" || last.ident == "BTreeMap")
        && let syn::PathArguments::AngleBracketed(args) = &last.arguments
        && args.args.len() == 2
    {
        let mut inner = args.args.iter();
        if let (Some(syn::GenericArgument::Type(key)), Some(syn::GenericArgument::Type(item))) =
            (inner.next(), inner.next())
        {
            return Ok((key.clone(), item.clone()));
        }
    }
    Err(syn::Error::new_spanned(
        span,
        format!("{outer}.{field}: map converts read HashMap or BTreeMap fields alone"),
    ))
}

/// Build the derive expansion for one table struct.
pub(crate) fn expand_struct_table(
    name: &syn::Ident,
    data: &syn::DataStruct,
    attrs: &[Attribute],
    validate: &Option<syn::Path>,
    rename: &Option<String>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
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
    let mut encodes = Vec::new();
    let mut members = Vec::new();
    let mut idents = Vec::new();
    let mut keys: Vec<String> = Vec::new();
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
        if flags.ignore {
            decodes.push(quote! {
                let #ident: #ty = <#ty as ::std::default::Default>::default();
            });
            idents.push(ident);
            continue;
        }
        if flags.live {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!("{outer}.{field_name}: face option 'live' reads callback shapes alone"),
            ));
        }
        if flags.optional && is_option(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "{outer}.{field_name}: Option fields read optional alone; \
                     drop #[face(optional)]"
                ),
            ));
        }
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
        if flags.default.is_some() && is_option(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "{outer}.{field_name}: face option 'default' reads plain \
                     types alone"
                ),
            ));
        }
        if flags.convert.is_some() && flags.proxy.is_none() {
            if flags.optional || flags.default.is_some() || flags.rename.is_some() {
                return Err(syn::Error::new_spanned(
                    &field.ident,
                    format!(
                        "{outer}.{field_name}: sibling convert reads the table \
                         alone with no key options"
                    ),
                ));
            }
            let path = format!("{outer}.{field_name}");
            let flagged = if flags.opaque {
                format!("{path} (opaque)")
            } else {
                path.clone()
            };
            let convert = flags.convert.as_ref();
            decodes.push(quote! {
                let #ident: #ty = #convert(lua, &table).map_err(|error| {
                    ::confit_binding::value::Mismatch {
                        path: #flagged.to_owned(),
                        want: error.want,
                        found: error.found,
                    }
                })?;
            });
            members.push(quote! {
                <#ty as ::confit_binding::face::Face>::collect(collector)?;
            });
            idents.push(ident);
            continue;
        }
        if flags.key_convert.is_some() || flags.value_convert.is_some() {
            if flags.proxy.is_some()
                || flags.convert.is_some()
                || flags.encode.is_some()
                || flags.opaque
            {
                return Err(syn::Error::new_spanned(
                    &field.ident,
                    format!(
                        "{outer}.{field_name}: map converts read key_convert with \
                         value_convert alone"
                    ),
                ));
            }
            if is_option(ty) {
                return Err(syn::Error::new_spanned(
                    ty,
                    format!("{outer}.{field_name}: map converts read plain maps alone"),
                ));
            }
            let lua_key = flags.rename.clone().unwrap_or_else(|| field_name.clone());
            let docs = doc_lines(&field.attrs).join(" ");
            let path = format!("{outer}.{lua_key}");
            let (key_ty, value_ty) = map_parts(&outer, &field_name, ty, &field.ident)?;
            let decode_key: Tokens = match flags.key_convert.as_ref() {
                Some(convert) => quote! {
                    #convert(lua, &key_value).map_err(|error| {
                        ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #path, error.path),
                            want: error.want,
                            found: error.found,
                        }
                    })?
                },
                None => quote! {
                    <#key_ty as ::confit_binding::value::Decode>::decode(&key_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #path, error.path),
                            want: error.want,
                            found: error.found,
                        })?
                },
            };
            let decode_item: Tokens = match flags.value_convert.as_ref() {
                Some(convert) => quote! {
                    #convert(lua, &item_value).map_err(|error| {
                        ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #path, error.path),
                            want: error.want,
                            found: error.found,
                        }
                    })?
                },
                None => quote! {
                    <#value_ty as ::confit_binding::value::Decode>::decode(&item_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #path, error.path),
                            want: error.want,
                            found: error.found,
                        })?
                },
            };
            let decode_map = quote! {
                let source = ::confit_binding::value::table_value(&raw_value, #path)?;
                let mut mapped: #ty = <#ty as ::std::default::Default>::default();
                for pair in source.pairs::<::mlua::Value, ::mlua::Value>() {
                    let (key_value, item_value) = pair.map_err(|error| {
                        ::confit_binding::value::Mismatch {
                            path: #path.to_owned(),
                            want: "readable entries".to_owned(),
                            found: error.to_string(),
                        }
                    })?;
                    let key: #key_ty = #decode_key;
                    let item: #value_ty = #decode_item;
                    mapped.insert(key, item);
                }
                mapped
            };
            if flags.default.is_some() || flags.optional {
                let fallback = match flags.default.as_ref() {
                    Some(path) => default_expr(ty, path),
                    None => quote! { <#ty as ::std::default::Default>::default() },
                };
                descriptors.push(quote! {
                    ::confit_binding::face::FieldDescriptor {
                        name: #lua_key,
                        documentation: #docs,
                        face: ::std::borrow::Cow::Owned(
                            ::confit_binding::stub::emit_optional(
                                &<#ty as ::confit_binding::face::Face>::face_name(),
                            ),
                        ),
                        optional: true,
                    }
                });
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                    let #ident: #ty = if matches!(raw_value, ::mlua::Value::Nil) {
                        #fallback
                    } else {
                        #decode_map
                    };
                });
            } else {
                descriptors.push(quote! {
                    ::confit_binding::face::FieldDescriptor {
                        name: #lua_key,
                        documentation: #docs,
                        face: ::std::borrow::Cow::Owned(
                            <#ty as ::confit_binding::face::Face>::face_name(),
                        ),
                        optional: false,
                    }
                });
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                    if matches!(raw_value, ::mlua::Value::Nil) {
                        return ::std::result::Result::Err(
                            ::confit_binding::value::Mismatch {
                                path: #path.to_owned(),
                                want: <#ty as ::confit_binding::face::Face>::face_name(),
                                found: "nil".to_owned(),
                            },
                        );
                    }
                    let #ident: #ty = #decode_map;
                });
            }
            members.push(quote! {
                <#key_ty as ::confit_binding::face::Face>::collect(collector)?;
                <#value_ty as ::confit_binding::face::Face>::collect(collector)?;
            });
            encodes.push(quote! {
                table.set(
                    #lua_key,
                    <#ty as ::confit_binding::value::Encode>::encode(&self.#ident, lua)?,
                )?;
            });
            idents.push(ident);
            if !keys.iter().any(|key| key == &lua_key) {
                keys.push(lua_key);
            }
            continue;
        }
        let lua_key = flags.rename.clone().unwrap_or_else(|| field_name.clone());
        let docs = doc_lines(&field.attrs).join(" ");
        let path = format!("{outer}.{lua_key}");
        let flagged = if flags.opaque {
            format!("{path} (opaque)")
        } else {
            path.clone()
        };
        let face_tokens: Tokens = match flags.proxy.as_ref() {
            Some(proxy) => quote! { #proxy },
            None => quote! { #ty },
        };
        let inferred = is_option(ty);
        if flags.default.is_some() || inferred {
            descriptors.push(quote! {
                ::confit_binding::face::FieldDescriptor {
                    name: #lua_key,
                    documentation: #docs,
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
                    documentation: #docs,
                    face: ::std::borrow::Cow::Owned(
                        <#face_tokens as ::confit_binding::face::Face>::face_name(),
                    ),
                    optional: #marked,
                }
            });
        }
        if let Some(convert) = flags.convert.as_ref() {
            if flags.optional || inferred {
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                    let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                        <#ty as ::std::default::Default>::default()
                    } else {
                        ::confit_binding::value::decode_proxy(
                            lua,
                            &raw_value,
                            #flagged,
                            #convert,
                        )?
                    };
                });
            } else if let Some(default) = flags.default.as_ref() {
                let fallback = default_expr(ty, default);
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                    let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                        #fallback
                    } else {
                        ::confit_binding::value::decode_proxy(
                            lua,
                            &raw_value,
                            #flagged,
                            #convert,
                        )?
                    };
                });
            } else {
                decodes.push(quote! {
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
                    let #ident = ::confit_binding::value::decode_proxy(
                        lua,
                        &raw_value,
                        #flagged,
                        #convert,
                    )?;
                });
            }
        } else if flags.optional || inferred {
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    <#ty as ::std::default::Default>::default()
                } else {
                    <#ty as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #flagged, error.path),
                            want: error.want,
                            found: error.found,
                        })?
                };
            });
        } else if let Some(default) = flags.default.as_ref() {
            let fallback = default_expr(ty, default);
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
                let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    #fallback
                } else {
                    <#ty as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|error| ::confit_binding::value::Mismatch {
                            path: ::std::format!("{}: {}", #flagged, error.path),
                            want: error.want,
                            found: error.found,
                        })?
                };
            });
        } else {
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #outer, #lua_key)?;
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
                            path: ::std::format!("{}: {}", #flagged, error.path),
                            want: error.want,
                            found: error.found,
                        })?;
            });
        }
        members.push(quote! {
            <#face_tokens as ::confit_binding::face::Face>::collect(collector)?;
        });
        match flags.encode.as_ref() {
            Some(encode) => encodes.push(quote! {
                table.set(
                    #lua_key,
                    lua.create_string(#encode(self.#ident.clone()))?,
                )?;
            }),
            None => encodes.push(quote! {
                table.set(
                    #lua_key,
                    <#ty as ::confit_binding::value::Encode>::encode(&self.#ident, lua)?,
                )?;
            }),
        }
        idents.push(ident);
        if !keys.iter().any(|key| key == &lua_key) {
            keys.push(lua_key);
        }
    }
    let epilogue = validate_epilogue(&outer, validate);
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                #display.to_owned()
            }

            fn face_stub() -> ::std::string::String {
                ::confit_binding::stub::emit_class(
                    #display,
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
                let result = Self {
                    #(#idents),*
                };
                #epilogue
            }
        }

        impl ::confit_binding::value::Encode for #name {
            fn encode(
                &self,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<::mlua::Value, ::mlua::Error> {
                let table = lua.create_table()?;
                #(#encodes)*
                ::std::result::Result::Ok(::mlua::Value::Table(table))
            }
        }
    })
}

/// Build the derive expansion for one userdata struct.
pub(crate) fn expand_struct_userdata(
    name: &syn::Ident,
    data: &syn::DataStruct,
    attrs: &[Attribute],
    validate: &Option<syn::Path>,
    rename: &Option<String>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
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
    }
    let epilogue = validate_epilogue(&outer, validate);
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                #display.to_owned()
            }

            fn face_stub() -> ::std::string::String {
                ::confit_binding::stub::emit_class(#display, #struct_docs, &[])
            }

            fn kind() -> ::confit_binding::face::FaceKind {
                ::confit_binding::face::FaceKind::Userdata
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) -> ::confit_binding::error::Result<()> {
                <Self as ::confit_binding::userdata::UserdataClass>::collect(collector)
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
                        let result = found.borrow::<Self>().map(|held| held.clone()).map_err(|_| {
                            ::confit_binding::value::Mismatch {
                                path: stringify!(#name).to_owned(),
                                want: want.clone(),
                                found: "foreign userdata".to_owned(),
                            }
                        })?;
                        #epilogue
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

        impl ::confit_binding::value::Encode for #name {
            fn encode(
                &self,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<::mlua::Value, ::mlua::Error> {
                lua.create_userdata(self.clone()).map(::mlua::Value::UserData)
            }
        }
    })
}
