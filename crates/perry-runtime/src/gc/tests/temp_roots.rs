//! #6951 — expression temporaries generated code pushes onto the temp-root
//! stack must be precise roots: marked by the mark phase and relocated by the
//! rewrite phase, with no help from the conservative native-stack scan.
//!
//! Every test here pins `ConservativeStackScanMode::Disabled`. The unit-test
//! build defaults to `Full`, whose native-stack scan would find the raw
//! pointer these tests hold in a Rust local and rescue the object — turning a
//! missing precise root into a green test and a production-only
//! use-after-free (production resolves `Auto -> SkipDisabled`). A test here
//! that does not pin the scan off proves nothing.

use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

/// Put the temp-root scanner back into a test-isolated thread's registry.
///
/// `GcTestIsolationGuard` / `CopyingNurseryTestGuard` `mem::take` the thread's
/// `MUTABLE_ROOT_SCANNERS` so a collection sees exactly the roots the test
/// installs — and the temp-root scanner goes with it. Without this the temp
/// root under test is decorative and the test passes for the wrong reason.
fn register_temp_root_scanner_for_tests() {
    gc_register_budgeted_mutable_root_scanner_with_source(
        scan_temp_roots_mut,
        scan_temp_roots_mut_step,
        new_temp_root_scan_state,
        MutableRootScannerSource::RuntimeMutableScanner,
    );
}

/// The end-to-end regression: an object whose ONLY reference is a temp-root
/// slot must survive a real collection with precise roots only.
///
/// This is the unit-level form of the #6951 repro. In a compiled program the
/// slot holds the argument accumulator of `console.log("label", churn())`,
/// which used to live in nothing but an LLVM SSA register — swept
/// mid-statement, with the next `js_array_push_f64` landing in recycled
/// memory, so the label silently vanished from the output.
#[test]
fn temp_rooted_value_survives_a_real_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_temp_root_scanner_for_tests();
    reset_temp_roots();
    reset_old_reclaim_pressure();

    let dead_headers = allocate_dead_malloc_churn_headers(8);
    let live = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live);
    }
    assert!(
        malloc_user_ptr_tracked(live),
        "precondition: live is tracked"
    );

    // Both word forms the `gc::root_words` contract admits, because generated
    // code pushes both: NaN-boxed values from `lower_expr`, and bare `i64`
    // array pointers threaded through `js_array_alloc` / `js_array_push_f64`.
    let bare = js_gc_temp_root_push(live as u64);
    let tagged = js_gc_temp_root_push(POINTER_TAG | live as u64);

    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    assert_eq!(
        tracked_malloc_headers_matching(&dead_headers),
        0,
        "the sweep must actually have run for this test to mean anything"
    );

    // Read the survivor back out of the ROOT, never from the pre-collection
    // register — that is the contract generated code follows, and it is the
    // stronger assertion: it also proves the slot itself was maintained.
    let survivor = js_gc_temp_root_get(bare);
    assert_ne!(
        survivor, 0,
        "the bare temp-root slot must still hold a value"
    );
    assert!(
        malloc_user_ptr_tracked(survivor as *mut u8),
        "an object reachable only through a temp root must be marked, not swept (#6951)"
    );
    assert_eq!(
        js_gc_temp_root_get(tagged) & POINTER_MASK,
        survivor,
        "the NaN-boxed slot must resolve to the same object as the bare slot"
    );
    unsafe {
        assert_eq!(
            (*(survivor as *mut crate::closure::ClosureHeader)).type_tag,
            crate::closure::CLOSURE_MAGIC,
            "surviving object must still be intact"
        );
    }

    js_gc_temp_root_truncate(bare);
    assert_eq!(temp_root_depth(), 0);
}

/// Truncation is a stack cut, not a pop: releasing a base slot must drop every
/// slot pushed above it, which is what lets one `js_gc_temp_root_truncate` at
/// the end of an argument list release the whole group.
#[test]
fn truncate_drops_every_slot_above_the_base() {
    let _guard = GcTestIsolationGuard::new();
    reset_temp_roots();

    let base = js_gc_temp_root_push(0x1000);
    js_gc_temp_root_push(0x2000);
    js_gc_temp_root_push(0x3000);
    assert_eq!(temp_root_depth(), 3);

    js_gc_temp_root_truncate(base);
    assert_eq!(temp_root_depth(), 0);

    // Out-of-range accesses are release-safe no-ops, the same discipline
    // `js_shadow_slot_set` follows: a malformed index from generated code must
    // not abort the host program.
    assert_eq!(js_gc_temp_root_get(99), 0);
    js_gc_temp_root_set(99, 0x4000);
    js_gc_temp_root_truncate(99);
    assert_eq!(temp_root_depth(), 0);
}

