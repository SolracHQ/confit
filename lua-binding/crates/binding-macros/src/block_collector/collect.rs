//! Collects receiver-checked methods and static functions from an impl block.

use syn::{FnArg, Pat, ReturnType, Type, ext::IdentExt};

use crate::parse::doc_lines;

use super::attribute::{Operator, param_optional};
use super::collected::{FunctionShape, MethodShape, ParameterShape, ReceiverKind, StaticParam};
use super::docs::{describe_params, describe_returns, split_sections};
use super::shapes::{
    is_lua_ref, is_option, is_result, is_tuple, names_lua, return_arity, unwrap_result,
};

/// Returns whether a method carries a receiver.
pub(crate) fn has_receiver(method: &syn::ImplItemFn) -> bool {
    matches!(method.sig.inputs.first(), Some(FnArg::Receiver(_)))
}

/// Checks that a flagged method holds the fixed hook signature.
pub(crate) fn check_operator(
    class_name: &str,
    operator: &Operator,
    shape: &MethodShape,
) -> Result<(), syn::Error> {
    let operands = operator.operands();
    if shape.params.len() != operands {
        return Err(syn::Error::new_spanned(
            &shape.ident,
            format!(
                "{class_name}.{}: operator methods take {operands} params",
                shape.name
            ),
        ));
    }
    match &shape.returns {
        Some(shape) if !is_tuple(shape) => Ok(()),
        _ => Err(syn::Error::new_spanned(
            &shape.ident,
            format!(
                "{class_name}.{}: operator methods return one value",
                shape.name
            ),
        )),
    }
}

/// Collects the receiver kind of a method.
fn method_receiver(class_name: &str, method: &syn::ImplItemFn) -> Result<ReceiverKind, syn::Error> {
    let method_name = method.sig.ident.unraw().to_string();
    match method.sig.inputs.first() {
        Some(FnArg::Receiver(receiver)) => match &receiver.kind {
            syn::ReceiverKind::Reference(_, _, mutability) => Ok(if mutability.is_some() {
                ReceiverKind::Mutable
            } else {
                ReceiverKind::Shared
            }),
            _ => Err(syn::Error::new_spanned(
                &method.sig.ident,
                format!("{class_name}.{method_name}: shape methods take &self or &mut self"),
            )),
        },
        _ => Err(syn::Error::new_spanned(
            &method.sig.ident,
            format!("{class_name}.{method_name}: shape methods take &self or &mut self"),
        )),
    }
}

/// Collects a method.
pub(crate) fn parse_method(
    class_name: &str,
    method: &syn::ImplItemFn,
) -> Result<MethodShape, syn::Error> {
    let method_name = method.sig.ident.to_string();
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            format!("{class_name}.{method_name}: shape methods do not support generics"),
        ));
    }
    let receiver = method_receiver(class_name, method)?;
    let inputs = method.sig.inputs.iter().skip(1);
    let mut params = Vec::new();
    let mut takes_lua = false;
    let mut first = true;
    for input in inputs {
        let FnArg::Typed(typed) = input else {
            return Err(syn::Error::new_spanned(
                input,
                format!("{class_name}.{method_name}: shape methods take typed params"),
            ));
        };
        if first {
            first = false;
            if is_lua_ref(&typed.ty) {
                takes_lua = true;
                continue;
            }
            if names_lua(&typed.ty) {
                return Err(syn::Error::new_spanned(
                    typed.ty.as_ref(),
                    format!(
                        "{class_name}.{method_name}: shape methods take the \
                         Lua context by reference"
                    ),
                ));
            }
        }
        if matches!(typed.ty.as_ref(), Type::Reference(_)) {
            return Err(syn::Error::new_spanned(
                typed.ty.as_ref(),
                format!(
                    "{class_name}.{method_name}: shape params take owned \
                     values"
                ),
            ));
        }
        let label = match typed.pat.as_ref() {
            Pat::Ident(pattern) => pattern.ident.unraw().to_string(),
            Pat::Wild(_) => "_".to_owned(),
            _ => {
                return Err(syn::Error::new_spanned(
                    typed.pat.as_ref(),
                    format!(
                        "{class_name}.{method_name}: shape params take names \
                         or wildcards"
                    ),
                ));
            }
        };
        if param_optional(
            class_name,
            &method_name,
            &label,
            &typed.attrs,
            &method.sig.ident,
        )? {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                format!(
                    "{class_name}.{method_name}.{label}: statics are expected for optional params"
                ),
            ));
        }
        params.push(ParameterShape {
            label,
            binding: typed.pat.as_ref().clone(),
            shape: typed.ty.as_ref().clone(),
            description: String::new(),
        });
    }
    let (returns, fallible) = match &method.sig.output {
        ReturnType::Default => (None, false),
        ReturnType::Type(_, ty) => (Some(unwrap_result(ty)), is_result(ty)),
    };
    let lines = doc_lines(&method.attrs);
    let split = split_sections(class_name, &method_name, &lines, &method.sig.ident)?;
    let labels: Vec<String> = params.iter().map(|entry| entry.label.clone()).collect();
    let param_docs = describe_params(
        class_name,
        &method_name,
        &split.arguments,
        split.has_arguments,
        &labels,
        &method.sig.ident,
    )?;
    let arity = return_arity(&returns);
    let return_docs = describe_returns(
        class_name,
        &method_name,
        &split.returns,
        split.has_returns,
        arity,
        &method.sig.ident,
    )?;
    for (entry, description) in params.iter_mut().zip(param_docs) {
        entry.description = description;
    }
    Ok(MethodShape {
        ident: method.sig.ident.clone(),
        name: method.sig.ident.unraw().to_string(),
        docs: split.summary.join(" "),
        receiver,
        params,
        returns,
        return_docs,
        takes_lua,
        fallible,
    })
}

