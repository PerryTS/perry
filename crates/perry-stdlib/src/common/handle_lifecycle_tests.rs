use super::super::handle::{
    common_handle_registry_domain, drop_handle, handle_exists, register_handle,
    register_reclaimable_handle, with_handle,
};
use super::*;
use perry_runtime::gc::RuntimeHandleScope;

fn is_parked(id: Handle) -> bool {
    PARKED.with(|map| map.borrow().0.contains_key(&id))
}

const UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

fn boxed(id: Handle) -> u64 {
    0x7FFD_0000_0000_0000 | id as u64
}

/// Two full collections: the first ages every parked id past its young
/// epoch, the second decides by reachability alone.
fn collect() {
    perry_runtime::gc::js_gc_collect();
    perry_runtime::gc::js_gc_collect();
}

/// Each test runs on its own mutator: the GC, its roots and the parked set
/// are per-thread, so unrelated fixtures' unrooted locals stay out of it.
fn on_mutator(f: impl FnOnce() + Send + 'static) {
    let _serial = super::super::handle::REGISTRATION_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    std::thread::spawn(move || {
        perry_runtime::gc::gc_init();
        f();
    })
    .join()
    .unwrap();
}

#[test]
fn an_unreachable_reclaimable_payload_is_dropped_and_its_id_recycled() {
    on_mutator(|| {
        let traces = FULL_TRACES.with(|n| n.get());
        let id = register_reclaimable_handle(11_u64);
        assert!(is_parked(id));
        collect();
        assert!(
            FULL_TRACES.with(|n| n.get()) >= traces + 2,
            "no full trace ran"
        );
        assert!(
            !handle_exists(id),
            "nothing names the id, so its payload must go"
        );
        // Orphans adopted from earlier test threads may still be parked here.
        assert!(!is_parked(id));
        assert!(
            REGISTRATIONS.identity(id).is_none(),
            "the old registration must be gone"
        );
    });
}

#[test]
fn a_rooted_reclaimable_payload_survives_until_its_last_holder_dies() {
    on_mutator(|| {
        let scope = RuntimeHandleScope::new();
        let id = register_reclaimable_handle(String::from("held"));
        let root = scope.root_nanbox_u64(boxed(id));
        collect();
        collect();
        assert_eq!(
            with_handle::<String, _, _>(id, |s| s.clone()).as_deref(),
            Some("held")
        );
        root.set_nanbox_u64(UNDEFINED);
        collect();
        assert!(!handle_exists(id));
    });
}

#[test]
fn a_reclaimable_id_held_in_a_heap_array_is_kept() {
    on_mutator(|| {
        let scope = RuntimeHandleScope::new();
        let id = register_reclaimable_handle(5_u32);
        let array = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(1));
        let ptr =
            perry_runtime::js_array_push_f64(array.get_raw_mut_ptr(), f64::from_bits(boxed(id)));
        array.set_raw_mut_ptr(ptr);
        collect();
        assert!(handle_exists(id), "an array element names the id");
        array.set_raw_mut_ptr(std::ptr::null_mut::<perry_runtime::ArrayHeader>());
        collect();
        assert!(!handle_exists(id));
    });
}

/// #11453's safety requirement: a dropped handle's id must never resolve to
/// a new object while any JS value still holds the old number — however many
/// registrations and collections pass in between.
#[test]
fn a_dropped_id_still_held_by_js_is_never_reissued() {
    on_mutator(|| {
        let scope = RuntimeHandleScope::new();
        let stale = register_handle(1_u64);
        let holder = scope.root_nanbox_u64(boxed(stale));
        assert!(drop_handle(stale));
        for n in 0..20_000_u64 {
            let fresh = register_reclaimable_handle(n);
            assert_ne!(
                fresh, stale,
                "a held stale id was reissued after {n} registrations"
            );
            if n % 2_000 == 0 {
                collect();
            }
        }
        assert!(!handle_exists(stale));
        // Once nothing names it, the id returns to the pool.
        holder.set_nanbox_u64(UNDEFINED);
        collect();
        let identity = REGISTRATIONS
            .begin_registration_with_id_in_domain(
                common_handle_registry_domain(),
                stale,
                perry_ffi::NativeRegistrationKind::Payload,
            )
            .expect("an unreferenced retired id must become reusable");
        assert!(REGISTRATIONS.publish(identity));
        assert!(REGISTRATIONS.begin_retirement_of(identity));
        assert!(REGISTRATIONS.finish_retirement_reusable(identity));
    });
}

#[test]
fn a_stream_used_payload_is_retained_strongly() {
    on_mutator(|| {
        let id = register_reclaimable_handle(3_u8);
        assert!(retain_strongly(id));
        collect();
        assert!(
            handle_exists(id),
            "a strongly retained payload is not GC-owned"
        );
        assert!(drop_handle(id));
    });
}

#[test]
fn an_aborted_trace_releases_nothing() {
    on_mutator(|| {
        let id = register_reclaimable_handle(9_u16);
        assert!(phase(0), "a parked id arms the trace");
        assert!(phase(2));
        assert!(handle_exists(id));
        collect();
        assert!(!handle_exists(id));
    });
}

