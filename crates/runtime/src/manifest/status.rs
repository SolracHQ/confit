//! Lifecycle status against previous manifests.

use std::collections::{BTreeMap, BTreeSet};

use confit_core::sha::Sha;
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
pub fn status(
    document: &Document,
    previous: &[Document],
    hashes: &BTreeMap<String, Sha>,
    previous_hashes: &BTreeMap<String, Sha>,
) -> DocumentStatus {
    let recorded = previous
        .iter()
        .find(|recorded| key(recorded) == key(document));
    match recorded {
        None => match find_opaque_sibling(previous, document) {
            Some(_) => DocumentStatus::Update,
            None => DocumentStatus::Create,
        },
        Some(recorded)
            if hashes.get(&document.path.display())
                == previous_hashes.get(&recorded.path.display())
                && mode(recorded) == mode(document) =>
        {
            DocumentStatus::Unchanged
        }
        Some(_) => DocumentStatus::Update,
    }
}

/// Returns whether a recorded document yields to the desired documents.
pub fn superseded_by(document: &Document, desired: &[Document]) -> bool {
    find_opaque_sibling(desired, document).is_some()
}

/// Returns the kind key with the display path.
pub fn key(document: &Document) -> String {
    let kind = match &document.data {
        Data::Structured(_) => "structured",
        Data::Text(_) => "text",
        Data::Link(_) => "link",
        Data::Rc(_) => "rc",
        Data::Opaque(_) => "opaque",
        Data::Tree(_) => "tree",
    };
    format!("{kind}:{}", document.path.display())
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

/// Finds one opaque sibling at the same destination with a different kind.
pub fn find_opaque_sibling<'a>(
    documents: &'a [Document],
    document: &Document,
) -> Option<&'a Document> {
    documents.iter().find(|recorded| {
        recorded.path == document.path
            && key(recorded) != key(document)
            && (is_opaque(recorded) || is_opaque(document))
    })
}

/// Counts the documents in each lifecycle state against the previous documents.
pub fn summary(
    built: &[Document],
    built_hashes: &BTreeMap<String, Sha>,
    previous: &[Document],
    previous_hashes: &BTreeMap<String, Sha>,
) -> Summary {
    let mut counts = Summary {
        create: 0,
        update: 0,
        delete: 0,
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for document in built {
        seen.insert(key(document));
        match status(document, previous, built_hashes, previous_hashes) {
            DocumentStatus::Create => counts.create += 1,
            DocumentStatus::Update => counts.update += 1,
            DocumentStatus::Unchanged => {}
        }
    }
    for recorded in previous {
        if !seen.contains(&key(recorded)) && !superseded_by(recorded, built) {
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
        let counts = summary(&built, &BTreeMap::new(), &previous, &BTreeMap::new());
        assert_eq!(
            (counts.create, counts.update, counts.delete),
            (1, 0, 1),
            "the built document is a create and the previous document is a delete"
        );
    }
}
