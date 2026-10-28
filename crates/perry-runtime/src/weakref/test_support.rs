use super::WEAK_HOLDERS;

#[test]
fn cancellation_releases_finalization_records_without_delivering_cleanup_callbacks() {
    use super::*;
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    extern "C" fn forbidden(
        _: *const crate::closure::ClosureHeader,
        _: crate::closure::JsThis,
        _: f64,
    ) -> f64 {
        panic!("cancelled finalization callback ran");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(forbidden, 1),
        0,
    ));
    let callback = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
    ));
    let registry = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_finreg_new(
        callback.get_nanbox_f64(),
    ) as i64));
    let owner = crate::async_context::job_domain::JobDomain::new();
    for held in [41.0, 42.0] {
        let target = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        js_finreg_register(
            registry.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(target.get_raw_mut_ptr::<ObjectHeader>() as i64),
            held,
            f64::from_bits(TAG_UNDEFINED),
        );
        let reg = js_nanbox_get_pointer(registry.get_nanbox_f64()) as *mut ObjectHeader;
        let entries = unsafe { object_field_bits(reg, FINREG_ENTRIES_FIELD) };
        let entries = (entries & POINTER_MASK) as *mut ArrayHeader;
        let record = js_array_get_f64(entries, js_array_length(entries) - 1);
        PENDING_FINALIZATION_JOBS.with(|jobs| {
            jobs.borrow_mut().push(PendingFinalizationJob {
                registry: registry.get_nanbox_f64(),
                record,
                callback: callback.get_nanbox_f64(),
                held,
                domain: Some(owner.token()),
            })
        });
    }
    assert_eq!(pending_finalization_jobs_count(), 2);
    let before = registry.get_nanbox_f64().to_bits();
    crate::gc::gc_collect_minor();
    assert_ne!(before, registry.get_nanbox_f64().to_bits());
    owner.cancel();
    discard_cancelled_finalization_jobs();
    assert_eq!(pending_finalization_jobs_count(), 0);
    let reg = js_nanbox_get_pointer(registry.get_nanbox_f64()) as *mut ObjectHeader;
    let entries = unsafe { object_field_bits(reg, FINREG_ENTRIES_FIELD) };
    assert_eq!(
        js_array_length((entries & POINTER_MASK) as *mut ArrayHeader),
        0
    );
    assert_eq!(queue_pending_finalization_callbacks_after_gc(), 0);
    crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
}

thread_local! {
    static WEAK_ENTRY_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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

pub(crate) fn note_weak_entry_visit() {
    WEAK_ENTRY_VISITS.with(|visits| visits.set(visits.get() + 1));
}

pub(crate) fn reset_weak_entry_visits() {
    WEAK_ENTRY_VISITS.with(|visits| visits.set(0));
}

pub(crate) fn weak_entry_visits() -> usize {
    WEAK_ENTRY_VISITS.with(std::cell::Cell::get)
}

pub(crate) fn weak_entry_extent(map: f64) -> u32 {
    unsafe {
        let map = super::js_nanbox_get_pointer(map) as *mut super::ObjectHeader;
        let store = super::storage::owned_storage(map);
        if store.is_null() {
            0
        } else {
            (*store).len
        }
    }
}

pub(crate) fn reserve_weak_storage_for_tests(map: f64, capacity: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let map = scope.root_nanbox_f64(map);
    loop {
        unsafe {
            let owner =
                super::js_nanbox_get_pointer(map.get_nanbox_f64()) as *mut super::ObjectHeader;
            let table = super::storage::owned_storage(owner);
            if !table.is_null() && (*table).capacity >= capacity {
                break;
            }
            super::storage::grow(map);
        }
    }
}

#[test]
fn empty_checkpoint_delivers_recorded_finalization_job() {
    let result = std::thread::spawn(|| {
        use super::*;
        use std::sync::atomic::{AtomicU64, Ordering};
        static DELIVERED: AtomicU64 = AtomicU64::new(0);
        extern "C" fn cleanup(
            _: *const crate::closure::ClosureHeader,
            _this: crate::closure::JsThis,
            held: f64,
        ) -> f64 {
            DELIVERED.store(held.to_bits(), Ordering::Relaxed);
            0.0
        }
        crate::gc::js_gc_init();
        let closure =
            crate::closure::js_closure_alloc(crate::fn_info!(cleanup, 1; with_declared(1)), 0);
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
        let owner = crate::async_context::job_domain::JobDomain::new();
        let sibling = crate::async_context::job_domain::JobDomain::new();
        PENDING_FINALIZATION_JOBS.with(|jobs| {
            jobs.borrow_mut().push(PendingFinalizationJob {
                registry: registry.get_nanbox_f64(),
                record,
                callback: callback.get_nanbox_f64(),
                held: 41.0,
                domain: Some(owner.token()),
            })
        });
        assert!(!crate::promise::microtasks::empty_checkpoint_eligible_for_test());
        sibling.run(|| {
            assert!(!eligible_finalization_jobs_pending());
            crate::promise::microtasks::js_promise_run_microtasks_checkpoint();
            assert_eq!(DELIVERED.load(Ordering::Relaxed), 0);
            assert_eq!(pending_finalization_jobs_count(), 1);
        });
        owner.run(|| {
            assert!(eligible_finalization_jobs_pending());
            assert!(crate::promise::js_promise_run_microtasks_event_loop() > 0);
        });
        assert_eq!(DELIVERED.load(Ordering::Relaxed), 41.0f64.to_bits());
        assert_eq!(pending_finalization_jobs_count(), 0);
    })
    .join();
    // `js_gc_init` disables the process-wide class-field inline gate in test
    // builds; the spawned thread does not isolate that, so restore it.

    result.unwrap();
}
