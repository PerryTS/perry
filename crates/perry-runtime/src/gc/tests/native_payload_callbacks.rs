//! Runtime witnesses for CALLBACK-DESIGN T1–T12. Tiny C-shaped calls keep the
//! runtime contract independent of the later sqlite/net conversion lanes.
use super::super::*;
use super::support::*;
use crate::native_payload::{
    self as np, CallEnd, CloseOutcome, NativePayloadFamily, OwnerLink, PayloadMiss,
};
use crate::value::TAG_UNDEFINED;
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static FINAL_JS: AtomicUsize = AtomicUsize::new(0);

#[derive(Default)]
struct Probe {
    link: Option<OwnerLink>,
}
impl Drop for Probe {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
        if let Some(link) = self.link {
            // Simulate a C xFinal called by native resource destruction.
            if unsafe { np::link_owner(link) }.is_some() {
                FINAL_JS.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}
fn install(_: &mut np::PayloadPrototype) {}
static FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::CRYPTO_HASH,
    name: "CallbackProbe",
    constructor_export: None,
    constructor_length: 0,
    links_owner: true,
    install_prototype: install,
};
struct Reset;
impl Reset {
    fn new() -> Self {
        np::reset_payload_prototypes_for_tests();
        gc_register_mutable_root_scanner(np::scan_payload_prototype_roots_mut);
        register_runtime_handle_root_scanner_for_tests();
        gc_register_named_mutable_root_scanner(
            "shape_table",
            crate::object::shapes::scan_shape_table_rekey_mut,
        );
        gc_register_named_mutable_root_scanner(
            "pinned",
            crate::gc::pin::scan_pinned_object_roots_mut,
        );
        DROPS.store(0, Ordering::SeqCst);
        CALLS.store(0, Ordering::SeqCst);
        FINAL_JS.store(0, Ordering::SeqCst);
        Self
    }
}
impl Drop for Reset {
    fn drop(&mut self) {
        np::reset_payload_prototypes_for_tests();
    }
}
fn owner() -> f64 {
    np::alloc(&FAMILY, Probe::default(), 0, &[])
}
fn cell(link: OwnerLink) -> *mut crate::native_handle::NativeHandleHeader {
    link.0 as *mut crate::native_handle::NativeHandleHeader
}
fn full() {
    // Manual GC uses complete precise roots and skips register-conservative
    // block persistence. Direct automatic cycles may retain unrooted neighbors
    // of a live prototype; that must not mask a missing trace in these tests.
    let before = crate::gc::block_persist_force_mark_count();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));
    assert_eq!(crate::gc::block_persist_force_mark_count(), before);
}

fn closure(info: *const crate::closure::JsFunctionInfo) -> f64 {
    crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(info, 0) as i64)
}
extern "C" fn throws(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    err: f64,
) -> f64 {
    CALLS.fetch_add(1, Ordering::SeqCst);
    crate::exception::js_throw(err)
}
extern "C" fn returns(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
    CALLS.fetch_add(1, Ordering::SeqCst);
    19.0
}

#[test]
fn t1_owner_moves_during_native_call() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let value = owner();
    js_shadow_slot_set(0, value.to_bits());
    let link = np::owner_link(value, &FAMILY).unwrap();
    let mut sites = np::CallbackSites::new();
    let userdata = sites.site(link, 7);
    for i in 0..4096 {
        sites.site(link, i);
    }
    let site = unsafe { &*(userdata as *const np::CallbackSite) };
    assert_eq!(site.index, 7, "growing sites must preserve C userdata");
    assert_eq!(site.link, link);
    let guard = np::enter(value, &FAMILY).unwrap();
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(trace.copying_nursery.eligible);
    let moved = f64::from_bits(js_shadow_slot_get(0));
    assert_ne!(
        value.to_bits(),
        moved.to_bits(),
        "fixture must move its owner"
    );
    assert_eq!(
        unsafe { np::link_owner(link) }.unwrap().to_bits(),
        moved.to_bits()
    );
    assert_eq!(guard.finish(), Ok(()));
    assert_eq!(unsafe { (*cell(link)).busy }, 0);
}

