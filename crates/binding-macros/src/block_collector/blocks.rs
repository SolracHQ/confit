//! Block expansion
//! Userdata plus namespace expansion for impl blocks.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, Type, ext::IdentExt};

use super::collect::{has_receiver, parse_method, parse_static};
use super::emit::{descriptor_of, method_of, static_function_of, static_stub_block};

/// Strip face markers from one impl block for output.
fn strip_face_attrs(block: &ItemImpl) -> ItemImpl {
    let mut clean = block.clone();
    for item in &mut clean.items {
        if let ImplItem::Fn(method) = item {
            method.attrs.retain(|attr| !attr.path().is_ident("face"));
            for input in &mut method.sig.inputs {
                if let FnArg::Typed(typed) = input {
                    typed.attrs.retain(|attr| !attr.path().is_ident("face"));
                }
            }
        }
    }
    clean
}

/// Build the collection expansion for one checked block.
pub(crate) fn expand_block(
    block: &ItemImpl,
    namespace: Option<String>,
) -> Result<Tokens, syn::Error> {
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
            "inherent impl blocks are expected for `#[face_impl]`",
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
    match namespace {
        None => expand_userdata_block(block, &class_name),
        Some(space) => expand_namespace_block(block, &class_name, &space),
    }
}

/// Build userdata collection for one block without a namespace.
fn expand_userdata_block(block: &ItemImpl, class_name: &str) -> Result<Tokens, syn::Error> {
    let self_ty = &block.self_ty;
    let mut faces = Vec::new();
    let mut registrations: Vec<Tokens> = Vec::new();
    let mut collectors: Vec<Tokens> = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                if !has_receiver(method) {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name}.{}: static methods read a namespace; add namespace to #[face_impl]",
                            method.sig.ident.unraw()
                        ),
                    ));
                }
                let face = parse_method(class_name, method)?;
                for param in &face.params {
                    let shape = &param.shape;
                    collectors.push(quote! {
                        <#shape as ::confit_binding::face::Face>::collect(collector)?;
                    });
                }
                if let Some(returns) = &face.returns {
                    collectors.push(quote! {
                        <#returns as ::confit_binding::face::Face>::collect(collector)?;
                    });
                }
                registrations.push(method_of(&face));
                faces.push(descriptor_of(&face));
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
    let clean = strip_face_attrs(block);
    Ok(quote! {
        impl ::mlua::UserData for #self_ty {
            fn add_methods<M: ::mlua::UserDataMethods<Self>>(methods: &mut M) {
                #(#registrations)*
            }
        }

        impl ::confit_binding::userdata::UserdataClass for #self_ty {
            fn class_name() -> &'static str {
                #class_name
            }

            fn class_stub() -> String {
                let mut out =
                    <Self as ::confit_binding::face::Face>::face_stub();
                out.push('\n');
                out.push_str(Self::class_name());
                out.push_str(" = {}");
                for method in Self::methods() {
                    out.push_str("\n\n");
                    out.push_str(&::confit_binding::stub::emit_method(
                        Self::class_name(),
                        &method,
                    ));
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

        #clean
    })
}

/// Build namespace collection for one static-only block.
fn expand_namespace_block(
    block: &ItemImpl,
    class_name: &str,
    namespace: &str,
) -> Result<Tokens, syn::Error> {
    let self_ty = &block.self_ty;
    let mut statics = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                if has_receiver(method) {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name} holds &self method {} inside a namespace block; namespace blocks hold statics alone",
                            method.sig.ident.unraw()
                        ),
                    ));
                }
                statics.push(parse_static(class_name, method)?);
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
    let verbs: Vec<String> = statics.iter().map(|entry| entry.name.clone()).collect();
    let mut builders = Vec::new();
    let mut stubs = Vec::new();
    for face in &statics {
        builders.push(static_function_of(face));
        stubs.push(static_stub_block(face));
    }
    let register = if statics.len() == 1 {
        let builder = &builders[0];
        quote! {
            /// Build the bare namespace function value.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn register(lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Value> {
                #builder.map(::mlua::Value::Function)
            }
        }
    } else {
        let names: Vec<String> = statics.iter().map(|entry| entry.name.clone()).collect();
        quote! {
            /// Build the read-only namespace table.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn register(lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Value> {
                let table = lua.create_table()?;
                #(table.set(#names, #builders?)?;)*
                ::confit_binding::namespace::readonly(lua, Self::NAMESPACE, table)
            }
        }
    };
    let clean = strip_face_attrs(block);
    Ok(quote! {
        impl #self_ty {
            /// The dotted Lua path.
            pub const NAMESPACE: &'static str = #namespace;
            /// The verb names in registration order.
            pub const VERBS: &[&str] = &[#(#verbs),*];

            #register

            /// Collect every static face with all referenced faces.
            ///
            /// # Errors
            ///
            /// - [`::confit_binding::error::BindingError`] for repeated registration.
            pub fn stubs(
                collector: &mut ::confit_binding::registry::Registry,
            ) -> ::confit_binding::error::Result<()> {
                #(#stubs)*
                ::std::result::Result::Ok(())
            }
        }

        #clean
    })
}
