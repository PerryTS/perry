//! #11549 direction 2: the fixed per-minor cost of two tables that a copying
//! minor used to walk whole on every pass.
//!
//! * The intern table (8192 slots) now carries a young-entry log
//!   (`gc/young_log.rs`): a minor visits only slots that may name a young
//!   string.
//! * The two array-tail transition tables (2 x 8192 slots) are skipped
//!   outright while no entry has ever been published on the thread.
//!
//! Each gets the young-log proof shape: the young entry still MOVES through
//! the narrowed walk, the walk really was narrowed (a skip needs a counter),
//! and a writer that forgets to arm is caught (sabotage).

use super::super::*;
use super::support::*;

const INTERN_LOG: &str = "string.intern_table";

/// Leaves both tables empty however the test exits, including by the
/// expected panic of a sabotage test.
struct ClearTablesOnDrop;

impl Drop for ClearTablesOnDrop {
    fn drop(&mut self) {
        crate::string::test_clear_intern_table();
        crate::object::array_tail_transition::test_clear();
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// A young string interned through the production writer and reachable ONLY
/// through the intern table is evacuated by a copying minor, and the table
/// slot is rewritten to the new address — through the log, not a whole-table
/// walk.
#[test]
fn young_interned_string_is_rewritten_through_the_log() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _clear = ClearTablesOnDrop;
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    crate::string::test_clear_intern_table();

    let bytes = b"minor-fixed-cost-young-intern";
    let hash = fnv(bytes);
    let young = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    assert!(crate::arena::pointer_in_nursery(young as usize));
    assert_eq!(crate::string::js_string_intern(young, hash), young);

    let _ = gc_collect_minor();

    let after = crate::string::test_intern_slot_ptr(hash);
    assert_ne!(after, 0, "the interned string must still be tabled");
    assert_ne!(
        after, young as usize,
        "the slot must name the evacuated copy, not from-space"
    );
    unsafe {
        assert_string_bytes(after as *const crate::StringHeader, bytes);
    }
    let row = young_log::last_walk(INTERN_LOG).expect("intern walk recorded");
    assert!(
        row.partial,
        "a copying minor must take the logged walk: {row:?}"
    );
    assert!(row.visited >= 1, "the young slot must be visited: {row:?}");
    assert!(
        row.visited < row.table_len,
        "an 8192-slot table must not be walked whole: {row:?}"
    );
}

/// An OLD interned string notes nothing, so a minor visits no intern slot at
/// all even though the table is not empty — the skip fired.
#[test]
fn old_interned_string_is_not_visited_by_a_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _clear = ClearTablesOnDrop;
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    crate::string::test_clear_intern_table();

    let bytes = b"minor-fixed-cost-old-intern";
    let hash = fnv(bytes);
    let old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING) as *mut crate::StringHeader;
    unsafe {
        crate::string::test_init_string_bytes(old, bytes);
    }
    assert!(!young_log::addr_is_minor_relevant(old as usize));
    assert_eq!(crate::string::js_string_intern(old, hash), old);

    let _ = gc_collect_minor();

    assert_eq!(crate::string::test_intern_slot_ptr(hash), old as usize);
    let row = young_log::last_walk(INTERN_LOG).expect("intern walk recorded");
    assert!(row.partial, "{row:?}");
    assert_eq!(
        row.visited, 0,
        "an old interned string must not be visited: {row:?}"
    );
}

/// SABOTAGE (rule 2): a writer that publishes a young string into a slot
/// without noting it is caught by the log-completeness check the minor-scoped
/// walk runs first. This is the check that turns "someone added an intern
/// writer and forgot `arm_intern_young`" into a red test instead of a
/// from-space pointer left in the table.
#[test]
#[should_panic(expected = "young log for string.intern_table does not name")]
fn intern_writer_that_skips_the_log_is_caught() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _clear = ClearTablesOnDrop;
    crate::string::test_clear_intern_table();
    let young = young_leaf();
    crate::string::test_write_intern_slot_without_logging(7, young);
    crate::string::test_check_intern_young_logged();
}

/// SABOTAGE for the array-tail skip: an entry published without arming
/// `array_tail_occupied` is caught where the skip relies on the flag.
#[test]
#[should_panic(expected = "without arming the flag first")]
fn array_tail_publish_without_arming_is_caught() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _clear = ClearTablesOnDrop;
    crate::object::array_tail_transition::test_clear();
    crate::object::array_tail_transition::test_publish_without_arming(3);
    crate::object::array_tail_transition::test_prune();
}

/// The skip's positive half: with nothing published the scan and the prune
/// visit nothing, and once the production writer has armed the flag they walk
/// the tables again.
#[test]
fn array_tail_tables_are_skipped_only_while_unarmed() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _clear = ClearTablesOnDrop;
    crate::object::array_tail_transition::test_clear();
    assert!(!crate::object::array_tail_transition::test_tables_may_hold_entries());
    let _ = gc_collect_minor();
    assert!(!crate::object::array_tail_transition::test_tables_may_hold_entries());
    crate::object::array_tail_transition::test_arm_and_publish(3);
    assert!(crate::object::array_tail_transition::test_tables_may_hold_entries());
    crate::object::array_tail_transition::test_clear();
}
