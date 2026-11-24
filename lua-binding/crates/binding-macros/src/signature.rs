//! Expands signature shapes into Lua function shapes.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Attribute, Type, ext::IdentExt};

use super::parse::{doc_lines, parse_field_flags};

/// One parameter in a signature shape.
struct SignatureField {
    /// The parameter binding.
    binding: syn::Ident,
    /// The parameter label.
    label: String,
    /// The parameter description.
    description: String,
    /// The parameter shape.
    shape: Type,
    /// The live table marker.
    borrowed: bool,
}

/// Returns whether a type is a Lua function.
fn is_function_ty(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Function";
    }
    false
}

/// Collects the described parameters of a signature shape.
fn collect_fields(
    outer: &str,
    name: &syn::Ident,
    data: &syn::DataStruct,
) -> Result<Vec<SignatureField>, syn::Error> {
    let fields = match &data.fields {
        syn::Fields::Named(named) => &named.named,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                format!("{outer}: structs with named fields are expected for Shape derive"),
            ));
        }
    };
    let mut collected = Vec::new();
    for field in fields {
        let Some(ident) = field.ident.clone() else {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}: structs with named fields are expected for Shape derive"),
            ));
        };
        let field_name = ident.unraw().to_string();
        let flags = parse_field_flags(outer, &field_name, &field.attrs)?;
        if flags.optional
            || flags.opaque
            || flags.ignore
            || flags.flatten
            || flags.name.is_some()
            || flags.default.is_some()
        {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!(
                    "{outer}.{field_name}: plain or borrowed fields are expected for function shapes"
                ),
            ));
        }
        if is_function_ty(&field.ty) {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!("{outer}.{field_name}: a signature param cannot be a Lua function"),
            ));
        }
        let description = doc_lines(&field.attrs).join(" ");
        if description.is_empty() {
            return Err(syn::Error::new_spanned(
                &field.ident,
                format!("{outer}.{field_name}: signature params carry /// docs"),
            ));
        }
        collected.push(SignatureField {
            binding: ident,
            label: field_name,
            description,
            shape: field.ty.clone(),
            borrowed: flags.borrowed,
        });
    }
    Ok(collected)
}

