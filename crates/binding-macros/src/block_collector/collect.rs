//! Method collection
//! Receiver-checked method and static collection.

use syn::{FnArg, Pat, ReturnType, Type, ext::IdentExt};

use crate::parse::doc_lines;

use super::attribute::param_optional;
use super::docs::{document_params, document_returns, split_sections};
use super::faces::{MethodFace, ParamFace, StaticFace, StaticParam};
use super::shapes::{is_lua_ref, is_result, names_lua, return_arity, unwrap_result};

/// Report whether one method carries a receiver.
pub(crate) fn has_receiver(method: &syn::ImplItemFn) -> bool {
    matches!(method.sig.inputs.first(), Some(FnArg::Receiver(_)))
}

/// Collect one method with its stub faces.
pub(crate) fn parse_method(
    class_name: &str,
    method: &syn::ImplItemFn,
) -> Result<MethodFace, syn::Error> {
    let method_name = method.sig.ident.to_string();
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            format!("{class_name}.{method_name}: face methods do not support generics"),
        ));
    }
    let mut inputs = method.sig.inputs.iter();
    match inputs.next() {
        Some(FnArg::Receiver(receiver))
            if matches!(receiver.kind, syn::ReceiverKind::Reference(_, _, None)) => {}
        _ => {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                format!("{class_name}.{method_name}: face methods take &self alone"),
            ));
        }
    }
    let mut params = Vec::new();
    let mut takes_lua = false;
    let mut first = true;
    for input in inputs {
        let FnArg::Typed(typed) = input else {
            return Err(syn::Error::new_spanned(
                input,
                format!("{class_name}.{method_name}: face methods hold typed params alone"),
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
                        "{class_name}.{method_name}: face methods take the \
                         Lua context by reference alone"
                    ),
                ));
            }
        }
        if matches!(typed.ty.as_ref(), Type::Reference(_)) {
            return Err(syn::Error::new_spanned(
                typed.ty.as_ref(),
                format!(
                    "{class_name}.{method_name}: face params hold owned \
                     values alone"
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
                        "{class_name}.{method_name}: face params take names \
                         or wildcards alone"
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
        params.push(ParamFace {
            label,
            binding: typed.pat.as_ref().clone(),
            shape: typed.ty.as_ref().clone(),
            documentation: String::new(),
        });
    }
    let (returns, fallible) = match &method.sig.output {
        ReturnType::Default => (None, false),
        ReturnType::Type(_, ty) => (Some(unwrap_result(ty)), is_result(ty)),
    };
    let lines = doc_lines(&method.attrs);
    let split = split_sections(class_name, &method_name, &lines, &method.sig.ident)?;
    let labels: Vec<String> = params.iter().map(|entry| entry.label.clone()).collect();
    let param_docs = document_params(
        class_name,
        &method_name,
        &split.arguments,
        split.has_arguments,
        &labels,
        &method.sig.ident,
    )?;
    let arity = return_arity(&returns);
    let return_docs = document_returns(
        class_name,
        &method_name,
        &split.returns,
        split.has_returns,
        arity,
        &method.sig.ident,
    )?;
    for (entry, documentation) in params.iter_mut().zip(param_docs) {
        entry.documentation = documentation;
    }
    Ok(MethodFace {
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

/// Collect one static with its stub faces.
pub(crate) fn parse_static(
    class_name: &str,
    method: &syn::ImplItemFn,
) -> Result<StaticFace, syn::Error> {
    let method_name = method.sig.ident.to_string();
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            format!("{class_name}.{method_name}: face statics do not support generics"),
        ));
    }
    let mut params = Vec::new();
    let mut takes_lua = false;
    let mut first = true;
    for input in &method.sig.inputs {
        let FnArg::Typed(typed) = input else {
            return Err(syn::Error::new_spanned(
                input,
                format!("{class_name}.{method_name}: face statics hold typed params alone"),
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
                        "{class_name}.{method_name}: face statics take the \
                         Lua context by reference alone"
                    ),
                ));
            }
        }
        if matches!(typed.ty.as_ref(), Type::Reference(_)) {
            return Err(syn::Error::new_spanned(
                typed.ty.as_ref(),
                format!(
                    "{class_name}.{method_name}: face params hold owned \
                     values alone"
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
                        "{class_name}.{method_name}: face params take names \
                         or wildcards alone"
                    ),
                ));
            }
        };
        let optional = param_optional(
            class_name,
            &method_name,
            &label,
            &typed.attrs,
            &method.sig.ident,
        )?;
        params.push(StaticParam {
            label,
            binding: typed.pat.as_ref().clone(),
            shape: typed.ty.as_ref().clone(),
            documentation: String::new(),
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
    let param_docs = document_params(
        class_name,
        &method_name,
        &split.arguments,
        split.has_arguments,
        &labels,
        &method.sig.ident,
    )?;
    let arity = return_arity(&returns);
    let return_docs = document_returns(
        class_name,
        &method_name,
        &split.returns,
        split.has_returns,
        arity,
        &method.sig.ident,
    )?;
    for (entry, documentation) in params.iter_mut().zip(param_docs) {
        entry.documentation = documentation;
    }
    Ok(StaticFace {
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
