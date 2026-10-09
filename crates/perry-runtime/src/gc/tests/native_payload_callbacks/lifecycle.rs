//! LIFECYCLE-DESIGN L4/L5/L8/L9 and runtime reopen/terminal-event contracts.
use super::*;
use np::{AttachMiss, Lifecycle};

fn finalized() -> usize {
    crate::native_handle::PAYLOAD_FINALIZED.load(Ordering::SeqCst)
}

#[test]
fn subclass_attachment_reopens_the_same_cell_and_rejects_open_or_finalized() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(777, 0));
    let value = || {
        obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
            crate::value::js_nanbox_pointer(obj as i64)
        })
    };
    assert!(np::attach_to_object(value(), &FAMILY, Probe::default(), 0));
    let cell_ptr = obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| unsafe {
        ((*(*obj).meta).native_state & POINTER_MASK)
            as *mut crate::native_handle::NativeHandleHeader
    });
    assert!(!np::attach_to_object(value(), &FAMILY, Probe::default(), 0));
    assert_eq!(DROPS.load(Ordering::SeqCst), 1, "rejected input is dropped");
    assert!(np::close_attached::<Probe>(value(), &FAMILY));
    assert!(np::attach_to_object(value(), &FAMILY, Probe::default(), 0));
    obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| unsafe {
        assert_eq!(
            ((*(*obj).meta).native_state & POINTER_MASK) as *mut _,
            cell_ptr
        );
        assert_eq!((*obj).class_id, 777);
    });
    unsafe { crate::native_handle::finalize_native_handle_at_teardown(cell_ptr) };
    assert!(!np::attach_to_object(value(), &FAMILY, Probe::default(), 0));
    assert_eq!(DROPS.load(Ordering::SeqCst), 4);
}

#[test]
fn l4_release_then_unrooted_sweep_finalizes_without_another_drop() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _no_stack = ConservativeScanDisabledGuard::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    unsafe {
        np::payload_mut::<Probe>(value.get_nanbox_f64(), &FAMILY)
            .unwrap()
            .link = Some(link)
    };
    let before = finalized();
    assert_eq!(
        np::close(value.get_nanbox_f64(), &FAMILY),
        CloseOutcome::Closed
    );
    assert_eq!(
        np::lifecycle(value.get_nanbox_f64(), &FAMILY),
        Ok(Lifecycle::Closed)
    );
    assert_eq!(finalized(), before, "release must not finalize");
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(unsafe { (*cell(link)).refs }, 0);
    drop(scope);
    full();
    assert_eq!(finalized(), before + 1, "an unrooted closed cycle must die");
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(FINAL_JS.load(Ordering::SeqCst), 0);
}

#[test]
fn l5_teardown_finalized_cell_cannot_attach() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    unsafe { crate::native_handle::finalize_native_handle_at_teardown(cell(link)) };
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(
        np::attach(value.get_nanbox_f64(), &FAMILY, Probe::default(), 0),
        Err(AttachMiss::Finalized)
    );
    assert_eq!(DROPS.load(Ordering::SeqCst), 2, "rejected input is dropped");
    assert_eq!(unsafe { np::link_event_owner(link) }, None);
    assert!(unsafe { (*cell(link)).resource_ptr.is_null() });
}

// Same size/alignment as Probe, but a different destructor. A safe attach
// must never install this under Probe's retained drop thunk.
struct OtherProbe([usize; 2]);
impl Drop for OtherProbe {
    fn drop(&mut self) {
        CALLS.fetch_add(self.0[0], Ordering::SeqCst);
    }
}