#[test]
fn t2_cell_ref_keeps_owner_and_callback_through_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _no_stack = ConservativeScanDisabledGuard::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let callback = closure(crate::fn_info!(returns, 0));
    np::set_callback(value.get_nanbox_f64(), &FAMILY, 127, callback);
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    unsafe {
        np::link_ref(link);
        np::link_ref(link);
    }
    let owner_addr = (value.get_nanbox_f64().to_bits() & POINTER_MASK) as usize;
    drop(scope);
    // Prove marking itself reaches the owner. Reading stale-but-not-reused
    // bytes after a sweep would otherwise let a missing mark look green.
    clear_marks();
    clear_mark_seeds();
    let valid_ptrs = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid_ptrs);
    drain_incremental_mark_barrier_seeds(&valid_ptrs);
    assert_ne!(
        unsafe { (*header_from_user_ptr(owner_addr as *const u8)).gc_flags } & GC_FLAG_MARKED,
        0,
        "a ref'ed cell must mark its otherwise unreferenced owner"
    );
    clear_marks();
    clear_mark_seeds();
    full();
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        0,
        "the pinned cell must trace its owner"
    );
    // A missing mark can leave an apparently valid stale owner word. Prove its
    // allocation survived before reading it.
    let value = unsafe { np::link_owner(link) }.unwrap();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let array = np::callbacks(value.get_nanbox_f64(), &FAMILY);
    let cb = crate::array::js_array_get_f64(
        (array.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader,
        127,
    );
    let guard = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    assert_eq!(
        unsafe {
            np::call_from_native(
                value.get_nanbox_f64(),
                cb,
                f64::from_bits(TAG_UNDEFINED),
                &[],
            )
        },
        Ok(19.0)
    );
    assert_eq!(guard.finish(), Ok(()));
    unsafe {
        np::link_unref(link);
    }
    drop(scope);
    full();
    assert_eq!(DROPS.load(Ordering::SeqCst), 0, "one ref still remains");
    unsafe {
        np::link_unref(link);
    }
    full();
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        1,
        "last unref releases the cycle"
    );
}

#[test]
fn t3_owner_store_has_a_remembered_barrier() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // Arm remembering before the store: lazy reconstruction must not hide a
    // dropped barrier by discovering the edge afterwards.
    let _ = barrier::remembered_dirty_snapshot();
    reset_remembered_set();
    let value = owner();
    let link = np::owner_link(value, &FAMILY).unwrap();
    let header = unsafe { header_from_user_ptr(cell(link) as *const u8) } as usize;
    let snapshot = barrier::remembered_dirty_snapshot();
    assert!(
        snapshot
            .external_dirty_entries
            .iter()
            .any(|&(_, h)| h == header)
            || snapshot.fallback_headers.contains(&header),
        "malloc -> young owner must be remembered"
    );
    js_shadow_slot_set(0, value.to_bits());
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(trace.copying_nursery.eligible);
    assert_ne!(value.to_bits(), js_shadow_slot_get(0));
    assert_eq!(
        unsafe { np::link_owner(link) }.unwrap().to_bits(),
        js_shadow_slot_get(0)
    );
}

#[test]
fn t4_t5_throw_identity_first_throw_wins_and_c_returns() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let cb = scope.root_nanbox_f64(closure(crate::fn_info!(throws, 1)));
    let err = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let err_value = err.with_mut_ptr::<crate::object::ObjectHeader, _>(|p| {
        crate::value::js_nanbox_pointer(p as i64)
    });
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let depth = crate::exception::current_try_depth();
    let guard = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    let returned = crate::exception::catch_js_throw(|| unsafe {
        assert_eq!(
            np::call_from_native(
                value.get_nanbox_f64(),
                cb.get_nanbox_f64(),
                f64::from_bits(TAG_UNDEFINED),
                &[err_value]
            ),
            Err(())
        );
        assert_eq!(
            np::call_from_native(
                value.get_nanbox_f64(),
                cb.get_nanbox_f64(),
                f64::from_bits(TAG_UNDEFINED),
                &[87.0]
            ),
            Err(())
        );
        true // C regained control, including after the second callback.
    });
    assert_eq!(returned, Ok(true), "throw must never cross C");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(thrown(guard.finish()).to_bits(), err_value.to_bits());
    assert_eq!(crate::exception::current_try_depth(), depth);
    assert_eq!(unsafe { (*cell(link)).busy }, 0);
    let guard = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    let cb = closure(crate::fn_info!(returns, 0));
    assert_eq!(
        unsafe {
            np::call_from_native(
                value.get_nanbox_f64(),
                cb,
                f64::from_bits(TAG_UNDEFINED),
                &[],
            )
        },
        Ok(19.0)
    );
    assert_eq!(
        guard.finish(),
        Ok(()),
        "owner is reusable after consuming the throw"
    );
}