/// Steady-state churn across both registries sharing the band: more than a
/// million allocate/drop cycles of common (GC-reclaimed) and FFI (tick
/// quarantined) handles, with at most a handful live at once. Without
/// reclamation the 262k-id band runs out within the first quarter.
#[test]
fn a_million_mixed_common_and_ffi_cycles_never_exhaust_the_band() {
    on_mutator(|| {
        for n in 0..1_050_000_u64 {
            register_reclaimable_handle(n);
            if n % 4 == 0 {
                let ffi = perry_ffi::register_handle(n);
                assert!(perry_ffi::drop_handle(ffi));
                if n % 1024 == 0 {
                    perry_ffi::drain_quarantined_handles();
                }
            }
            if n % 3 == 0 {
                let strong = register_handle(n);
                assert!(drop_handle(strong));
            }
            perry_runtime::gc::js_gc_loop_safepoint();
        }
        collect();
        assert_eq!(parked_handle_count(), 0, "every parked id must be decided");
        assert!(
            REGISTRATIONS.available_ids() > BAND_RESERVE,
            "the band must not have drained"
        );
    });
}

#[test]
fn native_property_bag_follows_a_reachable_reclaimable_payload() {
    on_mutator(|| {
        unsafe {
            crate::common::dispatch::js_stdlib_init_dispatch();
        }
        let id = register_reclaimable_handle(73_u64);
        let roots = RuntimeHandleScope::new();
        let owner = roots.root_nanbox_u64(boxed(id));
        let value = perry_runtime::object::js_object_alloc(0, 1);
        {
            perry_runtime::object::js_object_set_field_f64(value, 0, 73.0);
        }
        let boxed_value = perry_runtime::value::js_nanbox_pointer(value as i64);
        perry_runtime::object::handle_expando::handle_expando_set(id, "payload", boxed_value);
        collect();
        let stored =
            perry_runtime::object::handle_expando::handle_expando_get(id, "payload").unwrap();
        {
            assert_eq!(
                perry_runtime::object::js_object_get_field_f64(
                    perry_runtime::value::js_nanbox_get_pointer(stored)
                        as *mut perry_runtime::ObjectHeader,
                    0
                ),
                73.0
            );
        }
        owner.set_nanbox_u64(UNDEFINED);
        collect();
        assert!(
            !handle_exists(id),
            "property bag must not become an independent owner"
        );
    });
}

#[test]
fn native_property_bag_to_handle_cycle_dies_with_its_last_js_owner() {
    on_mutator(|| {
        unsafe {
            crate::common::dispatch::js_stdlib_init_dispatch();
        }
        let id = register_reclaimable_handle(41_u64);
        perry_runtime::object::handle_expando::handle_expando_set(
            id,
            "self",
            f64::from_bits(boxed(id)),
        );
        collect();
        assert!(
            !handle_exists(id),
            "an unreachable record/bag cycle is not a C-frame owner"
        );
    });
}

#[test]
fn ffi_payload_record_owns_and_releases_its_property_bag() {
    on_mutator(|| {
        unsafe {
            crate::common::dispatch::js_stdlib_init_dispatch();
        }
        let id = perry_ffi::register_handle(29_u64);
        perry_runtime::object::handle_expando::handle_expando_set(id, "payload", 73.0);
        collect();
        assert_eq!(
            perry_runtime::object::handle_expando::handle_expando_get(id, "payload"),
            Some(73.0)
        );
        perry_ffi::drop_handle(id);
        assert!(perry_ffi::native_handle_props(id).is_none());
        assert!(perry_runtime::object::handle_expando::handle_expando_get(id, "payload").is_none());
    });
}

/// A property set on a native handle from another thread lands in that
/// thread's own bag on the record, the way the former per-thread tables kept
/// it: it neither throws nor touches the publishing thread's properties.
#[test]
fn another_thread_sets_its_own_property_bag_on_a_shared_handle() {
    on_mutator(|| {
        unsafe {
            crate::common::dispatch::js_stdlib_init_dispatch();
        }
        let id = register_handle(17_u64);
        perry_runtime::object::handle_expando::handle_expando_set(id, "a", 1.0);
        std::thread::spawn(move || {
            perry_runtime::gc::gc_init();
            unsafe {
                crate::common::dispatch::js_stdlib_init_dispatch();
            }
            assert_eq!(
                perry_runtime::object::handle_expando::handle_expando_get(id, "a"),
                None,
                "the publishing thread's bag is not this thread's"
            );
            perry_runtime::object::handle_expando::handle_expando_set(id, "b", 2.0);
            perry_runtime::gc::js_gc_collect();
            assert_eq!(
                perry_runtime::object::handle_expando::handle_expando_get(id, "b"),
                Some(2.0)
            );
        })
        .join()
        .unwrap();
        collect();
        assert_eq!(
            perry_runtime::object::handle_expando::handle_expando_get(id, "a"),
            Some(1.0)
        );
        assert_eq!(
            perry_runtime::object::handle_expando::handle_expando_get(id, "b"),
            None
        );
        drop_handle(id);
    });
}
