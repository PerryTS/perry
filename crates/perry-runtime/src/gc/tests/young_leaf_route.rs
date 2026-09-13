//! #10169: a document-sized JSON leaf born old under young pressure gives the
//! nursery minor one-time priority over old-reclaim, and only while the young
//! generation is unmeasured. Both halves are asserted: the priority fires
//! exactly once per leaf, and a measured young generation buys none.

use super::super::policy::{
    gc_budgeted_due_trigger, note_young_leaf_born_old, BudgetedGcTrigger,
    ScavengeNurseryCapTestGuard, GC_OLD_RECLAIM_PENDING,
};
use super::super::*;
use super::support::*;

#[test]
fn young_leaf_born_old_prioritises_the_nursery_minor_until_measured() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _cap_due = ScavengeNurseryCapTestGuard::due_at_bytes(1);
    // The isolated arena starts empty; one young allocation makes "due at one
    // byte" actually due.
    let filler = [b'y'; 64];
    crate::string::js_string_from_bytes(filler.as_ptr(), filler.len() as u32);
    assert!(
        crate::arena::copying_from_space_in_use_bytes() >= 1,
        "fixture: the young generation must hold something for the cap to be due"
    );
    let previous_survival = last_young_survival_permille();
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::OldReclaim),
        "fixture: old-reclaim must be due before the leaf can outrank it"
    );

    // Unmeasured young generation: the leaf buys the nursery minor exactly one
    // decision, then old-reclaim is back.
    clear_young_survival_for_tests();
    note_young_leaf_born_old();
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::YoungScavengeCap)
    );
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::OldReclaim)
    );

    // Measured as retained: no priority, and the flag is still consumed.
    seed_young_survival_for_tests(999);
    note_young_leaf_born_old();
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::OldReclaim)
    );
    clear_young_survival_for_tests();
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::OldReclaim),
        "a consumed flag must not be honoured later"
    );

    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    match previous_survival {
        Some(permille) => seed_young_survival_for_tests(permille),
        None => clear_young_survival_for_tests(),
    }
}