#[test]
fn t4_native_validation_parks_typeerror_without_a_throw() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let guard = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    let message =
        crate::string::js_string_from_bytes(b"callback must return an integer".as_ptr(), 31);
    let error = scope.root_raw_mut_ptr(crate::error::js_typeerror_new(message));
    let error_bits = error.with_mut_ptr::<crate::error::ErrorHeader, _>(|p| {
        crate::value::js_nanbox_pointer(p as i64)
    });
    assert_eq!(
        np::set_pending_exception(value.get_nanbox_f64(), error_bits),
        Ok(())
    );
    assert_eq!(
        np::set_pending_exception(value.get_nanbox_f64(), 99.0),
        Err(())
    );
    assert_eq!(thrown(guard.finish()).to_bits(), error_bits.to_bits());
}

#[test]
fn t6_t11_finalization_callbacks_stay_out_of_js() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _no_stack = ConservativeScanDisabledGuard::new();
    let value = owner();
    let link = np::owner_link(value, &FAMILY).unwrap();
    unsafe {
        np::payload_mut::<Probe>(value, &FAMILY).unwrap().link = Some(link);
    }
    assert_eq!(np::close(value, &FAMILY), CloseOutcome::Closed);
    assert_eq!(unsafe { np::link_owner(link) }, None);
    assert_eq!(FINAL_JS.load(Ordering::SeqCst), 0);
    let value = owner();
    let link = np::owner_link(value, &FAMILY).unwrap();
    unsafe {
        np::payload_mut::<Probe>(value, &FAMILY).unwrap().link = Some(link);
    }
    full(); // owner and cell die together; drop must not dereference the owner.
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
    assert_eq!(FINAL_JS.load(Ordering::SeqCst), 0);
    // Exercise the actual worker cell teardown entry without destroying the
    // test harness's unrelated TLS roots.
    let value = owner();
    let link = np::owner_link(value, &FAMILY).unwrap();
    unsafe {
        np::payload_mut::<Probe>(value, &FAMILY).unwrap().link = Some(link);
        crate::native_handle::finalize_native_handle_at_teardown(cell(link));
    }
    assert_eq!(DROPS.load(Ordering::SeqCst), 3);
    assert_eq!(FINAL_JS.load(Ordering::SeqCst), 0);
    std::thread::spawn(|| {
        let value = owner();
        let link = np::owner_link(value, &FAMILY).unwrap();
        unsafe {
            np::payload_mut::<Probe>(value, &FAMILY).unwrap().link = Some(link);
        }
        // Leave the payload open: the worker's malloc TLS destructor owns cleanup.
    })
    .join()
    .unwrap();
    assert_eq!(DROPS.load(Ordering::SeqCst), 4);
    assert_eq!(FINAL_JS.load(Ordering::SeqCst), 0);
}

#[test]
fn t7_t8_close_defers_until_reentrant_calls_return() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let value = owner();
    let link = np::owner_link(value, &FAMILY).unwrap();
    let outer = np::enter(value, &FAMILY).unwrap();
    let inner = np::enter(value, &FAMILY).unwrap();
    assert_eq!(unsafe { (*cell(link)).busy }, 2);
    assert_eq!(np::close(value, &FAMILY), CloseOutcome::Deferred);
    assert_eq!(
        unsafe { np::payload_mut::<Probe>(value, &FAMILY) }.err(),
        Some(PayloadMiss::Closed)
    );
    assert_eq!(unsafe { np::link_owner(link) }, None);
    assert_eq!(np::close(value, &FAMILY), CloseOutcome::AlreadyClosed);
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);
    assert_eq!(inner.finish(), Err(CallEnd::Closed));
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);
    assert_eq!(outer.finish(), Err(CallEnd::Closed));
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(unsafe { (*cell(link)).busy }, 0);
}

