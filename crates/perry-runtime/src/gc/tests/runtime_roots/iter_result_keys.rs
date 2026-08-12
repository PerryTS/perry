//! #7564 — the per-thread `{ value, done }` / `{ done, value }` keys arrays
//! that every runtime-built iterator result shares.
//!
//! This is the "runtime-side cache of a raw heap pointer" shape CLAUDE.md
//! warns about: `scripts/gc_root_dominance_check.py` reads emitted LLVM IR, so
//! a thread-local holding a `*mut ArrayHeader` into the heap is structurally
//! invisible to it. The runtime scanner is the only thing standing between
//! that cache and a use-after-free, and — being a cache rather than a register
//! — it would go bad at collection #0 and stay bad, corrupting every later
//! `.next()` on the thread rather than failing intermittently.
//!
//! Two halves have to hold, and marking alone is not enough: a marked but
//! un-rewritten slot still hands out a pre-move address after a copying minor,
//! which is the whole failure. So there is a MARK test and a REWRITE test,
//! plus a registration check — a scanner a test can call directly is a no-op
//! in production until `gc_init` names it.
//!
//! The shape invariants the sharing DEPENDS on are asserted here too. Sharing
//! one keys array across every result object is only sound because the array
//! carries `GC_FLAG_SHAPE_SHARED`, which is what makes `result.extra = 1`
//! clone before appending instead of mutating the array every other result is
//! using. Lose that flag and one user-added property corrupts every iterator
//! result in the program.

use super::*;
use crate::array::ArrayHeader;
use crate::iter_result::IterResultOrder;

/// Empties the cache on entry and on exit, so a test starts from slots it
/// populated itself and a later test on this thread does not inherit a slot
/// pointing into this test's arena.
///
/// It also pins the GC triggers for the body: these tests hand-build the
/// evacuation the collector would perform, and a real collection landing
/// between `build_valid_pointer_set` and the forwarding write would move the
/// arrays out from under it and go red for a reason unrelated to the scanner.
struct IterResultKeysGuard {
    _triggers: GcTriggerThresholdTestGuard,
}

impl IterResultKeysGuard {
    fn new() -> Self {
        let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        crate::iter_result::reset_shared_keys_for_test();
        Self {
            _triggers: triggers,
        }
    }
}

impl Drop for IterResultKeysGuard {
    fn drop(&mut self) {
        crate::iter_result::reset_shared_keys_for_test();
    }
}

/// Allocate an old-gen destination and forward `from` → `to`, the shape an
/// evacuating minor leaves behind.
fn evacuate_array(from: *mut ArrayHeader) -> *mut ArrayHeader {
    let to = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
    unsafe {
        set_forwarding_address(header_from_user_ptr(from as *const u8), to);
    }
    to as *mut ArrayHeader
}

/// MARK. The cache is the ONLY reference to these arrays — the result objects
/// that point at them are short-lived while the cache outlives them — so an
/// unmarked slot is a swept slot, and every later `.next()` on the thread
/// installs a freed keys array as an object's shape.
#[test]
fn iter_result_keys_cache_is_marked_by_the_collector() {
    let _guard = IterResultKeysGuard::new();
    clear_marks();
    clear_mark_seeds();

    let arrays = crate::iter_result::populate_shared_keys_for_test();
    let valid_ptrs = build_valid_pointer_set();

    crate::iter_result::scan_iter_result_keys_roots_mut(&mut RuntimeRootVisitor::for_mark(
        &valid_ptrs,
    ));

    for (i, arr) in arrays.iter().enumerate() {
        assert!(!arr.is_null(), "keys slot {i} should have been populated");
        assert_marked_user_ptr(
            *arr as usize,
            &format!("iterator-result keys array {i} (nothing else references it)"),
        );
    }

    clear_marks();
    clear_mark_seeds();
}

