//! NaN-boxed JSValue ↔ V8 value conversions, including BigInt, plain-data
//! snapshots, and array/object round-trips.

use super::*;

use super::fetch_handle::native_object_to_v8;
use super::handles::{
    make_js_handle_value, store_js_handle, store_native_promise_resolver,
    take_native_promise_resolver,
};
use super::native_class::native_class_to_v8;

/// Fix up a native value for JS interop boundary.
/// Raw pointers (non-NaN-boxed I64 values bitcast to F64) need POINTER_TAG
/// so that native_to_v8 can properly convert them to V8 arrays/objects.
pub fn fixup_native_for_v8(value: f64) -> f64 {
    let bits = value.to_bits();
    // Raw heap pointers on arm64 are typically 0x0000_0001_xxxx_xxxx to 0x0000_000F_xxxx_xxxx
    // These appear as subnormal f64 values (exponent = 0, mantissa != 0)
    // No legitimate JS number would have bits in this range
    if bits > 0x0000_0001_0000_0000 && bits < 0x0001_0000_0000_0000 {
        // Raw pointer - add POINTER_TAG so native_to_v8 can convert it
        f64::from_bits(POINTER_TAG | (bits & POINTER_MASK))
    } else {
        value
    }
}

/// Convert a native NaN-boxed value to a V8 value
pub fn native_to_v8<'s>(scope: &mut v8::PinScope<'s, '_>, value: f64) -> v8::Local<'s, v8::Value> {
    let bits = value.to_bits();

    // Check special values
    if bits == TAG_UNDEFINED {
        return v8::undefined(scope).into();
    }
    if bits == TAG_NULL {
        return v8::null(scope).into();
    }
    if bits == TAG_FALSE {
        return v8::Boolean::new(scope, false).into();
    }
    if bits == TAG_TRUE {
        return v8::Boolean::new(scope, true).into();
    }

    let tag = bits & TAG_MASK;

    // Check for JS handle (V8 object reference)
    if tag == JS_HANDLE_TAG {
        let handle_id = bits & POINTER_MASK;
        if let Some(v8_val) = super::handles::get_js_handle(scope, handle_id) {
            return v8_val;
        }
        return v8::undefined(scope).into();
    }

    // Check for int32
    if tag == INT32_TAG {
        let int_val = (bits & 0xFFFF_FFFF) as i32;
        // Perry encodes class references as INT32_TAG | class_id (see
        // `Expr::ClassRef` codegen). When such a value crosses into V8 we
        // surface it as a stable constructor-like function so JS code can use
        // it as a metadata target. NOTE: this means raw integers that happen
        // to equal a registered class id (low positive numbers, the common
        // range) cannot round-trip through the bridge — they materialize as
        // the class function on the JS side. Decorator metadata is the only
        // existing caller, where the input is always a real class ref. If a
        // future caller needs int round-trip, switch class refs to a
        // dedicated NaN-box tag (see review on #754).
        if int_val > 0 && perry_runtime::object::is_class_id_registered(int_val as u32) {
            return native_class_to_v8(scope, int_val as u32);
        }
        return v8::Integer::new(scope, int_val).into();
    }

    // Check for string pointer
    if tag == STRING_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            let rust_str = unsafe { native_string_to_rust(ptr) };
            if let Some(v8_str) = v8::String::new(scope, &rust_str) {
                return v8_str.into();
            }
        }
        return v8::String::empty(scope).into();
    }

    if tag == SHORT_STRING_TAG {
        let value = JSValue::from_bits(bits);
        let mut buf = [0u8; perry_runtime::value::SHORT_STRING_MAX_LEN];
        let len = value.short_string_to_buf(&mut buf);
        let rust_str = String::from_utf8_lossy(&buf[..len]);
        if let Some(v8_str) = v8::String::new(scope, &rust_str) {
            return v8_str.into();
        }
        return v8::String::empty(scope).into();
    }

    // Check for BigInt pointer
    if tag == BIGINT_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            return native_bigint_to_v8(scope, ptr);
        }
        return v8::BigInt::new_from_i64(scope, 0).into();
    }

    // Check for object/array pointer
    if tag == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            return native_object_to_v8(scope, ptr);
        }
        return v8::null(scope).into();
    }

    // Otherwise it's a regular f64 number
    // Check if it's a valid IEEE 754 number (not NaN with our special tags)
    if (bits & 0x7FF0_0000_0000_0000) != 0x7FF0_0000_0000_0000
        || (bits & 0x000F_FFFF_FFFF_FFFF) == 0
    {
        return v8::Number::new(scope, value).into();
    }

    // Fallback to undefined for unrecognized values
    v8::undefined(scope).into()
}