#[test]
fn t9_wrong_thread_link_owner_never_throws() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let value = owner();
    let link = np::owner_link(value, &FAMILY).unwrap();
    let result = std::thread::spawn(move || {
        crate::exception::catch_js_throw(|| unsafe {
            (np::link_owner(link), np::enter_link(link).err())
        })
    })
    .join()
    .unwrap();
    assert_eq!(result, Ok((None, Some(PayloadMiss::Closed))));
    assert_eq!(
        unsafe { np::link_owner(link) }.map(f64::to_bits),
        Some(value.to_bits())
    );
}

#[test]
fn t10_two_hundred_thousand_owner_cycles_have_flat_rss() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _no_stack = ConservativeScanDisabledGuard::new();
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut rss: Vec<usize> = Vec::new();
    for batch in 0..20 {
        for _ in 0..10_000 {
            let scope = RuntimeHandleScope::new();
            let value = scope.root_nanbox_f64(owner());
            let state = np::js_state(value.get_nanbox_f64(), &FAMILY, true);
            let key = crate::string::intern_ascii_literal(b"capturedOwner");
            // Model a closure capturing the owner: the same strong GC cycle.
            crate::object::js_object_set_field_by_name(
                (state.to_bits() & POINTER_MASK) as *mut crate::object::ObjectHeader,
                key,
                value.get_nanbox_f64(),
            );
            if np::callback_sabotage("churn") {
                unsafe {
                    np::link_ref(np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap());
                }
            }
        }
        full();
        assert_eq!(DROPS.load(Ordering::SeqCst), (batch + 1) * 10_000);
        #[cfg(target_os = "linux")]
        {
            let stat = std::fs::read_to_string("/proc/self/statm").unwrap();
            let pages: usize = stat.split_whitespace().nth(1).unwrap().parse().unwrap();
            rss.push(pages * 4096);
        }
    }
    if rss.len() == 20 {
        let warm = rss[4..].iter().copied().min().unwrap();
        let peak = rss[4..].iter().copied().max().unwrap();
        eprintln!(
            "callback churn: created=200000 finalized={} warm_rss={} peak_rss={} delta={}",
            DROPS.load(Ordering::SeqCst),
            warm,
            peak,
            peak - warm
        );
        assert!(
            peak - warm < 16 * 1024 * 1024,
            "RSS must plateau after warmup"
        );
    }
}

extern "C" fn nested(_: *const crate::closure::ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let value = this.as_f64();
    let caught = crate::exception::catch_js_throw(|| {
        let guard = np::enter(value, &FAMILY).unwrap();
        let cb = closure(crate::fn_info!(throws, 1));
        assert_eq!(
            unsafe { np::call_from_native(value, cb, f64::from_bits(TAG_UNDEFINED), &[43.0]) },
            Err(())
        );
        // Planted conversion throw inside the span: finish is skipped.
        if np::callback_sabotage("conversion") {
            crate::exception::js_throw(43.0);
        }
        if let Err(CallEnd::Threw(err)) = guard.finish() {
            crate::exception::js_throw(err);
        }
    });
    assert_eq!(caught, Err(43.0));
    23.0
}
#[test]
fn t12_caught_nested_throw_leaves_busy_zero() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let cb = closure(crate::fn_info!(nested, 0));
    let depth = crate::exception::current_try_depth();
    let guard = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    assert_eq!(
        unsafe { np::call_from_native(value.get_nanbox_f64(), cb, value.get_nanbox_f64(), &[]) },
        Ok(23.0)
    );
    assert_eq!(guard.finish(), Ok(()));
    assert_eq!(unsafe { (*cell(link)).busy }, 0);
    assert_eq!(crate::exception::current_try_depth(), depth);
}

