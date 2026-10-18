//! Callback faces
//! Expand callback shape declarations into function faces.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Type, ext::IdentExt};

use super::parse::{doc_lines, parse_field_flags};

/// One callback parameter with its call shape.
struct CallbackField {
    /// The parameter binding.
    binding: syn::Ident,
    /// The parameter label.
    label: String,
    /// The parameter documentation.
    documentation: String,
    /// The parameter face.
    shape: Type,
    /// The live table marker.
    live: bool,
}

/// Report whether one type is a Lua function.
fn is_function_ty(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Function";
    }
    false
}

/// Collect the documented parameters of one callback shape.
fn collect_fields(
    outer: &str,
    name: &syn::Ident,
    data: &syn::DataStruct,
) -> Result<Vec<CallbackField>, syn::Error> {
    let fields = match &data.fields {
        syn::Fields::Named(named) => &named.named,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                format!("{outer}: structs with named fields are expected for Face derive"),
            ));
        }
    };
    let mut collected = Vec::new();
    for field in fields {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}: structs with named fields are expected for Face derive"),
            ));
        };
        let field_name = ident.unraw().to_string();
        let flags = parse_field_flags(outer, &field_name, &field.attrs)?;
        if flags.optional
            || flags.opaque
            || flags.ignore
            || flags.flatten
            || flags.proxy.is_some()
            || flags.decode_hook.is_some()
            || flags.encode_hook.is_some()
            || flags.rename.is_some()
            || flags.default.is_some()
        {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!(
                    "{outer}.{field_name}: plain or live fields are expected for callback shapes"
                ),
            ));
        }
        if is_function_ty(&field.ty) {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}.{field_name}: callback shapes hold params alone"),
            ));
        }
        let documentation = doc_lines(&field.attrs).join(" ");
        if documentation.is_empty() {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!("{outer}.{field_name}: callback params carry /// docs"),
            ));
        }
        collected.push(CallbackField {
            binding: ident,
            label: field_name,
            documentation,
            shape: field.ty.clone(),
            live: flags.live,
        });
    }
    Ok(collected)
}

