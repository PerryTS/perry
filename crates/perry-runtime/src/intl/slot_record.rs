//! The internal-slot record of an Intl service instance.
//!
//! ECMA-402 keeps a service's state in internal slots (`[[Locale]]`,
//! `[[Usage]]`, `[[BoundCompare]]`, ...), never in properties: node's
//! `new Intl.Collator()` has no own keys at all. An instance that keeps its
//! slots here holds ONE private runtime slot (`owned_state_bag::INTL_SLOTS`,
//! object-owned state on the instance's meta record, traced and moved with
//! it) naming an ordinary runtime-owned object whose keys are the slots. No
//! key of the instance names the record, so `getOwnPropertyNames`,
//! `hasOwnProperty`, `in`, for-in, `JSON.stringify` and spread see none of it.
//!
//! Every receiver check resolves the record first ([`slot_holder`]), so the
//! slot readers are the same for an instance with a record and for one that
//! still keeps its slots as hidden own fields.

use super::*;

/// The slot record `obj` carries, if any. Never allocates.
pub(super) fn slot_record(obj: *const ObjectHeader) -> Option<*mut ObjectHeader> {
    if obj.is_null()
        || unsafe { crate::object::header::object_type(obj as *mut ObjectHeader) }
            != crate::gc::GC_TYPE_OBJECT
    {
        return None;
    }
    let word = unsafe {
        crate::object::owned_state_bag::load(
            obj as usize,
            crate::object::owned_state_bag::INTL_SLOTS,
        )
    };
    let js = JSValue::from_bits(word.to_bits());
    js.is_pointer()
        .then(|| js.as_pointer::<ObjectHeader>() as *mut ObjectHeader)
}

/// Where `obj`'s internal slots live: its slot record, else `obj` itself.
pub(super) fn slot_holder(obj: *mut ObjectHeader) -> *mut ObjectHeader {
    slot_record(obj).unwrap_or(obj)
}

/// Give the instance behind `obj` a fresh, empty slot record and return it.
pub(super) fn attach_slot_record(obj: &crate::gc::RuntimeHandle<'_>) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let record = scope.root_raw_mut_ptr(js_object_alloc(0, 10));
    share_slot_record(obj, &record);
    record.with_mut_ptr(|record| record)
}

/// Make `record` the slot record of `obj` (a subclass `this` shares the record
/// of the instance its Intl parent constructor built).
pub(super) fn share_slot_record(
    obj: &crate::gc::RuntimeHandle<'_>,
    record: &crate::gc::RuntimeHandle<'_>,
) {
    let value = record.with_mut_ptr::<ObjectHeader, _>(|record| js_nanbox_pointer(record as i64));
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        crate::object::owned_state_bag::store(
            obj as usize,
            crate::object::owned_state_bag::INTL_SLOTS,
            value,
        )
    });
}

/// `intl_subclass_super`: a `class X extends Intl.Collator` instance answers
/// with the slots of the instance the parent constructor built.
pub(super) fn share_slot_record_of(dst: *mut ObjectHeader, instance: f64) {
    let Some(src) = object_ptr_from_value(instance) else {
        return;
    };
    let Some(record) = slot_record(src) else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let dst = scope.root_raw_mut_ptr(dst);
    let record = scope.root_raw_mut_ptr(record);
    share_slot_record(&dst, &record);
}