#[test]
fn callback_cell_slot_moves_grows_replaces_and_clears() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let cb = scope.root_nanbox_f64(closure(crate::fn_info!(returns, 0)));
    np::set_callback(value.get_nanbox_f64(), &FAMILY, 0, cb.get_nanbox_f64());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let before = unsafe { *np::callback_slot_address(cell(link)).unwrap() };
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(trace.copying_nursery.eligible);
    let after = np::callbacks(value.get_nanbox_f64(), &FAMILY).to_bits();
    assert_ne!(before, after, "fixture must move the callbacks array");
    assert_eq!(
        unsafe { *np::callback_slot_address(cell(link)).unwrap() },
        after
    );
    assert_eq!(
        unsafe { np::callback_from_link(link, 0) }.to_bits(),
        cb.get_nanbox_f64().to_bits()
    );
    np::set_callback(value.get_nanbox_f64(), &FAMILY, 4096, cb.get_nanbox_f64());
    assert_eq!(
        unsafe { np::callback_from_link(link, 4096) }.to_bits(),
        cb.get_nanbox_f64().to_bits()
    );
    let replacement = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(0) as i64,
    ));
    np::state_set(
        value.get_nanbox_f64(),
        &FAMILY,
        b"callbacks",
        replacement.get_nanbox_f64(),
    );
    assert_eq!(
        unsafe { np::callback_from_link(link, 0) }.to_bits(),
        TAG_UNDEFINED
    );
    np::set_callback(value.get_nanbox_f64(), &FAMILY, 0, cb.get_nanbox_f64());
    crate::state_key_memo!(static MEMO_TEST_CALLBACKS);
    np::state_set_memo(
        value.get_nanbox_f64(),
        &FAMILY,
        b"callbacks",
        f64::from_bits(TAG_UNDEFINED),
        &MEMO_TEST_CALLBACKS,
    );
    assert_eq!(
        unsafe { np::callback_from_link(link, 0) }.to_bits(),
        TAG_UNDEFINED
    );
}

#[test]
fn callback_cell_slot_marks_its_array_without_another_edge() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    // The guard isolates scanner registration, but realm towers from earlier
    // libtest cases still live on this thread. Preserve their production
    // roots during this mark walk; none points at this callbacks array.
    gc_register_named_mutable_root_scanner(
        "object_cache",
        crate::object::scan_object_cache_roots_mut,
    );
    let _no_stack = ConservativeScanDisabledGuard::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    np::set_callback(
        value.get_nanbox_f64(),
        &FAMILY,
        0,
        closure(crate::fn_info!(returns, 0)),
    );
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let array_addr =
        unsafe { *np::callback_slot_address(cell(link)).unwrap() } as usize & POINTER_MASK as usize;
    // Remove the duplicate state edge without using the synchronizing API:
    // only the cell slot may keep this array alive in this fixture.
    let state = np::js_state(value.get_nanbox_f64(), &FAMILY, false);
    let key = crate::string::intern_ascii_literal(b"callbacks");
    crate::object::js_object_set_field_by_name(
        (state.to_bits() & POINTER_MASK) as *mut crate::object::ObjectHeader,
        key,
        f64::from_bits(TAG_UNDEFINED),
    );
    clear_marks();
    clear_mark_seeds();
    let valid_ptrs = build_valid_pointer_set();
    mark_mutable_registered_roots(&valid_ptrs);
    drain_incremental_mark_barrier_seeds(&valid_ptrs);
    assert_ne!(
        unsafe { (*header_from_user_ptr(array_addr as *const u8)).gc_flags } & GC_FLAG_MARKED,
        0,
        "the cell's callback slot must mark the array"
    );
}

#[test]
fn callback_cell_slot_barrier_is_exact_for_malloc_parent() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let cb = closure(crate::fn_info!(returns, 0));
    np::set_callback(value.get_nanbox_f64(), &FAMILY, 0, cb);
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let _ = barrier::remembered_dirty_snapshot();
    reset_remembered_set();
    let replacement = crate::value::js_nanbox_pointer(crate::array::js_array_alloc(0) as i64);
    np::state_set(value.get_nanbox_f64(), &FAMILY, b"callbacks", replacement);
    let header = unsafe { header_from_user_ptr(cell(link) as *const u8) } as usize;
    let snapshot = barrier::remembered_dirty_snapshot();
    assert!(
        snapshot
            .external_dirty_entries
            .iter()
            .any(|&(_, h)| h == header)
            || snapshot.fallback_headers.contains(&header),
        "callback slot must remember its malloc parent"
    );
}