/// Convert a V8 value to a native NaN-boxed value
///
/// For simple values (undefined, null, boolean, number, string), this converts
/// them to Perry's native NaN-boxed representation.
///
/// For complex values (objects, arrays, functions), this stores them in the
/// handle table and returns a JS handle. This preserves V8 objects for
/// subsequent method calls.
pub fn v8_to_native(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> f64 {
    if value.is_undefined() {
        return f64::from_bits(TAG_UNDEFINED);
    }

    if value.is_null() {
        return f64::from_bits(TAG_NULL);
    }

    if value.is_boolean() {
        let b = value.is_true();
        return f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
    }

    // Check number before int32 as numbers can also be int32
    if value.is_number() && !value.is_int32() {
        let num = value.number_value(scope).unwrap_or(f64::NAN);
        return num;
    }

    if value.is_int32() {
        let int_val = value.int32_value(scope).unwrap_or(0);
        return f64::from_bits(INT32_TAG | (int_val as u32 as u64));
    }

    if value.is_string() {
        let v8_str = value.to_string(scope).unwrap();
        let rust_str = v8_str.to_rust_string_lossy(scope);
        let ptr = rust_string_to_native(&rust_str);
        return f64::from_bits(STRING_TAG | (ptr as u64 & POINTER_MASK));
    }

    // Check for BigInt (used by ethers.js and other blockchain libraries)
    if value.is_big_int() {
        let bigint = v8::Local::<v8::BigInt>::try_from(value).unwrap();
        let ptr = v8_bigint_to_native(scope, bigint);
        return f64::from_bits(BIGINT_TAG | (ptr as u64 & POINTER_MASK));
    }

    // For functions, always store as JS handle to preserve callability
    if value.is_function() {
        let handle_id = store_js_handle(scope, value);
        return make_js_handle_value(handle_id);
    }

    // For arrays and objects, store as JS handle to preserve V8 methods and prototype chain
    // This is critical for objects returned from JS function calls (e.g., express())
    // which may have methods we need to call later (e.g., app.use(), app.get())
    if value.is_array() || value.is_object() {
        let handle_id = store_js_handle(scope, value);
        return make_js_handle_value(handle_id);
    }

    // Fallback to undefined
    f64::from_bits(TAG_UNDEFINED)
}

/// Convert JS module-export values to Perry values.
///
/// Frozen plain data objects exported from JS modules are safe to snapshot into
/// native Perry objects. That keeps follow-on property reads on constants like
/// `MODULE_METADATA.PROVIDERS` native instead of bouncing back into V8 for each
/// field. Mutable objects, accessors, proxies, custom prototypes, functions,
/// promises, arrays, symbols, or nested non-data values stay as V8 handles.
pub fn v8_to_native_export_value(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> f64 {
    if let Some(snapshot) = v8_plain_data_object_to_native(scope, value, 0) {
        return snapshot;
    }

    v8_to_native(scope, value)
}

/// Convert a V8 value to a native NaN-boxed value, converting arrays to native arrays
///
/// This variant converts arrays to native Perry arrays instead of JS handles.
/// Use this when you know the result should be a native array (e.g., for Array operations).
#[allow(dead_code)]
pub fn v8_to_native_array(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> f64 {
    // For arrays, convert to native Perry array
    if value.is_array() {
        let array = v8::Local::<v8::Array>::try_from(value).unwrap();
        let ptr = v8_array_to_native(scope, array);
        return f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK));
    }

    // For everything else, use the standard conversion
    v8_to_native(scope, value)
}

fn v8_plain_data_object_to_native(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
    depth: usize,
) -> Option<f64> {
    if depth > 4
        || value.is_function()
        || value.is_array()
        || value.is_promise()
        || v8_value_is_proxy(scope, value)
        || !value.is_object()
    {
        return None;
    }

    let obj = v8::Local::<v8::Object>::try_from(value).ok()?;
    if !is_plain_object(scope, obj) {
        return None;
    }
    if !v8_object_is_frozen(scope, obj)? {
        return None;
    }

    let mut names_args = v8::GetPropertyNamesArgsBuilder::new();
    let names = obj.get_own_property_names(
        scope,
        names_args
            .mode(v8::KeyCollectionMode::OwnOnly)
            .property_filter(v8::PropertyFilter::ALL_PROPERTIES)
            .index_filter(v8::IndexFilter::IncludeIndices)
            .key_conversion(v8::KeyConversionMode::ConvertToString)
            .build(),
    )?;
    if names.length() == 0 {
        return None;
    }
    let mut fields: Vec<(String, f64)> = Vec::with_capacity(names.length() as usize);

    for i in 0..names.length() {
        let key = names.get_index(scope, i)?;
        if key.is_symbol() {
            return None;
        }
        let key_string = key.to_string(scope)?.to_rust_string_lossy(scope);
        let field_value = frozen_data_descriptor_value(scope, obj, key)?;
        let native_value =
            if let Some(snapshot) = v8_plain_data_object_to_native(scope, field_value, depth + 1) {
                snapshot
            } else if is_plain_data_leaf(field_value) {
                v8_to_native(scope, field_value)
            } else {
                return None;
            };
        fields.push((key_string, native_value));
    }

    let native_obj = perry_runtime::js_object_alloc(0, 0);
    for (key, value) in fields {
        let key_ptr = perry_runtime::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        perry_runtime::js_object_set_field_by_name(native_obj, key_ptr, value);
    }

    Some(f64::from_bits(
        POINTER_TAG | (native_obj as u64 & POINTER_MASK),
    ))
}

fn is_plain_data_leaf(value: v8::Local<v8::Value>) -> bool {
    value.is_undefined()
        || value.is_null()
        || value.is_boolean()
        || value.is_number()
        || value.is_string()
        || value.is_big_int()
}

fn v8_value_is_proxy(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> bool {
    if value.is_proxy() {
        return true;
    }

    let global = scope.get_current_context().global(scope);
    let Some(deno_key) = v8::String::new(scope, "Deno") else {
        return false;
    };
    let Some(deno_value) = global.get(scope, deno_key.into()) else {
        return false;
    };
    let Ok(deno) = v8::Local::<v8::Object>::try_from(deno_value) else {
        return false;
    };
    let Some(core_key) = v8::String::new(scope, "core") else {
        return false;
    };
    let Some(core_value) = deno.get(scope, core_key.into()) else {
        return false;
    };
    let Ok(core) = v8::Local::<v8::Object>::try_from(core_value) else {
        return false;
    };

    if call_v8_boolean_method(scope, core, "isProxy", value).unwrap_or(false) {
        return true;
    }

    let Some(ops_key) = v8::String::new(scope, "ops") else {
        return false;
    };
    let Some(ops_value) = core.get(scope, ops_key.into()) else {
        return false;
    };
    let Ok(ops) = v8::Local::<v8::Object>::try_from(ops_value) else {
        return false;
    };
    call_v8_boolean_method(scope, ops, "op_is_proxy", value).unwrap_or(false)
}

fn call_v8_boolean_method(
    scope: &mut v8::PinScope<'_, '_>,
    receiver: v8::Local<v8::Object>,
    method_name: &str,
    arg: v8::Local<v8::Value>,
) -> Option<bool> {
    let key = v8::String::new(scope, method_name)?;
    let method_value = receiver.get(scope, key.into())?;
    let method = v8::Local::<v8::Function>::try_from(method_value).ok()?;
    let result = method.call(scope, receiver.into(), &[arg])?;
    if result.is_boolean() {
        Some(result.boolean_value(scope))
    } else {
        None
    }
}

fn is_plain_object(scope: &mut v8::PinScope<'_, '_>, obj: v8::Local<v8::Object>) -> bool {
    let Some(proto) = obj.get_prototype(scope) else {
        return false;
    };
    if proto.is_null() {
        return true;
    }

    EXPORT_SNAPSHOT_INTRINSICS.with(|cell| {
        let intrinsics = cell.borrow();
        let Some(intrinsics) = intrinsics.as_ref() else {
            return false;
        };
        let object_proto = v8::Local::new(scope, &intrinsics.object_prototype);
        proto.strict_equals(object_proto)
    })
}

fn v8_object_is_frozen(
    scope: &mut v8::PinScope<'_, '_>,
    obj: v8::Local<v8::Object>,
) -> Option<bool> {
    EXPORT_SNAPSHOT_INTRINSICS.with(|cell| {
        let intrinsics = cell.borrow();
        let intrinsics = intrinsics.as_ref()?;
        let is_frozen = v8::Local::new(scope, &intrinsics.object_is_frozen);
        let receiver = v8::undefined(scope).into();
        let obj_value: v8::Local<v8::Value> = obj.into();
        let result = is_frozen.call(scope, receiver, &[obj_value])?;
        if result.is_boolean() {
            Some(result.boolean_value(scope))
        } else {
            None
        }
    })
}

fn frozen_data_descriptor_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    obj: v8::Local<v8::Object>,
    key: v8::Local<v8::Value>,
) -> Option<v8::Local<'s, v8::Value>> {
    let name = v8::Local::<v8::Name>::try_from(key).ok()?;
    let descriptor_value = obj.get_own_property_descriptor(scope, name)?;
    if descriptor_value.is_undefined() || !descriptor_value.is_object() {
        return None;
    }
    let descriptor = v8::Local::<v8::Object>::try_from(descriptor_value).ok()?;

    let get_key = v8::String::new(scope, "get")?;
    let getter = descriptor.get(scope, get_key.into())?;
    if !getter.is_undefined() {
        return None;
    }

    let set_key = v8::String::new(scope, "set")?;
    let setter = descriptor.get(scope, set_key.into())?;
    if !setter.is_undefined() {
        return None;
    }

    let writable_key = v8::String::new(scope, "writable")?;
    let writable = descriptor.get(scope, writable_key.into())?;
    if !writable.is_boolean() || writable.boolean_value(scope) {
        return None;
    }

    let configurable_key = v8::String::new(scope, "configurable")?;
    let configurable = descriptor.get(scope, configurable_key.into())?;
    if !configurable.is_boolean() || configurable.boolean_value(scope) {
        return None;
    }

    let value_key = v8::String::new(scope, "value")?;
    if !descriptor.has(scope, value_key.into())? {
        return None;
    }
    let descriptor_value = descriptor.get(scope, value_key.into())?;
    let current_value = obj.get(scope, key)?;
    if !current_value.same_value(descriptor_value) {
        return None;
    }

    Some(descriptor_value)
}

