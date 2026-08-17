use super::super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static YOUNG_LEAF_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub(super) fn reset_shadow_stack() {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        s.clear_slots_for_reset();
        s.frame_top = usize::MAX;
    });
}

pub(super) fn reset_global_roots() {
    GLOBAL_ROOTS.with(|roots| roots.borrow_mut().clear());
}

pub(super) struct ShadowAndGlobalRootResetGuard;

impl Drop for ShadowAndGlobalRootResetGuard {
    fn drop(&mut self) {
        reset_shadow_stack();
        reset_global_roots();
    }
}

pub(super) unsafe fn test_heap_child_slots_for_user(user_ptr: *mut u8) -> Vec<HeapChildSlot> {
    let header = header_from_user_ptr(user_ptr as *const u8);
    gc_child_slots(header).collect()
}

pub(super) fn test_heap_child_slot_count(user_ptr: *mut u8) -> usize {
    unsafe {
        test_heap_child_slots_for_user(user_ptr)
            .into_iter()
            .filter(|slot| matches!(slot, HeapChildSlot::Child(_, _)))
            .count()
    }
}

pub(super) fn assert_marked_user_ptr(ptr: usize, label: &str) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "{label} should be marked"
        );
    }
}

pub(super) fn malloc_user_ptr_tracked(ptr: *mut u8) -> bool {
    let header = unsafe { header_from_user_ptr(ptr) };
    MALLOC_STATE.with(|s| s.borrow().objects.iter().any(|&tracked| tracked == header))
}

pub(super) unsafe fn alloc_old_test_promise() -> *mut crate::promise::Promise {
    let ptr = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::promise::Promise>(),
        std::mem::align_of::<crate::promise::Promise>(),
        GC_TYPE_PROMISE,
    ) as *mut crate::promise::Promise;
    std::ptr::write(
        ptr,
        crate::promise::Promise {
            state: crate::promise::PromiseState::Pending,
            value: 0.0,
            reason: 0.0,
            on_fulfilled: std::ptr::null(),
            on_rejected: std::ptr::null(),
            next: std::ptr::null_mut(),
            async_id: 0,
            trigger_async_id: 0,
        },
    );
    ptr
}

pub(super) unsafe fn alloc_old_test_error() -> *mut crate::error::ErrorHeader {
    let ptr = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::error::ErrorHeader>(),
        std::mem::align_of::<crate::error::ErrorHeader>(),
        GC_TYPE_ERROR,
    ) as *mut crate::error::ErrorHeader;
    std::ptr::write(
        ptr,
        crate::error::ErrorHeader {
            object_type: crate::error::OBJECT_TYPE_ERROR,
            error_kind: crate::error::ERROR_KIND_ERROR,
            flags: 0,
            message: std::ptr::null_mut(),
            name: std::ptr::null_mut(),
            stack: std::ptr::null_mut(),
            cause: f64::from_bits(crate::value::TAG_UNDEFINED),
            errors: std::ptr::null_mut(),
        },
    );
    ptr
}

pub(super) unsafe fn alloc_old_test_set(
    capacity: u32,
) -> (*mut crate::set::SetHeader, *mut u64, std::alloc::Layout) {
    let set = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::set::SetHeader>(),
        8,
        GC_TYPE_SET,
    ) as *mut crate::set::SetHeader;
    let layout = std::alloc::Layout::from_size_align((capacity as usize * 8).max(8), 8)
        .expect("valid set elements layout");
    let elements = std::alloc::alloc_zeroed(layout) as *mut u64;
    assert!(!elements.is_null());
    (*set).size = 0;
    (*set).capacity = capacity;
    (*set).elements = elements as *mut f64;
    (set, elements, layout)
}

pub(super) unsafe fn retire_old_test_set(
    set: *mut crate::set::SetHeader,
    elements: *mut u64,
    layout: std::alloc::Layout,
) {
    (*set).size = 0;
    (*set).capacity = 0;
    (*set).elements = std::ptr::null_mut();
    std::alloc::dealloc(elements as *mut u8, layout);
}

pub(super) fn activate_malloc_registry_for_tests() {
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        ensure_set_built(&mut s);
    });
}

pub(super) fn gc_collection_count() -> u64 {
    GC_STATS.with(|s| s.borrow().collection_count)
}

