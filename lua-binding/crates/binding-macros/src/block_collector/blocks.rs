//! Expands Lua impl blocks into object, module, and global glue.

use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, Type, ext::IdentExt};

use super::attribute::{ImplTarget, parse_operator};
use super::collect::{check_operator, has_receiver, parse_method, parse_static};
use super::emit::{
    meta_method_of, method_doc_of, method_of, operator_doc_of, static_function_of,
    static_stub_block,
};

/// Strips shape markers.
fn strip_shape_attrs(block: &ItemImpl) -> ItemImpl {
    let mut clean = block.clone();
    for item in &mut clean.items {
        if let ImplItem::Fn(method) = item {
            method
                .attrs
                .retain(|attr| !attr.path().is_ident("shape") && !attr.path().is_ident("operator"));
            for input in &mut method.sig.inputs {
                if let FnArg::Typed(typed) = input {
                    typed.attrs.retain(|attr| !attr.path().is_ident("shape"));
                }
            }
        }
    }
    clean
}

/// Builds the collection expansion.
pub(crate) fn expand_block(block: &ItemImpl, mode: ImplTarget) -> Result<Tokens, syn::Error> {
    let attribute = mode.attribute();
    if !block.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &block.generics,
            format!("`#[{attribute}]` expects impl blocks with no generics"),
        ));
    }
    if let Some(clause) = &block.generics.where_clause {
        return Err(syn::Error::new_spanned(
            clause,
            format!("`#[{attribute}]` expects impl blocks with no where clause"),
        ));
    }
    if block.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            format!("inherent impl blocks are expected for `#[{attribute}]`"),
        ));
    }
    let Type::Path(path) = &*block.self_ty else {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            format!("`#[{attribute}]` expects a simple path type"),
        ));
    };
    let Some(last) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            format!("`#[{attribute}]` expects a simple path type"),
        ));
    };
    if !matches!(last.arguments, syn::PathArguments::None) {
        return Err(syn::Error::new_spanned(
            &block.self_ty,
            format!("`#[{attribute}]` expects a simple path type"),
        ));
    }
    let class_name = last.ident.to_string();
    match mode {
        ImplTarget::Object => expand_object_block(block, &class_name),
        ImplTarget::Module(space) => expand_module_block(block, &class_name, &space),
        ImplTarget::Global(space) => expand_global_block(block, &class_name, &space),
    }
}

/// Builds object collection.
fn expand_object_block(block: &ItemImpl, class_name: &str) -> Result<Tokens, syn::Error> {
    let self_ty = &block.self_ty;
    let mut shapes = Vec::new();
    let mut operators = Vec::new();
    let mut registrations: Vec<Tokens> = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                let hook = parse_operator(class_name, method)?;
                if let Some(hook) = hook {
                    if method
                        .attrs
                        .iter()
                        .any(|attr| attr.path().is_ident("shape"))
                    {
                        return Err(syn::Error::new_spanned(
                            &method.sig.ident,
                            format!(
                                "{class_name}.{}: shape options stay apart from operator methods",
                                method.sig.ident.unraw()
                            ),
                        ));
                    }
                    let shape = parse_method(class_name, method)?;
                    check_operator(class_name, &hook, &shape)?;
                    registrations.push(meta_method_of(&shape, &hook));
                    if let Some(doc) = operator_doc_of(&shape, &hook) {
                        operators.push(doc);
                    }
                } else {
                    if !has_receiver(method) {
                        return Err(syn::Error::new_spanned(
                            &method.sig.ident,
                            format!(
                                "{class_name}.{}: static functions need `#[lua_module]`",
                                method.sig.ident.unraw()
                            ),
                        ));
                    }
                    let shape = parse_method(class_name, method)?;
                    registrations.push(method_of(&shape));
                    shapes.push(method_doc_of(&shape));
                }
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    item,
                    format!(
                        "{class_name}: `#[lua_methods]` registers every method \
                         in its block; move non-method items out"
                    ),
                ));
            }
        }
    }
    let clean = strip_shape_attrs(block);
    Ok(quote! {
        impl ::mlua::UserData for #self_ty {
            fn add_fields<F: ::mlua::UserDataFields<Self>>(fields: &mut F) {
                <Self as ::lua_binding::object::ObjectFields>::add_fields(fields);
            }

            fn add_methods<M: ::mlua::UserDataMethods<Self>>(methods: &mut M) {
                #(#registrations)*
            }
        }

        impl ::lua_binding::object::Object for #self_ty {
            fn name() -> &'static str {
                #class_name
            }

            fn methods() -> Vec<::lua_binding::object::Method> {
                vec![#(#shapes),*]
            }

            fn fields() -> Vec<::lua_binding::object::ObjectField> {
                <Self as ::lua_binding::object::ObjectFields>::field_metadata()
            }

            fn operators() -> Vec<::lua_binding::object::Operator> {
                vec![#(#operators),*]
            }

            fn collect(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                let fields = Self::fields();
                let methods = Self::methods();
                let operators = Self::operators();
                let shape = <Self as ::lua_binding::shape::Shape>::shape_data();
                if collector.add_object(
                    <Self as ::lua_binding::object::Object>::name(),
                    shape.description(),
                    fields.clone(),
                    methods.clone(),
                    operators.clone(),
                ) {
                    for field in &fields {
                        field.collect_referenced(collector);
                    }
                    for method in &methods {
                        method.collect_referenced(collector);
                    }
                    for operator in &operators {
                        operator.collect_referenced(collector);
                    }
                }
            }
        }

        #clean
    })
}