extern "C" fn native_promise_v8_resolve(
    closure: *const perry_runtime::closure::ClosureHeader,
    value: f64,
) -> f64 {
    bump_v8_entry(V8EntryKind::NativePromiseResolve);
    let resolver_id = perry_runtime::closure::js_closure_get_capture_f64(closure, 0) as u64;
    crate::with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        if let Some(resolver) = take_native_promise_resolver(scope, resolver_id) {
            let v8_value = native_to_v8(scope, value);
            let _ = resolver.resolve(scope, v8_value);
        }
    });
    perry_runtime::event_pump::js_notify_main_thread();
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn native_promise_v8_reject(
    closure: *const perry_runtime::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    bump_v8_entry(V8EntryKind::NativePromiseReject);
    let resolver_id = perry_runtime::closure::js_closure_get_capture_f64(closure, 0) as u64;
    crate::with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        if let Some(resolver) = take_native_promise_resolver(scope, resolver_id) {
            let v8_reason = native_to_v8(scope, reason);
            let _ = resolver.reject(scope, v8_reason);
        }
    });
    perry_runtime::event_pump::js_notify_main_thread();
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn native_promise_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    promise: *mut perry_runtime::promise::Promise,
) -> v8::Local<'s, v8::Value> {
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        return v8::undefined(scope).into();
    };
    let v8_promise = resolver.get_promise(scope);
    match perry_runtime::promise::js_promise_state(promise) {
        1 => {
            bump_v8_entry(V8EntryKind::NativePromiseResolve);
            let value = perry_runtime::promise::js_promise_value(promise);
            let v8_value = native_to_v8(scope, value);
            let _ = resolver.resolve(scope, v8_value);
        }
        2 => {
            bump_v8_entry(V8EntryKind::NativePromiseReject);
            let reason = perry_runtime::promise::js_promise_reason(promise);
            let v8_reason = native_to_v8(scope, reason);
            let _ = resolver.reject(scope, v8_reason);
        }
        _ => {
            let resolver_id = store_native_promise_resolver(scope, resolver);
            let resolve_closure =
                perry_runtime::closure::js_closure_alloc(native_promise_v8_resolve as *const u8, 1);
            let reject_closure =
                perry_runtime::closure::js_closure_alloc(native_promise_v8_reject as *const u8, 1);
            perry_runtime::closure::js_closure_set_capture_f64(
                resolve_closure,
                0,
                resolver_id as f64,
            );
            perry_runtime::closure::js_closure_set_capture_f64(
                reject_closure,
                0,
                resolver_id as f64,
            );
            let _ =
                perry_runtime::promise::js_promise_then(promise, resolve_closure, reject_closure);
        }
    }
    v8_promise.into()
}

