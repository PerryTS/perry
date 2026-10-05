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
        crate::exception::catch_js_throw(|| unsafe { np::link_owner(link) })
    })
    .join()
    .unwrap();
    assert_eq!(result, Ok(None));
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
fn every_sabotage_makes_its_runtime_witness_red() {
    let exe = std::env::current_exe().unwrap();
    for (fault, witness) in [
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