/// Builds module collection.
fn expand_module_block(
    block: &ItemImpl,
    class_name: &str,
    module: &str,
) -> Result<Tokens, syn::Error> {
    let self_ty = &block.self_ty;
    let mut statics = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                if parse_operator(class_name, method)?.is_some() {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name}.{}: operator methods belong to object blocks",
                            method.sig.ident.unraw()
                        ),
                    ));
                }
                if has_receiver(method) {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name} types hold &self method {} in a module block; use static functions instead",
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
                        "{class_name}: `#[lua_module]` registers every function \
                         in its block; move other items out"
                    ),
                ));
            }
        }
    }
    let functions: Vec<String> = statics.iter().map(|entry| entry.name.clone()).collect();
    let mut builders = Vec::new();
    let mut stubs = Vec::new();
    for shape in &statics {
        builders.push(static_function_of(shape));
        stubs.push(static_stub_block(shape));
    }
    let add = if statics.len() == 1 {
        let builder = &builders[0];
        quote! {
            /// Creates the module function value.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn add(lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Value> {
                #builder.map(::mlua::Value::Function)
            }
        }
    } else {
        let names: Vec<String> = statics.iter().map(|entry| entry.name.clone()).collect();
        quote! {
            /// Creates the read-only module table.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn add(lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Value> {
                let table = lua.create_table()?;
                #(table.set(#names, #builders?)?;)*
                ::lua_binding::module::readonly(lua, Self::MODULE, table)
            }
        }
    };
    let clean = strip_shape_attrs(block);
    Ok(quote! {
        impl #self_ty {
            /// The dotted Lua path.
            pub const MODULE: &'static str = #module;
            /// The function names in addition order.
            pub const FUNCTIONS: &[&str] = &[#(#functions),*];

            #add

            /// Adds every static shape and its referenced shapes to the
            /// generator.
            pub fn stubs(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                #(#stubs)*
            }

            /// Renders the module stub.
            pub fn stub() -> ::lua_binding::stub::Stub {
                let mut collector = ::lua_binding::generator::Generator::new();
                Self::stubs(&mut collector);
                ::lua_binding::stub::generate_module(Self::MODULE, Self::FUNCTIONS, &collector)
            }
        }

        #clean
    })
}

/// Builds global collection.
fn expand_global_block(
    block: &ItemImpl,
    class_name: &str,
    stub: &str,
) -> Result<Tokens, syn::Error> {
    let self_ty = &block.self_ty;
    let mut statics = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                if parse_operator(class_name, method)?.is_some() {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name}.{}: operator methods belong to object blocks",
                            method.sig.ident.unraw()
                        ),
                    ));
                }
                if has_receiver(method) {
                    return Err(syn::Error::new_spanned(
                        &method.sig.ident,
                        format!(
                            "{class_name} types hold &self method {} in a globals block; use static functions instead",
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
                        "{class_name}: `#[lua_globals]` registers every function \
                         in its block; move other items out"
                    ),
                ));
            }
        }
    }
    let globals: Vec<String> = statics.iter().map(|entry| entry.name.clone()).collect();
    let mut builders = Vec::new();
    let mut stubs = Vec::new();
    for shape in &statics {
        builders.push(static_function_of(shape));
        stubs.push(static_stub_block(shape));
    }
    let clean = strip_shape_attrs(block);
    Ok(quote! {
        impl #self_ty {
            /// The global names in registration order.
            pub const GLOBALS: &[&str] = &[#(#globals),*];

            /// Installs every global into a chunk environment.
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

            /// Adds every static shape and its referenced shapes to the
            /// generator.
            pub fn stubs(
                collector: &mut ::lua_binding::generator::Generator,
            ) {
                #(#stubs)*
            }

            /// Renders the globals stub.
            pub fn stub() -> ::lua_binding::stub::Stub {
                let mut collector = ::lua_binding::generator::Generator::new();
                Self::stubs(&mut collector);
                ::lua_binding::stub::generate_globals(#stub, &collector)
            }
        }

        #clean
    })
}