/// Convert a native BigInt pointer to a V8 BigInt
pub(super) fn native_bigint_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> v8::Local<'s, v8::Value> {
    use perry_runtime::bigint::BigIntHeader;

    if ptr.is_null() {
        return v8::BigInt::new_from_i64(scope, 0).into();
    }

    let header = ptr as *const BigIntHeader;
    let limbs = unsafe { (*header).limbs };

    // Check if the value fits in i64 (most common case)
    if limbs[1] == 0 && limbs[2] == 0 && limbs[3] == 0 {
        // Fits in a single limb - check sign
        let val = limbs[0];
        if val <= i64::MAX as u64 {
            return v8::BigInt::new_from_i64(scope, val as i64).into();
        }
        // Value is positive but too large for i64, use u64
        return v8::BigInt::new_from_u64(scope, val).into();
    }

    // Check if it's a negative number (two's complement: high bit set in top limb)
    let is_negative = (limbs[3] >> 63) == 1;

    if is_negative {
        // Convert from two's complement to magnitude
        let mut magnitude = limbs;
        // Subtract 1 and invert
        let mut borrow = 1u64;
        for limb in magnitude.iter_mut() {
            let (result, underflow) = limb.overflowing_sub(borrow);
            *limb = !result;
            borrow = if underflow { 1 } else { 0 };
        }
        // Find the actual word count (trim trailing zeros)
        let word_count = magnitude
            .iter()
            .rposition(|&x| x != 0)
            .map(|i| i + 1)
            .unwrap_or(1);
        v8::BigInt::new_from_words(scope, true, &magnitude[..word_count])
            .map(|bi| bi.into())
            .unwrap_or_else(|| v8::BigInt::new_from_i64(scope, 0).into())
    } else {
        // Positive number with multiple limbs
        // Find the actual word count (trim trailing zeros)
        let word_count = limbs
            .iter()
            .rposition(|&x| x != 0)
            .map(|i| i + 1)
            .unwrap_or(1);
        v8::BigInt::new_from_words(scope, false, &limbs[..word_count])
            .map(|bi| bi.into())
            .unwrap_or_else(|| v8::BigInt::new_from_i64(scope, 0).into())
    }
}