#[test]
fn runtime_callback_slots_never_call_an_accessor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let cb = scope.root_nanbox_f64(closure(crate::fn_info!(returns, 0)));
    np::set_callback(value.get_nanbox_f64(), &FAMILY, 1, cb.get_nanbox_f64());
    let arr = scope.root_nanbox_f64(np::callbacks(value.get_nanbox_f64(), &FAMILY));
    let desc = scope.root_raw_mut_ptr(crate::object::js_object_alloc_null_proto(0, 1));
    let get = crate::string::intern_ascii_literal(b"get");
    crate::object::js_object_set_field_by_name(desc.get_raw_mut_ptr(), get, cb.get_nanbox_f64());
    let key = scope.root_string_ptr(crate::string::intern_ascii_literal(b"0"));
    crate::object::js_object_define_property(
        arr.get_nanbox_f64(),
        crate::value::js_nanbox_string(key.get_raw_const_ptr::<crate::StringHeader>() as i64),
        crate::value::js_nanbox_pointer(
            desc.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
        ),
    );
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    CALLS.store(0, Ordering::SeqCst);
    let result = crate::exception::catch_js_throw(|| unsafe { np::callback_from_link(link, 0) });
    assert_eq!(result.map(f64::to_bits), Ok(TAG_UNDEFINED));
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        0,
        "callback lookup must never run JS"
    );
    np::state_set(
        value.get_nanbox_f64(),
        &FAMILY,
        b"callbacks",
        crate::value::js_nanbox_pointer(
            desc.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
        ),
    );
    assert_eq!(
        unsafe { np::callback_from_link(link, 0) }.to_bits(),
        TAG_UNDEFINED,
        "non-array storage is not a callback array"
    );
}

extern "C" fn clears_new_target_and_throws(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    crate::object::js_new_target_set(f64::from_bits(TAG_UNDEFINED));
    crate::exception::js_throw(43.0)
}

#[test]
fn native_call_reuses_catch_refreshes_roots_and_pops() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let returns = scope.root_nanbox_f64(closure(crate::fn_info!(returns, 0)));
    let throws = scope.root_nanbox_f64(closure(crate::fn_info!(throws, 1)));
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let depth = crate::exception::current_try_depth();
    let guard = unsafe { np::enter_link(link) }.unwrap();
    let ((), end) = unsafe {
        guard.call(|| {
            assert_eq!(
                np::call_from_link(link, returns.get_nanbox_f64(), &[]),
                Ok(19.0)
            );
            assert_eq!(crate::exception::current_try_depth(), depth + 1);
            // The second trampoline has argument-conversion roots absent from the
            // first. A throw must retain them instead of restoring the old depth.
            // Two roots: the first callback's own capture (inside its pending-slot
            // scope) sits one root above the guard, so one root here would
            // coincide with it and hide a missing refresh.
            let extra = RuntimeHandleScope::new();
            let object = extra.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            let _second = extra.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            let handles = crate::gc::runtime_handle_stack_savepoint();
            assert_eq!(
                np::call_from_link(link, throws.get_nanbox_f64(), &[43.0]),
                Err(())
            );
            assert_eq!(crate::gc::runtime_handle_stack_savepoint(), handles);
            assert!(!object
                .get_raw_mut_ptr::<crate::object::ObjectHeader>()
                .is_null());
            let cached = &*(*(cell(link) as *mut np::NativeCallbackCell)).catch;
            assert_eq!(
                cached.captures(),
                1,
                "many callbacks share one full savepoint"
            );
        })
    };
    assert_eq!(end, Err(CallEnd::Threw(43.0)));
    assert_eq!(crate::exception::current_try_depth(), depth);
    assert_eq!(unsafe { (*cell(link)).busy }, 0);
    assert!(unsafe {
        (*(cell(link) as *mut np::NativeCallbackCell))
            .catch
            .is_null()
    });
    // Native calls with no callback never push a handler.
    let guard = unsafe { np::enter_link(link) }.unwrap();
    assert_eq!(
        unsafe { guard.call(|| crate::exception::current_try_depth()) },
        (depth, Ok(()))
    );
}

