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

/// A function the PROGRAM created, as opposed to a builtin method closure the
/// runtime installed on a prototype (`install_proto_method` marks every one of
/// those non-constructable; no compiled user function is in that set).
fn is_user_function(value: JSValue) -> bool {
    if (value.bits() & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return false;
    }
    let ptr = (value.bits() & crate::value::POINTER_MASK) as usize;
    crate::closure::is_closure_ptr(ptr)
        && !crate::object::builtin_closure_is_non_constructable_value(f64::from_bits(value.bits()))
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
    // The lookup allocates (the key, and a getter may run user code), so the
    // receiver and every argument are read back from roots afterwards.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(object);
    let original_args: Vec<f64> = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    } else {
        Vec::new()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&original_args);

    let key = crate::string::canonical_key(name);
    let receiver_ptr =
        JSValue::from_bits(receiver.get_nanbox_f64().to_bits()).as_pointer::<ObjectHeader>();
    let method = js_object_get_field_by_name(receiver_ptr, key);
    if !is_user_function(method) {
        let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
        return js_native_call_method(
            receiver.get_nanbox_f64(),
            method_name_ptr,
            method_name_len,
            args.as_ptr(),
            args.len(),
        );
    }
    let method = scope.root_nanbox_f64(f64::from_bits(method.bits()));
    let bound = crate::closure::clone_closure_rebind_this(
        method.get_nanbox_f64().to_bits(),
        receiver.get_nanbox_f64(),
    );
    let bound = scope.root_nanbox_u64(bound);
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    // A non-arrow function body reads its receiver from its `this` argument;
    // `clone_closure_rebind_this` only covers a closure that captured `this`.
    crate::closure::native_call_value_this(
        bound.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
        args.as_ptr(),
        args.len(),
    )
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
