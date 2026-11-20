//! #11549 direction 2: the fixed per-minor cost of two tables that a copying
//! minor used to walk whole on every pass.
//!
//! * The intern table (8192 slots) is weak and invalidated before heap changes;
//!   atoms are immortal image leaves. Neither has a root scanner.
//! * The two array-tail transition tables (2 x 8192 slots) are skipped
//!   outright while no entry has ever been published on the thread.
//!
//! The witnesses check reachability, movement and image lifetime. Existing
//! array-tail and small-string controls still check their skip prerequisites.

use super::super::*;
use super::support::*;
use crate::codegen_abi::LayoutWord;

/// Leaves both tables empty however the test exits, including by the
/// expected panic of a negative-control test.
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

/// A runtime string reachable only through weak interning dies in a minor.
#[test]
fn weak_intern_does_not_keep_a_young_string_alive() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _clear = ClearTablesOnDrop;
    let bytes = b"minor-fixed-cost-weak-intern";
    let hash = fnv(bytes);
    let young = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::string::js_string_intern(young, hash);
    assert_eq!(crate::string::test_intern_slot_ptr(hash), young as usize);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(crate::string::test_intern_slot_ptr(hash), 0);
}

/// Weak runtime entries release old strings on full sweep as well.
#[test]
fn weak_intern_does_not_keep_an_old_string_alive() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _clear = ClearTablesOnDrop;
    let bytes = b"minor-fixed-cost-old-weak-intern";
    let hash = fnv(bytes);
    let old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING) as *mut crate::StringHeader;
    unsafe {
        crate::string::test_init_string_bytes(old, bytes);
    }
    crate::string::js_string_intern(old, hash);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(crate::string::test_intern_slot_ptr(hash), 0);
    assert!(!build_valid_pointer_set().contains(&(old as usize)));
}

/// A strongly reachable string moves normally; the weak cache is invalidated,
/// and interning the rooted copy installs its current address.
#[test]
fn live_interned_string_moves_without_an_intern_scanner() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _clear = ClearTablesOnDrop;
    let bytes = b"minor-fixed-cost-live-intern";
    let hash = fnv(bytes);
    let young = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::string::js_string_intern(young, hash);
    js_shadow_slot_set(0, crate::value::JSValue::string_ptr(young).bits());
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    assert_ne!(after as usize, young as usize);
    assert_eq!(crate::string::test_intern_slot_ptr(hash), 0);
    assert_eq!(crate::string::js_string_intern(after, hash), after);
    unsafe {
        assert_string_bytes(after, bytes);
    }
}

/// Negative control for the array-tail skip: an entry published without arming
/// `array_tail_occupied` is caught where the skip relies on the flag.
#[test]
#[should_panic(expected = "without arming the flag first")]
fn array_tail_publish_without_arming_is_caught() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _clear = ClearTablesOnDrop;
    crate::object::array_tail_transition::test_clear();
    crate::object::array_tail_transition::test_publish_without_arming(LayoutWord(3));
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
    crate::object::array_tail_transition::test_arm_and_publish(LayoutWord(3));
    assert!(crate::object::array_tail_transition::test_tables_may_hold_entries());
    crate::object::array_tail_transition::test_clear();
}

/// The small-int cache is skipped by a minor outright. Its
/// real entries (longlived, pinned) satisfy the skip's precondition.
#[test]
fn small_string_caches_hold_only_pinned_non_young_strings() {
    let _guard = CopyingNurseryTestGuard::new(0);
    // Fill a few entries through the production writers.
    let _ = crate::string::js_number_to_string(7.0);
    let _ = crate::string::js_number_to_string(200.0);
    let _ = gc_collect_minor();
    crate::string::debug_assert_small_string_caches_not_minor_relevant();
}

/// Negative control: a writer that publishes a young string into the cache breaks
/// the precondition of the minor skip, and the check says so.
#[test]
#[should_panic(expected = "not a pinned non-young string")]
fn small_int_cache_writer_publishing_a_young_string_is_caught() {
    let _guard = CopyingNurseryTestGuard::new(0);
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::string::test_write_small_int_cache_slot(255, std::ptr::null_mut());
        }
    }
    let _restore = Restore;
    let young = young_leaf() as *mut crate::StringHeader;
    crate::string::test_write_small_int_cache_slot(255, young);
    crate::string::debug_assert_small_string_caches_not_minor_relevant();
}

/// Image atoms keep their address without an atom scanner, across moving
/// minors and full sweep, and preserve shared and eligibility semantics.
#[test]
fn image_atom_survives_without_a_scanner() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let bytes = b"minor-fixed-cost-image-atom";
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    let atom = crate::string::js_string_pool_atom(bytes.as_ptr(), bytes.len() as u32, hash, 0);
    assert_eq!(
        crate::arena::classify_heap_space(atom as usize),
        crate::arena::HeapSpace::Longlived
    );
    let header = unsafe { header_from_user_ptr(atom.cast()) };
    assert_ne!(unsafe { (*header).gc_flags } & GC_FLAG_PINNED, 0);
    assert_eq!(unsafe { (*header).gc_flags } & GC_FLAG_INTERNED, 0);
    assert_eq!(unsafe { (*atom).refcount }, 0);
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    assert_eq!(
        crate::string::atom_lookup(bytes, hash),
        Some(atom as *const _)
    );
    assert_eq!(
        crate::string::js_string_pool_atom(bytes.as_ptr(), bytes.len() as u32, hash, 0),
        atom
    );
    unsafe {
        assert_string_bytes(atom, bytes);
    }
}

/// Empty, long and lone-surrogate pools use the same immortal birth funnel.
#[test]
fn non_atom_pool_literals_are_immortal_image_strings() {
    let _guard = CopyingNurseryTestGuard::new(0);
    for (bytes, wtf8) in [
        (&b""[..], 0),
        (&[b'x'; 100][..], 0),
        (&[0xed, 0xa0, 0x80][..], 1),
    ] {
        let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
        let string =
            crate::string::js_string_pool_atom(bytes.as_ptr(), bytes.len() as u32, hash, wtf8);
        assert_eq!(
            crate::arena::classify_heap_space(string as usize),
            crate::arena::HeapSpace::Longlived
        );
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
        unsafe {
            assert_string_bytes(string, bytes);
        }
        assert_eq!(unsafe { (*string).refcount }, 0);
    }
}
