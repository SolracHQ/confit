//! Enum userdata
//! Expand userdata enums into opaque handle classes.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Fields};

use super::parse::doc_lines;

/// Build the derive expansion for one userdata enum.
///
/// - Enums stay opaque with docs and methods alone.
/// - Method blocks dispatch on the held enum value.
/// - Stubs stay opaque: variant shapes never reach Lua docs.
///
/// # Errors
///
/// - `syn::Error` for empty enums, struct variants, and variant face options.
pub(crate) fn expand_enum_userdata(
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
            format!("{outer}: userdata faces need one variant at least"),
        ));
    }
    let type_docs = doc_lines(attrs).join(" ");
    for variant in &data.variants {
        let variant_name = variant.ident.to_string();
        if variant
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("face"))
        {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!("{outer}.{variant_name}: userdata variants take no face options"),
            ));
        }
        if matches!(&variant.fields, Fields::Named(_)) {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!(
                    "{outer}.{variant_name}: userdata faces read unit with \
                     tuple variants alone"
                ),
            ));
        }
    }
    let epilogue = validate_epilogue(&outer, validate);
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                #display.to_owned()
            }

            fn face() -> ::confit_binding::face::FaceData {
                ::confit_binding::face::FaceData::Userdata {
                    documentation: #type_docs,
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
    })
}

/// Render the validate epilogue for one decoded userdata value.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Expand one userdata enum source.
    fn expand(source: &str) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Enum(data) = &input.data else {
            panic!("want enum input");
        };
        expand_enum_userdata(&input.ident, data, &input.attrs, &None, &None)
            .unwrap()
            .to_string()
    }

    /// Reject one foreign userdata shape.
    fn reject(source: &str) {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Enum(data) = &input.data else {
            panic!("want enum input");
        };
        assert!(
            expand_enum_userdata(&input.ident, data, &input.attrs, &None, &None).is_err(),
            "foreign shape fails the build"
        );
    }

    #[test]
    fn payload_faces_collect() {
        let expanded = expand(
            r#"
            #[face(userdata)]
            pub enum Handle {
                /// Cached entry.
                Cached(String),
                /// Empty marker.
                Nothing,
            }
            "#,
        );
        assert!(expanded.contains("FaceData :: Userdata"));
        assert!(expanded.contains("UserdataClass"));
        assert!(expanded.contains("foreign userdata"));
    }

    #[test]
    fn foreign_shapes_fail_the_build() {
        reject(
            r#"
            #[face(userdata)]
            pub enum Empty {}
            "#,
        );
        reject(
            r#"
            #[face(userdata)]
            pub enum Shaped {
                Inline { content: String },
            }
            "#,
        );
        reject(
            r#"
            #[face(userdata)]
            pub enum Flagged {
                /// Cached entry.
                #[face(skip_decode)]
                Cached(String),
            }
            "#,
        );
    }
}