pub(super) fn complete_budgeted_gc_cycle() -> JsGcStepResult {
    let mut result = JsGcStepResult::default();
    for _ in 0..500_000 {
        js_gc_step_work_units(1, &mut result);
        match result.status {
            JS_GC_STEP_STATUS_ACTIVE => continue,
            JS_GC_STEP_STATUS_COMPLETED => return result,
            other => panic!("budgeted GC cycle stopped before completion: status {other}"),
        }
    }
    panic!("budgeted GC cycle did not complete within step limit");
}

/// Helper for write-barrier tests: clear the remembered set
/// to a known-empty state.
pub(super) fn reset_remembered_set() {
    remembered_set_clear();
    crate::arena::old_arena_page_index_clear_for_tests();
}

pub(super) struct IncrementalMarkBarrierTestGuard<'a> {
    _valid_ptrs: &'a ValidPointerSet,
}

impl<'a> IncrementalMarkBarrierTestGuard<'a> {
    pub(super) fn new(valid_ptrs: &'a ValidPointerSet) -> Self {
        incremental_mark_barrier_enable(valid_ptrs, /* minor_only = */ false);
        Self {
            _valid_ptrs: valid_ptrs,
        }
    }
}

impl Drop for IncrementalMarkBarrierTestGuard<'_> {
    fn drop(&mut self) {
        incremental_mark_barrier_disable();
        clear_mark_seeds();
    }
}

static COPYING_NURSERY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serializes every test that mutates PROCESS-GLOBAL runtime side tables.
/// The guards' state reset clears global stores (e.g. `CLOSURE_PROPS` via
/// `test_clear_closure_side_tables`) from whatever test thread runs it, so a
/// test elsewhere in the crate that populates-then-asserts one of those
/// globals under only its own private lock races the reset (observed:
/// `closure::dynamic_props::tests_1802` losing its parked entry mid-test).
/// Such tests must take this lock too — reachable crate-wide as
/// `crate::gc::global_side_table_test_lock()`.
pub(crate) fn copying_nursery_isolation_lock() -> std::sync::MutexGuard<'static, ()> {
    COPYING_NURSERY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn root_scanner_registry_counts() -> (usize, usize, usize, usize) {
    let rust_roots = ROOT_SCANNERS.with(|scanners| scanners.borrow().len());
    let mutable_roots = MUTABLE_ROOT_SCANNERS.with(|scanners| scanners.borrow().len());
    let ffi_roots = FFI_ROOT_SCANNERS.with(|scanners| scanners.borrow().len());
    let ffi_mutable_roots = FFI_MUTABLE_ROOT_SCANNERS.with(|scanners| scanners.borrow().len());
    (rust_roots, mutable_roots, ffi_roots, ffi_mutable_roots)
}

// `ConservativeScanAutoGuard` used to live here. It opted a thread out of the
// test build's `Full` conservative-scan default, back to production's `Auto`.
// The test build now defaults to `Auto` for every thread
// (`gc::roots::conservative_stack_scan_mode`), so the guard set the value it
// was already going to get — it could no longer fail, and per CLAUDE.md's
// kill-policy a mode that cannot be exercised is deleted rather than left for a
// future bisect to trust. Tests needing the scan *provably* off should pin
// `ConservativeScanDisabledGuard`, which asserts the stronger property.

/// Put the runtime-handle mutable-root scanner back into this thread's
/// registry.
///
/// REQUIRED before a `RuntimeHandleScope` roots anything inside
/// `ScopedRootScannerRegistryGuard` / `GcTestIsolationGuard` /
/// `CopyingNurseryTestGuard`: those guards `mem::take` the thread's
/// `MUTABLE_ROOT_SCANNERS` so a collection sees exactly the roots the test
/// installs, and the runtime-handle scanner goes with it. Without this call a
/// `RuntimeHandleScope` inside such a test is decorative — its handles are
/// neither marked nor rewritten, so a raw pointer held across a GC-capable
/// call is silently unrooted and the test can pass for the wrong reason.
pub(crate) fn register_runtime_handle_root_scanner_for_tests() {
    gc_register_budgeted_mutable_root_scanner_with_source(
        scan_runtime_handle_roots_mut,
        scan_runtime_handle_roots_mut_step,
        new_runtime_handle_root_scan_state,
        MutableRootScannerSource::RuntimeHandles,
    );
}

