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

/// #6530 (size-gate split from `get_field_by_name_tail.rs` — pure
/// relocation): resolve the `constructor` special key for an instance
/// receiver. Own `constructor` data field wins; then WeakMap/WeakSet,
/// the per-evaluation class-object registry (capture-carrying classes),
/// vtable `constructor` methods, boxed primitives, anon shapes, the
/// function-class table, and the INT32 ClassRef synthesis. `None` means
/// "not resolved here" — the caller falls through to the generic walk.
pub(super) unsafe fn instance_constructor_value(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if let Some(v) = own_data_field_by_name(obj, key) {
        return Some(v);
    }
    let class_id = (*obj).class_id;
    // #6530: a capture-carrying class has no ClassRef value — the
    // class VALUE is the per-evaluation class OBJECT registered at
    // `js_object_mark_class` time. Return that same object so
    // identity holds (`x.constructor === Sub`, bundled zod's
    // `describe()` re-construction via `this.constructor`
    // yielding the SUBCLASS instead of collapsing to the base).
    // The arms below would otherwise hand back a bound
    // constructor-method closure (whose underlying func is the
    // nearest ancestor ctor — reporting the BASE class's name for
    // every subclass) or the bare INT32 ClassRef.
    if let Some(v) = super::super::class_registry::class_object_value_for_cid(class_id) {
        return Some(JSValue::from_bits(v.to_bits()));
    }
    // #5834: WeakMap/WeakSet instances carry a reserved class_id
    // (not a registered declared-class one), so none of the
    // arms below resolve them and `(new WeakMap()).constructor`
    // fell through to `undefined`.
    if class_id == crate::weakref::CLASS_ID_WEAKMAP || class_id == crate::weakref::CLASS_ID_WEAKSET
    {
        let name: &[u8] = if class_id == crate::weakref::CLASS_ID_WEAKMAP {
            b"WeakMap"
        } else {
            b"WeakSet"
        };
        let v = js_get_global_this_builtin_value(name.as_ptr(), name.len());
        return Some(JSValue::from_bits(v.to_bits()));
    }
    if class_id != 0 && class_has_own_method(class_id, "constructor") {
        let value = class_prototype_method_value_for_name(class_id, "constructor");
        return Some(JSValue::from_bits(value.to_bits()));
    }
    if matches!(
        class_id,
        CLASS_ID_BOXED_NUMBER
            | CLASS_ID_BOXED_STRING
            | CLASS_ID_BOXED_BOOLEAN
            | CLASS_ID_BOXED_BIGINT
            | CLASS_ID_BOXED_SYMBOL
    ) {
        let name = match class_id {
            CLASS_ID_BOXED_NUMBER => b"Number".as_slice(),
            CLASS_ID_BOXED_STRING => b"String".as_slice(),
            CLASS_ID_BOXED_BOOLEAN => b"Boolean".as_slice(),
            CLASS_ID_BOXED_BIGINT => b"BigInt".as_slice(),
            CLASS_ID_BOXED_SYMBOL => b"Symbol".as_slice(),
            _ => unreachable!(),
        };
        let v = js_get_global_this_builtin_value(name.as_ptr(), name.len());
        return Some(JSValue::from_bits(v.to_bits()));
    }
    // Object-literal instances (`{ x: 1 }`) carry a synthetic
    // `__AnonShape_*` class id. Spec says their `.constructor`
    // is the global `Object`, not the synthetic class — so
    // resolve through the globalThis singleton so the value
    // matches the bare `Object` identifier (`x.constructor
    // === Object`, date-fns `constructFrom`, drizzle's
    // `isPlainObject` duck check).
    if class_id != 0 && is_anon_shape_class_id(class_id) {
        let v = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
        return Some(JSValue::from_bits(v.to_bits()));
    }
    if let Some(func_value) = super::super::class_registry::function_value_for_class_id(class_id) {
        return Some(JSValue::from_bits(func_value.to_bits()));
    }
    if class_id != 0 && is_class_id_registered(class_id) {
        let bits = 0x7FFE_0000_0000_0000u64 | (class_id as u64);
        return Some(JSValue::from_bits(bits));
    }
    // class_id == 0 fallback: plain ObjectHeader allocated
    // without an HIR shape (Object.create(null) hybrids, raw
    // empty `{}` produced by JSON.parse, etc.). Report
    // `Object` so duck-type tests don't trip undefined.
    if class_id == 0 {
        // #6537 review: an EXPLICIT null-prototype object
        // (`Object.create(null)` / `js_object_alloc_null_proto`, marked with
        // `OBJ_FLAG_NULL_PROTO` on the GC header) has NO `constructor` —
        // fall through (→ undefined) instead of reporting `Object`. The
        // `Object` report stays for ordinary shapeless objects (raw `{}`
        // from JSON.parse etc.), which spec-correctly inherit
        // `Object.prototype.constructor`.
        if let Some(gc_header) = crate::value::addr_class::try_read_gc_header(obj as usize) {
            if gc_header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                return None;
            }
        }
        let v = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
        return Some(JSValue::from_bits(v.to_bits()));
    }
    None
}
