//! Block collector
//! Impl-block method collection for userdata classes.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, Pat, ReturnType, Type, ext::IdentExt, parse_macro_input};

use super::parse::doc_lines;

/// One collected method with its stub faces.
struct MethodFace {
    name: String,
    docs: String,
    params: Vec<(String, Type)>,
    returns: Option<Type>,
}

/// Report whether one type reads as the Lua context by reference.
fn is_lua_ref(ty: &Type) -> bool {
    if let Type::Reference(reference) = ty
        && reference.mutability.is_none()
        && let Type::Path(path) = &*reference.elem
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Lua";
    }
    false
}

/// Report whether one type names the Lua context in any shape.
fn names_lua(ty: &Type) -> bool {
    match ty {
        Type::Reference(reference) => names_lua(&reference.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|last| last.ident == "Lua"),
        _ => false,
    }
}

/// Unwrap the success type of one Result return.
fn unwrap_result(ty: &Type) -> Type {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
        && last.ident == "Result"
        && let syn::PathArguments::AngleBracketed(args) = &last.arguments
        && let Some(syn::GenericArgument::Type(first)) = args.args.first()
    {
        return first.clone();
    }
    ty.clone()
}

/// Collect one method with its stub faces.
fn parse_method(class_name: &str, method: &syn::ImplItemFn) -> Result<MethodFace, syn::Error> {
    let method_name = method.sig.ident.to_string();
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            format!("{class_name}.{method_name}: face methods take no generics"),
        ));
    }
    let mut inputs = method.sig.inputs.iter();
    match inputs.next() {
        Some(FnArg::Receiver(receiver))
            if matches!(receiver.kind, syn::ReceiverKind::Reference(..)) => {}
        _ => {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                format!(
                    "{class_name}.{method_name}: face methods take &self or \
                     &mut self"
                ),
            ));
        }
    }
    let mut params = Vec::new();
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
        params.push((label, typed.ty.as_ref().clone()));
    }
    let returns = match &method.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => Some(unwrap_result(ty)),
    };
    Ok(MethodFace {
        name: method.sig.ident.unraw().to_string(),
        docs: doc_lines(&method.attrs).join(" "),
        params,
        returns,
    })
}

/// Build the descriptor tokens for one collected method.
fn descriptor_of(class_name: &str, method: &MethodFace) -> Tokens {
    let name = &method.name;
    let docs = &method.docs;
    let labels = method.params.iter().map(|entry| &entry.0);
    let types = method.params.iter().map(|entry| &entry.1);
    let tail: Tokens = match &method.returns {
        Some(returns) => {
            quote! {
                text.push_str(
                    <#returns as ::confit_binding::face::Face>::face_name().as_str(),
                );
            }
        }
        None => quote! { text.push_str("nil"); },
    };
    quote! {
        ::confit_binding::userdata::MethodDescriptor {
            name: #name,
            documentation: #docs,
            signature: {
                let mut text = ::std::string::String::from(#class_name);
                text.insert_str(0, "fun(self: ");
                #(
                    text.push_str(", ");
                    text.push_str(#labels);
                    text.push_str(": ");
                    text.push_str(
                        <#types as ::confit_binding::face::Face>::face_name().as_str(),
                    );
                )*
                text.push_str("): ");
                #tail
                text
            },
        }
    }
}

/// Expand method collection for one impl block.
pub fn expand(attribute: TokenStream, item: TokenStream) -> TokenStream {
    if !attribute.is_empty() {
        return syn::Error::new_spanned(
            proc_macro2::TokenStream::from(attribute),
            "`#[face_impl]` takes no arguments",
        )
        .to_compile_error()
        .into();
    }
    let block = parse_macro_input!(item as ItemImpl);
    let expanded = expand_block(&block);
    match expanded {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Build the collection expansion for one checked block.
fn expand_block(block: &ItemImpl) -> Result<Tokens, syn::Error> {
    if !block.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &block.generics,
            "`#[face_impl]` reads impl blocks with no generics",
        ));
    }
    if let Some(clause) = &block.generics.where_clause {
        return Err(syn::Error::new_spanned(
            clause,
            "`#[face_impl]` reads impl blocks with no where clause",
        ));
    }
    if block.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "`#[face_impl]` reads inherent impl blocks alone",
        ));
    }
    let Type::Path(path) = &*block.self_ty else {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "`#[face_impl]` reads a simple path type",
        ));
    };
    let Some(last) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "`#[face_impl]` reads a simple path type",
        ));
    };
    if !matches!(last.arguments, syn::PathArguments::None) {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "`#[face_impl]` reads a simple path type",
        ));
    }
    let class_name = last.ident.to_string();
    let self_ty = &block.self_ty;
    let mut faces = Vec::new();
    let mut collectors: Vec<Tokens> = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                let face = parse_method(&class_name, method)?;
                for (_, ty) in &face.params {
                    collectors.push(quote! {
                        <#ty as ::confit_binding::face::Face>::collect(collector)?;
                    });
                }
                if let Some(returns) = &face.returns {
                    collectors.push(quote! {
                        <#returns as ::confit_binding::face::Face>::collect(collector)?;
                    });
                }
                faces.push(descriptor_of(&class_name, &face));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    item,
                    format!(
                        "{class_name}: `#[face_impl]` registers every method \
                         in its block; move non-method items out"
                    ),
                ));
            }
        }
    }
    Ok(quote! {
        impl ::mlua::UserData for #self_ty {}

        impl ::confit_binding::userdata::UserdataClass for #self_ty {
            fn class_name() -> &'static str {
                #class_name
            }

            fn class_stub() -> String {
                let mut out =
                    <Self as ::confit_binding::face::Face>::face_stub();
                for method in Self::methods() {
                    out.push('\n');
                    out.push_str(&::confit_binding::stub::emit_method_field(&method));
                }
                out
            }

            fn methods() -> Vec<::confit_binding::userdata::MethodDescriptor> {
                vec![#(#faces),*]
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) -> ::confit_binding::error::Result<()> {
                if collector.register(Self::class_name().to_owned(), Self::class_stub()) {
                    #(#collectors)*
                }
                ::std::result::Result::Ok(())
            }
        }

        #block
    })
}
