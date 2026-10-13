//! Block collector
//! Impl-block method collection for userdata classes.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, Pat, ReturnType, Type, ext::IdentExt, parse_macro_input};

use super::parse::doc_lines;

/// One collected method parameter with its stub face.
struct ParamFace {
    /// The parameter label.
    label: String,
    /// The parameter binding.
    binding: Pat,
    /// The parameter shape.
    shape: Type,
}

/// One collected method with its stub faces.
struct MethodFace {
    /// The method ident.
    ident: syn::Ident,
    /// The method name.
    name: String,
    /// The method documentation.
    docs: String,
    /// The collected parameters.
    params: Vec<ParamFace>,
    /// The unwrapped return shape.
    returns: Option<Type>,
    /// The Lua context marker.
    takes_lua: bool,
    /// The fallible return marker.
    fallible: bool,
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

/// Report whether one type contains the Lua context in any shape.
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

/// Report whether one type reads as a result.
fn is_result(ty: &Type) -> bool {
    if let Type::Path(path) = ty
        && let Some(last) = path.path.segments.last()
    {
        return last.ident == "Result";
    }
    false
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
        params.push(ParamFace {
            label,
            binding: typed.pat.as_ref().clone(),
            shape: typed.ty.as_ref().clone(),
        });
    }
    let (returns, fallible) = match &method.sig.output {
        ReturnType::Default => (None, false),
        ReturnType::Type(_, ty) => (Some(unwrap_result(ty)), is_result(ty)),
    };
    Ok(MethodFace {
        ident: method.sig.ident.clone(),
        name: method.sig.ident.unraw().to_string(),
        docs: doc_lines(&method.attrs).join(" "),
        params,
        returns,
        takes_lua,
        fallible,
    })
}

/// Report whether one type reads as a tuple return.
fn is_tuple(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(_))
}

/// Build the element indices of one tuple return.
fn tuple_indices(ty: &Type) -> Vec<syn::Index> {
    let Type::Tuple(tuple) = ty else {
        return Vec::new();
    };
    (0..tuple.elems.len()).map(syn::Index::from).collect()
}

/// Build the descriptor tokens for one collected method.
fn descriptor_of(method: &MethodFace) -> Tokens {
    let name = &method.name;
    let docs = &method.docs;
    let labels = method.params.iter().map(|entry| &entry.label);
    let types = method.params.iter().map(|entry| &entry.shape);
    let returns: Tokens = match &method.returns {
        Some(Type::Tuple(tuple)) => {
            let elems: Vec<&Type> = tuple.elems.iter().collect();
            quote! {
                #(::std::borrow::Cow::Owned(
                    <#elems as ::confit_binding::face::Face>::face_name(),
                )),*
            }
        }
        Some(returns) => quote! {
            ::std::borrow::Cow::Owned(
                <#returns as ::confit_binding::face::Face>::face_name(),
            )
        },
        None => quote! {
            ::std::borrow::Cow::Owned(::std::string::String::from("nil"))
        },
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
                }),*
            ],
            returns: vec![#returns],
        }
    }
}

/// Build the runtime registration for one collected method.
fn method_of(method: &MethodFace) -> Tokens {
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
        let slot: Tokens = match &param.binding {
            Pat::Ident(_) => {
                let binding = &param.binding;
                quote! { #binding }
            }
            _ => quote! { #caller },
        };
        takes.push(quote! {
            let #slot: #shape = match ::confit_binding::value::take_arg(&args, &mut next) {
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
    let mut registrations: Vec<Tokens> = Vec::new();
    let mut collectors: Vec<Tokens> = Vec::new();
    for item in &block.items {
        match item {
            ImplItem::Fn(method) => {
                let face = parse_method(&class_name, method)?;
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

        #block
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expansion_of(source: &str) -> String {
        let block: ItemImpl = syn::parse_str(source).unwrap();
        expand_block(&block).unwrap().to_string()
    }

    fn rejection_of(source: &str) -> String {
        let block: ItemImpl = syn::parse_str(source).unwrap();
        expand_block(&block).unwrap_err().to_string()
    }

    #[test]
    fn lua_method_registers_decoded_params() {
        let output = expansion_of(
            "impl Roster {
                pub fn members(&self, lua: &Lua, prefix: String) -> mlua::Result<Vec<String>> {
                    let _ = (lua, prefix);
                    Ok(Vec::new())
                }
            }",
        );
        assert!(output.contains("add_methods"));
        assert!(output.contains("\"members\""));
        assert!(output.contains("take_arg"));
        assert!(output.contains("Decode"));
        assert!(output.contains("Encode"));
        assert!(output.contains("members.prefix"));
        assert!(output.contains("Variadic"));
    }

    #[test]
    fn bare_method_registers_without_params() {
        let output = expansion_of(
            "impl Roster {
                pub fn ping(&self) -> String {
                    String::new()
                }
            }",
        );
        assert!(output.contains("add_methods"));
        assert!(output.contains("\"ping\""));
        assert!(output.contains("Encode"));
        assert!(!output.contains("take_arg"));
    }

    #[test]
    fn pair_return_encodes_each_element() {
        let output = expansion_of(
            "impl Roster {
                pub fn pair(&self) -> mlua::Result<(Option<String>, Option<String>)> {
                    Ok((None, None))
                }
            }",
        );
        assert!(output.contains("MultiValue"));
        assert!(output.contains("push_back"));
        assert!(output.contains("outcome . 0"));
        assert!(output.contains("outcome . 1"));
    }

    #[test]
    fn shared_receivers_fail_the_build() {
        let mutable = rejection_of("impl Roster { pub fn bump(&mut self) {} }");
        assert!(mutable.contains("&self alone"));
        let owned = rejection_of("impl Roster { pub fn bump(self) {} }");
        assert!(owned.contains("&self alone"));
    }
}
