//! Registration emission
//! Descriptor plus runtime registration tokens.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Pat, Type};

use super::faces::{MethodFace, StaticFace};
use super::shapes::{is_tuple, tuple_indices};

/// Build the descriptor tokens for one collected method.
pub(crate) fn descriptor_of(method: &MethodFace) -> Tokens {
    let name = &method.name;
    let docs = &method.docs;
    let labels = method.params.iter().map(|entry| &entry.label);
    let types = method.params.iter().map(|entry| &entry.shape);
    let param_docs = method.params.iter().map(|entry| &entry.documentation);
    let returns: Vec<Tokens> = match &method.returns {
        Some(Type::Tuple(tuple)) => tuple
            .elems
            .iter()
            .zip(method.return_docs.iter())
            .map(|(elem, documentation)| {
                quote! {
                    ::confit_binding::userdata::MethodReturn {
                        face: ::std::borrow::Cow::Owned(
                            <#elem as ::confit_binding::face::Face>::face_name(),
                        ),
                        documentation: #documentation,
                    }
                }
            })
            .collect(),
        Some(returns) => {
            let documentation = method.return_docs.first().map_or("", String::as_str);
            vec![quote! {
                ::confit_binding::userdata::MethodReturn {
                    face: ::std::borrow::Cow::Owned(
                        <#returns as ::confit_binding::face::Face>::face_name(),
                    ),
                    documentation: #documentation,
                }
            }]
        }
        None => Vec::new(),
    };
    quote! {
        ::confit_binding::userdata::MethodDescriptor {
            name: #name,
            documentation: #docs,
            parameters: vec![
                #(::confit_binding::userdata::MethodParameter {
                    name: #labels,
                    face: ::std::borrow::Cow::Owned(
                        <#types as ::confit_binding::face::Face>::face_name(),
                    ),
                    documentation: #param_docs,
                }),*
            ],
            returns: vec![#(#returns),*],
        }
    }
}

