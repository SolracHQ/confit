//! Block expansion
//! Userdata and namespace expansion for impl blocks.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, Type, ext::IdentExt};

use super::attribute::{ImplMode, parse_face_meta};
use super::collect::{check_meta, has_receiver, parse_method, parse_static};
use super::emit::{
    descriptor_of, meta_method_of, method_of, static_function_of, static_stub_block,
};

/// Strip face markers from one impl block for output.
fn strip_face_attrs(block: &ItemImpl) -> ItemImpl {
    let mut clean = block.clone();
    for item in &mut clean.items {
        if let ImplItem::Fn(method) = item {
            method
                .attrs
                .retain(|attr| !attr.path().is_ident("face") && !attr.path().is_ident("face_meta"));
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
pub(crate) fn expand_block(block: &ItemImpl, mode: ImplMode) -> Result<Tokens, syn::Error> {
    if !block.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &block.generics,
            "`#[face_impl]` expected impl blocks with no generics",
        ));
    }
    if let Some(clause) = &block.generics.where_clause {
        return Err(syn::Error::new_spanned(
            clause,
            "`#[face_impl]` expected impl blocks with no where clause",
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
            "`#[face_impl]` expected a simple path type",
        ));
    };
    let Some(last) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "`#[face_impl]` expected a simple path type",
        ));
    };
    if !matches!(last.arguments, syn::PathArguments::None) {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            "`#[face_impl]` expected a simple path type",
        ));
    }
    let class_name = last.ident.to_string();
    match mode {
        ImplMode::Userdata => expand_userdata_block(block, &class_name),
        ImplMode::Namespace(space) => expand_namespace_block(block, &class_name, &space),
        ImplMode::Global => expand_global_block(block, &class_name),
    }
}

/// Build userdata collection for one receiver block.
fn expand_userdata_block(block: &ItemImpl, class_name: &str) -> Result<Tokens, syn::Error> {
    let self_ty = &block.self_ty;
    let mut faces = Vec::new();
    let mut registrations: Vec<Tokens> = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                let hook = parse_face_meta(class_name, method)?;
                if let Some(hook) = hook {
                    if method.attrs.iter().any(|attr| attr.path().is_ident("face")) {
                        return Err(syn::Error::new_spanned(
                            &method.sig.ident,
                            format!(
                                "{class_name}.{}: face options stay apart from face_meta methods",
                                method.sig.ident.unraw()
                            ),
                        ));
                    }
                    let face = parse_method(class_name, method)?;
                    check_meta(class_name, &face)?;
                    registrations.push(meta_method_of(&face, &hook));
                } else {
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
                    registrations.push(method_of(&face));
                    faces.push(descriptor_of(&face));
                }
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

            fn methods() -> Vec<::confit_binding::userdata::MethodDescriptor> {
                vec![#(#faces),*]
            }

            fn collect(
                collector: &mut ::confit_binding::registry::Registry,
            ) {
                let methods = Self::methods();
                let face = <Self as ::confit_binding::face::Face>::face();
                if collector.register_userdata(
                    Self::class_name(),
                    face.documentation(),
                    methods.clone(),
                ) {
                    for method in &methods {
                        method.collect_referenced(collector);
                    }
                }
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
                if parse_face_meta(class_name, method)?.is_some() {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name}.{}: face_meta methods need userdata blocks alone",
                            method.sig.ident.unraw()
                        ),
                    ));
                }
                if has_receiver(method) {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name} types hold &self method {} inside a namespace block; namespace blocks hold statics alone",
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
            pub fn stubs(
                collector: &mut ::confit_binding::registry::Registry,
            ) {
                #(#stubs)*
            }

            /// Build the stub document for this namespace.
            pub fn stub_document() -> ::confit_binding::stub::StubDocument {
                let mut collector = ::confit_binding::registry::Registry::new();
                Self::stubs(&mut collector);
                ::confit_binding::stub::module_document(Self::NAMESPACE, Self::VERBS, &collector)
            }
        }

        #clean
    })
}

/// Build global collection for one static-only block.
fn expand_global_block(block: &ItemImpl, class_name: &str) -> Result<Tokens, syn::Error> {
    let self_ty = &block.self_ty;
    let mut statics = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                if parse_face_meta(class_name, method)?.is_some() {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name}.{}: face_meta methods need userdata blocks alone",
                            method.sig.ident.unraw()
                        ),
                    ));
                }
                if has_receiver(method) {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name} types hold &self method {} inside a namespace block; namespace blocks hold statics alone",
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
    let globals: Vec<String> = statics.iter().map(|entry| entry.name.clone()).collect();
    let mut builders = Vec::new();
    let mut stubs = Vec::new();
    for face in &statics {
        builders.push(static_function_of(face));
        stubs.push(static_stub_block(face));
    }
    let clean = strip_face_attrs(block);
    Ok(quote! {
        impl #self_ty {
            /// The global names in registration order.
            pub const GLOBALS: &[&str] = &[#(#globals),*];

            /// Install every global into one chunk environment.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn install(
                lua: &::mlua::Lua,
                env: &::mlua::Table,
            ) -> ::mlua::Result<()> {
                #(env.set(#globals, #builders?)?;)*
                ::std::result::Result::Ok(())
            }

            /// Collect every static face with all referenced faces.
            pub fn stubs(
                collector: &mut ::confit_binding::registry::Registry,
            ) {
                #(#stubs)*
            }

            /// Build the globals stub document.
            pub fn stub_document() -> ::confit_binding::stub::StubDocument {
                let mut collector = ::confit_binding::registry::Registry::new();
                Self::stubs(&mut collector);
                ::confit_binding::stub::globals_document("confit", &collector)
            }
        }

        #clean
    })
}
