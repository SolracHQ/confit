//! Manifest compare.
//! Lifecycle status against previous manifests.

use std::collections::{BTreeMap, BTreeSet};

use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;
use confit_driver::sha::Sha;
use confit_model::document::Data;
use mlua::{Lua, Value};

use confit_model::document::Document;

/// Per-document lifecycle status against previous documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table, string)]
pub enum DocumentStatus {
    /// Document absent from previous documents.
    Create,
    /// Document present with a differing data hash.
    Update,
    /// Document present with an equal data hash.
    Unchanged,
}

/// Lifecycle counts for one bundle against previous documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table)]
pub struct Summary {
    /// Documents absent from previous documents.
    #[face(proxy = u64, decode_hook = decode_count)]
    pub create: usize,
    /// Documents with a differing data hash.
    #[face(proxy = u64, decode_hook = decode_count)]
    pub update: usize,
    /// Previous documents missing from the built documents.
    #[face(proxy = u64, decode_hook = decode_count)]
    pub delete: usize,
}

/// Report the lifecycle status against previous documents.
///
/// - Hashes arrive keyed by path display from both manifests.
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

/// Report whether a recorded document yields to desired documents.
///
/// The flag is true while an opaque same-destination sibling exists in desired.
pub fn superseded_by(document: &Document, desired: &[Document]) -> bool {
    find_opaque_sibling(desired, document).is_some()
}

/// Build one kind with display key.
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

/// Report whether the document carries opaque bytes.
pub fn is_opaque(document: &Document) -> bool {
    matches!(document.data, Data::Opaque(_))
}

/// Report the document mode when the kind carries one.
pub fn mode(document: &Document) -> Option<u32> {
    match &document.data {
        Data::Text(found) => Some(found.mode),
        Data::Opaque(found) => Some(found.mode),
        Data::Structured(_) | Data::Link(_) | Data::Rc(_) | Data::Tree(_) => None,
    }
}

/// Find one opaque same-destination sibling with a different kind.
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

/// Count lifecycle states against previous documents.
///
/// - Opaque kind changes count as updates, other kind changes count as create
///   and delete.
/// - Hashes arrive keyed by path display from both manifests.
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

/// Decode one count field into its size value.
///
/// # Errors
///
/// - [`Mismatch`] for values outside the count shape.
fn decode_count(lua: &Lua, value: &Value) -> std::result::Result<usize, Mismatch> {
    let found = u64::decode(value, lua)?;
    usize::try_from(found).map_err(|_| Mismatch {
        path: "count".to_owned(),
        want: "a small non-negative integer".to_owned(),
        found: found.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::TextDocument;
    use confit_model::routes::{Route, RouteBase};

    #[test]
    fn summary_counts_create_and_delete() {
        let doc = |name: &str| Document {
            path: Route::new(RouteBase::Home, name).unwrap(),
            data: Data::Text(TextDocument {
                content: "hi".into(),
                mode: 0o644,
                unmanaged: false,
            }),
        };
        let built = vec![doc("fresh")];
        let previous = vec![doc("stale")];
        let counts = summary(&built, &BTreeMap::new(), &previous, &BTreeMap::new());
        assert_eq!(
            (counts.create, counts.update, counts.delete),
            (1, 0, 1),
            "fresh is create and stale is delete"
        );
    }
}