/// `js_array_push_f64_temp_rooted` is the fused accumulator push: it must read
/// the array out of the slot, push, and write the possibly-reallocated pointer
/// back — otherwise a growth reallocation strands the root on the old header.
#[test]
fn fused_array_push_writes_the_reallocated_pointer_back() {
    let _guard = GcTestIsolationGuard::new();
    reset_temp_roots();

    let arr = crate::array::js_array_alloc(0);
    let idx = js_gc_temp_root_push(arr as u64);

    // Push past the initial capacity so the array is forced to grow and hand
    // back a different header.
    let capacity = unsafe { (*arr).capacity };
    for i in 0..(capacity + 4) {
        js_array_push_f64_temp_rooted(idx, i as f64);
    }

    let grown = js_gc_temp_root_get(idx) as *const crate::array::ArrayHeader;
    assert!(!grown.is_null(), "the slot must hold the grown array");
    unsafe {
        assert_eq!(
            (*grown).length,
            capacity + 4,
            "every fused push must have landed in the array the slot points at"
        );
    }

    js_gc_temp_root_truncate(idx);
}

/// A `longjmp` unwind past an argument list skips its
/// `js_gc_temp_root_truncate`. The shadow-stack savepoint the exception path
/// already takes carries the temp-root depth, so the orphaned slots are
/// dropped with the orphaned frames rather than retained forever.
#[test]
fn shadow_savepoint_restores_the_temp_root_depth() {
    let _guard = GcTestIsolationGuard::new();
    reset_temp_roots();
    reset_shadow_stack();

    let outer = js_gc_temp_root_push(0x1000);
    let savepoint = shadow_stack_savepoint();
    js_gc_temp_root_push(0x2000);
    js_gc_temp_root_push(0x3000);
    assert_eq!(temp_root_depth(), 3);

    shadow_stack_restore(savepoint);
    assert_eq!(
        temp_root_depth(),
        1,
        "the unwind must drop the slots pushed inside the protected region"
    );
    assert_eq!(js_gc_temp_root_get(outer), 0x1000);

    js_gc_temp_root_truncate(outer);
}

/// Rewriting a slot must re-aim the root: the NEW value becomes reachable and
/// the replaced one does not stay alive on the strength of having once been in
/// that slot.
///
/// This is the semantics `String.prototype.concat` depends on (#6971). Its
/// accumulator is a bare `StringHeader*` that changes address on every
/// iteration — each `js_string_concat` returns a *new* string — so codegen
/// writes the result back with `js_gc_temp_root_set` before lowering the next
/// argument. If a write-back rooted the old address instead of the new one,
/// the accumulator under construction would be the thing swept.
///
/// Pins `ConservativeStackScanMode::Disabled`: with the unit-test default
/// (`Full`) the native-stack scan finds both raw pointers in these Rust locals
/// and the test passes without proving anything about precise roots.
#[test]
fn rewriting_a_slot_roots_the_new_value_and_releases_the_replaced_one() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_temp_root_scanner_for_tests();
    reset_temp_roots();
    reset_old_reclaim_pressure();

    let dead_headers = allocate_dead_malloc_churn_headers(8);

    let replaced = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    let successor = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(replaced);
        init_test_closure(successor);
    }
    assert!(
        malloc_user_ptr_tracked(replaced) && malloc_user_ptr_tracked(successor),
        "precondition: both objects are tracked"
    );

    // The accumulator pattern: root the first value, then hand the slot the
    // successor the way `js_string_concat`'s result is written back.
    let slot = js_gc_temp_root_push(replaced as u64);
    js_gc_temp_root_set(slot, successor as u64);

    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    assert_eq!(
        tracked_malloc_headers_matching(&dead_headers),
        0,
        "the sweep must actually have run for this test to mean anything"
    );

    let survivor = js_gc_temp_root_get(slot);
    assert_eq!(
        survivor, successor as u64,
        "the slot must still hold the value written back into it"
    );
    assert!(
        malloc_user_ptr_tracked(survivor as *mut u8),
        "the value a slot was re-aimed at must be marked, not swept (#6971)"
    );
    unsafe {
        assert_eq!(
            (*(survivor as *mut crate::closure::ClosureHeader)).type_tag,
            crate::closure::CLOSURE_MAGIC,
            "the surviving accumulator must still be intact"
        );
    }
    assert!(
        !malloc_user_ptr_tracked(replaced),
        "the replaced value must NOT be retained by a slot that no longer \
         points at it — otherwise every concat round leaks its input"
    );

    js_gc_temp_root_truncate(slot);
    assert_eq!(temp_root_depth(), 0);
}
