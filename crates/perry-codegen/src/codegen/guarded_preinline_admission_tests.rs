use super::*;

/// Written against the functions' own values rather than literals, so a
/// retuned default cannot silently turn these into vacuous assertions.
#[test]
fn source_small_arm_admits_a_large_lattice_but_a_statement_bound_still_bounds_it() {
    let raised = guarded_specialization_source_small_max_ir_bytes();
    let cap = inline_hot_small_size_cap();
    assert!(
        raised > GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES,
        "the new arm only means something if its ceiling is higher than the original's",
    );

    // The case this change exists for: one-statement leaves whose guard
    // lattice lowers well past the original 16 KiB ceiling.
    let past_original = GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES + 1;
    assert!(!guarded_specialization_fits_preinline_budget(past_original));
    assert!(guarded_specialization_admits_preinline(past_original, 1));
    assert!(guarded_specialization_admits_preinline(raised, cap));

    // #8583's protection, and the reason the statement count is a BOUND
    // rather than an admission test: the giant bundled IIFEs are thousands
    // of statements, so no IR size may let them through this arm.
    assert!(!guarded_specialization_admits_preinline(raised, cap + 1));
    assert!(!guarded_specialization_admits_preinline(
        past_original,
        5_000
    ));

    // The raised ceiling is still a ceiling.
    assert!(!guarded_specialization_admits_preinline(raised + 1, 1));

    // The original arm is unchanged: within 16 KiB, statement count is
    // irrelevant, exactly as before this change.
    assert!(guarded_specialization_admits_preinline(
        GUARDED_SPECIALIZATION_PREINLINE_MAX_IR_BYTES,
        5_000,
    ));
    assert!(guarded_specialization_admits_preinline(0, usize::MAX));
}
