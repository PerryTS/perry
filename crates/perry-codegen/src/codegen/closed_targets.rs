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
//! (`note_closed_call`) on the calling function, `compile_method` records
//! which bodies fit the budget (`LlFunction::closed_target_preinline`), and
//! `apply` joins the two once the module is lowered. Every site the facts
//! close over one body reports through `note_closed_call`, wherever and on
//! whichever thread it is lowered (a callable worker included): the record
//! is part of the calling function and never reaches the binary.

use crate::function::LlFunction;
use crate::module::LlModule;
use std::collections::BTreeSet;

/// A site closed over more targets than this is a dispatch table, not a
/// devirtualized call; each target would be copied into the one site.
pub(crate) const MAX_CLOSED_SITE_TARGETS: usize = 2;

/// A facts-proven site of `caller`, closed over `distinct` bodies, calls
/// `target` directly.
pub(crate) fn note_closed_call(caller: &mut LlFunction, target: &str, distinct: usize) {
    if distinct == 0 || distinct > MAX_CLOSED_SITE_TARGETS {
        return;
    }
    if !caller.closed_callees.iter().any(|t| t == target) {
        caller.closed_callees.push(target.to_string());
    }
}

/// Admit the closed targets whose lowered body fits the budget.
pub(crate) fn apply(llmod: &mut LlModule) {
    let mut closed: BTreeSet<String> = BTreeSet::new();
    for i in 0..llmod.function_count() {
        if let Some(f) = llmod.function_mut(i) {
            closed.extend(std::mem::take(&mut f.closed_callees));
        }
    }
    // A closed body is inlined only when no call in it can record the frame
    // chain: an error thrown in it or below it lists its frame
    // (`gc_call_effects::frame_silent_functions`). A body that may throw
    // stays a call.
    let silent: std::collections::HashSet<String> = if closed.is_empty() {
        Default::default()
    } else {
        let funcs: Vec<&crate::function::LlFunction> = llmod.diagnostic_functions().collect();
        crate::gc_call_effects::frame_silent_functions(&llmod.gc_effects(), &funcs)
    };
    for i in 0..llmod.function_count() {
        let Some(f) = llmod.function_mut(i) else {
            continue;
        };
        if f.closed_target_preinline
            && !f.no_inline
            && closed.contains(&f.name)
            && silent.contains(&f.name)
        {
            f.pre_statepoint_inline = true;
            f.guarded_preinline = true;
        }
    }
    // Admission is per body; bound copying after every direct caller and
    // guarded clone is visible, including expansion through other candidates.
    super::helpers::preinline_growth::apply(llmod);
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

    fn caller(m: &mut LlModule) -> &mut LlFunction {
        if m.function_named("caller").is_none() {
            let f = m.define_function("caller", DOUBLE, vec![(DOUBLE, "%this_arg".into())]);
            f.create_block("entry").ret(DOUBLE, "%this_arg");
        }
        m.function_named_mut("caller").unwrap()
    }

    fn inlined(m: &LlModule, name: &str) -> bool {
        m.function_named(name).unwrap().pre_statepoint_inline
    }

    #[test]
    fn only_a_noted_target_that_fits_is_admitted() {
        let mut m = module_with(&[
            ("noted_fits", true),
            ("noted_big", false),
            ("unnoted", true),
        ]);
        note_closed_call(caller(&mut m), "noted_fits", 2);
        note_closed_call(caller(&mut m), "noted_big", 2);
        apply(&mut m);
        assert!(inlined(&m, "noted_fits"));
        assert!(!inlined(&m, "noted_big"), "over budget stays out of line");
        assert!(!inlined(&m, "unnoted"), "no proof, no admission");
    }

    #[test]
    fn a_dispatch_table_is_not_a_closed_call() {
        let mut m = module_with(&[("a", true), ("b", true), ("c", true)]);
        for t in ["a", "b", "c"] {
            note_closed_call(caller(&mut m), t, 3);
        }
        apply(&mut m);
        assert!(!inlined(&m, "a") && !inlined(&m, "b") && !inlined(&m, "c"));
    }

    #[test]
    fn the_record_does_not_cross_modules() {
        let mut other = module_with(&[]);
        note_closed_call(caller(&mut other), "a", 1);
        let mut m = module_with(&[("a", true)]);
        apply(&mut m);
        assert!(!inlined(&m, "a"));
    }

    #[test]
    fn the_record_crosses_the_worker_boundary_with_its_caller() {
        // A body lowered on a callable worker travels as an OwnedFunction;
        // its closed sites must arrive with it.
        let mut f = LlFunction::new("worker_caller", DOUBLE, vec![]);
        note_closed_call(&mut f, "a", 1);
        note_closed_call(&mut f, "a", 1);
        let f = f.into_owned().into_builder();
        assert_eq!(f.closed_callees, vec!["a".to_string()]);
    }
}