/// Pin this thread's conservative-scan mode to `Disabled` for the guard's
/// lifetime, restoring the prior override on drop.
///
/// `Auto` and `Disabled` both resolve to
/// `ConservativeStackScanDecision::SkipDisabled`, but `Disabled` says so
/// unconditionally and cannot be re-interpreted by a future `Auto` policy
/// change. Tests that assert an object survives *only* because a precise
/// root marked it (#6910) must use this: under the test build's `Full`
/// default the native-stack scan accepts bare addresses and would rescue the
/// object, hiding a missing precise mark.
pub(super) struct ConservativeScanDisabledGuard {
    prev: Option<crate::gc::ConservativeStackScanMode>,
}

impl ConservativeScanDisabledGuard {
    pub(super) fn new() -> Self {
        Self {
            prev: crate::gc::set_conservative_stack_scan_override(Some(
                crate::gc::ConservativeStackScanMode::Disabled,
            )),
        }
    }
}

impl Drop for ConservativeScanDisabledGuard {
    fn drop(&mut self) {
        crate::gc::set_conservative_stack_scan_override(self.prev);
    }
}

pub(super) struct ScopedRootScannerRegistryGuard {
    rust_roots_len: usize,
    /// The thread's mutable scanner registry is taken whole (not just length-
    /// recorded) so a prior test's lazy `ensure_gc_initialized` registration
    /// can't leak into this test's controlled root set. Restored on drop.
    saved_mutable_roots: Vec<MutableRootScannerEntry>,
    ffi_roots_len: usize,
    ffi_mutable_roots_len: usize,
    prev_auto_init_suppressed: bool,
    prev_conservative_override: Option<crate::gc::ConservativeStackScanMode>,
}

impl ScopedRootScannerRegistryGuard {
    pub(super) fn new() -> Self {
        let (rust_roots_len, _mutable_roots_len, ffi_roots_len, ffi_mutable_roots_len) =
            root_scanner_registry_counts();
        // Take control of the thread's mutable scanner registry: snapshot it,
        // clear it so the test starts from a known-empty set, and suppress the
        // runtime's lazy auto-init (`ensure_gc_initialized`) for the guard's
        // lifetime so collections see exactly the roots the test installs.
        let saved_mutable_roots =
            MUTABLE_ROOT_SCANNERS.with(|scanners| std::mem::take(&mut *scanners.borrow_mut()));
        let prev_auto_init_suppressed = crate::gc::set_auto_gc_init_suppressed(true);
        // Opt out of the test build's full-conservative-scan default (see
        // `conservative_stack_scan_mode`): GC tests verify collection of objects
        // they hold only as native-stack locals, so they need the native scan
        // *skipped* — exactly the production `Auto` behavior.
        let prev_conservative_override = crate::gc::set_conservative_stack_scan_override(Some(
            crate::gc::ConservativeStackScanMode::Auto,
        ));
        Self {
            rust_roots_len,
            saved_mutable_roots,
            ffi_roots_len,
            ffi_mutable_roots_len,
            prev_auto_init_suppressed,
            prev_conservative_override,
        }
    }
}

impl Drop for ScopedRootScannerRegistryGuard {
    fn drop(&mut self) {
        ROOT_SCANNERS.with(|scanners| scanners.borrow_mut().truncate(self.rust_roots_len));
        MUTABLE_ROOT_SCANNERS.with(|scanners| {
            *scanners.borrow_mut() = std::mem::take(&mut self.saved_mutable_roots);
        });
        FFI_ROOT_SCANNERS.with(|scanners| scanners.borrow_mut().truncate(self.ffi_roots_len));
        FFI_MUTABLE_ROOT_SCANNERS.with(|scanners| {
            scanners.borrow_mut().truncate(self.ffi_mutable_roots_len);
        });
        crate::gc::set_conservative_stack_scan_override(self.prev_conservative_override);
        crate::gc::set_auto_gc_init_suppressed(self.prev_auto_init_suppressed);
    }
}