/// Builds the signature shape expansion.
pub(crate) fn expand_struct_function(
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
            format!("{outer}: function shapes do not support validate hooks"),
        ));
    }
    if rename.is_some() {
        return Err(syn::Error::new_spanned(
            name,
            format!("{outer}: function shapes do not support name"),
        ));
    }
    let struct_docs = doc_lines(attrs).join(" ");
    let fields = collect_fields(&outer, name, data)?;
    let labels: Vec<String> = fields.iter().map(|field| field.label.clone()).collect();
    let descriptions: Vec<String> = fields
        .iter()
        .map(|field| field.description.clone())
        .collect();
    let shapes: Vec<Type> = fields.iter().map(|field| field.shape.clone()).collect();
    let count = fields.len();
    let want_arg_count = if count == 1 {
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
        if field.borrowed {
            parameters.push(quote! { ::mlua::Table });
            pushes.push(quote! {
                arguments.push(::mlua::Value::Table(#binding));
            });
        } else {
            parameters.push(quote! { &'args #shape });
            pushes.push(quote! {
                arguments.push(
                    <#shape as ::lua_binding::value::Encode>::encode(#binding, lua)?,
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
            ::std::vec![::lua_binding::shape::Return {
                shape: ::std::borrow::Cow::Owned(
                    <#path as ::lua_binding::shape::Shape>::name(),
                ),
                description: "",
                collect: <#path as ::lua_binding::shape::Shape>::collect,
            }]
        },
        None => quote! { ::std::vec::Vec::new() },
    };
    let call_shape_body: Tokens = match returns {
        Some(path) => quote! {
            ::std::format!(
                "fun({}): {}",
                [#(::std::format!(
                    "{}: {}",
                    #labels,
                    <#shapes as ::lua_binding::shape::Shape>::name()
                )),*].join(", "),
                <#path as ::lua_binding::shape::Shape>::name()
            )
        },
        None => quote! {
            ::std::format!(
                "fun({})",
                [#(::std::format!(
                    "{}: {}",
                    #labels,
                    <#shapes as ::lua_binding::shape::Shape>::name()
                )),*].join(", ")
            )
        },
    };
    Ok(quote! {
        impl ::lua_binding::shape::Shape for #name {
            fn name() -> ::std::string::String {
                stringify!(#name).to_owned()
            }

            fn shape_data() -> ::lua_binding::shape::ShapeData {
                ::lua_binding::shape::ShapeData::Function {
                    description: #struct_docs,
                    parameters: ::std::vec![
                        #(::lua_binding::shape::Parameter {
                            name: #labels,
                            description: #descriptions,
                            shape: ::std::borrow::Cow::Owned(
                                <#shapes as ::lua_binding::shape::Shape>::name(),
                            ),
                            optional: false,
                            collect: <#shapes as ::lua_binding::shape::Shape>::collect,
                        }),*
                    ],
                    returns: #returns_data,
                }
            }

            fn collect(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                let data = <Self as ::lua_binding::shape::Shape>::shape_data();
                if collector.add_signature(
                    <Self as ::lua_binding::callable::Signature>::call_shape(),
                    &data,
                ) {
                    data.collect_referenced(collector);
                }
            }
        }

        impl ::lua_binding::callable::Signature for #name {
            type Args<'args> = (#(#parameters,)*);
            type Return = #return_type;

            fn arg_count() -> ::std::primitive::usize {
                #count
            }

            fn call_shape() -> ::std::string::String {
                #call_shape_body
            }

            fn decode_function(
                value: &::mlua::Value,
                lua: &::mlua::Lua,
            ) -> ::std::result::Result<::mlua::Function, ::lua_binding::error::DecodeError>
            {
                match value {
                    ::mlua::Value::Function(found) => {
                        let info = found.info();
                        if !info.is_vararg && info.num_params as ::std::primitive::usize != #count
                        {
                            return ::std::result::Result::Err(
                                ::lua_binding::error::DecodeError {
                                    segments: ::std::vec::Vec::new(),
                                    reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                                        require: #want_arg_count.to_owned(),
                                        found: ::std::format!(
                                            "a function with {} param{}",
                                            info.num_params,
                                            if info.num_params == 1 { "" } else { "s" },
                                        ),
                                    },
                                },
                            );
                        }
                        ::std::result::Result::Ok(found.clone())
                    }
                    _ => ::std::result::Result::Err(::lua_binding::error::DecodeError {
                        segments: ::std::vec::Vec::new(),
                        reason: ::lua_binding::error::DecodeTag::ShapeMismatch {
                            require: <#name as ::lua_binding::shape::Shape>::name(),
                            found: value.type_name().to_owned(),
                        },
                    }),
                }
            }

            fn call(
                function: &::mlua::Function,
                lua: &::mlua::Lua,
                parameters: Self::Args<'_>,
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
        expand_struct_function(&input.ident, data, &input.attrs, validate, &None, &None)
            .unwrap()
            .to_string()
    }

    fn expand_returns(source: &str, returns: &Option<syn::Path>) -> String {
        let input: syn::DeriveInput = syn::parse_str(source).unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        expand_struct_function(&input.ident, data, &input.attrs, &None, &None, returns)
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
            expand_struct_function(&input.ident, data, &input.attrs, &None, &None, &None).is_err(),
            "foreign shape fails the build"
        );
    }

    #[test]
    fn shape_renders_function_stub_with_named_params() {
        let expanded = expand(
            r#"
            #[shape(function)]
            pub struct CallArgs {
                /// Target name of the run process.
                name: String,
                /// Live process under running.
                #[shape(borrowed)]
                process: Process,
            }
            "#,
        );
        assert!(expanded.contains("Signature"));
        assert!(expanded.contains("Args"));
        assert!(expanded.contains("arg_count"));
        assert!(expanded.contains("decode_function"));
        assert!(expanded.contains("call"));
        assert!(expanded.contains("add_signature"));
        assert!(expanded.contains("shape"));
        assert!(expanded.contains("Target name of the run process"));
        assert!(expanded.contains("Live process under running"));
        assert!(expanded.contains("Encode"));
        assert!(expanded.contains("Value"));
        assert!(expanded.contains("Function"));
        assert!(expanded.contains("ShapeData :: Function"));
    }

    #[test]
    fn single_field_signature_renders_tuple_params() {
        let expanded = expand(
            r#"
            #[shape(function)]
            pub struct KeepArgs {
                /// Target name of the pending process.
                name: String,
            }
            "#,
        );
        assert!(expanded.contains("Signature"));
        assert!(expanded.contains("arg_count"));
        assert!(expanded.contains("a function with 1 param"));
        assert!(expanded.contains("Args"));
    }

    #[test]
    fn wrong_signature_shapes_fail_the_build() {
        reject(
            r#"
            #[shape(function)]
            pub struct CallArgs {
                name: String,
            }
            "#,
        );
        reject(
            r#"
            #[shape(function)]
            pub struct CallArgs {
                /// Stored function.
                run: mlua :: Function,
            }
            "#,
        );
        reject(
            r#"
            #[shape(function)]
            pub struct CallArgs {
                /// Target name.
                #[shape(name = "target")]
                name: String,
            }
            "#,
        );
        let input: syn::DeriveInput = syn::parse_str(
            r#"
            #[shape(function)]
            pub struct CallArgs {
                /// Target name.
                name: String,
            }
            "#,
        )
        .unwrap();
        let syn::Data::Struct(data) = &input.data else {
            panic!("want struct input");
        };
        let hook: syn::Path = syn::parse_str("check").unwrap();
        assert!(
            expand_struct_function(&input.ident, data, &input.attrs, &Some(hook), &None, &None)
                .is_err(),
            "foreign hook fails the build"
        );
    }

    #[test]
    fn return_shape_renders_typed_call_shape() {
        let returns: syn::Path = syn::parse_str("bool").unwrap();
        let expanded = expand_returns(
            r#"
            #[shape(function)]
            pub struct KeepArgs {
                /// Target name of the pending process.
                name: String,
            }
            "#,
            &Some(returns),
        );
        assert!(expanded.contains("type Return"));
        assert!(expanded.contains("Return"));
        assert!(expanded.contains("fun({}): {}"));
    }

    #[test]
    fn bare_return_shapes_keep_empty_returns() {
        let expanded = expand(
            r#"
            #[shape(function)]
            pub struct CallArgs {
                /// Target name.
                name: String,
            }
            "#,
        );
        assert!(expanded.contains("type Return"));
        assert!(expanded.contains("Vec :: new"));
        assert!(expanded.contains("fun({})"));
    }
}
