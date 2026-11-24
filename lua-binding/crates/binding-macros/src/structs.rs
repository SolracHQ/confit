//! Expands struct declarations into table, object, and proxy shapes.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Type, ext::IdentExt};

use super::parse::{FieldOptions, doc_lines, parse_field_flags};

/// Returns whether a type is the Lua trace owner.
fn is_trace(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "LuaTrace";
    }
    false
}

/// Returns whether a type is an `Option` with inner content.
fn is_option(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Option";
    }
    false
}

/// Returns whether field options carry a marker that access flags refuse.
fn has_foreign_field_markers(flags: &FieldOptions) -> bool {
    flags.optional
        || flags.opaque
        || flags.borrowed
        || flags.ignore
        || flags.flatten
        || flags.lua_trace
        || flags.default.is_some()
}

/// Renders the default expression.
fn default_expr(ty: &Type, default: &Option<syn::Path>) -> Tokens {
    match default {
        Some(path) => quote! { #path() },
        None => quote! { <#ty as ::std::default::Default>::default() },
    }
}

/// Renders the validate epilogue.
fn validate_epilogue(outer: &str, validate: &Option<syn::Path>) -> Tokens {
    match validate {
        Some(hook) => quote! {
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
        None => quote! { ::std::result::Result::Ok(result) },
    }
}

/// Builds the table struct expansion.
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
                format!("{outer}: structs with named fields are expected for Shape derive"),
            ));
        }
    };
    let struct_docs = doc_lines(attrs).join(" ");
    let mut field_shapes = Vec::new();
    let mut decodes = Vec::new();
    let mut encodes = Vec::new();
    let mut idents = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut flat: Option<(syn::Ident, Type)> = None;
    for field in fields {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}: structs with named fields are expected for Shape derive"),
            ));
        };
        let field_name = ident.unraw().to_string();
        let flags = parse_field_flags(&outer, &field_name, &field.attrs)?;
        if flags.lua_trace {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!("{outer}.{field_name}: lua_trace belongs to object structs"),
            ));
        }
        if flags.get || flags.set {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!(
                    "{outer}.{field_name}: shape options 'get' and 'set' belong to object structs"
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
                    format!("{outer}.{field_name}: flatten needs a plain type"),
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
        if flags.borrowed {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!(
                    "{outer}.{field_name}: function shapes are expected for shape option 'borrowed'"
                ),
            ));
        }
        if flags.optional && is_option(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "{outer}.{field_name}: optional is inferred for Option fields; \
                     drop #[shape(optional)]"
                ),
            ));
        }
        if flags.optional && !is_option(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "{outer}.{field_name}: shape option 'optional' needs \
                     Option with inner content; wrap the field type or drop \
                     the option"
                ),
            ));
        }
        if flags.default.is_some() && is_option(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                format!("{outer}.{field_name}: default needs a plain type"),
            ));
        }
        let lua_key = flags.name.clone().unwrap_or_else(|| field_name.clone());
        let docs = doc_lines(&field.attrs).join(" ");
        let shape_tokens = ty;
        let inferred = is_option(ty);
        if flags.default.is_some() || inferred {
            field_shapes.push(quote! {
                ::lua_binding::shape::Field {
                    name: #lua_key,
                    description: #docs,
                    shape: ::std::borrow::Cow::Owned(
                        ::lua_binding::stub::emit_optional(
                            &<#shape_tokens as ::lua_binding::shape::Shape>::name(),
                        ),
                    ),
                    optional: true,
                    collect: <#shape_tokens as ::lua_binding::shape::Shape>::collect,
                }
            });
        } else {
            let marked = flags.optional;
            field_shapes.push(quote! {
                ::lua_binding::shape::Field {
                    name: #lua_key,
                    description: #docs,
                    shape: ::std::borrow::Cow::Owned(
                        <#shape_tokens as ::lua_binding::shape::Shape>::name(),
                    ),
                    optional: #marked,
                    collect: <#shape_tokens as ::lua_binding::shape::Shape>::collect,
                }
            });
        }
        if flags.optional || inferred {
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::lua_binding::value::read_field(&table, #lua_key)?;
                let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    <#ty as ::std::default::Default>::default()
                } else {
                    <#ty as ::lua_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::lua_binding::error::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?
                };
            });
        } else if let Some(default) = flags.default.as_ref() {
            let fallback = default_expr(ty, default);
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::lua_binding::value::read_field(&table, #lua_key)?;
                let #ident = if matches!(raw_value, ::mlua::Value::Nil) {
                    #fallback
                } else {
                    <#ty as ::lua_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::lua_binding::error::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?
                };
            });
        } else {
            decodes.push(quote! {
                let raw_value: ::mlua::Value =
                    ::lua_binding::value::read_field(&table, #lua_key)?;
                if matches!(raw_value, ::mlua::Value::Nil) {
                    return ::std::result::Result::Err(
                        ::lua_binding::error::DecodeError {
                            segments: ::std::vec![::lua_binding::error::Segment::Field(
                                #lua_key.to_owned(),
                            )],
                            reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                                require: <#ty as ::lua_binding::shape::Shape>::name(),
                                found: "nil".to_owned(),
                            },
                        },
                    );
                }
                let #ident =
                    <#ty as ::lua_binding::value::Decode>::decode(&raw_value, lua)
                        .map_err(|mut error| {
                            error.segments.insert(
                                0,
                                ::lua_binding::error::Segment::Field(#lua_key.to_owned()),
                            );
                            error
                        })?;
            });
        }
        encodes.push(quote! {
            table.set(
                #lua_key,
                <#ty as ::lua_binding::value::Encode>::encode(&self.#ident, lua)?,
            )?;
        });
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
                let #binding: #shape = <#shape as ::lua_binding::value::Decode>::decode_ignored(
                    &::mlua::Value::Table(table.clone()),
                    lua,
                    &through,
                )?;
            });
            let flat_want = format!("{outer}: flattened shape encodes a table");
            encodes.push(quote! {
                match <#shape as ::lua_binding::value::Encode>::encode(
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
                ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                    Self::decode_ignored(value, lua, &[])
                }

                fn decode_ignored(
                    value: &::mlua::Value,
                    lua: &::mlua::Lua,
                    ignored: &[&str],
                ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                    let table = ::lua_binding::value::table_value(value)?;
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
            ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                Self::decode_ignored(value, lua, &[])
            }

            fn decode_ignored(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
                ignored: &[&str],
            ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                let table = ::lua_binding::value::table_value(value)?;
                ::lua_binding::value::check_keys_ignored(
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
            let shape = <Self as ::lua_binding::shape::Shape>::shape_data();
            let inner = <#shape as ::lua_binding::shape::Shape>::shape_data();
            if ::lua_binding::stub::add_crossed(
                collector,
                #display,
                &shape,
                &inner,
            ) {
                shape.collect_referenced(collector);
            }
        },
        None => quote! {
            let data = <Self as ::lua_binding::shape::Shape>::shape_data();
            if collector.add(<Self as ::lua_binding::shape::Shape>::name(), &data) {
                data.collect_referenced(collector);
            }
        },
    };
    Ok(quote! {
        impl ::lua_binding::shape::Shape for #name {
            fn name() -> ::std::string::String {
                #display.to_owned()
            }

            fn shape_data() -> ::lua_binding::shape::ShapeData {
                ::lua_binding::shape::ShapeData::Table {
                    description: #struct_docs,
                    fields: ::std::vec![#(#field_shapes),*],
                }
            }

            fn collect(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                #collect_body
            }
        }

        impl ::lua_binding::value::Decode for #name {
            #decode_impl
        }

        impl ::lua_binding::value::Encode for #name {
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

/// Builds the object struct expansion.
pub(crate) fn expand_struct_object(
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
                format!("{outer}: structs with named fields are expected for Shape derive"),
            ));
        }
    };
    let struct_docs = doc_lines(attrs).join(" ");
    let mut traces: Vec<syn::Ident> = Vec::new();
    let mut registrations: Vec<Tokens> = Vec::new();
    let mut metadata: Vec<Tokens> = Vec::new();
    for field in fields {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}: structs with named fields are expected for Shape derive"),
            ));
        };
        let field_name = ident.unraw().to_string();
        if field.attrs.iter().any(|attr| attr.path().is_ident("shape")) {
            let flags = parse_field_flags(&outer, &field_name, &field.attrs)?;
            if flags.get || flags.set {
                if has_foreign_field_markers(&flags) {
                    return Err(syn::Error::new_spanned(
                        &field.ident,
                        format!(
                            "{outer}.{field_name}: shape options 'get' and 'set' stay apart \
                             from other field markers"
                        ),
                    ));
                }
                let ty = &field.ty;
                let lua_key = flags.name.clone().unwrap_or_else(|| field_name.clone());
                let docs = doc_lines(&field.attrs).join(" ");
                if flags.get {
                    registrations.push(quote! {
                        fields.add_field_function_get(#lua_key, |lua, this| {
                            let this = this.borrow::<Self>()?;
                            <#ty as ::lua_binding::value::Encode>::encode(&this.#ident, lua)
                        });
                    });
                }
                if flags.set {
                    let failure = format!("{display}.{lua_key}: {{error}}");
                    registrations.push(quote! {
                        fields.add_field_function_set(#lua_key, |lua, this, value: ::mlua::Value| {
                            let mut this = this.borrow_mut::<Self>()?;
                            this.#ident =
                                <#ty as ::lua_binding::value::Decode>::decode(&value, lua)
                                    .map_err(|error| {
                                        ::mlua::Error::runtime(::std::format!(#failure))
                                    })?;
                            ::std::result::Result::Ok(())
                        });
                    });
                } else {
                    let message = format!("{display}.{lua_key}: field is read-only");
                    registrations.push(quote! {
                        fields.add_field_function_set(
                            #lua_key,
                            |_lua, _this, _value: ::mlua::Value| -> ::mlua::Result<()> {
                                ::std::result::Result::Err(::mlua::Error::runtime(#message))
                            },
                        );
                    });
                }
                let readable = flags.get;
                let writable = flags.set;
                metadata.push(quote! {
                    ::lua_binding::object::ObjectField {
                        name: #lua_key,
                        description: #docs,
                        shape: ::std::borrow::Cow::Owned(
                            <#ty as ::lua_binding::shape::Shape>::name(),
                        ),
                        readable: #readable,
                        writable: #writable,
                        collect: <#ty as ::lua_binding::shape::Shape>::collect,
                    }
                });
                continue;
            }
            if !flags.lua_trace {
                return Err(syn::Error::new_spanned(
                    &field.ident,
                    format!(
                        "{outer}.{field_name}: table structs are expected for shape field options"
                    ),
                ));
            }
            if !is_trace(&field.ty) {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    format!("{outer}.{field_name}: lua_trace needs a LuaTrace field"),
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
                    owned.#trace = ::lua_binding::trace::LuaTrace::capture(lua)
                        .unwrap_or_default();
                }
            })
            .collect();
        quote! {
            impl #name {
                /// Fills the trace field from the Lua call stack.
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
        impl ::lua_binding::shape::Shape for #name {
            fn name() -> ::std::string::String {
                #display.to_owned()
            }

            fn shape_data() -> ::lua_binding::shape::ShapeData {
                ::lua_binding::shape::ShapeData::Object {
                    description: #struct_docs,
                }
            }

            fn collect(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                <Self as ::lua_binding::object::Object>::collect(collector);
            }
        }

        impl ::lua_binding::object::ObjectFields for #name {
            fn add_fields<F: ::mlua::UserDataFields<Self>>(fields: &mut F) {
                #(#registrations)*
            }

            fn field_metadata() -> ::std::vec::Vec<::lua_binding::object::ObjectField> {
                ::std::vec![#(#metadata),*]
            }
        }

        impl ::lua_binding::value::Decode for #name {
            fn decode(
                value: &::mlua::Value,
                _lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                let want =
                    ::std::format!("a {} handle", stringify!(#name));
                match value.as_userdata() {
                    ::std::option::Option::Some(found) => {
                        let result = found.borrow::<Self>().map(|held| held.clone()).map_err(|_| {
                            ::lua_binding::error::DecodeError {
                                segments: ::std::vec::Vec::new(),
                                reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                                    require: want.clone(),
                                    found: "foreign userdata".to_owned(),
                                },
                            }
                        })?;
                        #epilogue
                    }
                    ::std::option::Option::None => {
                        ::std::result::Result::Err(::lua_binding::error::DecodeError {
                            segments: ::std::vec::Vec::new(),
                            reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                                require: want.clone(),
                                found: value.type_name().to_owned(),
                            },
                        })
                    }
                }
            }
        }

        impl ::lua_binding::value::Encode for #name {
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

