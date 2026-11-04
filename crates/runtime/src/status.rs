//! Lifecycle status against previous manifests.

use std::collections::BTreeSet;

use confit_model::document::Data;
use confit_model::document::Document;

/// The lifecycle status of one document against previous documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    /// A document that the previous documents lack.
    Create,
    /// A document whose data hash differs from the previous one.
    Update,
    /// A document whose data hash matches the previous one.
    Unchanged,
}

/// The document counts against the previous documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// The count of documents absent from the previous documents.
    pub create: usize,
    /// The count of documents with a differing data hash.
    pub update: usize,
    /// The count of previous documents absent from the built documents.
    pub delete: usize,
}

/// Returns the lifecycle status of one document against the previous documents.
pub fn status(document: &Document, previous: &[Document]) -> DocumentStatus {
    let recorded = previous
        .iter()
        .find(|recorded| recorded.path == document.path);
    match recorded {
        None => DocumentStatus::Create,
        Some(recorded) => {
            let same_mode = mode(recorded) == mode(document);
            if same_mode && hash_matches(document, recorded) {
                return DocumentStatus::Unchanged;
            }
            if mode(recorded) != mode(document) {
                return DocumentStatus::Update;
            }
            if is_opaque(document) || is_opaque(recorded) {
                return DocumentStatus::Update;
            }
            match (recorded.render(), document.render()) {
                (Ok(old_bytes), Ok(new_bytes)) if old_bytes == new_bytes => {
                    DocumentStatus::Unchanged
                }
                _ => DocumentStatus::Update,
            }
        }
    }
}

/// Returns whether both documents hash to the same content hash.
fn hash_matches(document: &Document, recorded: &Document) -> bool {
    match (document.hash(), recorded.hash()) {
        (Ok(next), Ok(old)) => next == old,
        _ => false,
    }
}

/// Returns the display path that keys one document.
pub fn key(document: &Document) -> String {
    document.path.display()
}

/// Returns whether the document carries opaque bytes.
pub fn is_opaque(document: &Document) -> bool {
    matches!(document.data, Data::Opaque(_))
}

/// Returns the document mode when its kind carries one.
pub fn mode(document: &Document) -> Option<u32> {
    match &document.data {
        Data::Text(found) => Some(found.mode.bits()),
        Data::Opaque(found) => Some(found.mode.bits()),
        Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => None,
    }
}

/// Counts the documents in each lifecycle state against the previous documents.
pub fn summary(built: &[Document], previous: &[Document]) -> Summary {
    let mut counts = Summary {
        create: 0,
        update: 0,
        delete: 0,
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for document in built {
        seen.insert(key(document));
        match status(document, previous) {
            DocumentStatus::Create => counts.create += 1,
            DocumentStatus::Update => counts.update += 1,
            DocumentStatus::Unchanged => {}
        }
    }
    for recorded in previous {
        if !seen.contains(&key(recorded)) {
            counts.delete += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::{FileMode, TextDocument};
    use confit_model::routes::{Route, RouteBase};

    #[test]
    fn summary_counts_create_and_delete() {
        let doc = |name: &str| Document {
            path: Route::new(RouteBase::Home, name).unwrap(),
            data: Data::Text(TextDocument {
                content: "hi".into(),
                mode: FileMode::new(0o644),
                unmanaged: false,
            }),
        };
        let built = vec![doc("fresh")];
        let previous = vec![doc("stale")];
        let counts = summary(&built, &previous);
        assert_eq!(
            (counts.create, counts.update, counts.delete),
            (1, 0, 1),
            "the built document is a create and the previous document is a delete"
        );
    }
}
