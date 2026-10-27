//! Struct faces
//! Expand struct declarations into faces.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Type, ext::IdentExt};

use super::parse::{doc_lines, parse_field_flags};

/// Report whether one type is the Lua trace holder.
fn is_trace(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "LuaTrace";
    }
    false
}

/// Report whether one type is Option with inner content.
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
        format!("{outer}.{field}: HashMap or BTreeMap fields are expected for map converts"),
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
                format!("{outer}: structs with named fields are expected for Face derive"),
            ));
        }
    };
    let struct_docs = doc_lines(attrs).join(" ");
    let mut descriptors = Vec::new();
    let mut decodes = Vec::new();
    let mut encodes = Vec::new();
    let mut extras = Vec::new();
    let mut idents = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut flat: Option<(syn::Ident, Type)> = None;
    for field in fields {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}: structs with named fields are expected for Face derive"),
            ));
        };
        let field_name = ident.unraw().to_string();
        let flags = parse_field_flags(&outer, &field_name, &field.attrs)?;
        if flags.lua_trace {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!(
                    "{outer}.{field_name}: face option 'lua_trace' needs userdata structs alone"
                ),
            ));
        }
        let ty = &field.ty;
        if flags.flatten {
            if flat.is_some() {
                return Err(syn::Error::new_spanned(
                    &field.ident,
                    format!("{outer}.{field_name}: structs hold one flattened field at most"),
                ));
            }
            if is_option(ty) {
                return Err(syn::Error::new_spanned(
                    ty,
                    format!(
                        "{outer}.{field_name}: face option 'flatten' expected plain \
                         types alone"
                    ),
                ));
            }
            flat = Some((ident, ty.clone()));
            continue;
        }
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
                format!(
                    "{outer}.{field_name}: callback shapes are expected for face option 'live'"
                ),
            ));
        }
        if flags.optional && is_option(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "{outer}.{field_name}: optional is inferred for Option fields; \
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
                    "{outer}.{field_name}: face option 'default' expected plain \
                     types alone"
                ),
            ));
        }
        if flags.decode_hook.is_some() && flags.proxy.is_none() {
            if flags.optional || flags.default.is_some() || flags.rename.is_some() {
                return Err(syn::Error::new_spanned(
                    &field.ident,
                    format!(
                        "{outer}.{field_name}: sibling decode_hook expected the table \
                         alone with no key options"
                    ),
                ));
            }
            let decode_hook = flags.decode_hook.as_ref();
            decodes.push(quote! {
                let #ident: #ty = #decode_hook(lua, &table)?;
            });
            extras.push(quote! {
                <#ty as ::confit_binding::face::Face>::collect(collector);
            });
            idents.push(ident);
            continue;
        }
        if flags.key_convert.is_some() || flags.value_convert.is_some() {
            if flags.proxy.is_some()
                || flags.decode_hook.is_some()
                || flags.encode_hook.is_some()
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
                    format!("{outer}.{field_name}: plain maps are expected for map converts"),
                ));
            }
            let lua_key = flags.rename.clone().unwrap_or_else(|| field_name.clone());
            let docs = doc_lines(&field.attrs).join(" ");
            let (key_ty, value_ty) = map_parts(&outer, &field_name, ty, &field.ident)?;
            let decode_key: Tokens = match flags.key_convert.as_ref() {
                Some(convert) => quote! {
                    #convert(lua, &key_value).map_err(|mut error| {
                        error.segments.insert(
                            0,
                            ::confit_binding::fault::Segment::Field(#lua_key.to_owned()),
                        );
                        error
                    })?
                },
                None => quote! {
                    <#key_ty as ::confit_binding::value::Decode>::decode(&key_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::confit_binding::fault::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?
                },
            };
            let decode_item: Tokens = match flags.value_convert.as_ref() {
                Some(convert) => quote! {
                    #convert(lua, &item_value).map_err(|mut error| {
                        error.segments.insert(
                            0,
                            ::confit_binding::fault::Segment::Field(#lua_key.to_owned()),
                        );
                        error
                    })?
                },
                None => quote! {
                    <#value_ty as ::confit_binding::value::Decode>::decode(&item_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::confit_binding::fault::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?
                },
            };
            let decode_map = quote! {
                let source = ::confit_binding::value::table_value(&raw_value)?;
                let mut mapped: #ty = <#ty as ::std::default::Default>::default();
                for pair in source.pairs::<::mlua::Value, ::mlua::Value>() {
                    let (key_value, item_value) = pair.map_err(|error| {
                        ::confit_binding::fault::FaceFault {
                            segments: ::std::vec::Vec::new(),
                            reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                require: "readable entries".to_owned(),
                                found: error.to_string(),
                            },
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
                        collect: <#ty as ::confit_binding::face::Face>::collect,
                    }
                });
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #lua_key)?;
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
                        collect: <#ty as ::confit_binding::face::Face>::collect,
                    }
                });
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #lua_key)?;
                    if matches!(raw_value, ::mlua::Value::Nil) {
                        return ::std::result::Result::Err(
                            ::confit_binding::fault::FaceFault {
                                segments: ::std::vec![::confit_binding::fault::Segment::Field(
                                    #lua_key.to_owned(),
                                )],
                                reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                    require: <#ty as ::confit_binding::face::Face>::face_name(),
                                    found: "nil".to_owned(),
                                },
                            },
                        );
                    }
                    let #ident: #ty = #decode_map;
                });
            }
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
                    collect: <#face_tokens as ::confit_binding::face::Face>::collect,
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
                    collect: <#face_tokens as ::confit_binding::face::Face>::collect,
                }
            });
        }
        if let Some(decode_hook) = flags.decode_hook.as_ref() {
            if flags.optional || inferred {
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #lua_key)?;
                    let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                        <#ty as ::std::default::Default>::default()
                    } else {
                        ::confit_binding::value::decode_proxy(
                            lua,
                            &raw_value,
                            #lua_key,
                            #decode_hook,
                        )?
                    };
                });
            } else if let Some(default) = flags.default.as_ref() {
                let fallback = default_expr(ty, default);
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #lua_key)?;
                    let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                        #fallback
                    } else {
                        ::confit_binding::value::decode_proxy(
                            lua,
                            &raw_value,
                            #lua_key,
                            #decode_hook,
                        )?
                    };
                });
            } else {
                decodes.push(quote! {
                    let raw_value: ::mlua::Value =
                        ::confit_binding::value::read_field(&table, #lua_key)?;
                    if matches!(raw_value, ::mlua::Value::Nil) {
                        return ::std::result::Result::Err(
                            ::confit_binding::fault::FaceFault {
                                segments: ::std::vec![::confit_binding::fault::Segment::Field(
                                    #lua_key.to_owned(),
                                )],
                                reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                    require: <#face_tokens as ::confit_binding::face::Face>::face_name(),
                                    found: "nil".to_owned(),
                                },
                            },
                        );
                    }
                    let #ident = ::confit_binding::value::decode_proxy(
                        lua,
                        &raw_value,
                        #lua_key,
                        #decode_hook,
                    )?;
                });
            }
        } else if flags.optional || inferred {
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #lua_key)?;
                let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    <#ty as ::std::default::Default>::default()
                } else {
                    <#ty as ::confit_binding::value::Decode>::decode(&raw_value, lua)
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
            let fallback = default_expr(ty, default);
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #lua_key)?;
                let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    #fallback
                } else {
                    <#ty as ::confit_binding::value::Decode>::decode(&raw_value, lua)
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
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::confit_binding::value::read_field(&table, #lua_key)?;
                if matches!(raw_value, ::mlua::Value::Nil) {
                    return ::std::result::Result::Err(
                        ::confit_binding::fault::FaceFault {
                            segments: ::std::vec![::confit_binding::fault::Segment::Field(
                                #lua_key.to_owned(),
                            )],
                            reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                require: <#ty as ::confit_binding::face::Face>::face_name(),
                                found: "nil".to_owned(),
                            },
                        },
                    );
                }
                let #ident =
                    <#ty as ::confit_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::confit_binding::fault::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?;
            });
        }
        if let Some(hook) = flags.encode_hook.as_ref() {
            encodes.push(quote! {
                table.set(#lua_key, #hook(lua, &self.#ident)?)?;
            });
        } else {
            encodes.push(quote! {
                table.set(
                    #lua_key,
                    <#ty as ::confit_binding::value::Encode>::encode(&self.#ident, lua)?,
                )?;
            });
        }
        idents.push(ident);
        if !keys.iter().any(|key| key == &lua_key) {
            keys.push(lua_key);
        }
    }
    let epilogue = validate_epilogue(&outer, validate);
    let decode_impl = match flat.as_ref() {
        Some((binding, shape)) => {
            decodes.push(quote! {
                let mut through: ::std::vec::Vec<&str> = ignored.to_vec();
                through.extend([#(#keys),*]);
                let #binding: #shape = <#shape as ::confit_binding::value::Decode>::decode_ignored(
                    &::mlua::Value::Table(table.clone()),
                    lua,
                    &through,
                )?;
            });
            let flat_want = format!("{outer}: flattened face encodes a table");
            encodes.push(quote! {
                match <#shape as ::confit_binding::value::Encode>::encode(
                    &self.#binding,
                    lua,
                )? {
                    ::mlua::Value::Table(inner) => {
                        for pair in inner.pairs::<::mlua::Value, ::mlua::Value>() {
                            let (key, item) = pair?;
                            table.set(key, item)?;
                        }
                    }
                    ::mlua::Value::Nil => {}
                    _ => {
                        return ::std::result::Result::Err(::mlua::Error::RuntimeError(
                            #flat_want.to_owned(),
                        ));
                    }
                }
            });
            quote! {
                fn decode(
                    value: &::mlua::Value,
                    lua: &::mlua::Lua,
                ) -> ::std::result::Result<Self, ::confit_binding::fault::FaceFault> {
                    Self::decode_ignored(value, lua, &[])
                }

                fn decode_ignored(
                    value: &::mlua::Value,
                    lua: &::mlua::Lua,
                    ignored: &[&str],
                ) -> ::std::result::Result<Self, ::confit_binding::fault::FaceFault> {
                    let table = ::confit_binding::value::table_value(value)?;
                    #(#decodes)*
                    let result = Self {
                        #(#idents,)*
                        #binding
                    };
                    #epilogue
                }
            }
        }
        None => quote! {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::fault::FaceFault> {
                Self::decode_ignored(value, lua, &[])
            }

            fn decode_ignored(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
                ignored: &[&str],
            ) -> ::std::result::Result<Self, ::confit_binding::fault::FaceFault> {
                let table = ::confit_binding::value::table_value(value)?;
                ::confit_binding::value::check_keys_ignored(
                    &table,
                    &[#(#keys),*],
                    ignored,
                )?;
                #(#decodes)*
                let result = Self {
                    #(#idents),*
                };
                #epilogue
            }
        },
    };
    let collect_body = match flat.as_ref() {
        Some((_, shape)) => quote! {
            let holder = Self::face();
            let inner = <#shape as ::confit_binding::face::Face>::face();
            if ::confit_binding::stub::register_crossed(
                collector,
                #display,
                &holder,
                &inner,
            ) {
                holder.collect_referenced(collector);
                #(#extras)*
            }
        },
        None => quote! {
            let data = Self::face();
            if collector.register(Self::face_name(), &data) {
                data.collect_referenced(collector);
                #(#extras)*
            }
        },
    };
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                #display.to_owned()
            }

            fn face() -> ::confit_binding::face::FaceData {
                ::confit_binding::face::FaceData::Class {
                    documentation: #struct_docs,
                    fields: ::std::vec![#(#descriptors),*],
                }
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) {
                #collect_body
            }
        }

        impl ::confit_binding::value::Decode for #name {
            #decode_impl
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
    traced: bool,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
    let fields = match &data.fields {
        syn::Fields::Named(named) => &named.named,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                format!("{outer}: structs with named fields are expected for Face derive"),
            ));
        }
    };
    let struct_docs = doc_lines(attrs).join(" ");
    let mut traces: Vec<syn::Ident> = Vec::new();
    for field in fields {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}: structs with named fields are expected for Face derive"),
            ));
        };
        let field_name = ident.unraw().to_string();
        if field.attrs.iter().any(|attr| attr.path().is_ident("face")) {
            let flags = parse_field_flags(&outer, &field_name, &field.attrs)?;
            if !flags.lua_trace {
                return Err(syn::Error::new_spanned(
                    &field.ident,
                    format!(
                        "{outer}.{field_name}: table structs are expected for face field options"
                    ),
                ));
            }
            if !is_trace(&field.ty) {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    format!(
                        "{outer}.{field_name}: face option 'lua_trace' expected LuaTrace fields alone"
                    ),
                ));
            }
            traces.push(ident);
        }
    }
    let epilogue = validate_epilogue(&outer, validate);
    let traced_impl = if traced {
        let fills: Vec<Tokens> = traces
            .iter()
            .map(|trace| {
                quote! {
                    owned.#trace = ::confit_binding::trace::LuaTrace::capture(lua)
                        .unwrap_or_default();
                }
            })
            .collect();
        quote! {
            impl #name {
                /// Fill the trace field from the Lua call stack.
                pub fn traced(lua: &::mlua::Lua, mut owned: Self) -> Self {
                    #(#fills)*
                    owned
                }
            }
        }
    } else {
        quote! {}
    };
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                #display.to_owned()
            }

            fn face() -> ::confit_binding::face::FaceData {
                ::confit_binding::face::FaceData::Userdata {
                    documentation: #struct_docs,
                }
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) {
                <Self as ::confit_binding::userdata::UserdataClass>::collect(collector);
            }
        }

        impl ::confit_binding::value::Decode for #name {
            fn decode(
                value: &::mlua::Value,
                _lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::confit_binding::fault::FaceFault> {
                let want =
                    ::std::format!("a {} handle", stringify!(#name));
                match value.as_userdata() {
                    ::std::option::Option::Some(found) => {
                        let result = found.borrow::<Self>().map(|held| held.clone()).map_err(|_| {
                            ::confit_binding::fault::FaceFault {
                                segments: ::std::vec::Vec::new(),
                                reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                    require: want.clone(),
                                    found: "foreign userdata".to_owned(),
                                },
                            }
                        })?;
                        #epilogue
                    }
                    ::std::option::Option::None => {
                        ::std::result::Result::Err(::confit_binding::fault::FaceFault {
                            segments: ::std::vec::Vec::new(),
                            reason: ::confit_binding::fault::FaultReason::ShapeMismatch {
                                require: want.clone(),
                                found: value.type_name().to_owned(),
                            },
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

        #traced_impl
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(source: &str) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        expand_struct_table(&input.ident, data, &input.attrs, &None, &None)
            .unwrap()
            .to_string()
    }

    fn reject(source: &str) {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        assert!(
            expand_struct_table(&input.ident, data, &input.attrs, &None, &None).is_err(),
            "foreign shape fails the build"
        );
    }

    fn expand_userdata(source: &str) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        expand_struct_userdata(&input.ident, data, &input.attrs, &None, &None, true)
            .unwrap()
            .to_string()
    }

    fn reject_userdata(source: &str) {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        assert!(
            expand_struct_userdata(&input.ident, data, &input.attrs, &None, &None, true).is_err(),
            "foreign shape fails the build"
        );
    }

    #[test]
    fn plain_struct_threads_the_ignored_list() {
        let expanded = expand(
            r#"
            #[face(table)]
            pub struct Holder {
                /// The kept field.
                kept: String,
            }
            "#,
        );
        assert!(expanded.contains("decode_ignored"));
        assert!(expanded.contains("check_keys_ignored"));
        assert!(expanded.contains("face"));
        assert!(expanded.contains("FaceData :: Class"));
    }

    #[test]
    fn flattened_struct_merges_the_inner_table() {
        let expanded = expand(
            r#"
            #[face(table)]
            pub struct Holder {
                /// The outer field.
                path: String,
                /// The flattened body.
                #[face(flatten)]
                body: Inner,
            }
            "#,
        );
        assert!(expanded.contains("decode_ignored"));
        assert!(expanded.contains("register_crossed"));
        assert!(expanded.contains("collect_referenced"));
        assert!(!expanded.contains("collector . collect"));
        assert!(expanded.contains("flattened face encodes a table"));
        assert!(!expanded.contains("check_keys("));
        assert!(expanded.contains("face"));
        assert!(expanded.contains("FaceData :: Class"));
    }

    #[test]
    fn foreign_flattens_fail_the_build() {
        for option in ["optional", "default", "rename = \"body\""] {
            reject(&format!(
                r#"
                #[face(table)]
                pub struct Holder {{
                    path: String,
                    #[face(flatten, {option})]
                    body: Inner,
                }}
                "#,
            ));
        }
        reject(
            r#"
            #[face(table)]
            pub struct Holder {
                #[face(flatten)]
                first: First,
                #[face(flatten)]
                second: Second,
            }
            "#,
        );
        reject(
            r#"
            #[face(table)]
            pub struct Holder {
                path: String,
                #[face(flatten)]
                body: Option<Inner>,
            }
            "#,
        );
    }

    #[test]
    fn trace_mark_on_tables_fails_the_build() {
        reject(
            r#"
            #[face(table)]
            pub struct Holder {
                name: String,
                #[face(lua_trace)]
                trace: LuaTrace,
            }
            "#,
        );
    }

    #[test]
    fn traced_userdata_fills_the_marked_field() {
        let expanded = expand_userdata(
            r#"
            #[face(traced, userdata)]
            pub struct Holder {
                name: String,
                #[face(lua_trace)]
                trace: LuaTrace,
            }
            "#,
        );
        assert!(expanded.contains("traced"));
        assert!(expanded.contains("capture"));
        assert!(expanded.contains("owned . trace"));
    }

    #[test]
    fn trace_mark_on_foreign_fields_fails_the_build() {
        reject_userdata(
            r#"
            #[face(traced, userdata)]
            pub struct Holder {
                name: String,
                #[face(lua_trace)]
                trace: String,
            }
            "#,
        );
    }
}