/// Builds the proxy struct expansion.
pub(crate) fn expand_struct_proxy(
    name: &syn::Ident,
    proxy: &syn::Path,
) -> Result<Tokens, syn::Error> {
    Ok(quote! {
        impl ::lua_binding::shape::Shape for #name {
            fn name() -> ::std::string::String {
                <#proxy as ::lua_binding::shape::Shape>::name()
            }

            fn shape_data() -> ::lua_binding::shape::ShapeData {
                <#proxy as ::lua_binding::shape::Shape>::shape_data()
            }

            fn collect(collector: &mut ::lua_binding::generator::Generator) {
                <#proxy as ::lua_binding::shape::Shape>::collect(collector);
            }
        }

        impl ::lua_binding::value::Decode for #name {
            fn decode(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<Self, ::lua_binding::error::DecodeError> {
                let proxy = <#proxy as ::lua_binding::value::Decode>::decode(value, lua)?;
                <#name as ::lua_binding::value::Hooked<#proxy>>::decode(lua, proxy)
            }
        }

        impl ::lua_binding::value::Encode for #name {
            fn encode(
                &self,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<::mlua::Value, ::mlua::Error> {
                let proxy = <#name as ::lua_binding::value::Hooked<#proxy>>::encode(self, lua)?;
                <#proxy as ::lua_binding::value::Encode>::encode(&proxy, lua)
            }
        }
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

    fn expand_object(source: &str) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        expand_struct_object(&input.ident, data, &input.attrs, &None, &None, true)
            .unwrap()
            .to_string()
    }

    fn reject_object(source: &str) {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        assert!(
            expand_struct_object(&input.ident, data, &input.attrs, &None, &None, true).is_err(),
            "foreign shape fails the build"
        );
    }

    #[test]
    fn proxy_struct_wires_the_hook_impls() {
        let name: syn::Ident = syn::parse_quote!(Count);
        let proxy: syn::Path = syn::parse_quote!(String);
        let expanded = expand_struct_proxy(&name, &proxy).unwrap().to_string();
        assert!(expanded.contains("Hooked"), "the expansion calls the hook");
        assert!(
            expanded.contains("< String as"),
            "the expansion uses the proxy type"
        );
        assert!(
            expanded.contains("impl :: lua_binding :: shape :: Shape for Count"),
            "the expansion keeps the shape impl"
        );
        assert!(
            expanded.contains("impl :: lua_binding :: value :: Decode for Count"),
            "the expansion keeps the decode impl"
        );
        assert!(
            expanded.contains("impl :: lua_binding :: value :: Encode for Count"),
            "the expansion keeps the encode impl"
        );
    }

    #[test]
    fn plain_struct_threads_the_ignored_list() {
        let expanded = expand(
            r#"
            #[shape(table)]
            pub struct Table {
                /// The kept field.
                kept: String,
            }
            "#,
        );
        assert!(expanded.contains("decode_ignored"));
        assert!(expanded.contains("check_keys_ignored"));
        assert!(expanded.contains("shape"));
        assert!(expanded.contains("ShapeData :: Table"));
    }

    #[test]
    fn flattened_struct_merges_the_inner_table() {
        let expanded = expand(
            r#"
            #[shape(table)]
            pub struct Table {
                /// The outer field.
                path: String,
                /// The flattened body.
                #[shape(flatten)]
                body: Inner,
            }
            "#,
        );
        assert!(expanded.contains("decode_ignored"));
        assert!(expanded.contains("add_crossed"));
        assert!(expanded.contains("collect_referenced"));
        assert!(!expanded.contains("collector . collect"));
        assert!(expanded.contains("flattened shape encodes a table"));
        assert!(!expanded.contains("check_keys("));
        assert!(expanded.contains("shape"));
        assert!(expanded.contains("ShapeData :: Table"));
    }

    #[test]
    fn wrong_flattens_fail_the_build() {
        for option in ["optional", "default", "name = \"body\""] {
            reject(&format!(
                r#"
                #[shape(table)]
                pub struct Table {{
                    path: String,
                    #[shape(flatten, {option})]
                    body: Inner,
                }}
                "#,
            ));
        }
        reject(
            r#"
            #[shape(table)]
            pub struct Table {
                #[shape(flatten)]
                first: First,
                #[shape(flatten)]
                second: Second,
            }
            "#,
        );
        reject(
            r#"
            #[shape(table)]
            pub struct Table {
                path: String,
                #[shape(flatten)]
                body: Option<Inner>,
            }
            "#,
        );
    }

    #[test]
    fn trace_mark_on_tables_fails_the_build() {
        reject(
            r#"
            #[shape(table)]
            pub struct Table {
                name: String,
                #[shape(lua_trace)]
                trace: LuaTrace,
            }
            "#,
        );
    }

    #[test]
    fn traced_object_fills_the_marked_field() {
        let expanded = expand_object(
            r#"
            #[shape(traced, object)]
            pub struct Table {
                name: String,
                #[shape(lua_trace)]
                trace: LuaTrace,
            }
            "#,
        );
        assert!(expanded.contains("traced"));
        assert!(expanded.contains("capture"));
        assert!(expanded.contains("owned . trace"));
    }

    #[test]
    fn trace_mark_on_wrong_fields_fails_the_build() {
        reject_object(
            r#"
            #[shape(traced, object)]
            pub struct Table {
                name: String,
                #[shape(lua_trace)]
                trace: String,
            }
            "#,
        );
    }
}
