//! Facts-closed method targets and the early inliner.
//!
//! A virtual call whose receiver the whole-program facts prove to be one of a
//! small set of classes is a direct call of the bodies those classes resolve
//! to (the T0set read shape in `lower_call/property_get/dynamic_dispatch.rs`).
//! Such a body is the closed callee of that site: the facts name it, so the
//! call boundary is not a dispatch the optimizer has to guess at, and a body
//! that fits the guarded-specialization budget joins the pre-statepoint
//! inliner exactly as a proven-receiver clone does. Nothing is admitted by name or
//! by clone pattern: the site lowering records the targets its proof closed
//! (`note_closed_call`), `compile_method` records which bodies fit the budget
//! (`LlFunction::closed_target_preinline`), and `apply` joins the two once
//! the module is lowered. The record lives for one module's lowering on this
//! thread and never reaches the binary.

use crate::module::LlModule;
use std::cell::RefCell;
use std::collections::BTreeSet;

/// A site closed over more targets than this is a dispatch table, not a
/// devirtualized call; each target would be copied into the one site.
pub(crate) const MAX_CLOSED_SITE_TARGETS: usize = 2;

thread_local! {
    static CLOSED: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
}

pub(crate) fn begin_module() {
    CLOSED.with(|c| c.borrow_mut().clear());
}

/// A facts-proven site, closed over `distinct` bodies, calls `target` directly.
pub(crate) fn note_closed_call(target: &str, distinct: usize) {
    if distinct == 0 || distinct > MAX_CLOSED_SITE_TARGETS {
        return;
    }
    CLOSED.with(|c| {
        c.borrow_mut().insert(target.to_string());
    });
}

/// Admit the closed targets whose lowered body fits the budget.
pub(crate) fn apply(llmod: &mut LlModule) {
    let closed = CLOSED.with(|c| std::mem::take(&mut *c.borrow_mut()));
    for i in 0..llmod.function_count() {
        let Some(f) = llmod.function_mut(i) else {
            continue;
        };
        if f.closed_target_preinline && !f.no_inline && closed.contains(&f.name) {
            f.pre_statepoint_inline = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DOUBLE;

    fn module_with(names: &[(&str, bool)]) -> LlModule {
        let mut m = LlModule::new("x86_64-unknown-linux-gnu");
        for (n, fits) in names {
            let f = m.define_function(*n, DOUBLE, vec![(DOUBLE, "%this_arg".into())]);
            f.closed_target_preinline = *fits;
            f.create_block("entry").ret(DOUBLE, "%this_arg");
        }
        m
    }

    fn inlined(m: &LlModule, name: &str) -> bool {
        m.function_named(name).unwrap().pre_statepoint_inline
    }

    #[test]
    fn only_a_noted_target_that_fits_is_admitted() {
        begin_module();
        let mut m = module_with(&[
            ("noted_fits", true),
            ("noted_big", false),
            ("unnoted", true),
        ]);
        note_closed_call("noted_fits", 2);
        note_closed_call("noted_big", 2);
        apply(&mut m);
        assert!(inlined(&m, "noted_fits"));
        assert!(!inlined(&m, "noted_big"), "over budget stays out of line");
        assert!(!inlined(&m, "unnoted"), "no proof, no admission");
    }

    #[test]
    fn a_dispatch_table_is_not_a_closed_call() {
        begin_module();
        let mut m = module_with(&[("a", true), ("b", true), ("c", true)]);
        for t in ["a", "b", "c"] {
            note_closed_call(t, 3);
        }
        apply(&mut m);
        assert!(!inlined(&m, "a") && !inlined(&m, "b") && !inlined(&m, "c"));
    }

    #[test]
    fn the_record_does_not_cross_modules() {
        begin_module();
        note_closed_call("a", 1);
        begin_module();
        let mut m = module_with(&[("a", true)]);
        apply(&mut m);
        assert!(!inlined(&m, "a"));
    }
}
