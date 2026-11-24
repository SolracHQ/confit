//! Emits method docs and runtime registration tokens.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Pat, Type};

use super::attribute::Operator;
use super::collected::{FunctionShape, MethodShape, ReceiverKind};
use super::shapes::{is_tuple, tuple_indices};

/// Builds parameter metadata tokens.
fn parameter_metadata(method: &MethodShape) -> Vec<Tokens> {
    method
        .params
        .iter()
        .map(|entry| {
            let label = &entry.label;
            let shape = &entry.shape;
            let description = &entry.description;
            quote! {
                ::lua_binding::object::MethodParameter {
                    name: #label,
                    shape: ::std::borrow::Cow::Owned(
                        <#shape as ::lua_binding::shape::Shape>::name(),
                    ),
                    description: #description,
                    collect: <#shape as ::lua_binding::shape::Shape>::collect,
                }
            }
        })
        .collect()
}

/// Builds return metadata tokens.
fn return_metadata(method: &MethodShape) -> Vec<Tokens> {
    match &method.returns {
        Some(Type::Tuple(tuple)) => tuple
            .elems
            .iter()
            .zip(method.return_docs.iter())
            .map(|(elem, description)| {
                quote! {
                    ::lua_binding::object::MethodReturn {
                        shape: ::std::borrow::Cow::Owned(
                            <#elem as ::lua_binding::shape::Shape>::name(),
                        ),
                        description: #description,
                        collect: <#elem as ::lua_binding::shape::Shape>::collect,
                    }
                }
            })
            .collect(),
        Some(returns) => {
            let description = method.return_docs.first().map_or("", String::as_str);
            vec![quote! {
                ::lua_binding::object::MethodReturn {
                    shape: ::std::borrow::Cow::Owned(
                        <#returns as ::lua_binding::shape::Shape>::name(),
                    ),
                    description: #description,
                    collect: <#returns as ::lua_binding::shape::Shape>::collect,
                }
            }]
        }
        None => Vec::new(),
    }
}

/// Builds the doc tokens.
pub(crate) fn method_doc_of(method: &MethodShape) -> Tokens {
    let name = &method.name;
    let docs = &method.docs;
    let parameters = parameter_metadata(method);
    let returns = return_metadata(method);
    quote! {
        ::lua_binding::object::Method {
            name: #name,
            description: #docs,
            parameters: vec![#(#parameters),*],
            returns: vec![#(#returns),*],
        }
    }
}

/// Builds the operator doc tokens, absent for `__tostring`.
pub(crate) fn operator_doc_of(method: &MethodShape, operator: &Operator) -> Option<Tokens> {
    let lua = operator.lua_name()?;
    let parameters = parameter_metadata(method);
    let returns = return_metadata(method);
    Some(quote! {
        ::lua_binding::object::Operator {
            lua: #lua,
            parameters: vec![#(#parameters),*],
            returns: vec![#(#returns),*],
        }
    })
}

/// Builds the decode and call body for a method.
fn call_body(method: &MethodShape) -> Tokens {
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
                &format!("__shape_wildcard_{index}"),
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
            let #target: #shape = match ::lua_binding::value::take_arg(&args, &mut next) {
                ::std::option::Option::Some(raw) => <#shape as ::lua_binding::value::Decode>::decode(&raw, lua).map_err(|error| ::mlua::Error::runtime(::std::format!(#failure)))?,
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
            ::lua_binding::value::Encode::encode(&(), lua)
        },
        (Some(returns), true) if is_tuple(returns) => {
            let indices = tuple_indices(returns);
            quote! {
                let outcome: #returns = this.#ident(#(#call_inputs,)*)?;
                let mut __call_values = ::mlua::MultiValue::new();
                #(__call_values.push_back(::lua_binding::value::Encode::encode(&outcome.#indices, lua)?);)*
                ::std::result::Result::Ok(__call_values)
            }
        }
        (Some(returns), true) => quote! {
            let outcome: #returns = this.#ident(#(#call_inputs,)*)?;
            ::lua_binding::value::Encode::encode(&outcome, lua)
        },
        (Some(returns), false) if is_tuple(returns) => {
            let indices = tuple_indices(returns);
            quote! {
                let outcome: #returns = this.#ident(#(#call_inputs,)*);
                let mut __call_values = ::mlua::MultiValue::new();
                #(__call_values.push_back(::lua_binding::value::Encode::encode(&outcome.#indices, lua)?);)*
                ::std::result::Result::Ok(__call_values)
            }
        }
        (Some(returns), false) => quote! {
            let outcome: #returns = this.#ident(#(#call_inputs,)*);
            ::lua_binding::value::Encode::encode(&outcome, lua)
        },
    };
    let prelude: Tokens = if method.params.is_empty() {
        quote! { let _ = &args; }
    } else {
        quote! { let mut next = 0usize; }
    };
    quote! {
        #prelude
        #(#takes)*
        #call
    }
}

/// The runtime registration family.
#[derive(Clone, Copy)]
enum Registration {
    /// A named method.
    Method,
    /// A metamethod.
    MetaMethod,
}

impl Registration {
    /// Returns the registration method for a receiver kind.
    fn of(self, receiver: ReceiverKind) -> syn::Ident {
        let name = match (self, receiver) {
            (Self::Method, ReceiverKind::Shared) => "add_method",
            (Self::Method, ReceiverKind::Mutable) => "add_method_mut",
            (Self::MetaMethod, ReceiverKind::Shared) => "add_meta_method",
            (Self::MetaMethod, ReceiverKind::Mutable) => "add_meta_method_mut",
        };
        syn::Ident::new(name, proc_macro2::Span::call_site())
    }
}

/// Builds the runtime registration.
pub(crate) fn method_of(method: &MethodShape) -> Tokens {
    let name = &method.name;
    let register = Registration::Method.of(method.receiver);
    let body = call_body(method);
    quote! {
        methods.#register(#name, |lua, this, args: ::mlua::Variadic<::mlua::Value>| {
            #body
        });
    }
}

