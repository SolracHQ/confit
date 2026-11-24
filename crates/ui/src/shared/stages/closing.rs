//! Closing table and restore hints.

use confit_differ::view::View;
use extend::ext;

use crate::shared::{
    components::{
        strings::RESULTS,
        style::strExt as _,
        tables::{hints, hook_counts, results_table},
    },
    state::OutcomeMap,
};

#[ext(name = ClosingStage)]
pub(crate) impl View {
    /// Builds the closing table and restore hints.
    fn close(
        &self,
        outcomes: &OutcomeMap,
        undo: bool,
        rescued: bool,
        drifted: bool,
    ) -> Vec<String> {
        let docs = outcomes.doc_counts();
        let hooks = hook_counts(self);
        let mut out = Vec::new();
        if docs.added > 0
            || docs.changed > 0
            || docs.removed > 0
            || docs.failed > 0
            || !hooks.is_empty()
        {
            out.push(String::new());
            out.push(RESULTS.title());
            out.push(String::new());
            out.extend(results_table(&docs, &hooks).lines().map(str::to_owned));
            out.push(String::new());
        }
        out.extend(hints(undo, rescued, drifted));
        out
    }
}
