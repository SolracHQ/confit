//! Failure report without the error.

use confit_differ::view::View;
use extend::ext;

use crate::shared::{
    components::{
        strings::RESULTS,
        style::strExt as _,
        tables::{DOCUMENTS_LABEL, DocCounts, HOOKS_LABEL, clause, error_hook_counts, hints},
    },
    state::{HookLog, OutcomeMap},
};

#[ext(name = FailureStage)]
pub(crate) impl View {
    /// Builds the failure report without the error.
    fn failure(&self, outcomes: &OutcomeMap, hooks: &HookLog) -> Vec<String> {
        let mut out = vec![String::new(), RESULTS.title(), String::new()];
        let docs = outcomes.doc_counts();
        out.push(format!(
            "{} {}",
            DOCUMENTS_LABEL,
            clause(&docs, outcomes.pending(self),)
        ));
        let counts = error_hook_counts(self, hooks);
        if !counts.is_empty() {
            let executed = DocCounts {
                added: counts.executed.added,
                changed: counts.executed.changed,
                removed: counts.executed.destroyed,
                failed: 0,
            };
            out.push(format!(
                "{} {}",
                HOOKS_LABEL,
                clause(&executed, counts.pending,)
            ));
        }
        out.extend(hints(false, true, false));
        out
    }
}
