//! `Array.prototype.toString` resolution — the reflective override check for
//! `String(arr)` / `` `${arr}` `` / `"" + arr`, and the method used for an
//! explicit `array.toString()` call site.
//!
//! Split out of `to_string.rs`, which sits at the 2000-line cap (#8480 series).

use super::to_string_primitive::is_primitive_value;
use super::*;

/// Resolve `obj[method_name]` (own + prototype chain) and, if it is a
/// callable closure, invoke it with `this = obj` (no args). Returns whether
/// the result was a primitive, a non-primitive, or whether the method was
/// absent / non-callable.
/// `Array.prototype.toString` override check for `String(arr)` / `` `${arr}` ``
/// / `"" + arr` (test262 `S15.5.1.1_A1_T8`). Arrays are `GC_TYPE_ARRAY`, not
/// `ObjectHeader`s, so unlike ordinary objects their `toString` can't be
/// looked up as an "own/inherited field" on the array value itself — the
/// hardcoded `Array.prototype.join(",")` fallback normally used for arrays
/// must instead be skipped when `Array.prototype.toString` has been
/// reassigned away from its installed default (a noop thunk kept only for
/// `typeof`/`.name` introspection, see `populate_builtin_prototype_methods`).
/// Outcome of consulting `Array.prototype.toString` for the string-hint
/// `ToPrimitive`. `UseDefaultJoin` covers both "still the installed noop
/// default" and any shape we don't have a callable method for (e.g. the
/// property was overwritten with a non-callable value) — those fall back to
/// the ordinary `join(",")` behavior. A callable override that's actually
/// invoked must otherwise follow `OrdinaryToPrimitive`: a primitive result
/// wins, a non-primitive result exhausts `ToPrimitive` (no separate
/// `valueOf` override path exists for arrays here) and throws, rather than
/// silently falling back to `join`.
pub(crate) enum ArrayToStringOutcome {
    UseDefaultJoin,
    Primitive(f64),
    TypeError,
}

