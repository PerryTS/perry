//! Published buffers and span facts belong to the context's collecting bag.
use super::super::*;
use super::support::*;
#[test]
fn wasm_memory_binding_follows_moving_wrapper_and_is_collectible() {
    check(false);
}
#[test]
#[ignore = "negative control: erase the context's published-buffer edge"]
fn negative_control_objtables_wasm_memory_bindings() {
    check(true);
}
fn check(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::weakref::test_support::clear_weak_holders();
    let owner = crate::object::js_object_alloc(0, 0);
    let target = unsafe { alloc_old_test_object(0).0 };
    let target_value = f64::from_bits(ptr_bits(target as usize));
    let context = crate::wasm_instance_state::new(f64::from_bits(crate::value::TAG_UNDEFINED));
    crate::wasm_instance_state::attach(f64::from_bits(ptr_bits(owner as usize)), context);
    crate::wasm_instance_state::set_buffer(context, target_value);
    crate::wasm_instance_state::set_memory_span(
        context,
        crate::wasm_instance_state::MemorySpan {
            instance: 0,
            data: 37,
            len: 5,
            rebound: true,
        },
    );
    let weak = crate::weakref::js_weakref_new(target_value);
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    js_shadow_slot_set(1, ptr_bits(weak as usize));
    let _ = gc_collect_minor();
    let moved = f64::from_bits(js_shadow_slot_get(0));
    assert_ne!(js_shadow_slot_get(0) & POINTER_MASK, owner as u64);
    let context = crate::wasm_instance_state::from_owner(moved);
    let span = crate::wasm_instance_state::memory_span(context).unwrap();
    assert_eq!((span.data, span.len, span.rebound), (37, 5, true));
    if erase {
        crate::wasm_instance_state::set_buffer(
            context,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
    }
    js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        target_value.to_bits(),
        "only the context bag retains this weak target"
    );
    let fresh = crate::wasm_instance_state::new(f64::from_bits(crate::value::TAG_UNDEFINED));
    assert_eq!(
        crate::wasm_instance_state::buffer(fresh).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    js_shadow_slot_set(0, 0);
    js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}
