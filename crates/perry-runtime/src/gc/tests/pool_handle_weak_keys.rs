//! A WeakMap entry keyed by a shared-pool handle id has no heap object behind
//! its key, so the pool provider decides it: the full trace in which the
//! provider proves the id dead deletes the entry, while minors, a full trace
//! the provider did not arm, and a live verdict keep it. Both boxings of a band
//! id (`HANDLE_TAG` from perry-ffi, `POINTER_TAG` from the common registry)
//! get the same verdict.

use super::super::*;
use super::support::*;
use std::cell::Cell;
use std::ffi::c_void;

const DEAD_ID: u64 = 0x2345;
const LIVE_ID: u64 = 0x2346;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static ASKED: Cell<usize> = const { Cell::new(0) };
}

extern "C" fn armed_phase(phase: u32) -> bool {
    phase != 0 || ARMED.with(Cell::get)
}

extern "C" fn inert_phase(_: u32) -> bool {
    false
}

extern "C" fn observe_nothing(_: u64, _: extern "C" fn(u64, *mut c_void), _: *mut c_void) -> bool {
    true
}

extern "C" fn dead_id_is_unreached(bits: u64) -> bool {
    ASKED.with(|asked| asked.set(asked.get() + 1));
    bits & POINTER_MASK != DEAD_ID
}

extern "C" fn everything_live(_: u64) -> bool {
    true
}

fn has(key: u64) -> bool {
    let map = f64::from_bits(js_shadow_slot_get(0));
    crate::weakref::js_weakmap_has(map, f64::from_bits(key)).to_bits() == crate::value::TAG_TRUE
}

fn pool_keys_follow_the_provider(tag: u64) {
    std::thread::spawn(move || {
        let _nursery = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _scan = ConservativeScanDisabledGuard::new();
        let dead = (tag << 48) | DEAD_ID;
        let live = (tag << 48) | LIVE_ID;
        let map = crate::weakref::js_weakmap_new();
        js_shadow_slot_set(0, ptr_bits(map as usize));
        for key in [dead, live] {
            let value = crate::object::js_object_alloc(0, 0);
            crate::weakref::js_weakmap_set(
                f64::from_bits(js_shadow_slot_get(0)),
                f64::from_bits(key),
                f64::from_bits(ptr_bits(value as usize)),
            );
        }
        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(
            armed_phase,
            observe_nothing,
        );
        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_liveness(dead_id_is_unreached);

        ARMED.with(|armed| armed.set(false));
        js_gc_collect();
        assert!(
            has(dead),
            "a full trace the pool did not arm releases no id"
        );

        ARMED.with(|armed| armed.set(true));
        let _ = gc_collect_minor();
        assert!(has(dead), "a minor never decides a pool id");

        ASKED.with(|asked| asked.set(0));
        js_gc_collect();
        assert!(
            ASKED.with(Cell::get) > 0,
            "the armed trace must ask the pool"
        );
        assert!(
            !has(dead),
            "the trace that proves the id dead deletes its entry"
        );
        assert!(has(live), "a live id keeps its entry");
        let value = crate::weakref::js_weakmap_get(
            f64::from_bits(js_shadow_slot_get(0)),
            f64::from_bits(live),
        )
        .to_bits();
        assert_ne!(
            value,
            crate::value::TAG_UNDEFINED,
            "a live key keeps its value"
        );

        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_trace(
            inert_phase,
            observe_nothing,
        );
        crate::gc::full_trace::perry_ffi_gc_register_pool_handle_liveness(everything_live);
    })
    .join()
    .expect("pool weak-key fixture must not panic");
}

#[test]
fn a_handle_tag_weak_key_dies_in_the_trace_that_releases_its_id() {
    pool_keys_follow_the_provider(0xFFF9);
}

#[test]
fn a_pointer_tag_band_weak_key_dies_only_when_the_pool_says_so() {
    pool_keys_follow_the_provider(0x7FFD);
}