pub(super) struct GcTestIsolationGuard {
    _scanner_guard: ScopedRootScannerRegistryGuard,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl GcTestIsolationGuard {
    pub(super) fn new() -> Self {
        Self::build(RealmBootstrap::LeaveLazy)
    }

    /// [`GcTestIsolationGuard::new`] plus: run the lazy `globalThis` realm
    /// bootstrap BEFORE the measured window opens (#7975).
    ///
    /// REQUIRED by any test that asserts an unrooted object is DEAD at a
    /// collection and reaches the runtime through an API that can resolve the
    /// realm. Two facts compose into a scheduling-dependent false failure:
    ///
    /// 1. `object::set_property_attrs` and `js_object_set_field_by_name` both
    ///    consult the **process-global** memoized `Object.prototype` address
    ///    (`array::prototype_addr`), and a MISS runs the whole lazy
    ///    `globalThis` bootstrap — ~1.15 MB allocated, ~410 KB of it live,
    ///    rooted for the life of the thread — inside the caller. The cache
    ///    misses exactly once per PROCESS, so WHICH libtest thread pays is a
    ///    scheduling accident.
    /// 2. Arena block reset is all-or-nothing, so
    ///    `gc::trace::mark_block_persisting_arena_objects` force-MARKS every
    ///    object in a block that holds one reachable object. A test owner that
    ///    shares its block with a freshly-bootstrapped realm is therefore NOT
    ///    dead — and the death prune *correctly* declines to drop its
    ///    side-table entry, because a block that persists cannot recycle the
    ///    owner's address.
    ///
    /// Measured on `origin/main` before this existed: the two affected cases in
    /// `dead_owner_side_tables` failed **200/200** runs when scheduled first and
    /// **0/200** when any sibling resolved the cache first — 10/200 at
    /// `--test-threads=10` over the whole module (#7975).
    ///
    /// Bootstrapping here — inside the isolation lock, but before
    /// [`ScopedRootScannerRegistryGuard`] takes the thread's scanners and
    /// before `reset_global_roots` — puts the realm graph OUTSIDE the window:
    /// the guard then un-roots it, so it cannot keep the test's own block
    /// alive.
    pub(super) fn with_realm_bootstrapped() -> Self {
        Self::build(RealmBootstrap::RunItNow)
    }

    fn build(realm: RealmBootstrap) -> Self {
        let lock = copying_nursery_isolation_lock();
        if matches!(realm, RealmBootstrap::RunItNow) {
            let global = crate::object::js_get_global_this();
            assert!(
                crate::value::JSValue::from_bits(global.to_bits()).is_pointer(),
                "the realm bootstrap must have produced a singleton — otherwise \
                 it did not run here, and the confounder this guard exists to \
                 move out of the window is still inside it"
            );
        }
        let scanner_guard = ScopedRootScannerRegistryGuard::new();
        reset_copying_nursery_runtime_test_state();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        Self {
            _scanner_guard: scanner_guard,
            _lock: lock,
        }
    }
}

/// Whether [`GcTestIsolationGuard::build`] forces the lazy `globalThis`
/// bootstrap before opening the window. See
/// [`GcTestIsolationGuard::with_realm_bootstrapped`].
enum RealmBootstrap {
    LeaveLazy,
    RunItNow,
}

impl Drop for GcTestIsolationGuard {
    fn drop(&mut self) {
        reset_copying_nursery_runtime_test_state();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
    }
}

pub(crate) struct CopyingNurseryTestGuard {
    frame: u64,
    _scanner_guard: ScopedRootScannerRegistryGuard,
    _lock: std::sync::MutexGuard<'static, ()>,
}

pub(super) fn reset_copying_nursery_runtime_test_state() {
    // Age-sensitive tests assume the power-on tenuring threshold (promote at
    // the 4th survival); pin it so a heavy-influx test earlier on the same
    // thread cannot leak a lowered adaptive threshold in.
    crate::gc::tenuring::reset_for_test();
    // #7645: the young-pin latch is process-wide and monotone, so one
    // earlier pinning test would otherwise leave every later copying test
    // running the preflight — masking the skip path entirely. Callers hold
    // the copying-nursery isolation lock, so this reset is not racy.
    crate::gc::test_reset_young_pin_latch();
    activate_malloc_registry_for_tests();
    crate::object::test_clear_overflow_fields_root();
    crate::object::test_clear_transition_cache_root();
    crate::object::test_clear_object_cache_roots();
    crate::object::test_clear_class_side_table_roots();
    crate::object::test_clear_arguments_object_roots();
    crate::symbol::test_clear_symbol_side_table_roots();
    crate::json::test_clear_parse_roots();
    crate::set::test_clear_set_roots();
    crate::os::test_clear_process_event_listeners();
    crate::promise::test_clear_promise_scanner_roots();
    crate::timer::test_clear_all_timer_scanner_roots();
    crate::closure::test_clear_singleton_closure_caches();
    crate::closure::test_clear_closure_side_tables();
    crate::r#box::test_clear_box_registry();
    crate::builtins::test_set_console_log_singleton(0);
    crate::geisterhand_registry::test_clear_geisterhand_roots();
    crate::ui_text_registry::test_clear_ui_text_registry_roots();
    #[cfg(feature = "full")]
    {
        let _plugin_registry_guard = crate::plugin::PLUGIN_REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::plugin::test_clear_plugin_roots();
    }
}

impl CopyingNurseryTestGuard {
    pub(crate) fn new(slot_count: u32) -> Self {
        let lock = copying_nursery_isolation_lock();
        let scanner_guard = ScopedRootScannerRegistryGuard::new();
        reset_copying_nursery_runtime_test_state();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        js_gc_write_barriers_emitted(1);
        let frame = js_shadow_frame_push(slot_count);
        Self {
            frame,
            _scanner_guard: scanner_guard,
            _lock: lock,
        }
    }
}

impl Drop for CopyingNurseryTestGuard {
    fn drop(&mut self) {
        js_shadow_frame_pop(self.frame);
        reset_copying_nursery_runtime_test_state();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        js_gc_write_barriers_emitted(0);
    }
}

pub(crate) struct GcTriggerThresholdTestGuard {
    next_arena_trigger: usize,
    next_malloc_trigger: usize,
    malloc_step: usize,
}

impl GcTriggerThresholdTestGuard {
    pub(crate) fn suppress_automatic_triggers() -> Self {
        let next_arena_trigger = GC_NEXT_TRIGGER_BYTES.with(|trigger| {
            let previous = trigger.get();
            trigger.set(usize::MAX);
            previous
        });
        let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
            let previous = trigger.get();
            trigger.set(usize::MAX);
            previous
        });
        let malloc_step = GC_MALLOC_COUNT_STEP.with(|step| step.get());
        Self {
            next_arena_trigger,
            next_malloc_trigger,
            malloc_step,
        }
    }

    pub(super) fn make_malloc_sweep_due(&self) {
        let current = malloc_object_count();
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(current));
    }

    pub(super) fn make_arena_trigger_due(&self) {
        // Just-due, not zero: with debt-proportional assist pacing
        // (`gc_mutator_assist_scaled_work_units`), trigger=0 would read the
        // ENTIRE arena as debt and scale the first assist into a monolithic
        // collection — these tests want "trigger due, collector keeping up"
        // (debt ≈ 1 byte). Pacing-specific tests inflate debt explicitly.
        let just_due = crate::arena::arena_total_bytes().saturating_sub(1);
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(just_due));
    }
}