/// Convert a V8 object to a native object pointer
fn v8_object_to_native(scope: &mut v8::PinScope<'_, '_>, obj: v8::Local<v8::Object>) -> *mut u8 {
    use perry_runtime::{js_object_alloc, js_object_set_field};

    // Check if this object has a native pointer already
    let key = v8::String::new(scope, "__native_ptr__").unwrap();
    if let Some(val) = obj.get(scope, key.into()) {
        if val.is_external() {
            let external = v8::Local::<v8::External>::try_from(val).unwrap();
            return external.value() as *mut u8;
        }
    }

    // Get all own property names
    let names = obj
        .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
        .unwrap_or_else(|| v8::Array::new(scope, 0));

    let field_count = names.length();

    // Allocate native object
    let native_obj = js_object_alloc(0, field_count);

    // Set fields (keys handling is simplified for now)
    for i in 0..field_count {
        let key_val = names.get_index(scope, i).unwrap();

        // Get and convert the value
        if let Some(val) = obj.get(scope, key_val) {
            let native_val = v8_to_native(scope, val);
            // Convert f64 bits to JSValue
            let jsval = JSValue::from_bits(native_val.to_bits());
            js_object_set_field(native_obj, i, jsval);
        }
    }

    native_obj as *mut u8
}

/// Convert a V8 array to a native array pointer
fn v8_array_to_native(scope: &mut v8::PinScope<'_, '_>, array: v8::Local<v8::Array>) -> *mut u8 {
    use perry_runtime::{array::js_array_set_f64, js_array_alloc};

    let length = array.length();

    // Allocate native array
    let native_array = js_array_alloc(length);
    unsafe {
        (*native_array).length = length;
    }

    // Convert each element
    // We use js_array_set_f64 which takes the raw f64 bits
    for i in 0..length {
        if let Some(val) = array.get_index(scope, i) {
            let native_val = v8_to_native(scope, val);
            js_array_set_f64(native_array, i, native_val);
        }
    }

    native_array as *mut u8
}

