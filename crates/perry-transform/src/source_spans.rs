//! Report-only source-span propagation for transforms that clone LocalIds.

use perry_hir::types::LocalId;
use perry_hir::{Expr, LocalSourceSpan};
use std::cell::RefCell;
use std::collections::HashMap;

struct ActiveRemaps {
    first_fresh_id: LocalId,
    pairs: Vec<(LocalId, LocalId)>,
}

thread_local! {
    static ACTIVE_REMAPS: RefCell<Option<ActiveRemaps>> = const { RefCell::new(None) };
}

/// Scoped collector used by a transform that recursively creates LocalIds.
///
/// The inliner is synchronous but modules may be transformed on different
/// rayon workers, so the collector is thread-local. Only ids at or above the
/// transform's initial fresh-id boundary are recorded: a parameter mapped to
/// an existing caller local must keep the caller's location, not acquire the
/// callee parameter's location.
pub(crate) struct RemapSession {
    finished: bool,
}

impl RemapSession {
    pub(crate) fn start(first_fresh_id: LocalId) -> Self {
        ACTIVE_REMAPS.with(|active| {
            assert!(
                active.borrow().is_none(),
                "nested source-span remap session"
            );
            let _ = active.borrow_mut().replace(ActiveRemaps {
                first_fresh_id,
                pairs: Vec::new(),
            });
        });
        Self { finished: false }
    }

    pub(crate) fn finish(mut self, spans: &mut HashMap<LocalId, LocalSourceSpan>) {
        let pairs = ACTIVE_REMAPS.with(|active| {
            active
                .borrow_mut()
                .take()
                .map(|state| state.pairs)
                .unwrap_or_default()
        });
        self.finished = true;

        // Nested inlining can clone a local that was itself minted by an
        // earlier inline. Iterate to a fixed point so the original span walks
        // through an arbitrarily long old -> fresh -> fresher chain.
        loop {
            let mut changed = false;
            for &(old_id, new_id) in &pairs {
                if spans.contains_key(&new_id) {
                    continue;
                }
                if let Some(span) = spans.get(&old_id).copied() {
                    spans.insert(new_id, span);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
}

impl Drop for RemapSession {
    fn drop(&mut self) {
        if !self.finished {
            ACTIVE_REMAPS.with(|active| {
                active.borrow_mut().take();
            });
        }
    }
}

pub(crate) fn record_expr_remaps(remaps: &HashMap<LocalId, Expr>) {
    ACTIVE_REMAPS.with(|active| {
        let mut active = active.borrow_mut();
        let Some(state) = active.as_mut() else {
            return;
        };
        state
            .pairs
            .extend(remaps.iter().filter_map(|(&old_id, expr)| {
                let Expr::LocalGet(new_id) = expr else {
                    return None;
                };
                (*new_id >= state.first_fresh_id && *new_id != old_id).then_some((old_id, *new_id))
            }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloned_span_propagates_through_transitive_fresh_ids() {
        let session = RemapSession::start(100);
        record_expr_remaps(&HashMap::from([(7, Expr::LocalGet(100))]));
        record_expr_remaps(&HashMap::from([(100, Expr::LocalGet(101))]));
        // Existing caller ids are deliberately ignored.
        record_expr_remaps(&HashMap::from([(7, Expr::LocalGet(9))]));

        let span = LocalSourceSpan { start: 20, end: 25 };
        let mut spans = HashMap::from([(7, span)]);
        session.finish(&mut spans);

        assert_eq!(spans.get(&100), Some(&span));
        assert_eq!(spans.get(&101), Some(&span));
        assert_eq!(spans.get(&9), None);
    }
}