impl Drop for GcTriggerThresholdTestGuard {
    fn drop(&mut self) {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(self.next_arena_trigger));
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(self.next_malloc_trigger));
        GC_MALLOC_COUNT_STEP.with(|step| step.set(self.malloc_step));
    }
}

pub(super) fn collect_minor_trace(trigger_kind: GcTriggerKind) -> GcCycleTrace {
    gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: trigger_kind,
        steps_before: Some(GcStepSnapshot::current()),
    })
    .trace
    .expect("test requested GC trace capture")
}

pub(super) fn assert_copied_minor_trace(
    trace: &GcCycleTrace,
    eligible: bool,
    fallback_reason: CopiedMinorFallbackReason,
    malloc_sweep_due: bool,
) {
    assert_eq!(trace.copying_nursery.eligible, eligible);
    assert_eq!(trace.copying_nursery.fallback_reason, fallback_reason);
    assert_eq!(trace.copying_nursery.malloc_sweep_due, malloc_sweep_due);
}

// `EnvVarGuard` used to live here. It took `std::env::set_var` under a mutex,
// which serialized the twelve tests that SET a `PERRY_GC_*` knob against each
// other and did nothing whatsoever for the ~2 200 that READ one — the process
// environment is shared by every libtest thread, and the damage window is
// "between this test's set and another test's read".
//
// It was a live source of red builds (#7946), not a theoretical hazard:
// `PERRY_GC_FORCE_EVACUATE=1` is an input to `should_promote_young_in_place()`,
// so holding it for one test's duration silently turned in-place promotion off
// underneath `gc::tests::promote_in_place`'s policy cases — 5 failed runs in
// 100 across three of them.
//
// Knobs a test needs to move now have a PER-THREAD override next to the reader
// that consults them, the same shape `barrier_arming`'s `TEST_ARMED_OVERRIDE`,
// `oldgen_defrag`'s `OLD_DEFRAG_TEST_OVERRIDE` and `roots`'s
// `CONSERVATIVE_STACK_SCAN_OVERRIDE` already used. A knob whose *parse* or
// *precedence* is the subject gets a pure function taking the value, the way
// `parse_promote_in_place` does — never the live process environment.
pub(super) use crate::gc::knob_overrides::{ForcedEvacuationTestGuard, VerifyEvacuationTestGuard};