#[test]
fn reopen_preserves_object_cell_properties_and_serial_identity() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _reset = Reset::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(np::alloc_closed(&FAMILY, &[(b"own", 123.0)]));
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let before = value.get_nanbox_f64().to_bits();
    let proto = unsafe {
        crate::object::shapes::object_prototype_word(
            (before & POINTER_MASK) as *const crate::object::ObjectHeader,
        )
    };
    let shape = unsafe {
        crate::object::shapes::object_shape_stamp(
            (before & POINTER_MASK) as *const crate::object::ObjectHeader,
        )
    };
    let bytes = policy::external_side_live_bytes();
    assert_eq!(
        np::lifecycle(value.get_nanbox_f64(), &FAMILY),
        Ok(Lifecycle::Closed)
    );
    assert!(matches!(
        np::enter(value.get_nanbox_f64(), &FAMILY),
        Err(PayloadMiss::Closed)
    ));
    let mut serial = np::next_open_serial();
    for _ in 0..1000 {
        assert_eq!(
            np::attach(
                value.get_nanbox_f64(),
                &FAMILY,
                Probe { link: Some(link) },
                4096
            ),
            Ok(())
        );
        assert_eq!(policy::external_side_live_bytes(), bytes + 4096);
        assert_eq!(np::owner_link(value.get_nanbox_f64(), &FAMILY), Ok(link));
        assert_eq!(value.get_nanbox_f64().to_bits(), before);
        assert_eq!(
            np::lifecycle(value.get_nanbox_f64(), &FAMILY),
            Ok(Lifecycle::Open)
        );
        assert_eq!(
            np::attach(value.get_nanbox_f64(), &FAMILY, Probe::default(), 0),
            Err(AttachMiss::Open)
        );
        let old = serial;
        serial = np::next_open_serial();
        assert_ne!(
            old, serial,
            "children of a prior open have a distinct stamp"
        );
        assert_eq!(
            np::close(value.get_nanbox_f64(), &FAMILY),
            CloseOutcome::Closed
        );
        assert_eq!(policy::external_side_live_bytes(), bytes);
        assert_eq!(
            np::close(value.get_nanbox_f64(), &FAMILY),
            CloseOutcome::AlreadyClosed
        );
    }
    let obj =
        (value.get_nanbox_f64().to_bits() & POINTER_MASK) as *const crate::object::ObjectHeader;
    unsafe {
        assert_eq!(crate::object::shapes::object_prototype_word(obj), proto);
        assert_eq!(crate::object::shapes::object_shape_stamp(obj), shape);
        assert_eq!(
            crate::object::js_object_get_field(obj, 0).bits(),
            123.0f64.to_bits()
        );
    }
    assert_eq!(
        std::mem::size_of::<OtherProbe>(),
        std::mem::size_of::<Probe>()
    );
    assert_eq!(
        std::mem::align_of::<OtherProbe>(),
        std::mem::align_of::<Probe>()
    );
    assert_eq!(
        np::attach(value.get_nanbox_f64(), &FAMILY, OtherProbe([1, 0]), 0),
        Err(AttachMiss::Foreign)
    );
    assert_eq!(
        CALLS.load(Ordering::SeqCst),
        1,
        "rejected payload uses its own destructor"
    );
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(trace.copying_nursery.eligible);
    assert_ne!(before, value.get_nanbox_f64().to_bits());
    assert_eq!(
        unsafe { np::link_event_owner(link) }.map(f64::to_bits),
        Some(value.get_nanbox_f64().to_bits())
    );
    assert_eq!(
        np::attach(value.get_nanbox_f64(), &FAMILY, Probe::default(), 0),
        Ok(())
    );
    let outer = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    assert_eq!(
        np::close(value.get_nanbox_f64(), &FAMILY),
        CloseOutcome::Deferred
    );
    assert_eq!(
        np::lifecycle(value.get_nanbox_f64(), &FAMILY),
        Ok(Lifecycle::Closing)
    );
    assert_eq!(
        np::attach(value.get_nanbox_f64(), &FAMILY, Probe::default(), 0),
        Err(AttachMiss::Closing)
    );
    assert_eq!(outer.finish(), Err(CallEnd::Closed));
    assert_eq!(unsafe { (*cell(link)).finalized }, 0);
    assert_eq!(FINAL_JS.load(Ordering::SeqCst), 0);
}

#[test]
fn terminal_item_keeps_closed_owner_through_moves_and_reads_late_listener() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _no_stack = ConservativeScanDisabledGuard::new();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    unsafe { np::link_ref(link) }; // queue owns exactly one ref
    assert_eq!(
        np::close(value.get_nanbox_f64(), &FAMILY),
        CloseOutcome::Closed
    );
    assert_eq!(unsafe { np::link_owner(link) }, None);
    np::set_callback(
        value.get_nanbox_f64(),
        &FAMILY,
        0,
        closure(crate::fn_info!(returns, 0)),
    );
    let before = value.get_nanbox_f64().to_bits();
    drop(scope);
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(trace.copying_nursery.eligible);
    let moved = unsafe { np::link_event_owner(link) }.unwrap();
    assert_ne!(before, moved.to_bits(), "closed owner must be rewritten");
    full();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(unsafe { np::link_event_owner(link) }.unwrap());
    let callbacks = np::callbacks(value.get_nanbox_f64(), &FAMILY);
    let cb = crate::array::js_array_get_f64(
        (callbacks.to_bits() & POINTER_MASK) as *const crate::array::ArrayHeader,
        0,
    );
    // A pump runs JS directly, outside C frames; call_from_native is OPEN-only.
    let out = unsafe {
        crate::closure::native_call_value_this(
            cb,
            crate::closure::JsThis::from_f64(value.get_nanbox_f64()),
            std::ptr::null(),
            0,
        )
    };
    assert_eq!(out, 19.0);
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    unsafe { np::link_unref(link) };
    drop(scope);
    let before = finalized();
    full();
    assert_eq!(finalized(), before + 1);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
}

