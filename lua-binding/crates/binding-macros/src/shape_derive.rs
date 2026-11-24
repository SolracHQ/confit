//! Dispatches the `Shape` derive to the expansion for each strategy.

use proc_macro::TokenStream;
use syn::{Data, DeriveInput, parse_macro_input};

use super::enum_object::expand_enum_object;
use super::parse::{AliasStrategy, ShapeStrategy, parse_header};
use super::probe::expand_probe_enum;
use super::signature::expand_struct_function;
use super::str_alias::expand_str_alias_enum;
use super::structs::{expand_struct_object, expand_struct_proxy, expand_struct_table};
use super::typed_alias::expand_typed_alias_enum;
use super::untagged::expand_untagged_enum;

/// Expands the Shape derive.
pub fn expand(input: TokenStream) -> TokenStream {
    let declaration = parse_macro_input!(input as DeriveInput);
    let identifier = declaration.ident.clone();
    let expanded = match &declaration.data {
        Data::Struct(data) => match parse_header(&identifier, &declaration.attrs, false) {
            Ok(header) => {
                if let Some(proxy) = header.proxy.as_ref() {
                    expand_struct_proxy(&identifier, proxy)
                } else {
                    match header.strategy {
                        Some(ShapeStrategy::Table) => expand_struct_table(
                            &identifier,
                            data,
                            &declaration.attrs,
                            &header.validate,
                            &header.name,
                        ),
                        Some(ShapeStrategy::Function) => expand_struct_function(
                            &identifier,
                            data,
                            &declaration.attrs,
                            &header.validate,
                            &header.name,
                            &header.returns,
                        ),
                        Some(ShapeStrategy::Object) => expand_struct_object(
                            &identifier,
                            data,
                            &declaration.attrs,
                            &header.validate,
                            &header.name,
                            header.traced,
                        ),
                        None => Err(syn::Error::new_spanned(
                            &identifier,
                            format!("{identifier}: Shape derive wants a strategy marker"),
                        )),
                    }
                }
            }
            Err(error) => Err(error),
        },
        Data::Enum(data) => match parse_header(&identifier, &declaration.attrs, true) {
            Ok(header) => match (header.strategy, header.alias) {
                (Some(ShapeStrategy::Object), None) => expand_enum_object(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.name,
                ),
                (Some(_), Some(AliasStrategy::StrAlias)) => expand_str_alias_enum(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.name,
                ),
                (Some(_), Some(AliasStrategy::Untagged)) => expand_untagged_enum(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.probe,
                    &header.validate,
                    &header.name,
                ),
                (Some(_), Some(AliasStrategy::TypedAlias)) => expand_typed_alias_enum(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.name,
                ),
                (Some(_), Some(AliasStrategy::Probe)) => expand_probe_enum(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.name,
                ),
                _ => Err(syn::Error::new_spanned(
                    &identifier,
                    format!(
                        "{}: Shape derive wants one alias marker with `table`: \
                         str_alias, untagged, typed_alias, or probe",
                        identifier
                    ),
                )),
            },
            Err(error) => Err(error),
        },
        _ => Err(syn::Error::new_spanned(
            &identifier,
            format!("{identifier}: structs and enums are expected for Shape derive"),
        )),
    };
    match expanded {
        Ok(tokens) => {
            if std::env::var("SHAPE_DERIVE_DEBUG").is_ok() {
                eprintln!("Shape derive expansion for {identifier}:\n{tokens}");
            }
            tokens.into()
        }
        Err(error) => error.to_compile_error().into(),
    }
}
