//! Assembly
//!
//! Owned declarations into finished documents.

use confit_model::document::Document;

use confit_surface_api::Declaration;

use crate::error::{EngineError, Result};

/// Collects owned declarations into finished documents in
/// destination display order.
///
/// # Errors
///
/// - [`EngineError::Duplicate`] for repeated destinations.
/// - [`EngineError::Model`] for documents failing hashes.
pub fn assemble(declarations: Vec<Declaration>) -> Result<Vec<Document>> {
    let mut seen = std::collections::BTreeMap::new();
    for declaration in declarations {
        let display = declaration.destination().display();
        if seen.contains_key(&display) {
            return Err(EngineError::Duplicate { display });
        }
        seen.insert(display, declaration);
    }
    let mut documents = Vec::with_capacity(seen.len());
    for (_, declaration) in seen {
        let display = declaration.destination().display();
        let mut document = declaration.into_document();
        document
            .fill_hash()
            .map_err(|error| confit_model::error::Error::Unhashable {
                document: display,
                reason: error.to_string(),
            })?;
        documents.push(document);
    }
    Ok(documents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::routes::{Route, RouteBase};
    use confit_surface_api::{Body, Declaration, TextBody};

    fn declaration(relative: &str, text: &str) -> Declaration {
        Declaration::new(
            Route::new(RouteBase::Home, relative).unwrap(),
            Body::Text(TextBody {
                text: text.to_owned(),
            }),
        )
    }

    #[test]
    fn duplicates_fail_naming_destination() {
        match assemble(vec![
            declaration(".bashrc", "one"),
            declaration(".bashrc", "two"),
        ]) {
            Ok(_) => panic!("duplicate destinations pass"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("home:.bashrc"),
                    "names the destination: {text}"
                );
            }
        }
    }

    #[test]
    fn survivors_order_by_destination_display() {
        match assemble(vec![declaration("zebra", "z"), declaration("apple", "a")]) {
            Ok(documents) => {
                assert_eq!(documents.len(), 2);
                assert_eq!(documents[0].destination.display(), "home:apple");
                assert_eq!(documents[1].destination.display(), "home:zebra");
                assert!(
                    !documents[0].data_hash.is_empty(),
                    "hashes fill past the edge"
                );
            }
            Err(error) => panic!("distinct destinations assemble: {error}"),
        }
    }
}
