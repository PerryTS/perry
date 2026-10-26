//! The public generator is the sole root of its resume record and captures,
//! including the record's prototype control (its owner function while the
//! original prototype is unmaterialized).
use super::*;

extern "C" fn owner_body(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
extern "C" fn resume_body(
    c: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    _: f64,
    _: f64,
) -> f64 {
    crate::closure::js_closure_get_capture_f64(c, CHILD)
}

/// The first user capture after the six record controls.
const CHILD: u32 = 6;
/// The record's prototype control (`generator_state::PROTO_SLOT`).
const PROTO: u32 = 5;

fn make_generator() -> f64 {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(owner_body, 0; with_flags(crate::closure::FN_GENERATOR)),
        0,
    ));
    let state = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(resume_body, 2),
        7,
    ));
    let child = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field(child, 0, crate::value::JSValue::number(42.0));
    state.with_mut_ptr(|state| {
        crate::closure::js_closure_set_capture_f64(
            state,
            CHILD,
            crate::value::js_nanbox_pointer(child as i64),
        )
    });
    state.with_mut_ptr::<crate::closure::ClosureHeader, _>(|state| {
        owner.with_const_ptr::<crate::closure::ClosureHeader, _>(|owner| {
            crate::object::js_generator_create(crate::value::js_nanbox_pointer(state as i64), owner)
        })
    })
}

#[test]
fn generator_fixed_state_record_and_capture_follow_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::array::scan_prototype_addr_cache_roots_mut);
    let value = make_generator();
    js_shadow_slot_set(0, value.to_bits());
    // The birth layout names no prototype: the record's control holds the
    // owner function, whose original prototype nothing has materialized.
    let object = crate::value::js_nanbox_get_pointer(value) as *const crate::object::ObjectHeader;
    let shape = crate::object::shapes::shape_record_for_word(unsafe {
        crate::object::shapes::object_shape_stamp(object)
    })
    .unwrap();
    assert_eq!(
        shape.proto_id(),
        crate::object::shapes::PROTO_ID_RECORD_HELD,
        "the birth layout's prototype is record-held"
    );
    let record = crate::object::fixed_generator_state(value).unwrap();
    let owner_before = crate::closure::js_closure_get_capture_f64(record, PROTO).to_bits();
    assert!(
        crate::value::JSValue::from_bits(owner_before).is_pointer(),
        "subject: the control holds the unmaterialized owner"
    );
    let state_before = crate::object::fixed_generator_state(value).unwrap() as usize;
    let child_before =
        crate::closure::js_closure_get_capture_f64(state_before as *const _, CHILD).to_bits();
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(
        trace.copying_nursery.eligible,
        "subject: a copying minor ran"
    );
    let moved = f64::from_bits(js_shadow_slot_get(0));
    let state = crate::object::fixed_generator_state(moved).expect("generator retains its record");
    assert_ne!(
        state as usize, state_before,
        "subject: the private record moved"
    );
    let child = crate::closure::js_closure_get_capture_f64(state, CHILD);
    assert_ne!(
        child.to_bits(),
        child_before,
        "subject: the captured child moved"
    );
    let owner = crate::closure::js_closure_get_capture_f64(state, PROTO).to_bits();
    assert_ne!(
        owner, owner_before,
        "subject: the record's owner edge moved"
    );
    // Observing the prototype materializes the owner's original; the record
    // still names the owner, and every read agrees on one identity.
    let proto = crate::object::js_object_get_prototype_of(moved);
    assert!(crate::value::JSValue::from_bits(proto.to_bits()).is_pointer());
    assert_eq!(
        crate::object::js_object_get_prototype_of(f64::from_bits(js_shadow_slot_get(0))).to_bits(),
        proto.to_bits(),
        "the materialized original is the one prototype of the instance"
    );
    assert_eq!(
        crate::object::js_object_get_field(
            crate::value::js_nanbox_get_pointer(child) as *const _,
            0
        )
        .as_number(),
        42.0
    );
    for _ in 0..6 {
        let _ = collect_minor_trace(GcTriggerKind::MallocCount);
    }
    let state =
        crate::object::fixed_generator_state(f64::from_bits(js_shadow_slot_get(0))).unwrap();
    assert_ne!(
        unsafe { (*crate::gc::header_from_trusted_user_ptr(state.cast())).gc_flags }
            & crate::gc::GC_FLAG_TENURED,
        0,
        "subject: fixed resume frame is tenured before the young-slot write"
    );
    let young = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field(young, 0, crate::value::JSValue::number(73.0));
    unsafe {
        crate::object::js_generator_set_slot(
            state as *mut _,
            CHILD,
            crate::value::js_nanbox_pointer(young as i64).to_bits(),
            0,
        );
    }
    let trace = collect_minor_trace(GcTriggerKind::MallocCount);
    assert!(trace.copying_nursery.eligible);
    let state =
        crate::object::fixed_generator_state(f64::from_bits(js_shadow_slot_get(0))).unwrap();
    let child = crate::closure::js_closure_get_capture_f64(state, CHILD);
    assert_ne!(
        child.to_bits(),
        crate::value::js_nanbox_pointer(young as i64).to_bits(),
        "subject: shared helper retained and rewrote the newly copied child"
    );
    assert_eq!(
        crate::object::js_object_get_field(
            crate::value::js_nanbox_get_pointer(child) as *const _,
            0
        )
        .as_number(),
        73.0
    );
}
