//! Whole-program array-iteration and descriptor-write claims.
/// The program's array-iteration facts (object model step 7), from the
/// whole-program write census fixed before module codegen starts: bit 0, a
/// write may reach a `Symbol.iterator` member (or re-parent an array), so
/// iteration reads `%Array.prototype%`'s fixed slot (and an array's own
/// member); bit 1, `%ArrayIteratorPrototype%` may be named, so iteration also
/// checks its `next`. Zero: iteration is the builtin element walk, unchecked.
/// The whole-program facts (#13645) clear a bit the census sets when compiled
/// code provably never writes what it guards.
pub(crate) fn array_iteration_facts(wpfacts: &crate::codegen::ElisionFacts) -> u32 {
    if wpfacts.array_iteration_unwritten {
        crate::verify_facts::consumed(crate::verify_facts::Fact::ArrayIteration, || {
            serde_json::json!({})
        });
    }
    if wpfacts.array_iterator_close_unwritten {
        crate::verify_facts::consumed(crate::verify_facts::Fact::IteratorClose, || {
            serde_json::json!({})
        });
    }
    let names = crate::program_patched_proto_methods();
    let mut facts = 0;
    if !wpfacts.array_iteration_unwritten
        && names
            .iter()
            .any(|n| n == perry_hir::patched_builtins::ARRAY_ITERATOR_WRITABLE)
    {
        facts |= 1;
    }
    if !wpfacts.array_iterator_close_unwritten
        && names
            .iter()
            .any(|n| n == perry_hir::patched_builtins::ARRAY_ITERATOR_PROTO_REACHABLE)
    {
        facts |= 2;
    }
    facts
}

/// Immutable whole-program array facts handed to the runtime at startup.
pub(crate) fn array_program_facts(wpfacts: &crate::codegen::ElisionFacts) -> u32 {
    array_iteration_facts(wpfacts)
        | if crate::program_patched_proto_methods()
            .iter()
            .any(|name| name == perry_hir::patched_builtins::ARRAY_ELEMENT_DESCRIPTORS)
        {
            4
        } else {
            0
        }
}
