//! Manifest entries flattened to hash rows.

use std::collections::BTreeMap;

use confit_core::error::{ContextualCause, ContextualResult, contextual};
use confit_core::sha::Sha;
use confit_model::document::{Data, Document, FileMode, StructuredFormat};
use confit_model::manifest::Manifest;
use confit_model::routes::Route;
use extend::ext;

/// Comparison precision for one document pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rank {
    /// Only hash comparison.
    Opaque,
    /// Line comparison over decoded text.
    Text,
    /// Key comparison over parsed tables.
    Structured,
}

#[ext]
pub(crate) impl Data {
    /// Returns the comparison rank.
    fn rank(&self) -> Rank {
        match self {
            Self::Structured(_) => Rank::Structured,
            Self::Text(_) | Self::Link(_) | Self::Rc(_) => Rank::Text,
            Self::Opaque(_) | Self::Tree(_) => Rank::Opaque,
        }
    }

    /// Returns the manifest permission bits.
    fn mode(&self) -> FileMode {
        match self {
            Self::Text(found) => found.mode,
            Self::Opaque(found) => found.mode,
            Self::Structured(_) | Self::Link(_) | Self::Rc(_) | Self::Tree(_) => {
                FileMode::default()
            }
        }
    }

    /// Returns whether the content check covers existence only.
    fn unmanaged(&self) -> bool {
        match self {
            Self::Text(found) => found.unmanaged,
            Self::Opaque(found) => found.unmanaged,
            Self::Structured(_) | Self::Link(_) | Self::Rc(_) | Self::Tree(_) => false,
        }
    }

    /// Returns the table format.
    fn format(&self) -> Option<StructuredFormat> {
        match self {
            Self::Structured(found) => Some(found.format),
            Self::Text(_) | Self::Link(_) | Self::Opaque(_) | Self::Tree(_) | Self::Rc(_) => None,
        }
    }
}

#[ext]
pub(crate) impl Document {
    /// Builds one hash row for a non-tree document.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::RenderFailed`] when the
    ///   render fails.
    fn entry(&self) -> Result<Entry, ContextualCause> {
        let sha = self
            .hash()
            .map_err(|leaf| contextual(leaf).with_document(self.path.display()))?;
        Ok(Entry {
            path: self.path.clone(),
            group: None,
            sha,
            mode: self.data.mode(),
            unmanaged: self.data.unmanaged(),
            format: self.data.format(),
            link: matches!(self.data, Data::Link(_)),
        })
    }
}

/// One flattened manifest row.
pub(crate) struct Entry {
    /// Full route with tree members joined.
    pub(crate) path: Route,
    /// Tree route for member rows.
    pub(crate) group: Option<Route>,
    /// Content hash.
    pub(crate) sha: Sha,
    /// Permission bits.
    pub(crate) mode: FileMode,
    /// Content check by existence only.
    pub(crate) unmanaged: bool,
    /// Table format for structured parses.
    pub(crate) format: Option<StructuredFormat>,
    /// Disk comparison over the link target text.
    pub(crate) link: bool,
}

/// Flattens one manifest into hash rows keyed by route.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] when a
///   render fails.
pub(crate) fn rows(manifest: &Manifest) -> ContextualResult<BTreeMap<Route, Entry>> {
    let mut found = BTreeMap::new();
    for document in &manifest.documents {
        if let Data::Tree(tree) = &document.data {
            for (relative, member) in &tree.members {
                let path = document.path.join(relative.as_str());
                found.insert(
                    path.clone(),
                    Entry {
                        path,
                        group: Some(document.path.clone()),
                        sha: *member.handle().sha(),
                        mode: member.mode(),
                        unmanaged: false,
                        format: None,
                        link: false,
                    },
                );
            }
            continue;
        }
        let entry = document.entry()?;
        found.insert(entry.path.clone(), entry);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    use confit_core::error::{DocumentError, ErrorCause};
    use confit_core::resource_ref::ResourceRef;
    use confit_model::document::{MemberKey, StructuredDocument, TreeDocument, TreeMember};
    use confit_model::routes::RouteBase;

    /// Builds one literal route under the given folder.
    fn literal(dir: &std::path::Path, name: &str) -> Route {
        Route::new(RouteBase::Literal, dir.join(name)).unwrap()
    }

    /// Builds one empty manifest from documents.
    fn manifest(documents: Vec<Document>) -> Manifest {
        Manifest {
            version: [0, 9, 0],
            documents,
            hooks: Vec::new(),
        }
    }

    #[test]
    fn rows_expand_trees_to_member_rows() {
        let dir = tempfile::tempdir().unwrap();
        let tree = literal(dir.path(), "shared");
        let handle = ResourceRef::new(Sha::hash(b"member"));
        let document = Document::new(
            tree.clone(),
            Data::Tree(TreeDocument {
                members: [(
                    MemberKey::new("one.txt".to_owned()).unwrap(),
                    TreeMember::Handle(handle),
                )]
                .into_iter()
                .collect(),
            }),
        );
        let found = rows(&manifest(vec![document])).unwrap();
        let member = found.get(&tree.join("one.txt")).unwrap();
        assert_eq!(
            member.sha,
            *handle.sha(),
            "the member row keeps the handle hash"
        );
        assert_eq!(
            member.group,
            Some(tree.clone()),
            "the member row holds the tree group"
        );
        assert!(!found.contains_key(&tree), "only member rows surface");
    }

    #[test]
    fn rows_fail_on_unrenderable_documents() {
        let dir = tempfile::tempdir().unwrap();
        let document = Document::new(
            literal(dir.path(), "broken.toml"),
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Toml,
                data: [("empty".to_owned(), serde_json::Value::Null)]
                    .into_iter()
                    .collect(),
            }),
        );
        let error = match rows(&manifest(vec![document])) {
            Ok(_) => panic!("the broken document rows"),
            Err(error) => error,
        };
        assert!(
            matches!(
                error.cause(),
                ErrorCause::DocumentError(DocumentError::RenderFailed)
            ),
            "the broken document fails the execution: {error}"
        );
    }
}