/// Build the runtime registration for one collected method.
pub(crate) fn method_of(method: &MethodFace) -> Tokens {
    let name = &method.name;
    let ident = &method.ident;
    let mut takes = Vec::new();
    let mut callers: Vec<Tokens> = Vec::new();
    for (index, param) in method.params.iter().enumerate() {
        let shape = &param.shape;
        let path = format!("{}.{}", method.name, param.label);
        let missing = format!("missing param '{path}'");
        let failure = format!("{path}: {{error}}");
        let caller = match &param.binding {
            Pat::Ident(named) => named.ident.clone(),
            _ => syn::Ident::new(
                &format!("__face_wildcard_{index}"),
                proc_macro2::Span::call_site(),
            ),
        };
        let target: Tokens = match &param.binding {
            Pat::Ident(_) => {
                let binding = &param.binding;
                quote! { #binding }
            }
            _ => quote! { #caller },
        };
        takes.push(quote! {
            let #target: #shape = match ::confit_binding::value::take_arg(&args, &mut next) {
                ::std::option::Option::Some(raw) => <#shape as ::confit_binding::value::Decode>::decode(&raw, lua).map_err(|error| ::mlua::Error::runtime(::std::format!(#failure)))?,
                ::std::option::Option::None => return ::std::result::Result::Err(::mlua::Error::runtime(#missing)),
            };
        });
        callers.push(quote! { #caller });
    }
    let mut call_inputs: Vec<Tokens> = Vec::new();
    if method.takes_lua {
        call_inputs.push(quote! { lua });
    }
    call_inputs.extend(callers);
    let call: Tokens = match (&method.returns, method.fallible) {
        (None, _) => quote! {
            this.#ident(#(#call_inputs,)*);
            ::confit_binding::value::Encode::encode(&(), lua)
        },
        (Some(returns), true) if is_tuple(returns) => {
            let indices = tuple_indices(returns);
            quote! {
                let outcome: #returns = this.#ident(#(#call_inputs,)*)?;
                let mut __face_returns = ::mlua::MultiValue::new();
                #(__face_returns.push_back(::confit_binding::value::Encode::encode(&outcome.#indices, lua)?);)*
                ::std::result::Result::Ok(__face_returns)
            }
        }
        (Some(returns), true) => quote! {
            let outcome: #returns = this.#ident(#(#call_inputs,)*)?;
            ::confit_binding::value::Encode::encode(&outcome, lua)
        },
        (Some(returns), false) if is_tuple(returns) => {
            let indices = tuple_indices(returns);
            quote! {
                let outcome: #returns = this.#ident(#(#call_inputs,)*);
                let mut __face_returns = ::mlua::MultiValue::new();
                #(__face_returns.push_back(::confit_binding::value::Encode::encode(&outcome.#indices, lua)?);)*
                ::std::result::Result::Ok(__face_returns)
            }
        }
        (Some(returns), false) => quote! {
            let outcome: #returns = this.#ident(#(#call_inputs,)*);
            ::confit_binding::value::Encode::encode(&outcome, lua)
        },
    };
    let prelude: Tokens = if method.params.is_empty() {
        quote! { let _ = &args; }
    } else {
        quote! { let mut next = 0usize; }
    };
    quote! {
        methods.add_method(#name, |lua, this, args: ::mlua::Variadic<::mlua::Value>| {
            #prelude
            #(#takes)*
            #call
        });
    }
}

/// Build one static function value for one collected static.
pub(crate) fn static_function_of(method: &StaticFace) -> Tokens {
    let ident = &method.ident;
    let mut takes = Vec::new();
    let mut callers: Vec<Tokens> = Vec::new();
    for (index, param) in method.params.iter().enumerate() {
        let shape = &param.shape;
        let path = format!("{}.{}", method.name, param.label);
        let missing = format!("missing param '{path}'");
        let failure = format!("{path}: {{error}}");
        let caller = match &param.binding {
            Pat::Ident(named) => named.ident.clone(),
            _ => syn::Ident::new(
                &format!("__face_wildcard_{index}"),
                proc_macro2::Span::call_site(),
            ),
        };
        let target: Tokens = match &param.binding {
            Pat::Ident(_) => {
                let binding = &param.binding;
                quote! { #binding }
            }
            _ => quote! { #caller },
        };
        if param.optional {
            takes.push(quote! {
                let #target: #shape = match ::confit_binding::value::take_arg(&args, &mut next) {
                    ::std::option::Option::Some(raw) => <#shape as ::confit_binding::value::Decode>::decode(&raw, lua).map_err(|error| ::mlua::Error::runtime(::std::format!(#failure)))?,
                    ::std::option::Option::None => < #shape as ::std::default::Default >::default(),
                };
            });
        } else {
            takes.push(quote! {
                let #target: #shape = match ::confit_binding::value::take_arg(&args, &mut next) {
                    ::std::option::Option::Some(raw) => <#shape as ::confit_binding::value::Decode>::decode(&raw, lua).map_err(|error| ::mlua::Error::runtime(::std::format!(#failure)))?,
                    ::std::option::Option::None => return ::std::result::Result::Err(::mlua::Error::runtime(#missing)),
                };
            });
        }
        callers.push(quote! { #caller });
    }
    let mut call_inputs: Vec<Tokens> = Vec::new();
    if method.takes_lua {
        call_inputs.push(quote! { lua });
    }
    call_inputs.extend(callers);
    let call: Tokens = match (&method.returns, method.fallible) {
        (None, _) => quote! {
            Self::#ident(#(#call_inputs,)*);
            ::confit_binding::value::Encode::encode(&(), lua)
        },
        (Some(returns), true) if is_tuple(returns) => {
            let indices = tuple_indices(returns);
            quote! {
                let outcome: #returns = Self::#ident(#(#call_inputs,)*)?;
                let mut __face_returns = ::mlua::MultiValue::new();
                #(__face_returns.push_back(::confit_binding::value::Encode::encode(&outcome.#indices, lua)?);)*
                ::std::result::Result::Ok(__face_returns)
            }
        }
        (Some(returns), true) => quote! {
            let outcome: #returns = Self::#ident(#(#call_inputs,)*)?;
            ::confit_binding::value::Encode::encode(&outcome, lua)
        },
        (Some(returns), false) if is_tuple(returns) => {
            let indices = tuple_indices(returns);
            quote! {
                let outcome: #returns = Self::#ident(#(#call_inputs,)*);
                let mut __face_returns = ::mlua::MultiValue::new();
                #(__face_returns.push_back(::confit_binding::value::Encode::encode(&outcome.#indices, lua)?);)*
                ::std::result::Result::Ok(__face_returns)
            }
        }
        (Some(returns), false) => quote! {
            let outcome: #returns = Self::#ident(#(#call_inputs,)*);
            ::confit_binding::value::Encode::encode(&outcome, lua)
        },
    };
    let prelude: Tokens = if method.params.is_empty() {
        quote! { let _ = &args; }
    } else {
        quote! { let mut next = 0usize; }
    };
    quote! {
        lua.create_function(|lua, args: ::mlua::Variadic<::mlua::Value>| {
            #prelude
            #(#takes)*
            #call
        })
    }
}

/// Build one stub registration block for one collected static.
pub(crate) fn static_stub_block(method: &StaticFace) -> Tokens {
    let name = &method.name;
    let docs = &method.docs;
    let labels: Vec<&String> = method.params.iter().map(|entry| &entry.label).collect();
    let types: Vec<&Type> = method.params.iter().map(|entry| &entry.shape).collect();
    let param_docs: Vec<&String> = method
        .params
        .iter()
        .map(|entry| &entry.documentation)
        .collect();
    let optionals: Vec<bool> = method.params.iter().map(|entry| entry.optional).collect();
    let return_faces: Vec<Tokens> = match &method.returns {
        Some(Type::Tuple(tuple)) => tuple
            .elems
            .iter()
            .zip(method.return_docs.iter())
            .map(|(elem, documentation)| {
                quote! {
                    ::confit_binding::face::FunctionReturn {
                        face: ::std::borrow::Cow::Owned(
                            <#elem as ::confit_binding::face::Face>::face_name(),
                        ),
                        documentation: #documentation,
                    }
                }
            })
            .collect(),
        Some(returns) => {
            let documentation = method.return_docs.first().map_or("", String::as_str);
            vec![quote! {
                ::confit_binding::face::FunctionReturn {
                    face: ::std::borrow::Cow::Owned(
                        <#returns as ::confit_binding::face::Face>::face_name(),
                    ),
                    documentation: #documentation,
                }
            }]
        }
        None => Vec::new(),
    };
    let return_collects = collect_return_tokens(&method.returns);
    quote! {
        {
            let __face_stub = ::confit_binding::stub::emit_function(
                &::confit_binding::face::FunctionDescriptor {
                    name: #name,
                    documentation: #docs,
                },
                &[
                    #(::confit_binding::face::FunctionParameter {
                        name: #labels,
                        face: ::std::borrow::Cow::Owned(
                            <#types as ::confit_binding::face::Face>::face_name(),
                        ),
                        documentation: #param_docs,
                        optional: #optionals,
                    }),*
                ],
                &[
                    #(#return_faces),*
                ],
            );
            if collector.register(#name.to_owned(), __face_stub) {
                #(<#types as ::confit_binding::face::Face>::collect(collector)?;)*
                #(#return_collects)*
            }
        }
    }
}

/// Build the face collection for one return shape.
fn collect_return_tokens(returns: &Option<Type>) -> Vec<Tokens> {
    match returns {
        None => Vec::new(),
        Some(Type::Tuple(tuple)) => tuple
            .elems
            .iter()
            .map(|elem| {
                quote! {
                    <#elem as ::confit_binding::face::Face>::collect(collector)?;
                }
            })
            .collect(),
        Some(single) => vec![quote! {
            <#single as ::confit_binding::face::Face>::collect(collector)?;
        }],
    }
}