/// Build the derive expansion for one callback shape.
pub(crate) fn expand_struct_callback(
    name: &syn::Ident,
    data: &syn::DataStruct,
    attrs: &[Attribute],
    validate: &Option<syn::Path>,
    rename: &Option<String>,
    returns: &Option<syn::Path>,
) -> Result<Tokens, syn::Error> {
    let outer = name.to_string();
    if validate.is_some() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: callback shapes do not support validate hooks"),
        ));
    }
    if rename.is_some() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: callback shapes do not support rename"),
        ));
    }
    let struct_docs = doc_lines(attrs).join(" ");
    let fields = collect_fields(&outer, name, data)?;
    let labels: Vec<String> = fields.iter().map(|field| field.label.clone()).collect();
    let documentations: Vec<String> = fields
        .iter()
        .map(|field| field.documentation.clone())
        .collect();
    let faces: Vec<Type> = fields.iter().map(|field| field.shape.clone()).collect();
    let count = fields.len();
    let want_arity = if count == 1 {
        "a function with 1 param".to_owned()
    } else {
        format!("a function with {count} params")
    };
    let mut parameters = Vec::new();
    let mut bindings = Vec::new();
    let mut pushes = Vec::new();
    for field in &fields {
        let binding = &field.binding;
        let shape = &field.shape;
        bindings.push(quote! { #binding });
        if field.live {
            parameters.push(quote! { ::mlua::Table });
            pushes.push(quote! {
                arguments.push(::mlua::Value::Table(#binding));
            });
        } else {
            parameters.push(quote! { &'args #shape });
            pushes.push(quote! {
                arguments.push(
                    <#shape as ::confit_binding::value::Encode>::encode(#binding, lua)?,
                );
            });
        }
    }
    let return_type: Tokens = match returns {
        Some(path) => quote! { #path },
        None => quote! { () },
    };
    let returns_data: Tokens = match returns {
        Some(path) => quote! {
            ::std::vec![::confit_binding::face::FunctionReturn {
                face: ::std::borrow::Cow::Owned(
                    <#path as ::confit_binding::face::Face>::face_name(),
                ),
                documentation: "",
                collect: <#path as ::confit_binding::face::Face>::collect,
            }]
        },
        None => quote! { ::std::vec::Vec::new() },
    };
    let call_face_body: Tokens = match returns {
        Some(path) => quote! {
            ::std::format!(
                "fun({}): {}",
                [#(::std::format!(
                    "{}: {}",
                    #labels,
                    <#faces as ::confit_binding::face::Face>::face_name()
                )),*].join(", "),
                <#path as ::confit_binding::face::Face>::face_name()
            )
        },
        None => quote! {
            ::std::format!(
                "fun({})",
                [#(::std::format!(
                    "{}: {}",
                    #labels,
                    <#faces as ::confit_binding::face::Face>::face_name()
                )),*].join(", ")
            )
        },
    };
    Ok(quote! {
        impl ::confit_binding::face::Face for #name {
            fn face_name() -> ::std::string::String {
                stringify!(#name).to_owned()
            }

            fn face() -> ::confit_binding::face::FaceData {
                ::confit_binding::face::FaceData::Function {
                    documentation: #struct_docs,
                    parameters: ::std::vec![
                        #(::confit_binding::face::FunctionParameter {
                            name: #labels,
                            documentation: #documentations,
                            face: ::std::borrow::Cow::Owned(
                                <#faces as ::confit_binding::face::Face>::face_name(),
                            ),
                            optional: false,
                            collect: <#faces as ::confit_binding::face::Face>::collect,
                        }),*
                    ],
                    returns: #returns_data,
                }
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) -> ::confit_binding::error::Result<()> {
                let data = Self::face();
                if collector.register_callback(
                    <Self as ::confit_binding::callback::CallbackArgs>::call_face(),
                    &data,
                ) {
                    data.collect_referenced(collector)?;
                }
                ::std::result::Result::Ok(())
            }
        }

        impl ::confit_binding::callback::CallbackArgs for #name {
            type Parameters<'args> = (#(#parameters,)*);
            type Return = #return_type;

            fn arity() -> ::std::primitive::usize {
                #count
            }

            fn call_face() -> ::std::string::String {
                #call_face_body
            }

            fn decode_function(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<::mlua::Function, ::confit_binding::value::Mismatch>
            {
                match value {
                    ::mlua::Value::Function(found) => {
                        let info = found.info();
                        if !info.is_vararg && info.num_params as ::std::primitive::usize != #count
                        {
                            return ::std::result::Result::Err(
                                ::confit_binding::value::Mismatch {
                                    path: #outer.to_owned(),
                                    want: #want_arity.to_owned(),
                                    found: ::std::format!(
                                        "a function with {} param{}",
                                        info.num_params,
                                        if info.num_params == 1 { "" } else { "s" },
                                    ),
                                },
                            );
                        }
                        ::std::result::Result::Ok(found.clone())
                    }
                    _ => ::std::result::Result::Err(::confit_binding::value::Mismatch {
                        path: #outer.to_owned(),
                        want: <#name as ::confit_binding::face::Face>::face_name(),
                        found: value.type_name().to_owned(),
                    }),
                }
            }

            fn invoke(
                function: &::mlua::Function,
                lua: &::mlua::Lua,
                parameters: Self::Parameters<'_>,
            ) -> ::mlua::Result<::mlua::Value> {
                let (#(#bindings,)*) = parameters;
                let mut arguments: ::std::vec::Vec<::mlua::Value> =
                    ::std::vec::Vec::new();
                #(#pushes)*
                function.call(::mlua::Variadic::from(arguments))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand_with(source: &str, validate: &Option<syn::Path>) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        expand_struct_callback(&input.ident, data, &input.attrs, validate, &None, &None)
            .unwrap()
            .to_string()
    }

    fn expand_returns(source: &str, returns: &Option<syn::Path>) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        expand_struct_callback(&input.ident, data, &input.attrs, &None, &None, returns)
            .unwrap()
            .to_string()
    }

    fn expand(source: &str) -> String {
        expand_with(source, &None)
    }

    fn reject(source: &str) {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        assert!(
            expand_struct_callback(&input.ident, data, &input.attrs, &None, &None, &None).is_err(),
            "foreign shape fails the build"
        );
    }

    #[test]
    fn shape_renders_function_stub_with_named_params() {
        let expanded = expand(
            r#"
            #[face(callback_shape)]
            pub struct ApplyArgs {
                /// Target route of the patched document.
                route: String,
                /// Live document under patching.
                #[face(live)]
                document: Live,
            }
            "#,
        );
        assert!(expanded.contains("CallbackArgs"));
        assert!(expanded.contains("Parameters"));
        assert!(expanded.contains("arity"));
        assert!(expanded.contains("decode_function"));
        assert!(expanded.contains("invoke"));
        assert!(expanded.contains("register_callback"));
        assert!(expanded.contains("face"));
        assert!(expanded.contains("Target route of the patched document"));
        assert!(expanded.contains("Live document under patching"));
        assert!(expanded.contains("Encode"));
        assert!(expanded.contains("Value"));
        assert!(expanded.contains("Function"));
        assert!(expanded.contains("FaceData :: Function"));
    }

    #[test]
    fn single_field_renders_tuple_params() {
        let expanded = expand(
            r#"
            #[face(callback_shape)]
            pub struct FilterArgs {
                /// Target route of the candidate document.
                route: String,
            }
            "#,
        );
        assert!(expanded.contains("CallbackArgs"));
        assert!(expanded.contains("arity"));
        assert!(expanded.contains("a function with 1 param"));
        assert!(expanded.contains("Parameters"));
    }

    #[test]
    fn foreign_shapes_fail_the_build() {
        reject(
            r#"
            #[face(callback_shape)]
            pub struct ApplyArgs {
                route: String,
            }
            "#,
        );
        reject(
            r#"
            #[face(callback_shape)]
            pub struct ApplyArgs {
                /// Stored function.
                apply: mlua :: Function,
            }
            "#,
        );
        reject(
            r#"
            #[face(callback_shape)]
            pub struct ApplyArgs {
                /// Target route.
                #[face(rename = "target")]
                route: String,
            }
            "#,
        );
        let input: syn::DeriveInput = syn::parse_str(
            r#"
            #[face(callback_shape)]
            pub struct ApplyArgs {
                /// Target route.
                route: String,
            }
            "#,
        )
        .unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        let hook: syn::Path = syn::parse_str("check").unwrap();
        assert!(
            expand_struct_callback(&input.ident, data, &input.attrs, &Some(hook), &None, &None)
                .is_err(),
            "foreign hook fails the build"
        );
    }

    #[test]
    fn return_face_renders_typed_call_face() {
        let returns: syn::Path = syn::parse_str("bool").unwrap();
        let expanded = expand_returns(
            r#"
            #[face(callback_shape)]
            pub struct FilterArgs {
                /// Target route of the candidate document.
                route: String,
            }
            "#,
            &Some(returns),
        );
        assert!(expanded.contains("type Return"));
        assert!(expanded.contains("FunctionReturn"));
        assert!(expanded.contains("fun({}): {}"));
    }

    #[test]
    fn bare_shapes_keep_empty_returns() {
        let expanded = expand(
            r#"
            #[face(callback_shape)]
            pub struct ApplyArgs {
                /// Target route.
                route: String,
            }
            "#,
        );
        assert!(expanded.contains("type Return"));
        assert!(expanded.contains("Vec :: new"));
        assert!(expanded.contains("fun({})"));
    }
}
