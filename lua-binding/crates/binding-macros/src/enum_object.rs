//! Expands object enums into opaque Lua object shapes.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Fields};

use super::parse::doc_lines;

/// Builds the object enum expansion.
///
/// The object stays opaque and exposes its variants through methods. Method
/// blocks dispatch on the held enum value. Stubs describe the object and its
/// methods.
///
/// # Errors
///
/// - `syn::Error` for empty enums, struct variants, and variant shape options.
pub(crate) fn expand_enum_object(
    name: &syn::Ident,
    data: &syn::DataEnum,
    attrs: &[Attribute],
    validate: &Option<syn::Path>,
    rename: &Option<String>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    let display = rename.clone().unwrap_or_else(|| outer.clone());
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: object shapes need one variant at least"),
        ));
    }
    let type_docs = doc_lines(attrs).join(" ");
    for variant in &data.variants {
        let variant_name = variant.ident.to_string();
        if variant
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("shape"))
        {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}.{variant_name}: userdata variants take no shape options"),
            ));
        }
        if matches!(&variant.fields, Fields::Named(_)) {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!(
                    "{outer}.{variant_name}: object shapes take unit or \
                     tuple variants"
                ),
            ));
        }
    }
    let epilogue = validate_epilogue(&outer, validate);
    Ok(quote! {
        impl ::lua_binding::shape::Shape for #name {
            fn name() -> ::std::string::String {
                #display.to_owned()
            }

            fn shape_data() -> ::lua_binding::shape::ShapeData {
                ::lua_binding::shape::ShapeData::Object {
                    description: #type_docs,
                }
            }

            fn collect(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                <Self as ::lua_binding::object::Object>::collect(collector);
            }
        }

        impl ::lua_binding::object::ObjectFields for #name {
            fn add_fields<F: ::mlua::UserDataFields<Self>>(_fields: &mut F) {}

            fn field_metadata() -> ::std::vec::Vec<::lua_binding::object::ObjectField> {
                ::std::vec::Vec::new()
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
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Expands an object enum source.
    fn expand(source: &str) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Enum(data) = &input.data else {
            panic!("want enum input");
        };
        expand_enum_object(&input.ident, data, &input.attrs, &None, &None)
            .unwrap()
            .to_string()
    }

    /// Rejects a wrong object shape.
    fn reject(source: &str) {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Enum(data) = &input.data else {
            panic!("want enum input");
        };
        assert!(
            expand_enum_object(&input.ident, data, &input.attrs, &None, &None).is_err(),
            "foreign shape fails the build"
        );
    }

    #[test]
    fn object_payload_shapes_collect() {
        let expanded = expand(
            r#"
            #[shape(object)]
            pub enum Handle {
                /// Cached entry.
                Cached(String),
                /// Empty marker.
                Nothing,
            }
            "#,
        );
        assert!(expanded.contains("ShapeData :: Object"));
        assert!(expanded.contains("object :: Object"));
        assert!(expanded.contains("foreign userdata"));
    }

    #[test]
    fn wrong_object_shapes_fail_the_build() {
        reject(
            r#"
            #[shape(object)]
            pub enum Empty {}
            "#,
        );
        reject(
            r#"
            #[shape(object)]
            pub enum Shaped {
                Inline { content: String },
            }
            "#,
        );
    }
}