#[test]
fn native_call_cached_new_target_moves_before_later_throw() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_named_mutable_root_scanner("exception", crate::exception::scan_exception_roots_mut);
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let returns = scope.root_nanbox_f64(closure(crate::fn_info!(returns, 0)));
    let throws = scope.root_nanbox_f64(closure(crate::fn_info!(clears_new_target_and_throws, 0)));
    let target = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let before = crate::value::js_nanbox_pointer(
        target.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64,
    );
    let old_target = scope.root_nanbox_f64(crate::object::js_new_target_set(before));
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let guard = unsafe { np::enter_link(link) }.unwrap();
    let (_, end) = unsafe {
        guard.call(|| {
            assert_eq!(
                np::call_from_link(link, returns.get_nanbox_f64(), &[]),
                Ok(19.0)
            );
            let trace = collect_minor_trace(GcTriggerKind::MallocCount);
            assert!(trace.copying_nursery.eligible);
            let moved = crate::value::js_nanbox_pointer(
                target.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64,
            );
            assert_ne!(before.to_bits(), moved.to_bits());
            // The live new.target cell need not be registered in this fixture:
            // restore it to its correct moved value before the second callback.
            crate::object::js_new_target_set(moved);
            assert_eq!(
                np::call_from_link(link, throws.get_nanbox_f64(), &[]),
                Err(())
            );
            assert_eq!(
                crate::object::js_new_target_get().to_bits(),
                moved.to_bits()
            );
        })
    };
    crate::object::js_new_target_set(old_target.get_nanbox_f64());
    assert_eq!(end, Err(CallEnd::Threw(43.0)));
}

extern "C" fn reenters_cached_then_closes(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(this.as_f64());
    let cb = scope.root_nanbox_f64(closure(crate::fn_info!(returns, 0)));
    let guard = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    let link = guard.owner_link();
    let (_, end) = unsafe {
        guard.call(|| {
            assert_eq!((*cell(link)).busy, 2);
            assert_eq!(np::call_from_link(link, cb.get_nanbox_f64(), &[]), Ok(19.0));
            assert_eq!(
                np::close(value.get_nanbox_f64(), &FAMILY),
                CloseOutcome::Deferred
            );
            assert_eq!(DROPS.load(Ordering::SeqCst), 0);
        })
    };
    assert_eq!(end, Err(CallEnd::Closed));
    assert_eq!(unsafe { (*cell(link)).busy }, 1);
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);
    23.0
}

#[test]
fn native_call_reentrant_catch_restores_outer_then_defers_close() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let cb = scope.root_nanbox_f64(closure(crate::fn_info!(reenters_cached_then_closes, 0)));
    let guard = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    let link = guard.owner_link();
    let depth = crate::exception::current_try_depth();
    let (result, end) = unsafe {
        guard.call(|| {
            let result = np::call_from_native(
                value.get_nanbox_f64(),
                cb.get_nanbox_f64(),
                value.get_nanbox_f64(),
                &[],
            );
            assert_eq!(crate::exception::current_try_depth(), depth + 1);
            let outer_ptr = (*(cell(link) as *mut np::NativeCallbackCell)).catch;
            assert!(
                !outer_ptr.is_null(),
                "nested call must restore the outer catch token"
            );
            let outer = &*outer_ptr;
            assert_eq!(outer.captures(), 1);
            result
        })
    };
    assert_eq!(result, Ok(23.0));
    assert_eq!(end, Err(CallEnd::Closed));
    assert_eq!(crate::exception::current_try_depth(), depth);
    assert_eq!(unsafe { (*cell(link)).busy }, 0);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
}

#[test]
fn callback_cell_slot_resolves_array_growth_before_gc_rewrite() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let cb = scope.root_nanbox_f64(closure(crate::fn_info!(returns, 0)));
    np::set_callback(value.get_nanbox_f64(), &FAMILY, 0, cb.get_nanbox_f64());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let original = np::callbacks(value.get_nanbox_f64(), &FAMILY);
    let arr =
        crate::JSValue::from_bits(original.to_bits()).as_pointer::<crate::array::ArrayHeader>();
    // Model an array alias growing its storage without a JS-state setter.
    let grown = scope.root_raw_mut_ptr(unsafe {
        crate::array::js_array_set_f64_extend(arr as *mut _, 4096, cb.get_nanbox_f64())
    });
    assert_ne!(
        original.to_bits() & POINTER_MASK,
        grown.get_raw_mut_ptr::<crate::array::ArrayHeader>() as u64
    );
    assert_eq!(
        unsafe { *np::callback_slot_address(cell(link)).unwrap() },
        original.to_bits()
    );
    assert_eq!(
        unsafe { np::callback_from_link(link, 4096) }.to_bits(),
        cb.get_nanbox_f64().to_bits()
    );
}