/// REWRITE, both slots. Marking keeps the array alive; only the rewrite makes
/// the slot name the surviving copy. Both orders are forwarded, so a dropped
/// `visit(...)` — or a scanner that iterates only slot 0 — fails here.
#[test]
fn every_iter_result_keys_slot_is_rewritten_by_the_collector() {
    let _guard = IterResultKeysGuard::new();

    let before = crate::iter_result::populate_shared_keys_for_test();
    // The from-space objects must exist before the valid-pointer set is built:
    // that set is what tells the rewrite visitor an address is a real heap
    // object, exactly as in a real cycle.
    let valid_ptrs = build_valid_pointer_set();
    let expected: Vec<*mut ArrayHeader> = before.iter().map(|p| evacuate_array(*p)).collect();

    crate::iter_result::scan_iter_result_keys_roots_mut(&mut RuntimeRootVisitor::for_rewrite(
        &valid_ptrs,
    ));

    for (i, order) in [IterResultOrder::ValueDone, IterResultOrder::DoneValue]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            crate::iter_result::shared_keys_peek_for_test(order),
            expected[i],
            "iterator-result keys slot {i} ({order:?}) must be rewritten to the \
             relocated array. A marked-but-stale slot is worse than an \
             intermittent bug: it goes bad at collection #0 and then EVERY \
             `.next()` on this thread installs a from-space keys array as the \
             result object's shape (#7564)."
        );
    }
}

/// An empty cache is the state between process start and the first `.next()`,
/// and every cycle in that window scans it. A null slot must be skipped, not
/// treated as an address.
#[test]
fn scanning_an_empty_iter_result_keys_cache_is_a_no_op() {
    let _guard = IterResultKeysGuard::new();
    let valid_ptrs = build_valid_pointer_set();

    crate::iter_result::scan_iter_result_keys_roots_mut(&mut RuntimeRootVisitor::for_rewrite(
        &valid_ptrs,
    ));

    for order in [IterResultOrder::ValueDone, IterResultOrder::DoneValue] {
        assert!(
            crate::iter_result::shared_keys_peek_for_test(order).is_null(),
            "scanning must not populate the {order:?} keys slot"
        );
    }
}

/// …and it must actually be REGISTERED. The scanner can be called directly
/// from a test whether or not `gc_init` ever mentions it, so the wiring is
/// asserted separately: an unregistered scanner is a no-op in production,
/// which is precisely the bug this cache would otherwise introduce.
#[test]
fn iter_result_keys_scanner_is_registered() {
    crate::gc::gc_init();
    let registered = |scanner: MutableRootScanner| {
        crate::gc::roots::MUTABLE_ROOT_SCANNERS.with(|scanners| {
            scanners
                .borrow()
                .iter()
                .any(|entry| entry.scanner as usize == scanner as usize)
        })
    };

    assert!(
        registered(crate::iter_result::scan_iter_result_keys_roots_mut as MutableRootScanner),
        "scan_iter_result_keys_roots_mut must be registered in gc_init — unregistered, \
         the shared iterator-result keys arrays are swept by the first minor and every \
         later `.next()` installs a freed array as the result object's shape (#7564)"
    );
}

/// The cache must be STABLE: a second `.next()` reuses the array the first one
/// built. If it did not, nothing would have been saved — and, because
/// `shape_id_for_keys_ensure` keys the shape table on the array's ADDRESS, a
/// fresh array per call is also a fresh shape id per call, which is what made
/// every read of `.value` off a result an inline-cache miss.
#[test]
fn iter_result_keys_are_built_once_per_order() {
    let _guard = IterResultKeysGuard::new();

    let first = crate::iter_result::populate_shared_keys_for_test();
    let second = crate::iter_result::populate_shared_keys_for_test();

    assert_eq!(
        first, second,
        "the shared keys arrays must be built once per thread per order; rebuilding \
         them per call restores both the four-allocation cost and the one-shape-id- \
         per-`.next()` inline-cache miss (#7564)"
    );
    assert_ne!(
        first[0], first[1],
        "`{{ value, done }}` and `{{ done, value }}` are DIFFERENT observable key \
         orders (the latter is `node:sqlite`'s) and must not share one array"
    );
}

/// The copy-on-write marker is the entire soundness argument for sharing.
/// `field_set_by_name`, `delete_rest` and `proxy::put_value` all consult
/// `GC_FLAG_SHAPE_SHARED` to decide whether to clone before mutating an
/// object's key list. Without it, `result.extra = 1` on ONE result object
/// appends to the array every other result is using.
#[test]
fn shared_iter_result_keys_are_marked_copy_on_write() {
    let _guard = IterResultKeysGuard::new();

    for arr in crate::iter_result::populate_shared_keys_for_test() {
        let gc_header = unsafe { &*header_from_user_ptr(arr as *const u8) };
        assert!(
            gc_header.gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0,
            "the shared iterator-result keys array must carry GC_FLAG_SHAPE_SHARED. \
             Without it a single `result.extra = 1` appends to the array EVERY other \
             iterator result on this thread is using, and they all silently grow a \
             third key (#7564)"
        );
    }
}
