use super::WEAK_HOLDERS;

thread_local! {
    static FULL_WEAK_PROCESSING_WORK_UNITS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    /// #7900: how many white objects the weak-READ barrier actually shaded.
    /// Tests assert this is non-zero so a green run cannot mean "the read
    /// happened to return an already-marked target" (CLAUDE.md: a gate must
    /// assert its subject was live).
    static WEAK_READ_BARRIER_SHADES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn weak_read_barrier_shades() -> usize {
    WEAK_READ_BARRIER_SHADES.with(std::cell::Cell::get)
}

pub(crate) fn reset_weak_read_barrier_shades() {
    WEAK_READ_BARRIER_SHADES.with(|shades| shades.set(0));
}

pub(crate) fn note_weak_read_barrier_shade() {
    WEAK_READ_BARRIER_SHADES.with(|shades| shades.set(shades.get().saturating_add(1)));
}

pub(crate) fn full_weak_processing_work_units() -> usize {
    FULL_WEAK_PROCESSING_WORK_UNITS.with(std::cell::Cell::get)
}

pub(crate) fn reset_full_weak_processing_work_units() {
    FULL_WEAK_PROCESSING_WORK_UNITS.with(|units| units.set(0));
}

pub(crate) fn note_full_weak_processing_work_unit() {
    FULL_WEAK_PROCESSING_WORK_UNITS.with(|units| units.set(units.get().saturating_add(1)));
}

pub(crate) fn clear_weak_holders() {
    WEAK_HOLDERS.with(|holders| holders.borrow_mut().clear());
}

pub(crate) fn weak_holder_addresses() -> Vec<usize> {
    let mut addresses =
        WEAK_HOLDERS.with(|holders| holders.borrow().iter().copied().collect::<Vec<_>>());
    addresses.sort_unstable();
    addresses
}

pub(crate) fn register_weak_holder_address(addr: usize) {
    WEAK_HOLDERS.with(|holders| {
        holders.borrow_mut().insert(addr);
    });
}

#[test]
fn empty_checkpoint_delivers_recorded_finalization_job() {
    std::thread::spawn(|| {
        use super::*;
        use std::sync::atomic::{AtomicU64, Ordering};
        static DELIVERED: AtomicU64 = AtomicU64::new(0);
        extern "C" fn cleanup(_: *const crate::closure::ClosureHeader, held: f64) -> f64 {
            DELIVERED.store(held.to_bits(), Ordering::Relaxed);
            0.0
        }
        crate::gc::js_gc_init();
        crate::closure::js_register_closure_arity(cleanup as *const u8, 1);
        let closure = crate::closure::js_closure_alloc(cleanup as *const u8, 0);
        let scope = crate::gc::RuntimeHandleScope::new();
        let callback = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(closure as i64));
        let registry = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_finreg_new(
            callback.get_nanbox_f64(),
        ) as i64));
        let target = crate::object::js_object_alloc(0, 0);
        js_finreg_register(
            registry.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(target as i64),
            41.0,
            f64::from_bits(TAG_UNDEFINED),
        );
        // Seed the post-collection delivery boundary with a real registry
        // record. Collection/weak liveness itself is covered by the weak-GC
        // tests; here no Promise or nextTick has been queued yet.
        let registry_ptr = js_nanbox_get_pointer(registry.get_nanbox_f64()) as *mut ObjectHeader;
        let entries = unsafe { object_field_bits(registry_ptr, FINREG_ENTRIES_FIELD) };
        let record = js_array_get_f64((entries & POINTER_MASK) as *mut ArrayHeader, 0);
        PENDING_FINALIZATION_JOBS.with(|jobs| {
            jobs.borrow_mut().push(PendingFinalizationJob {
                registry: registry.get_nanbox_f64(),
                record,
                callback: callback.get_nanbox_f64(),
                held: 41.0,
            })
        });
        assert!(!crate::promise::microtasks::empty_checkpoint_eligible_for_test());
        assert!(crate::promise::js_promise_run_microtasks_event_loop() > 0);
        assert_eq!(DELIVERED.load(Ordering::Relaxed), 41.0f64.to_bits());
        assert_eq!(pending_finalization_jobs_count(), 0);
    })
    .join()
    .unwrap();
}