#[test]
fn callback_cell_slot_reads_sparse_own_data() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let cb = scope.root_nanbox_f64(closure(crate::fn_info!(returns, 0)));
    let index = 1_048_576;
    np::set_callback(value.get_nanbox_f64(), &FAMILY, index, cb.get_nanbox_f64());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let array = np::callbacks(value.get_nanbox_f64(), &FAMILY);
    let arr = crate::JSValue::from_bits(array.to_bits()).as_pointer::<crate::array::ArrayHeader>();
    assert!(
        unsafe { (*arr).capacity } <= index,
        "fixture must take the sparse path"
    );
    assert_eq!(
        unsafe { np::callback_from_link(link, index) }.to_bits(),
        cb.get_nanbox_f64().to_bits()
    );
    assert_eq!(
        unsafe { np::callback_from_link(link, index - 1) }.to_bits(),
        TAG_UNDEFINED
    );
}

#[test]
fn every_sabotage_makes_its_runtime_witness_red() {
    let exe = std::env::current_exe().unwrap();
    for (fault, witness) in [
        (
            "callback_sparse_key",
            "callback_cell_slot_reads_sparse_own_data",
        ),
        (
            "callback_forwarding",
            "callback_cell_slot_resolves_array_growth_before_gc_rewrite",
        ),
        (
            "catch_reentry",
            "native_call_reentrant_catch_restores_outer_then_defers_close",
        ),
        (
            "catch_refresh",
            "native_call_reuses_catch_refreshes_roots_and_pops",
        ),
        (
            "catch_pop",
            "native_call_reuses_catch_refreshes_roots_and_pops",
        ),
        (
            "catch_new_target_trace",
            "native_call_cached_new_target_moves_before_later_throw",
        ),
        (
            "latch",
            "gc::tests::copying::latch::callback_cell_ref_does_not_arm_the_young_pin_latch",
        ),
        (
            "finalized_check",
            "t6_t11_finalization_callbacks_stay_out_of_js",
        ),
        ("rewrite", "t1_owner_moves_during_native_call"),
        (
            "mark",
            "t2_cell_ref_keeps_owner_and_callback_through_full_gc",
        ),
        (
            "pin",
            "t2_cell_ref_keeps_owner_and_callback_through_full_gc",
        ),
        ("barrier", "t3_owner_store_has_a_remembered_barrier"),
        (
            "callback_data_read",
            "runtime_callback_slots_never_call_an_accessor",
        ),
        (
            "callback_trace",
            "callback_cell_slot_moves_grows_replaces_and_clears",
        ),
        (
            "callback_trace",
            "callback_cell_slot_marks_its_array_without_another_edge",
        ),
        (
            "callback_sync",
            "callback_cell_slot_moves_grows_replaces_and_clears",
        ),
        (
            "callback_barrier",
            "callback_cell_slot_barrier_is_exact_for_malloc_parent",
        ),
        (
            "catch",
            "t4_t5_throw_identity_first_throw_wins_and_c_returns",
        ),
        (
            "pending",
            "t4_t5_throw_identity_first_throw_wins_and_c_returns",
        ),
        ("finalized", "t6_t11_finalization_callbacks_stay_out_of_js"),
        ("close", "t7_t8_close_defers_until_reentrant_calls_return"),
        ("reentry", "t7_t8_close_defers_until_reentrant_calls_return"),
        ("thread", "t9_wrong_thread_link_owner_never_throws"),
        (
            "churn",
            "t10_two_hundred_thousand_owner_cycles_have_flat_rss",
        ),
        ("conversion", "t12_caught_nested_throw_leaves_busy_zero"),
    ] {
        let name = if witness.starts_with("gc::") {
            witness.to_owned()
        } else {
            format!("gc::tests::native_payload_callbacks::{witness}")
        };
        let output = std::process::Command::new(&exe)
            .args(["--exact", &name, "--nocapture", "--test-threads=1"])
            .env("PERRY_TEST_CALLBACK_SABOTAGE", fault)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "sabotage must execute exactly one witness: {name}"
        );
        assert!(
            !output.status.success(),
            "{fault} must make {witness} RED; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("callback sabotage {fault}: RED ({})", output.status);
    }
}

fn thrown(result: Result<(), CallEnd>) -> f64 {
    match result {
        Err(CallEnd::Threw(err)) => err,
        other => panic!("expected callback throw, got {other:?}"),
    }
}

#[path = "native_payload_lifecycle.rs"]
mod lifecycle;