pub(super) fn v8_array_to_native_metadata(
    scope: &mut v8::PinScope<'_, '_>,
    array: v8::Local<v8::Array>,
) -> *mut u8 {
    use perry_runtime::{array::js_array_set_f64, js_array_alloc};

    let length = array.length();
    let native_array = js_array_alloc(length);
    unsafe {
        (*native_array).length = length;
    }

    for i in 0..length {
        if let Some(val) = array.get_index(scope, i) {
            let native_val = super::native_class::v8_to_native_metadata_value(scope, val);
            js_array_set_f64(native_array, i, native_val);
        }
    }

    native_array as *mut u8
}

/// Convert a V8 BigInt to a native BigInt pointer
fn v8_bigint_to_native(
    _scope: &mut v8::PinScope<'_, '_>,
    bigint: v8::Local<v8::BigInt>,
) -> *mut u8 {
    use perry_runtime::bigint::BigIntHeader;
    use std::alloc::{alloc, Layout};

    // Get the word count to determine the size needed
    let word_count = bigint.word_count();

    // Allocate a BigIntHeader (4 x u64 = 256 bits)
    let layout = Layout::new::<BigIntHeader>();
    let ptr = unsafe { alloc(layout) as *mut BigIntHeader };
    if ptr.is_null() {
        panic!("Failed to allocate BigInt");
    }

    use perry_runtime::bigint::BIGINT_LIMBS;

    if word_count == 0 {
        // Zero value
        unsafe {
            (*ptr).limbs = [0; BIGINT_LIMBS];
        }
        return ptr as *mut u8;
    }

    // Get the words from V8 BigInt
    let mut words = vec![0u64; word_count];
    let (sign_bit, _) = bigint.to_words_array(&mut words);

    // Copy words to our BigIntHeader (up to BIGINT_LIMBS limbs)
    unsafe {
        let mut limbs = [0u64; BIGINT_LIMBS];
        for (i, &word) in words.iter().enumerate().take(BIGINT_LIMBS) {
            limbs[i] = word;
        }

        // Handle negative numbers (two's complement)
        if sign_bit {
            // Negate: invert all bits and add 1
            for limb in limbs.iter_mut() {
                *limb = !*limb;
            }
            // Add 1
            let mut carry = 1u64;
            for limb in limbs.iter_mut() {
                let (result, overflow) = limb.overflowing_add(carry);
                *limb = result;
                carry = if overflow { 1 } else { 0 };
            }
        }

        (*ptr).limbs = limbs;
    }

    ptr as *mut u8
}

/// Convert a native array pointer to a V8 array
pub fn native_array_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> v8::Local<'s, v8::Array> {
    if ptr.is_null() {
        return v8::Array::new(scope, 0);
    }

    // ArrayHeader layout: { length: u32, capacity: u32 }
    #[repr(C)]
    struct ArrayHeader {
        length: u32,
        _capacity: u32,
    }

    let header = ptr as *const ArrayHeader;
    let length = unsafe { (*header).length };

    let array = v8::Array::new(scope, length as i32);

    for i in 0..length {
        // Read the f64 value directly from the array data
        let native_val = unsafe {
            let data_ptr = (ptr as *const u8).add(8) as *const f64;
            *data_ptr.add(i as usize)
        };
        let v8_val = native_to_v8(scope, native_val);
        array.set_index(scope, i, v8_val);
    }

    array
}