pub(crate) unsafe fn array_prototype_to_string_override(value: f64) -> ArrayToStringOutcome {
    // `value`, the prototype, its key, and the resolved method are all live
    // across allocating operations below. Keep each one visible to the moving
    // collector and re-read its address after every allocation.
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let key_handle = scope.root_string_ptr(crate::string::canonical_key(b"toString"));
    let proto = crate::object::builtin_prototype_value("Array");
    let proto_handle = scope.root_nanbox_f64(proto);
    let proto_bits = proto_handle.get_nanbox_f64().to_bits();
    if (proto_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    if (proto_bits & POINTER_MASK) == 0 {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    // #7341: `js_object_get_field_by_name` ALLOCATES, and on this key it does so
    // on a path that is reachable, not hypothetical — an ObjectHeader receiver
    // falls through to `get_field_by_name_object_tail`, whose accessor arms call
    // `invoke_accessor_getter`, so `Object.defineProperty(Array.prototype,
    // "toString", { get() {…} })` runs arbitrary user JS inside this call. (Its
    // `.size` arm opens a `RuntimeHandleScope` for the same reason, but that arm
    // is gated on the key being `"size"` and cannot fire here.) Neither raw
    // argument may therefore be bound before the call: the prototype address
    // gets a root of its own alongside the key, both are produced inside scoped
    // borrows, and neither is nameable once the lookup returns.
    let proto_ptr_handle =
        scope.root_raw_mut_ptr((proto_bits & POINTER_MASK) as *mut crate::object::ObjectHeader);
    let method = proto_ptr_handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|proto_ptr| {
        key_handle.with_const_ptr::<crate::string::StringHeader, _>(|key| {
            crate::object::js_object_get_field_by_name(proto_ptr, key)
        })
    });
    let method_handle = scope.root_nanbox_u64(method.bits());
    let method_bits = method_handle.get_nanbox_u64();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    let closure = method_ptr as *const crate::closure::ClosureHeader;
    if (*closure).func_ptr == crate::object::global_this_builtin_noop_thunk as *const u8 {
        return ArrayToStringOutcome::UseDefaultJoin;
    }
    let receiver = value_handle.get_nanbox_f64();
    let bound = crate::closure::clone_closure_rebind_this(method_bits, receiver);
    let bound_handle = scope.root_nanbox_u64(bound);
    let prev_this = crate::object::js_implicit_this_set(receiver);
    let prev_this_handle = scope.root_nanbox_f64(prev_this);
    let ret =
        crate::closure::js_native_call_value(bound_handle.get_nanbox_f64(), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this_handle.get_nanbox_f64());
    if is_primitive_value(ret) {
        ArrayToStringOutcome::Primitive(ret)
    } else {
        ArrayToStringOutcome::TypeError
    }
}

/// Resolve and invoke the current `Array.prototype.toString` method for a
/// source-level `array.toString()` call. Static array lowering must not replace
/// this with `join(",")`: the prototype property is writable and its live value
/// (for example `Object.prototype.toString`) must win.
pub(crate) fn call_array_prototype_to_string_method(
    value: f64,
    arg_handles: &[crate::gc::RuntimeHandle<'_>],
) -> f64 {
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver_handle = scope.root_nanbox_f64(value);
        let key_handle = scope.root_string_ptr(crate::string::canonical_key(b"toString"));
        let prototype_handle =
            scope.root_nanbox_f64(crate::object::builtin_prototype_value("Array"));
        let prototype_bits = prototype_handle.get_nanbox_f64().to_bits();
        if (prototype_bits & TAG_MASK) != POINTER_TAG {
            crate::error::js_throw_type_error_not_a_function(
                std::ptr::null(),
                0,
                b"toString".as_ptr(),
                8,
            );
        }

        // #7341: same contract as `array_prototype_to_string_override` above —
        // the lookup can allocate, so the prototype address is rooted rather
        // than held as a bare local and both raw arguments are produced inside
        // scoped borrows off their roots.
        let prototype_ptr_handle = scope.root_raw_const_ptr(
            (prototype_bits & POINTER_MASK) as *const crate::object::ObjectHeader,
        );
        let method =
            prototype_ptr_handle.with_const_ptr::<crate::object::ObjectHeader, _>(|prototype| {
                key_handle.with_const_ptr::<crate::string::StringHeader, _>(|key| {
                    crate::object::js_object_get_field_by_name(prototype, key)
                })
            });
        let method_handle = scope.root_nanbox_u64(method.bits());
        if !crate::object::value_is_callable(method_handle.get_nanbox_f64()) {
            crate::error::js_throw_type_error_not_a_function(
                std::ptr::null(),
                0,
                b"toString".as_ptr(),
                8,
            );
        }

        let receiver = receiver_handle.get_nanbox_f64();
        let rebound =
            crate::closure::rebind_explicit_this(method_handle.get_nanbox_f64(), receiver);
        let rebound_handle = scope.root_nanbox_f64(rebound);
        let previous = crate::object::js_implicit_this_set(receiver);
        let previous_handle = scope.root_nanbox_f64(previous);
        let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(arg_handles);
        let result = crate::closure::js_native_call_value(
            rebound_handle.get_nanbox_f64(),
            args.as_ptr(),
            args.len(),
        );
        crate::object::js_implicit_this_set(previous_handle.get_nanbox_f64());
        result
    }
}

/// Execute the generic `Array.prototype.toString` algorithm for a call-site
/// receiver. Kept here so both the reflective prototype thunk and the native
/// array method dispatcher use the same live `join` lookup and intrinsic
/// Object-toString fallback.
pub(crate) fn array_prototype_to_string(value: f64) -> f64 {
    let value_kind = JSValue::from_bits(value.to_bits());
    if value_kind.is_undefined() || value_kind.is_null() {
        crate::object::has_own_helpers::throw_to_object_nullish_type_error();
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_handle = scope.root_nanbox_f64(value);
    let join = unsafe {
        crate::value::js_get_property(
            receiver_handle.get_nanbox_f64(),
            b"join".as_ptr() as i64,
            b"join".len() as i64,
        )
    };
    let join_handle = scope.root_nanbox_f64(join);
    if !crate::object::value_is_callable(join_handle.get_nanbox_f64()) {
        return unsafe { crate::object::js_object_to_string(receiver_handle.get_nanbox_f64()) };
    }

    let receiver = receiver_handle.get_nanbox_f64();
    let rebound = crate::closure::rebind_explicit_this(join_handle.get_nanbox_f64(), receiver);
    let rebound_handle = scope.root_nanbox_f64(rebound);
    let previous = crate::object::js_implicit_this_set(receiver);
    let previous_handle = scope.root_nanbox_f64(previous);
    let result = unsafe {
        crate::closure::js_native_call_value(rebound_handle.get_nanbox_f64(), std::ptr::null(), 0)
    };
    crate::object::js_implicit_this_set(previous_handle.get_nanbox_f64());
    result
}
