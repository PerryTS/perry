//! #7211 — the runtime interns two sets of strings in thread-local
//! `Cell<*mut StringHeader>`s, and both are GC roots.
//!
//!   * `builtins::arithmetic`'s eight `typeof` results, so `typeof x` does not
//!     rebuild `"string"` on every call;
//!   * `json::raw_json`'s `"rawJSON"` key, so every `JSON.rawJSON(...)` writes
//!     its own property under the same key object.
//!
//! Both held a raw nursery pointer that nothing registered, so the first minor
//! collection swept or evacuated the string and the cell named abandoned bytes
//! for the rest of the process. That is a *deterministic* use-after-free
//! rather than the intermittent kind: it goes bad at collection #0, not when a
//! collection happens to land in a window. `scripts/gc_root_dominance_check.py`
//! cannot see either one — it reads emitted LLVM IR, and these are runtime-side
//! tables.
//!
//! Two halves have to hold for each, and marking alone is not enough: a marked
//! but un-rewritten cell still hands out a pre-move address after a copying
//! minor, which is the whole failure. So each scanner gets a MARK test and a
//! REWRITE test, plus the registration check — a scanner a test can call
//! directly is a no-op in production until `gc_init` names it.
//!
//! The `typeof` rewrite test drives all EIGHT cells. `scan_typeof_string_roots_mut`
//! is eight hand-written `visit(...)` calls; dropping one is invisible to a
//! test that only exercises a few, and `bigint`/`symbol` are the two hardest to
//! reach from a `.ts` gap test.

use super::*;
use crate::StringHeader;

/// Empties both caches on entry and on exit, so a test starts from cells it
/// populated itself and a later test on this thread does not inherit a cell
/// pointing into this test's arena.
///
/// It also pins the GC triggers to `usize::MAX` for the body. That is not
/// tidiness: these tests hand-build the evacuation the collector would
/// perform, and `evacuate_string` holds `from` across an
/// `arena_alloc_gc_old`, which reaches `gc_check_trigger()` on its
/// block-full slow path. A real collection landing in that window would move
/// `from` out from under the forwarding-address write and the test would go
/// red for a reason that has nothing to do with the scanner under test. This
/// makes "no collection happens here" enforced rather than merely observed.
struct InternedStringCacheGuard {
    _triggers: GcTriggerThresholdTestGuard,
}

impl InternedStringCacheGuard {
    fn new() -> Self {
        let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        crate::builtins::arithmetic::reset_typeof_string_cache_for_test();
        crate::json::raw_json::reset_raw_json_key_cache_for_test();
        Self {
            _triggers: triggers,
        }
    }
}

impl Drop for InternedStringCacheGuard {
    fn drop(&mut self) {
        crate::builtins::arithmetic::reset_typeof_string_cache_for_test();
        crate::json::raw_json::reset_raw_json_key_cache_for_test();
    }
}

/// Allocate an old-gen destination and forward `from` → `to`, the shape an
/// evacuating minor leaves behind. Only safe to call under
/// `InternedStringCacheGuard`, which is what keeps the allocation from
/// collecting `from` before the forwarding address is written.
fn evacuate_string(from: *mut StringHeader) -> *mut StringHeader {
    let to = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING);
    unsafe {
        set_forwarding_address(header_from_user_ptr(from as *const u8), to);
    }
    to as *mut StringHeader
}

/// MARK. The cache is the only reference to these strings, so an unmarked cell
/// is a swept cell — and the cache would then hand out freed memory forever.
#[test]
fn typeof_string_cache_is_marked_by_the_collector() {
    let _cache_guard = InternedStringCacheGuard::new();
    clear_marks();
    clear_mark_seeds();

    crate::builtins::arithmetic::populate_typeof_string_cache_for_test();
    let cells = crate::builtins::arithmetic::typeof_string_cache_cells_for_test();
    let valid_ptrs = build_valid_pointer_set();

    crate::builtins::arithmetic::scan_typeof_string_roots_mut(&mut RuntimeRootVisitor::for_mark(
        &valid_ptrs,
    ));

    for (i, cell) in cells.iter().enumerate() {
        assert!(!cell.is_null(), "cache cell {i} should have been populated");
        assert_marked_user_ptr(
            *cell as usize,
            &format!("typeof cache cell {i} (nothing else references it)"),
        );
    }

    clear_marks();
    clear_mark_seeds();
}