/// Builds the runtime registration for a flagged metamethod.
pub(crate) fn meta_method_of(method: &MethodShape, hook: &Operator) -> Tokens {
    let path = hook.meta_method();
    let register = Registration::MetaMethod.of(method.receiver);
    let body = call_body(method);
    quote! {
        methods.#register(#path, |lua, this, args: ::mlua::Variadic<::mlua::Value>| {
            #body
        });
    }
}

/// Builds a function value.
pub(crate) fn static_function_of(method: &FunctionShape) -> Tokens {
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
                &format!("__shape_wildcard_{index}"),
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
                let #target: #shape = match ::lua_binding::value::take_arg(&args, &mut next) {
                    ::std::option::Option::Some(raw) => <#shape as ::lua_binding::value::Decode>::decode(&raw, lua).map_err(|error| ::mlua::Error::runtime(::std::format!(#failure)))?,
                    ::std::option::Option::None => < #shape as ::std::default::Default >::default(),
                };
            });
        } else {
            takes.push(quote! {
                let #target: #shape = match ::lua_binding::value::take_arg(&args, &mut next) {
                    ::std::option::Option::Some(raw) => <#shape as ::lua_binding::value::Decode>::decode(&raw, lua).map_err(|error| ::mlua::Error::runtime(::std::format!(#failure)))?,
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
            ::lua_binding::value::Encode::encode(&(), lua)
        },
        (Some(returns), true) if is_tuple(returns) => {
            let indices = tuple_indices(returns);
            quote! {
                let outcome: #returns = Self::#ident(#(#call_inputs,)*)?;
                let mut __call_values = ::mlua::MultiValue::new();
                #(__call_values.push_back(::lua_binding::value::Encode::encode(&outcome.#indices, lua)?);)*
                ::std::result::Result::Ok(__call_values)
            }
        }
        (Some(returns), true) => quote! {
            let outcome: #returns = Self::#ident(#(#call_inputs,)*)?;
            ::lua_binding::value::Encode::encode(&outcome, lua)
        },
        (Some(returns), false) if is_tuple(returns) => {
            let indices = tuple_indices(returns);
            quote! {
                let outcome: #returns = Self::#ident(#(#call_inputs,)*);
                let mut __call_values = ::mlua::MultiValue::new();
                #(__call_values.push_back(::lua_binding::value::Encode::encode(&outcome.#indices, lua)?);)*
                ::std::result::Result::Ok(__call_values)
            }
        }
        (Some(returns), false) => quote! {
            let outcome: #returns = Self::#ident(#(#call_inputs,)*);
            ::lua_binding::value::Encode::encode(&outcome, lua)
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

/// Builds a stub registration block.
pub(crate) fn static_stub_block(method: &FunctionShape) -> Tokens {
    let name = &method.name;
    let docs = &method.docs;
    let labels: Vec<&String> = method.params.iter().map(|entry| &entry.label).collect();
    let types: Vec<&Type> = method.params.iter().map(|entry| &entry.shape).collect();
    let param_docs: Vec<&String> = method
        .params
        .iter()
        .map(|entry| &entry.description)
        .collect();
    let optionals: Vec<bool> = method.params.iter().map(|entry| entry.optional).collect();
    let return_shapes: Vec<Tokens> = match &method.returns {
        Some(Type::Tuple(tuple)) => tuple
            .elems
            .iter()
            .zip(method.return_docs.iter())
            .map(|(elem, description)| {
                quote! {
                    ::lua_binding::shape::Return {
                        shape: ::std::borrow::Cow::Owned(
                            <#elem as ::lua_binding::shape::Shape>::name(),
                        ),
                        description: #description,
                        collect: <#elem as ::lua_binding::shape::Shape>::collect,
                    }
                }
            })
            .collect(),
        Some(returns) => {
            let description = method.return_docs.first().map_or("", String::as_str);
            vec![quote! {
                ::lua_binding::shape::Return {
                    shape: ::std::borrow::Cow::Owned(
                        <#returns as ::lua_binding::shape::Shape>::name(),
                    ),
                    description: #description,
                    collect: <#returns as ::lua_binding::shape::Shape>::collect,
                }
            }]
        }
        None => Vec::new(),
    };
    quote! {
        {
            let data = ::lua_binding::shape::ShapeData::Function {
                description: #docs,
                parameters: ::std::vec![
                    #(::lua_binding::shape::Parameter {
                        name: #labels,
                        shape: ::std::borrow::Cow::Owned(
                            <#types as ::lua_binding::shape::Shape>::name(),
                        ),
                        description: #param_docs,
                        optional: #optionals,
                        collect: <#types as ::lua_binding::shape::Shape>::collect,
                    }),*
                ],
                returns: ::std::vec![
                    #(#return_shapes),*
                ],
            };
            if collector.add_function(#name, &data) {
                data.collect_referenced(collector);
            }
        }
    }
}
