//! Row and group labels.

use confit_differ::{
    change::{Change, Comparability},
    view::View,
};
use confit_model::document::{Data, Document};

use crate::shared::components::{
    strings::IncomparableReasonExt,
    style::Symbol,
    tables::{DocCounts, KindCounts as _, clause},
};

/// Text document kind.
pub(crate) const KIND_TEXT: &str = "text";
/// Link document kind.
pub(crate) const KIND_LINK: &str = "link";
/// Rc document kind.
pub(crate) const KIND_RC: &str = "rc";
/// Opaque document kind.
pub(crate) const KIND_OPAQUE: &str = "opaque";
/// Tree group kind.
pub(crate) const KIND_TREE: &str = "tree";

/// Returns the plan symbol.
pub(crate) fn row_symbol(change: &Change) -> Symbol {
    match &change.comparability {
        Comparability::Incomparable(_) => Symbol::Query,
        Comparability::Comparable => change.kind.into(),
    }
}

/// Returns the row text.
pub(crate) fn row_text(view: &View, change: &Change, name: &str) -> String {
    if let Comparability::Incomparable(reason) = &change.comparability {
        return format!("{name}: {}", reason.text());
    }
    match view.find_document(&change.path) {
        Some(document) => format!("{name}: {}", kind_label(document)),
        None => name.to_owned(),
    }
}

/// Returns the preview group text.
pub(crate) fn preview_group_text(members: &[&Change], name: &str) -> String {
    format!("{name}: {KIND_TREE} {}", preview_clause(members))
}

/// Returns the preview clause.
pub(crate) fn preview_clause(members: &[&Change]) -> String {
    let (added, changed, removed) = members.kind_counts();
    let docs = DocCounts {
        added,
        changed,
        removed,
        failed: 0,
    };
    clause(&docs, 0)
}

/// Returns the document kind label.
pub(crate) fn kind_label(document: &Document) -> &'static str {
    match &document.data {
        Data::Structured(found) => found.format.name(),
        Data::Text(_) => KIND_TEXT,
        Data::Link(_) => KIND_LINK,
        Data::Rc(_) => KIND_RC,
        Data::Opaque(_) => KIND_OPAQUE,
        Data::Tree(_) => KIND_TREE,
    }
}