/// REWRITE, all eight cells. Marking keeps the string alive; only the rewrite
/// makes the cell name the surviving copy. Every cell is forwarded, so a
/// `visit(...)` line dropped from the scanner fails here.
#[test]
fn every_typeof_string_cache_cell_is_rewritten_by_the_collector() {
    let _cache_guard = InternedStringCacheGuard::new();

    crate::builtins::arithmetic::populate_typeof_string_cache_for_test();
    let before = crate::builtins::arithmetic::typeof_string_cache_cells_for_test();
    // The from-space objects must exist before the valid-pointer set is built:
    // that set is what tells the rewrite visitor an address is a real heap
    // object, exactly as in a real cycle.
    let valid_ptrs = build_valid_pointer_set();
    let expected: Vec<*mut StringHeader> = before.iter().map(|p| evacuate_string(*p)).collect();

    crate::builtins::arithmetic::scan_typeof_string_roots_mut(
        &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
    );

    let after = crate::builtins::arithmetic::typeof_string_cache_cells_for_test();
    for i in 0..8 {
        assert_eq!(
            after[i], expected[i],
            "typeof cache cell {i} must be rewritten to the relocated string — \
             a marked-but-stale cell still hands `js_string_equals` a from-space \
             address on every later `typeof x === \"…\"` (#7211). \
             scan_typeof_string_roots_mut is eight hand-written visit() calls; \
             a missing one shows up here and nowhere else."
        );
    }
}

/// MARK. Same shape, and the reason it is a separate scanner: the `"rawJSON"`
/// key is written into every wrapper object, so a swept key means
/// `JSON.rawJSON(...)` stores its text under freed memory.
#[test]
fn raw_json_key_cache_is_marked_by_the_collector() {
    let _cache_guard = InternedStringCacheGuard::new();
    clear_marks();
    clear_mark_seeds();

    let key = crate::json::raw_json::raw_json_key_populate_for_test();
    assert!(!key.is_null());
    let valid_ptrs = build_valid_pointer_set();

    crate::json::raw_json::scan_raw_json_key_root_mut(&mut RuntimeRootVisitor::for_mark(
        &valid_ptrs,
    ));

    assert_marked_user_ptr(
        key as usize,
        "the interned \"rawJSON\" key (nothing else references it)",
    );

    clear_marks();
    clear_mark_seeds();
}

/// REWRITE.
#[test]
fn raw_json_key_cache_is_rewritten_by_the_collector() {
    let _cache_guard = InternedStringCacheGuard::new();

    let from = crate::json::raw_json::raw_json_key_populate_for_test();
    let valid_ptrs = build_valid_pointer_set();
    let to = evacuate_string(from);

    crate::json::raw_json::scan_raw_json_key_root_mut(&mut RuntimeRootVisitor::for_rewrite(
        &valid_ptrs,
    ));

    assert_eq!(
        crate::json::raw_json::raw_json_key_peek_for_test(),
        to,
        "the RAW_JSON_KEY cell must be rewritten to the relocated key — otherwise \
         every later JSON.rawJSON(...) sets its own property under a from-space \
         key (#7211)"
    );
}

/// An empty cache is the state between process start and the first `typeof`,
/// and every cycle in that window scans it. A null cell must be skipped, not
/// treated as an address.
#[test]
fn scanning_an_empty_cache_is_a_no_op() {
    let _cache_guard = InternedStringCacheGuard::new();
    let valid_ptrs = build_valid_pointer_set();

    crate::builtins::arithmetic::scan_typeof_string_roots_mut(
        &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
    );
    crate::json::raw_json::scan_raw_json_key_root_mut(&mut RuntimeRootVisitor::for_rewrite(
        &valid_ptrs,
    ));

    assert!(
        crate::builtins::arithmetic::typeof_string_cache_cells_for_test()
            .iter()
            .all(|p| p.is_null()),
        "scanning must not populate the typeof cache"
    );
    assert!(
        crate::json::raw_json::raw_json_key_peek_for_test().is_null(),
        "scanning must not populate the rawJSON key cache"
    );
}

/// …and both must actually be REGISTERED. Either scanner can be called
/// directly from a test whether or not `gc_init` ever mentions it, so the
/// wiring is asserted separately: an unregistered scanner is a no-op in
/// production, which is precisely the #7211 bug.
#[test]
fn interned_string_cache_scanners_are_registered() {
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
        registered(crate::builtins::arithmetic::scan_typeof_string_roots_mut as MutableRootScanner),
        "scan_typeof_string_roots_mut must be registered in gc_init — unregistered, \
         the eight interned `typeof` strings are swept by the first minor and every \
         later `typeof x === \"…\"` compares against from-space (#7211)"
    );
    assert!(
        registered(crate::json::raw_json::scan_raw_json_key_root_mut as MutableRootScanner),
        "scan_raw_json_key_root_mut must be registered in gc_init (#7211)"
    );
}