/// Collects a static function.
pub(crate) fn parse_static(
    class_name: &str,
    method: &syn::ImplItemFn,
) -> Result<FunctionShape, syn::Error> {
    let method_name = method.sig.ident.to_string();
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            format!("{class_name}.{method_name}: shape statics do not support generics"),
        ));
    }
    let mut params = Vec::new();
    let mut takes_lua = false;
    let mut first = true;
    for input in &method.sig.inputs {
        let FnArg::Typed(typed) = input else {
            return Err(syn::Error::new_spanned(
                input,
                format!("{class_name}.{method_name}: shape statics take typed params"),
            ));
        };
        if first {
            first = false;
            if is_lua_ref(&typed.ty) {
                takes_lua = true;
                continue;
            }
            if names_lua(&typed.ty) {
                return Err(syn::Error::new_spanned(
                    typed.ty.as_ref(),
                    format!(
                        "{class_name}.{method_name}: shape statics take the \
                         Lua context by reference"
                    ),
                ));
            }
        }
        if matches!(typed.ty.as_ref(), Type::Reference(_)) {
            return Err(syn::Error::new_spanned(
                typed.ty.as_ref(),
                format!(
                    "{class_name}.{method_name}: shape params take owned \
                     values"
                ),
            ));
        }
        let label = match typed.pat.as_ref() {
            Pat::Ident(pattern) => pattern.ident.unraw().to_string(),
            Pat::Wild(_) => "_".to_owned(),
            _ => {
                return Err(syn::Error::new_spanned(
                    typed.pat.as_ref(),
                    format!(
                        "{class_name}.{method_name}: shape params take names \
                         or wildcards"
                    ),
                ));
            }
        };
        let marked = param_optional(
            class_name,
            &method_name,
            &label,
            &typed.attrs,
            &method.sig.ident,
        )?;
        if marked && is_option(&typed.ty) {
            return Err(syn::Error::new_spanned(
                typed.ty.as_ref(),
                format!(
                    "{class_name}.{method_name}.{label}: optional is inferred for Option params; drop #[shape(optional)]"
                ),
            ));
        }
        let optional = marked || is_option(&typed.ty);
        params.push(StaticParam {
            label,
            binding: typed.pat.as_ref().clone(),
            shape: typed.ty.as_ref().clone(),
            description: String::new(),
            optional,
        });
    }
    let (returns, fallible) = match &method.sig.output {
        ReturnType::Default => (None, false),
        ReturnType::Type(_, ty) => (Some(unwrap_result(ty)), is_result(ty)),
    };
    let lines = doc_lines(&method.attrs);
    let split = split_sections(class_name, &method_name, &lines, &method.sig.ident)?;
    let labels: Vec<String> = params.iter().map(|entry| entry.label.clone()).collect();
    let param_docs = describe_params(
        class_name,
        &method_name,
        &split.arguments,
        split.has_arguments,
        &labels,
        &method.sig.ident,
    )?;
    let arity = return_arity(&returns);
    let return_docs = describe_returns(
        class_name,
        &method_name,
        &split.returns,
        split.has_returns,
        arity,
        &method.sig.ident,
    )?;
    for (entry, description) in params.iter_mut().zip(param_docs) {
        entry.description = description;
    }
    Ok(FunctionShape {
        ident: method.sig.ident.clone(),
        name: method.sig.ident.unraw().to_string(),
        docs: split.summary.join(" "),
        params,
        returns,
        return_docs,
        takes_lua,
        fallible,
    })
}