static GENERATED_BARRIER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) struct GeneratedWriteBarrierTestGuard {
    previous: usize,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl GeneratedWriteBarrierTestGuard {
    pub(super) fn active() -> Self {
        let lock = GENERATED_BARRIER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = GENERATED_WRITE_BARRIERS_EMITTED.swap(0, Ordering::AcqRel);
        js_gc_write_barriers_emitted(1);
        Self {
            previous,
            _lock: lock,
        }
    }

    pub(super) fn inactive() -> Self {
        let lock = GENERATED_BARRIER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = GENERATED_WRITE_BARRIERS_EMITTED.swap(0, Ordering::AcqRel);
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for GeneratedWriteBarrierTestGuard {
    fn drop(&mut self) {
        GENERATED_WRITE_BARRIERS_EMITTED.store(self.previous, Ordering::Release);
    }
}

thread_local! {
    static TEST_COPY_ONLY_ROOTS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

fn test_copy_only_root_scanner(mark: &mut dyn FnMut(f64)) {
    TEST_COPY_ONLY_ROOTS.with(|roots| {
        for &value in roots.borrow().iter() {
            mark(value);
        }
    });
}

extern "C" fn test_ffi_copy_only_root_scanner(mark: PerryFfiRootMarker, ctx: *mut c_void) {
    TEST_COPY_ONLY_ROOTS.with(|roots| {
        for &value in roots.borrow().iter() {
            mark(value, ctx);
        }
    });
}

enum TemporaryCopyOnlyRootScannerKind {
    Rust,
    Ffi,
}

pub(super) struct TemporaryCopyOnlyRootScanner {
    previous_rust_len: usize,
    previous_ffi_len: usize,
    previous_roots: Vec<f64>,
}

impl TemporaryCopyOnlyRootScanner {
    pub(super) fn rust_bits(bits: &[u64]) -> Self {
        Self::new(TemporaryCopyOnlyRootScannerKind::Rust, bits)
    }

    pub(super) fn ffi_bits(bits: &[u64]) -> Self {
        Self::new(TemporaryCopyOnlyRootScannerKind::Ffi, bits)
    }

    fn new(kind: TemporaryCopyOnlyRootScannerKind, bits: &[u64]) -> Self {
        let previous_roots = TEST_COPY_ONLY_ROOTS.with(|roots| {
            roots.replace(bits.iter().copied().map(f64::from_bits).collect::<Vec<_>>())
        });
        let previous_rust_len = ROOT_SCANNERS.with(|scanners| {
            let mut scanners = scanners.borrow_mut();
            let previous_rust_len = scanners.len();
            if matches!(kind, TemporaryCopyOnlyRootScannerKind::Rust) {
                scanners.push(test_copy_only_root_scanner);
            }
            previous_rust_len
        });
        let previous_ffi_len = FFI_ROOT_SCANNERS.with(|scanners| {
            let mut scanners = scanners.borrow_mut();
            let previous_ffi_len = scanners.len();
            if matches!(kind, TemporaryCopyOnlyRootScannerKind::Ffi) {
                scanners.push(test_ffi_copy_only_root_scanner);
            }
            previous_ffi_len
        });
        Self {
            previous_rust_len,
            previous_ffi_len,
            previous_roots,
        }
    }
}

impl Drop for TemporaryCopyOnlyRootScanner {
    fn drop(&mut self) {
        ROOT_SCANNERS.with(|scanners| {
            scanners.borrow_mut().truncate(self.previous_rust_len);
        });
        FFI_ROOT_SCANNERS.with(|scanners| {
            scanners.borrow_mut().truncate(self.previous_ffi_len);
        });
        TEST_COPY_ONLY_ROOTS.with(|roots| {
            roots.replace(std::mem::take(&mut self.previous_roots));
        });
    }
}

pub(super) fn young_leaf() -> usize {
    let id = YOUNG_LEAF_COUNTER.fetch_add(1, Ordering::Relaxed);
    let bytes = format!("young_leaf_{id:x}");
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as usize
}

pub(super) fn ptr_bits(addr: usize) -> u64 {
    POINTER_TAG | (addr as u64 & POINTER_MASK)
}

pub(super) fn string_bits(addr: usize) -> u64 {
    STRING_TAG | (addr as u64 & POINTER_MASK)
}

pub(super) unsafe fn assert_string_bytes(ptr: *const crate::StringHeader, expected: &[u8]) {
    assert!(!ptr.is_null(), "expected non-null string pointer");
    assert_eq!((*ptr).byte_len as usize, expected.len());
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, expected.len());
    assert_eq!(bytes, expected);
}

pub(super) fn old_page_dirty_for(page: usize) -> bool {
    crate::arena::old_page_meta_for_tests(page)
        .map(|meta| meta.dirty)
        .unwrap_or(false)
}

pub(super) extern "C" fn test_no_capture_singleton_func(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    0.0
}

pub(super) extern "C" fn test_captured_singleton_func(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    0.0
}

pub(super) unsafe fn init_test_closure(ptr: *mut u8) {
    let closure = ptr as *mut crate::closure::ClosureHeader;
    (*closure).func_ptr = std::ptr::null();
    (*closure).capture_count = 0;
    (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
}

pub(super) unsafe fn init_test_closure_with_one_capture(
    ptr: *mut u8,
    capture_bits: u64,
) -> *mut u64 {
    let closure = ptr as *mut crate::closure::ClosureHeader;
    (*closure).func_ptr = std::ptr::null();
    (*closure).capture_count = 1;
    (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
    let capture_slot = ptr.add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64;
    *capture_slot = capture_bits;
    layout_note_slot(ptr as usize, 0, capture_bits);
    let header = header_from_user_ptr(ptr as *const u8);
    if (*header).gc_flags & GC_FLAG_ARENA == 0 {
        runtime_write_barrier_external_slot(ptr as usize, capture_slot as usize, capture_bits);
    } else {
        runtime_write_barrier_slot(ptr as usize, capture_slot as usize, capture_bits);
    }
    capture_slot
}

#[inline(never)]
pub(super) fn allocate_dead_malloc_churn_headers(per_type: usize) -> Vec<usize> {
    let mut headers = Vec::with_capacity(per_type * 3);
    for _ in 0..per_type {
        let ptr = gc_malloc(32, GC_TYPE_STRING);
        unsafe {
            std::ptr::write_bytes(ptr, 0xA5, 32);
            headers.push(header_from_user_ptr(ptr) as usize);
        }
    }
    for _ in 0..per_type {
        let ptr = gc_malloc(
            std::mem::size_of::<crate::closure::ClosureHeader>(),
            GC_TYPE_CLOSURE,
        );
        unsafe {
            init_test_closure(ptr);
            headers.push(header_from_user_ptr(ptr) as usize);
        }
    }
    for _ in 0..per_type {
        let ptr = gc_malloc(
            std::mem::size_of::<crate::promise::Promise>(),
            GC_TYPE_PROMISE,
        ) as *mut crate::promise::Promise;
        unsafe {
            std::ptr::write(
                ptr,
                crate::promise::Promise {
                    state: crate::promise::PromiseState::Pending,
                    value: 0.0,
                    reason: 0.0,
                    on_fulfilled: std::ptr::null(),
                    on_rejected: std::ptr::null(),
                    next: std::ptr::null_mut(),
                    async_id: 0,
                    trigger_async_id: 0,
                },
            );
            headers.push(header_from_user_ptr(ptr as *const u8) as usize);
        }
    }
    headers
}

pub(super) fn tracked_malloc_headers_matching(headers: &[usize]) -> usize {
    MALLOC_STATE.with(|state| {
        let state = state.borrow();
        headers
            .iter()
            .filter(|&&addr| state.objects.iter().any(|&header| header as usize == addr))
            .count()
    })
}

pub(super) unsafe fn alloc_old_test_object(
    field_count: u32,
) -> (*mut crate::object::ObjectHeader, *mut u64) {
    // #8113: the live inline-slot bound lives ONLY in the ShapeId descriptor,
    // so a raw fixture has to publish one or the collector traces zero slots.
    // Mint the id BEFORE the object exists: minting inserts into the shape
    // table and can therefore collect, and this fixture holds no handle on the
    // fresh header.
    // A zero-slot fixture needs no descriptor at all — the derived bound is 0
    // either way — and minting one would perturb the descriptor-count
    // accounting that sibling tests assert on.
    let shape_id = if field_count == 0 {
        0
    } else {
        crate::object::shapes::shape_descriptor_ensure(std::ptr::null(), 0, field_count)
            .expect("shape id range exhausted in a test fixture")
    };
    let payload = std::mem::size_of::<crate::object::ObjectHeader>() + field_count as usize * 8;
    let obj = crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_OBJECT)
        as *mut crate::object::ObjectHeader;
    (*obj).class_id = 0;
    (*obj).parent_class_id = shape_id;
    (*obj).meta = std::ptr::null_mut();
    let fields =
        (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
    for i in 0..field_count as usize {
        *fields.add(i) = 0;
    }
    (obj, fields)
}

pub(super) unsafe fn alloc_nursery_test_object(
    field_count: u32,
) -> (*mut crate::object::ObjectHeader, *mut u64) {
    // #8113: see `alloc_old_test_object` — mint the descriptor first, then
    // stamp the fresh header with a plain store.
    // A zero-slot fixture needs no descriptor at all — the derived bound is 0
    // either way — and minting one would perturb the descriptor-count
    // accounting that sibling tests assert on.
    let shape_id = if field_count == 0 {
        0
    } else {
        crate::object::shapes::shape_descriptor_ensure(std::ptr::null(), 0, field_count)
            .expect("shape id range exhausted in a test fixture")
    };
    let payload = std::mem::size_of::<crate::object::ObjectHeader>() + field_count as usize * 8;
    let obj = crate::arena::arena_alloc_gc(payload, 8, GC_TYPE_OBJECT)
        as *mut crate::object::ObjectHeader;
    (*obj).class_id = 0;
    (*obj).parent_class_id = shape_id;
    (*obj).meta = std::ptr::null_mut();
    let fields =
        (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
    for i in 0..field_count as usize {
        *fields.add(i) = 0;
    }
    (obj, fields)
}

pub(super) unsafe fn init_test_symbol(ptr: *mut u8) {
    let id = YOUNG_LEAF_COUNTER.fetch_add(1, Ordering::Relaxed) as u64;
    let sym = ptr as *mut crate::symbol::SymbolHeader;
    (*sym).magic = crate::symbol::SYMBOL_MAGIC;
    (*sym).registered = 0;
    (*sym).description = std::ptr::null_mut();
    (*sym).id = 0x5A00_0000 | id;
}

pub(super) unsafe fn alloc_nursery_test_symbol() -> usize {
    let ptr = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::symbol::SymbolHeader>(),
        std::mem::align_of::<crate::symbol::SymbolHeader>(),
        GC_TYPE_STRING,
    );
    init_test_symbol(ptr);
    ptr as usize
}

pub(super) unsafe fn alloc_old_test_symbol() -> usize {
    let ptr = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::symbol::SymbolHeader>(),
        std::mem::align_of::<crate::symbol::SymbolHeader>(),
        GC_TYPE_STRING,
    );
    init_test_symbol(ptr);
    ptr as usize
}

pub(super) fn alloc_tracked_test_symbol() -> *mut crate::symbol::SymbolHeader {
    let ptr = gc_malloc(
        std::mem::size_of::<crate::symbol::SymbolHeader>(),
        GC_TYPE_STRING,
    );
    unsafe {
        init_test_symbol(ptr);
    }
    ptr as *mut crate::symbol::SymbolHeader
}

pub(super) unsafe fn alloc_old_test_array(
    length: u32,
) -> (*mut crate::array::ArrayHeader, *mut u64) {
    let payload = std::mem::size_of::<crate::array::ArrayHeader>() + length as usize * 8;
    let arr = crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_ARRAY)
        as *mut crate::array::ArrayHeader;
    (*arr).length = length;
    (*arr).capacity = length;
    let elements =
        (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
    for i in 0..length as usize {
        *elements.add(i) = 0;
    }
    (arr, elements)
}

pub(super) fn old_test_header_and_size(user: usize) -> (*mut GcHeader, usize) {
    let header = unsafe { header_from_user_ptr(user as *const u8) as *mut GcHeader };
    let total = unsafe { (*header).size as usize };
    (header, total)
}
