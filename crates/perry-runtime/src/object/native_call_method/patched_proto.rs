//! #11394: method calls whose name the program installs on a builtin
//! prototype (`Array.prototype.push = f`, `Map.prototype.get = f`,
//! `Function.prototype.bind = f`).
//!
//! `js_native_call_method` answers an array, Map/Set or function receiver by
//! matching the method NAME against its native arms, and never reads the
//! prototype slot. That is only sound while the slot still holds the builtin.
//! The write itself lands (a READ of `arr.push` already returns the
//! replacement); the call was the one place that kept running the original.
//!
//! The compiler's whole-program pre-scan (`perry_hir::patched_builtins`) knows
//! which method names the program writes onto a builtin prototype, and routes
//! exactly those call sites here instead of to a direct native lowering or to
//! the name-matching dispatcher. So a program that patches nothing never
//! reaches this entry, and a call site whose name is not patched never pays
//! for the lookup.
//!
//! The lookup is the spec's own `Get(recv, name)`: the receiver's own property
//! first, then its real `[[Prototype]]` chain. A USER function found there is
//! called with the receiver as `this`. Anything else — the prototype's
//! installed builtin method, or nothing at all — falls through to
//! `js_native_call_method`, unchanged. Only a user function decides the call:
//! calling a builtin thunk here instead would re-enter by-name dispatch, and
//! the native arms are the builtin's real implementation anyway.

use super::*;

/// Receiver kinds whose builtin methods `js_native_call_method` answers by
/// name. Every other receiver already resolves its methods through property
/// lookup (class vtables, own fields, prototype objects), so it goes straight
/// to the ordinary dispatcher.
unsafe fn receiver_has_by_name_builtins(receiver: f64) -> bool {
    let jsval = JSValue::from_bits(receiver.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let addr = (receiver.to_bits() & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr) {
        return false;
    }
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(header) => matches!(
            header.obj_type,
            crate::gc::GC_TYPE_ARRAY
                | crate::gc::GC_TYPE_LAZY_ARRAY
                | crate::gc::GC_TYPE_MAP
                | crate::gc::GC_TYPE_SET
                | crate::gc::GC_TYPE_CLOSURE
        ),
        None => false,
    }
}

/// `recv.<name>(args…)` where the program writes `<name>` onto a builtin
/// prototype somewhere. See the module docs.
///
/// # Safety
/// Same contract as [`js_native_call_method`]: `method_name_ptr` /
/// `method_name_len` describe a UTF-8 method name, and `args_ptr` points at
/// `args_len` NaN-boxed values.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_native_call_method_patched_proto(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if method_name_ptr.is_null() || method_name_len == 0 || !receiver_has_by_name_builtins(object) {
        return js_native_call_method(object, method_name_ptr, method_name_len, args_ptr, args_len);
    }
    let name = std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len);
    // #11915: the shared generic path — the spec `Get` and call, which throws
    // for a non-callable; only the builtin itself continues by name.
    if let Some(value) =
        crate::object::builtin_proto::redirect_call(object, name, args_ptr, args_len)
    {
        return value;
    }
    js_native_call_method(object, method_name_ptr, method_name_len, args_ptr, args_len)
}

/// Spread form of [`js_native_call_method_patched_proto`]: `args_array_handle`
/// is a JS array holding every regular and spread argument, already
/// concatenated by codegen (the `js_native_call_method_apply` shape).
///
/// # Safety
/// As [`js_native_call_method_patched_proto`]; `args_array_handle` is null or
/// a live array.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_native_call_method_patched_proto_apply(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_array_handle: i64,
) -> f64 {
    let arr = args_array_handle as *const crate::array::ArrayHeader;
    let len = if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr) as usize
    };
    let buf: Vec<f64> = (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i as u32))
        .collect();
    js_native_call_method_patched_proto(
        object,
        method_name_ptr,
        method_name_len,
        buf.as_ptr(),
        buf.len(),
    )
}
