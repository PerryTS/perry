//! #6497: `.name` on a heap class-expression value (`ClassExprFresh` — #6470
//! also routes capturing function-body class DECLARATIONS through it) must
//! expose the template's registry name, matching the INT32 class-ref path's
//! #2059 arm. Split from `get_field_by_name_tail.rs` for the file-size cap.

use super::*;

/// #4949: heap class-expression values (`ClassExprFresh`) are real
/// OBJECT_TYPE_CLASS objects, not INT32 class refs. Their `.prototype`
/// read must still expose the live declared-class prototype object so
/// tsc/tslib decorator code can inspect and mutate method descriptors.
pub(super) unsafe fn class_object_prototype_value(obj: *const ObjectHeader) -> JSValue {
    let class_id = (*obj).class_id;
    let value = super::super::class_registry::class_decl_prototype_value(class_id);
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        let value = super::super::class_prototype_ref_value(class_id);
        return JSValue::from_bits(value.to_bits());
    }
    JSValue::from_bits(value.to_bits())
}

/// Resolve `.name` for an `OBJECT_TYPE_CLASS` heap object. An explicit
/// `static name` member (an own field on the class object) wins; a deleted
/// key still reads `undefined` (returns `None`).
pub(super) unsafe fn class_object_name_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if let Some(v) = own_data_field_by_name(obj, key) {
        return Some(v);
    }
    let class_id = (*obj).class_id;
    if super::super::class_registry::class_is_key_deleted(class_id, "name") {
        return None;
    }
    let cname = super::super::class_registry::class_name_for_id(class_id)?;
    let s = crate::string::js_string_from_bytes(cname.as_ptr(), cname.len() as u32);
    Some(JSValue::from_bits(
        crate::js_nanbox_string(s as i64).to_bits(),
    ))
}