#[test]
fn l8_worker_discards_queued_items_before_finalization_without_dispatch() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let before = finalized();
    let attempts = std::thread::spawn(|| {
        let value = owner();
        let link = np::owner_link(value, &FAMILY).unwrap();
        unsafe { np::link_ref(link) };
        let mut queue = vec![link]; // plain queued data; Drop never dereferences a link
        assert_eq!(np::close(value, &FAMILY), CloseOutcome::Closed);
        let mut dispatches = 0;
        if np::lifecycle_sabotage("teardown_drain") {
            unsafe { crate::native_handle::finalize_native_handle_at_teardown(cell(link)) };
            // Deliberately enter the pump after teardown. The witness must
            // catch even a dispatch attempt, before it can touch freed memory.
            for item in queue.drain(..) {
                dispatches += 1;
                let _ = unsafe { np::link_event_owner(item) };
            }
        } else {
            queue.clear();
        }
        // TLS heap teardown finalizes even though the queue's ref pin remains.
        dispatches
    })
    .join()
    .unwrap();
    assert_eq!(attempts, 0, "teardown must discard rather than dispatch");
    assert_eq!(
        finalized(),
        before + 1,
        "a pending ref cannot leak the worker cell"
    );
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

extern "C" fn closes_then_throws(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    assert_eq!(np::close(this.as_f64(), &FAMILY), CloseOutcome::Deferred);
    crate::exception::js_throw(err)
}
#[test]
fn l9_callback_throw_wins_over_deferred_close() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(owner());
    let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
    let cb = closure(crate::fn_info!(closes_then_throws, 1));
    let guard = np::enter(value.get_nanbox_f64(), &FAMILY).unwrap();
    assert_eq!(
        unsafe {
            np::call_from_native(value.get_nanbox_f64(), cb, value.get_nanbox_f64(), &[47.0])
        },
        Err(())
    );
    assert_eq!(guard.finish(), Err(CallEnd::Threw(47.0)));
    assert_eq!(
        np::lifecycle(value.get_nanbox_f64(), &FAMILY),
        Ok(Lifecycle::Closed)
    );
    assert_eq!(unsafe { (*cell(link)).busy }, 0);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(
        np::attach(value.get_nanbox_f64(), &FAMILY, Probe::default(), 0),
        Ok(())
    );
}

#[test]
fn two_hundred_thousand_released_cells_have_flat_rss_and_exact_counts() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _reset = Reset::new();
    let _no_stack = ConservativeScanDisabledGuard::new();
    let before = finalized();
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut rss: Vec<usize> = Vec::new();
    for batch in 0..20 {
        for _ in 0..10_000 {
            let scope = RuntimeHandleScope::new();
            let value = scope.root_nanbox_f64(np::alloc_closed(&FAMILY, &[]));
            np::attach(value.get_nanbox_f64(), &FAMILY, Probe::default(), 0).unwrap();
            let link = np::owner_link(value.get_nanbox_f64(), &FAMILY).unwrap();
            unsafe { np::link_ref(link) }; // pending terminal item
            np::close(value.get_nanbox_f64(), &FAMILY);
            unsafe { np::link_unref(link) }; // dispatched
        }
        // Exercise the production generational path as well as full sweep.
        // Full-only collections protect the recent nursery block window,
        // so untouched bump pages can inflate RSS without retaining a cell.
        let trace = collect_minor_trace(GcTriggerKind::MallocCount);
        assert!(trace.copying_nursery.eligible);
        full();
        assert_eq!(finalized() - before, (batch + 1) * 10_000);
        assert_eq!(DROPS.load(Ordering::SeqCst), (batch + 1) * 10_000);
        #[cfg(target_os = "linux")]
        {
            let stat = std::fs::read_to_string("/proc/self/statm").unwrap();
            let pages: usize = stat.split_whitespace().nth(1).unwrap().parse().unwrap();
            rss.push(pages * 4096);
            eprintln!("lifecycle churn batch {} rss={}", batch + 1, pages * 4096);
        }
    }
    if rss.len() == 20 {
        let warm = *rss[4..].iter().min().unwrap();
        let peak = *rss[4..].iter().max().unwrap();
        eprintln!("lifecycle churn: created=200000 finalized={} drops={} warm_rss={warm} peak_rss={peak} delta={}", finalized() - before, DROPS.load(Ordering::SeqCst), peak - warm);
        assert!(
            peak - warm < 4 * 1024 * 1024,
            "RSS must stay within 4 MiB after warmup"
        );
    }
}

#[test]
fn every_lifecycle_sabotage_makes_its_witness_red() {
    for (fault, witness) in [
        (
            "close_pin",
            "l4_release_then_unrooted_sweep_finalizes_without_another_drop",
        ),
        (
            "attach_finalized",
            "l5_teardown_finalized_cell_cannot_attach",
        ),
        (
            "teardown_drain",
            "l8_worker_discards_queued_items_before_finalization_without_dispatch",
        ),
        (
            "closed_priority",
            "l9_callback_throw_wins_over_deferred_close",
        ),
    ] {
        let name = format!("gc::tests::native_payload_callbacks::lifecycle::{witness}");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture", "--test-threads=1"])
            .env("PERRY_TEST_LIFECYCLE_SABOTAGE", fault)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
        assert!(
            !output.status.success(),
            "{fault} must make {witness} RED: {:?}",
            output
        );
        eprintln!("lifecycle sabotage {fault}: RED ({})", output.status);
    }
}
