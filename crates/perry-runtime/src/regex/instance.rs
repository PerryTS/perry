//! Ordinary RegExp births. This factory memo retains only a ShapeId; the
//! shape is the authority for keys, attributes and prototype and carries all
//! collector edges. No instance or data cell is retained by the memo.
use super::{RegExpData, RegExpHeader, REGEXP_MATCHER};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::key_attrs::AttrsEdit;
use crate::value::js_nanbox_pointer;
use std::cell::Cell;

crate::perry_thread_local! {
    static BIRTH_SHAPE: Cell<u32> = const { Cell::new(0) };
}

pub(super) fn new(scope: &RuntimeHandleScope, data: &RuntimeHandle<'_>) -> *mut RegExpHeader {
    let shape = BIRTH_SHAPE.with(Cell::get);
    // ShapeIds are agent-local and never reused. Its prototype edge is
    // maintained by the shape collector; no receiver address is memoized.
    let prototype = crate::object::regex_proto_thunks::recorded_regexp_prototype();
    let cached = !prototype.is_null()
        && crate::object::shapes::shape_descriptor_by_id(shape).is_some()
        && crate::object::shapes::shape_prototype_word(shape)
            == js_nanbox_pointer(prototype as i64).to_bits();
    let receiver = scope.root_raw_mut_ptr(if cached {
        crate::object::object_alloc_plain_born(2, shape)
    } else {
        crate::object::object_alloc_plain(2)
    });
    if !cached {
        let intrinsic = crate::object::regex_proto_thunks::recorded_regexp_prototype();
        let prototype = scope.root_nanbox_f64(if intrinsic.is_null() {
            crate::object::builtin_prototype_value("RegExp")
        } else {
            js_nanbox_pointer(intrinsic as i64)
        });
        crate::object::intrinsic_private_add(
            receiver.with_const_ptr::<RegExpHeader, _>(|r| js_nanbox_pointer(r as i64)),
            REGEXP_MATCHER,
            data.with_const_ptr::<RegExpData, _>(|d| js_nanbox_pointer(d as i64)),
        );
        receiver.with_mut_ptr::<RegExpHeader, _>(|r| unsafe {
            // RegExpInitialize defines its own data slot; it must never
            // invoke a setter inherited from Object.prototype.
            crate::object::key_attrs::apply_edits(r, &[AttrsEdit::Data(b"lastIndex", 1)]);
            crate::object::store_object_field_slot(r, 1, 0.0f64.to_bits());
        });
        receiver.with_mut_ptr::<RegExpHeader, _>(|r| {
            crate::object::prototype_chain::object_link_created_prototype(
                r as usize,
                prototype.get_nanbox_f64().to_bits(),
            );
        });
        BIRTH_SHAPE.with(|memo| {
            memo.set(receiver.with_const_ptr::<RegExpHeader, _>(|r| unsafe {
                crate::object::shapes::object_shape_stamp(r)
            }))
        });
    } else {
        // Fresh ordinary birth: the existing newborn funnel checks the shape
        // representation and publishes barriers, without retiring an Array
        // subclass proof that this receiver has never carried.
        receiver.with_mut_ptr::<RegExpHeader, _>(|r| unsafe {
            crate::object::store_object_field_slot_layout_deferred(
                r,
                0,
                data.with_const_ptr::<RegExpData, _>(|d| js_nanbox_pointer(d as i64).to_bits()),
            );
            crate::object::store_object_field_slot_layout_deferred(r, 1, 0.0f64.to_bits());
        });
    }
    receiver.with_mut_ptr::<RegExpHeader, _>(|r| r)
}
