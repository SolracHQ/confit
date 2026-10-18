//! Face derive
//! Face derive expansion for table and userdata declarations.

use proc_macro::TokenStream;
use syn::{Data, DeriveInput, parse_macro_input};

use super::callback_shape::expand_struct_callback;
use super::literal::expand_string_enum;
use super::parse::{ContainerKind, UnionKind, parse_header};
use super::probe::expand_probe_enum;
use super::structs::{expand_struct_table, expand_struct_userdata};
use super::tagged::expand_tagged_enum;
use super::untagged::expand_untagged_enum;

/// Expand the Face derive for one declaration.
pub fn expand(input: TokenStream) -> TokenStream {
    let declaration = parse_macro_input!(input as DeriveInput);
    let identifier = declaration.ident.clone();
    let expanded = match &declaration.data {
        Data::Struct(data) => match parse_header(&identifier, &declaration.attrs, false) {
            Ok(header) => match header.kind {
                Some(ContainerKind::Table) => expand_struct_table(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.rename,
                ),
                Some(ContainerKind::CallbackShape) => expand_struct_callback(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.rename,
                    &header.returns,
                ),
                Some(ContainerKind::Userdata) => expand_struct_userdata(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.rename,
                ),
                None => Err(syn::Error::new_spanned(
                    &identifier,
                    format!("{identifier}: Face derive wants one kind marker"),
                )),
            },
            Err(error) => Err(error),
        },
        Data::Enum(data) => match parse_header(&identifier, &declaration.attrs, true) {
            Ok(header) => match (header.kind, header.union) {
                (Some(_), Some(UnionKind::String)) => expand_string_enum(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.rename,
                ),
                (Some(_), Some(UnionKind::Untagged)) => expand_untagged_enum(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.probe,
                    &header.validate,
                    &header.rename,
                ),
                (Some(_), Some(UnionKind::Tagged)) => expand_tagged_enum(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.rename,
                ),
                (Some(_), Some(UnionKind::Probe)) => expand_probe_enum(
                    &identifier,
                    data,
                    &declaration.attrs,
                    &header.validate,
                    &header.rename,
                ),
                _ => Err(syn::Error::new_spanned(
                    &identifier,
                    format!(
                        "{}: Face derive wants one union annotation, add \
                         #[face(string)], #[face(untagged)], #[face(tagged)], \
                         or #[face(probe)]",
                        identifier
                    ),
                )),
            },
            Err(error) => Err(error),
        },
        _ => Err(syn::Error::new_spanned(
            &identifier,
            format!("{identifier}: structs and enums are expected for Face derive"),
        )),
    };
    match expanded {
        Ok(tokens) => {
            if std::env::var("FACE_DERIVE_DEBUG").is_ok() {
                eprintln!("Face derive expansion for {identifier}:\n{tokens}");
            }
            tokens.into()
        }
        Err(error) => error.to_compile_error().into(),
    }
}
