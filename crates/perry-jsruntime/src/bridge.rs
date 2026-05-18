//! Value bridge between NaN-boxed JSValue and V8 values
//!
//! This module handles conversion between the Perry runtime's NaN-boxed
//! representation and V8's value system.
//!
//! ## V8 Object Handle Table
//!
//! V8 objects (objects, arrays, functions) returned to native code are stored
//! in a thread-local handle table. The native code receives a handle ID that
//! can be used to retrieve the V8 object for subsequent operations.

use deno_core::v8;
use perry_runtime::JSValue;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::interop::{bump_js_handle_released, bump_js_handle_stored, bump_v8_entry, V8EntryKind};

// NaN-boxing constants (must match perry-runtime/src/value.rs)
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;

/// Tag for V8 object handles - these are opaque references to V8 objects
/// stored in the handle table, NOT native Perry objects
const JS_HANDLE_TAG: u64 = 0x7FFB_0000_0000_0000;

const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

// Thread-local storage for V8 object handles
thread_local! {
    /// Maps handle IDs to V8 Global handles
    static JS_OBJECT_HANDLES: RefCell<HashMap<u64, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// Stable V8 constructor-like wrappers for Perry class references.
    static NATIVE_CLASS_HANDLES: RefCell<HashMap<u32, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// V8 Promise resolvers waiting on native Perry promises returned through callbacks.
    static NATIVE_PROMISE_RESOLVERS: RefCell<HashMap<u64, v8::Global<v8::PromiseResolver>>> = RefCell::new(HashMap::new());
    /// Snapshot of untampered intrinsics used by the conservative JS export
    /// data-object fast path. Captured during `js_runtime_init`, before user
    /// modules can replace `globalThis.Object` or its methods.
    static EXPORT_SNAPSHOT_INTRINSICS: RefCell<Option<ExportSnapshotIntrinsics>> = const { RefCell::new(None) };
    /// Counter for generating unique handle IDs
    static NEXT_HANDLE_ID: Cell<u64> = const { Cell::new(1) };
    static NEXT_NATIVE_PROMISE_RESOLVER_ID: Cell<u64> = const { Cell::new(1) };
}

struct ExportSnapshotIntrinsics {
    object_prototype: v8::Global<v8::Value>,
    object_is_frozen: v8::Global<v8::Function>,
}

pub fn capture_export_snapshot_intrinsics(scope: &mut v8::PinScope<'_, '_>) {
    let Some(intrinsics) = load_export_snapshot_intrinsics(scope) else {
        // If the lookup of `globalThis.Object` / its `prototype` / `isFrozen`
        // ever fails at runtime init, every export-data-object fast-path
        // eligibility check will silently return false (`is_plain_object`
        // requires the intrinsics cell to be set). That would manifest as a
        // perf cliff rather than a correctness bug — surface it loudly so
        // regressions don't hide as "slow but still working".
        eprintln!(
            "perry-jsruntime: failed to capture Object intrinsics at init; \
             JS export-data-object snapshot fast path disabled \
             (every export read will go through V8 fallback)"
        );
        return;
    };
    EXPORT_SNAPSHOT_INTRINSICS.with(|cell| {
        *cell.borrow_mut() = Some(intrinsics);
    });
}

fn load_export_snapshot_intrinsics(
    scope: &mut v8::PinScope<'_, '_>,
) -> Option<ExportSnapshotIntrinsics> {
    let global = scope.get_current_context().global(scope);
    let object_key = v8::String::new(scope, "Object")?;
    let object_value = global.get(scope, object_key.into())?;
    let object_ctor = v8::Local::<v8::Object>::try_from(object_value).ok()?;

    let prototype_key = v8::String::new(scope, "prototype")?;
    let object_prototype = object_ctor.get(scope, prototype_key.into())?;

    let is_frozen_key = v8::String::new(scope, "isFrozen")?;
    let is_frozen_value = object_ctor.get(scope, is_frozen_key.into())?;
    let object_is_frozen = v8::Local::<v8::Function>::try_from(is_frozen_value).ok()?;

    Some(ExportSnapshotIntrinsics {
        object_prototype: v8::Global::new(scope, object_prototype),
        object_is_frozen: v8::Global::new(scope, object_is_frozen),
    })
}

fn native_class_constructor(
    scope: &mut v8::PinScope<'_, '_>,
    _args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    retval.set(v8::Object::new(scope).into());
}

// Issue: Effect.pipe(map) chain — when a Perry closure (raw `*const
// ClosureHeader` pointer that's been NaN-boxed with POINTER_TAG) crosses
// into V8 as an argument, it must surface as a real v8::Function so JS
// code can invoke it. Without this wrapper, V8 saw a string/object proxy
// (from `native_object_to_v8`'s fallback paths) and threw "f is not a
// function" when Effect's internal pipeline tried to call the mapping
// function.
//
// Mirrors `native_callback_trampoline` (interop.rs) but stores the
// closure pointer directly in the v8::Function's `data` slot instead of
// going through the NATIVE_CALLBACKS registry — we already have the
// closure pointer in hand and don't need a stable callback_id for it.
fn perry_closure_v8_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let data = args.data();
    if !data.is_external() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let external = v8::Local::<v8::External>::try_from(data).unwrap();
    let closure_ptr = external.value() as i64;
    if closure_ptr == 0 {
        retval.set(v8::undefined(scope).into());
        return;
    }

    let arg_count = args.length();
    let mut native_args: Vec<f64> = Vec::with_capacity(arg_count as usize);
    for i in 0..arg_count {
        let arg = args.get(i);
        native_args.push(v8_to_native(scope, arg));
    }

    let _scope_guard = crate::stash_trampoline_scope(scope);

    type ClosureCallFn = unsafe extern "C" fn(i64, *const f64, i64) -> f64;
    let func: ClosureCallFn = perry_runtime::closure::js_closure_call_array;
    let result = unsafe { func(closure_ptr, native_args.as_ptr(), native_args.len() as i64) };

    let v8_result = native_to_v8(scope, result);
    retval.set(v8_result);
}

/// Wrap a Perry closure (raw pointer to a `ClosureHeader` with
/// `CLOSURE_MAGIC` at offset 12) as a `v8::Function`. Used by
/// `native_object_to_v8` when an argument passed to V8 turns out to be a
/// native closure — typically when a `LocalGet` holding an arrow function
/// is passed to a V8-imported call site like `Effect.map(fn)`.
fn native_closure_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> Option<v8::Local<'s, v8::Value>> {
    if ptr.is_null() {
        return None;
    }
    // Closure pointer is *const ClosureHeader. Stash the raw address in a
    // v8::External so the trampoline can recover it on invocation.
    let external = v8::External::new(scope, ptr as *mut std::ffi::c_void);
    let function = v8::Function::builder(perry_closure_v8_trampoline)
        .data(external.into())
        .build(scope)?;
    Some(function.into())
}

fn native_class_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    class_id: u32,
) -> v8::Local<'s, v8::Value> {
    if let Some(existing) = NATIVE_CLASS_HANDLES.with(|handles| {
        handles
            .borrow()
            .get(&class_id)
            .map(|global| v8::Local::new(scope, global))
    }) {
        return existing;
    }

    let function = v8::Function::builder(native_class_constructor)
        .build(scope)
        .unwrap_or_else(|| v8::Function::new(scope, native_class_constructor).unwrap());
    if let Some(key) = v8::String::new(scope, "__perry_native_class_id") {
        let value = v8::Integer::new_from_unsigned(scope, class_id);
        function.set(scope, key.into(), value.into());
    }
    let value: v8::Local<v8::Value> = function.into();
    NATIVE_CLASS_HANDLES.with(|handles| {
        handles
            .borrow_mut()
            .insert(class_id, v8::Global::new(scope, value));
    });
    value
}

fn native_class_id_from_v8(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> Option<u32> {
    if !(value.is_function() || value.is_object()) {
        return None;
    }
    let obj = v8::Local::<v8::Object>::try_from(value).ok()?;
    let key = v8::String::new(scope, "__perry_native_class_id")?;
    let id_value = obj.get(scope, key.into())?;
    if id_value.is_undefined() || id_value.is_null() || !id_value.is_uint32() {
        return None;
    }
    let id = id_value.uint32_value(scope)?;
    if id == 0 {
        return None;
    }
    Some(id)
}

pub fn v8_to_native_metadata_target(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> f64 {
    if let Some(class_id) = native_class_id_from_v8(scope, value) {
        return f64::from_bits(INT32_TAG | class_id as u64);
    }

    if value.is_object() {
        if let Ok(obj) = v8::Local::<v8::Object>::try_from(value) {
            if let Some(key) = v8::String::new(scope, "__native_ptr__") {
                if let Some(ptr_value) = obj.get(scope, key.into()) {
                    if ptr_value.is_external() {
                        let external = v8::Local::<v8::External>::try_from(ptr_value).unwrap();
                        return f64::from_bits(
                            POINTER_TAG | (external.value() as u64 & POINTER_MASK),
                        );
                    }
                }
            }
        }
    }

    v8_to_native(scope, value)
}

pub fn v8_to_native_metadata_value(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> f64 {
    if let Some(class_id) = native_class_id_from_v8(scope, value) {
        return f64::from_bits(INT32_TAG | class_id as u64);
    }

    if value.is_array() {
        let array = v8::Local::<v8::Array>::try_from(value).unwrap();
        let ptr = v8_array_to_native_metadata(scope, array);
        return f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK));
    }

    v8_to_native(scope, value)
}

/// Store a V8 value in the handle table and return a handle ID
pub fn store_js_handle(scope: &mut v8::PinScope<'_, '_>, value: v8::Local<v8::Value>) -> u64 {
    let handle_id = NEXT_HANDLE_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });
    let global = v8::Global::new(scope, value);
    JS_OBJECT_HANDLES.with(|handles| {
        handles.borrow_mut().insert(handle_id, global);
    });
    bump_js_handle_stored();
    handle_id
}

/// Retrieve a V8 value from the handle table
pub fn get_js_handle<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle: u64,
) -> Option<v8::Local<'s, v8::Value>> {
    JS_OBJECT_HANDLES.with(|handles| {
        handles
            .borrow()
            .get(&handle)
            .map(|g| v8::Local::new(scope, g))
    })
}

/// Release a V8 handle from the table
pub fn release_js_handle(handle: u64) -> bool {
    let released = JS_OBJECT_HANDLES.with(|handles| handles.borrow_mut().remove(&handle).is_some());
    if released {
        bump_js_handle_released();
    }
    released
}

/// Check if a NaN-boxed value is a JS handle
pub fn is_js_handle(value: f64) -> bool {
    let bits = value.to_bits();
    (bits & TAG_MASK) == JS_HANDLE_TAG
}

/// Extract handle ID from a NaN-boxed JS handle value
pub fn get_handle_id(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    if (bits & TAG_MASK) == JS_HANDLE_TAG {
        Some(bits & POINTER_MASK)
    } else {
        None
    }
}

/// Create a NaN-boxed value representing a JS handle
pub fn make_js_handle_value(handle_id: u64) -> f64 {
    f64::from_bits(JS_HANDLE_TAG | (handle_id & POINTER_MASK))
}

fn store_native_promise_resolver(
    scope: &mut v8::PinScope<'_, '_>,
    resolver: v8::Local<v8::PromiseResolver>,
) -> u64 {
    let resolver_id = NEXT_NATIVE_PROMISE_RESOLVER_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });
    NATIVE_PROMISE_RESOLVERS.with(|resolvers| {
        resolvers
            .borrow_mut()
            .insert(resolver_id, v8::Global::new(scope, resolver));
    });
    resolver_id
}

fn take_native_promise_resolver<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    resolver_id: u64,
) -> Option<v8::Local<'s, v8::PromiseResolver>> {
    NATIVE_PROMISE_RESOLVERS.with(|resolvers| {
        resolvers
            .borrow_mut()
            .remove(&resolver_id)
            .map(|resolver| v8::Local::new(scope, resolver))
    })
}

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
        if let Some(v8_val) = get_js_handle(scope, handle_id) {
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

/// Convert a native string pointer to a Rust String
unsafe fn native_string_to_rust(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }

    // StringHeader layout: { utf16_len: u32, byte_len: u32, capacity: u32, refcount: u32, flags: u32, data: [u8] }
    #[repr(C)]
    struct StringHeader {
        _utf16_len: u32,
        byte_len: u32,
        _capacity: u32,
        _refcount: u32,
        _flags: u32,
    }

    let header = ptr as *const StringHeader;
    let length = (*header).byte_len as usize;
    let data_ptr = ptr.add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, length);

    String::from_utf8_lossy(bytes).to_string()
}

/// Convert a Rust string to a native string pointer
fn rust_string_to_native(s: &str) -> *const u8 {
    use perry_runtime::js_string_from_bytes;

    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as *const u8
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

fn native_promise_to_v8<'s>(
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

/// Convert a native object pointer to a V8 object
fn native_object_to_v8<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ptr: *const u8,
) -> v8::Local<'s, v8::Value> {
    if ptr.is_null() {
        return v8::null(scope).into();
    }

    // Use GcHeader (8 bytes before user pointer) to reliably determine type.
    // All Perry arrays and objects are arena-allocated with GcHeader via arena_alloc_gc.
    let gc_header_ptr = (ptr as usize).wrapping_sub(perry_runtime::gc::GC_HEADER_SIZE);
    if gc_header_ptr > 0x1000 {
        let gc_header = unsafe { &*(gc_header_ptr as *const perry_runtime::gc::GcHeader) };
        let is_arena = (gc_header.gc_flags & perry_runtime::gc::GC_FLAG_ARENA) != 0;

        if gc_header.obj_type == perry_runtime::gc::GC_TYPE_PROMISE {
            return native_promise_to_v8(scope, ptr as *mut perry_runtime::promise::Promise);
        }

        // Issue: Effect.pipe(map) chain — a Perry closure passed to V8 as
        // an arg (e.g. `Effect.map(fn)` where `fn` is a local arrow) lands
        // here with POINTER_TAG. Confirm the `CLOSURE_MAGIC` tag before
        // wrapping so we don't misidentify a generic native object as a
        // closure. The HIR-level `JsCreateCallback` rewrite handles inline
        // `Closure` literals; this is the LocalGet / FuncRef fallback
        // path.
        if gc_header.obj_type == perry_runtime::gc::GC_TYPE_CLOSURE {
            const CLOSURE_TYPE_TAG_OFFSET: usize = 12;
            let type_tag = unsafe { *(ptr.add(CLOSURE_TYPE_TAG_OFFSET) as *const u32) };
            if type_tag == perry_runtime::closure::CLOSURE_MAGIC {
                if let Some(func_value) = native_closure_to_v8(scope, ptr) {
                    return func_value;
                }
            }
        }

        if is_arena && gc_header.obj_type == perry_runtime::gc::GC_TYPE_ARRAY {
            // GC-tracked array: ArrayHeader { length: u32, capacity: u32 } + f64 elements
            let header = ptr as *const perry_runtime::array::ArrayHeader;
            let length = unsafe { (*header).length };
            let elements_ptr = unsafe {
                ptr.add(std::mem::size_of::<perry_runtime::array::ArrayHeader>()) as *const f64
            };
            let v8_array = v8::Array::new(scope, length as i32);
            for i in 0..length {
                let elem_f64 = unsafe { *elements_ptr.add(i as usize) };
                let v8_elem = native_to_v8(scope, elem_f64);
                v8_array.set_index(scope, i, v8_elem);
            }
            return v8_array.into();
        }

        if is_arena && gc_header.obj_type == perry_runtime::gc::GC_TYPE_OBJECT {
            // GC-tracked object: ObjectHeader (24 bytes) + field values
            let obj_header = ptr as *const perry_runtime::object::ObjectHeader;
            let field_count = unsafe { (*obj_header).field_count };
            let keys_array = unsafe { (*obj_header).keys_array };

            let v8_obj = v8::Object::new(scope);

            if !keys_array.is_null() && field_count > 0 {
                // Object has named keys - iterate and set each field
                let keys_length = unsafe { (*keys_array).length };
                let keys_elements_ptr = unsafe {
                    (keys_array as *const u8)
                        .add(std::mem::size_of::<perry_runtime::array::ArrayHeader>())
                        as *const f64
                };
                // Fields are stored as f64 (NaN-boxed JSValues) right after ObjectHeader
                let fields_ptr = unsafe {
                    ptr.add(std::mem::size_of::<perry_runtime::object::ObjectHeader>())
                        as *const f64
                };

                let count = std::cmp::min(field_count, keys_length);
                for i in 0..count {
                    // Get key string from keys_array. Keys may be heap strings or
                    // inline short strings, so route through the general V8 bridge.
                    let key_f64 = unsafe { *keys_elements_ptr.add(i as usize) };
                    let key_val = native_to_v8(scope, key_f64);
                    let v8_key = match key_val.to_string(scope) {
                        Some(k) => k,
                        None => continue,
                    };

                    // Get field value (NaN-boxed f64)
                    let field_f64 = unsafe { *fields_ptr.add(i as usize) };
                    let v8_val = native_to_v8(scope, field_f64);

                    v8_obj.set(scope, v8_key.into(), v8_val);
                }
            }

            return v8_obj.into();
        }
    }

    // Safety check: If the pointer looks like a StringHeader (length + capacity match,
    // and data after header is valid UTF-8), convert it as a string instead of an array.
    // This handles the case where a string pointer accidentally gets POINTER_TAG instead of STRING_TAG.
    {
        let str_header = ptr as *const perry_runtime::string::StringHeader;
        let str_len = unsafe { (*str_header).byte_len } as usize;
        let str_cap = unsafe { (*str_header).capacity } as usize;
        if str_len > 0 && str_len <= 100_000 && str_cap >= str_len && str_cap <= str_len + 64 {
            // Capacity is close to length — looks like a string, not an array
            // (Arrays typically have capacity much larger than needed due to growth)
            let data =
                unsafe { ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>()) };
            let bytes = unsafe { std::slice::from_raw_parts(data, str_len) };
            if let Ok(s) = std::str::from_utf8(bytes) {
                if let Some(v8_str) = v8::String::new(scope, s) {
                    return v8_str.into();
                }
            }
        }
    }

    // Fallback: heuristic array detection for non-arena allocations (Maps, etc.)
    let header = ptr as *const perry_runtime::array::ArrayHeader;
    let length = unsafe { (*header).length };
    let capacity = unsafe { (*header).capacity };
    if length <= 100_000 && capacity >= length && capacity <= 200_000 {
        let elements_ptr = unsafe {
            ptr.add(std::mem::size_of::<perry_runtime::array::ArrayHeader>()) as *const f64
        };
        let v8_array = v8::Array::new(scope, length as i32);
        for i in 0..length {
            let elem_f64 = unsafe { *elements_ptr.add(i as usize) };
            let v8_elem = native_to_v8(scope, elem_f64);
            v8_array.set_index(scope, i, v8_elem);
        }
        return v8_array.into();
    }

    // Unknown type - wrap native pointer for opaque access
    let obj = v8::Object::new(scope);
    let external = v8::External::new(scope, ptr as *mut std::ffi::c_void);
    let key = v8::String::new(scope, "__native_ptr__").unwrap();
    obj.set(scope, key.into(), external.into());

    obj.into()
}

/// Convert a native BigInt pointer to a V8 BigInt
fn native_bigint_to_v8<'s>(
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
    use perry_runtime::js_array_alloc;

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
            unsafe {
                // Set the value directly using pointer arithmetic
                // ArrayHeader is { length: u32, capacity: u32 } = 8 bytes
                // Followed by array of f64 values
                let data_ptr = (native_array as *mut u8).add(8) as *mut f64;
                *data_ptr.add(i as usize) = native_val;
            }
        }
    }

    native_array as *mut u8
}

fn v8_array_to_native_metadata(
    scope: &mut v8::PinScope<'_, '_>,
    array: v8::Local<v8::Array>,
) -> *mut u8 {
    use perry_runtime::js_array_alloc;

    let length = array.length();
    let native_array = js_array_alloc(length);
    unsafe {
        (*native_array).length = length;
    }

    for i in 0..length {
        if let Some(val) = array.get_index(scope, i) {
            let native_val = v8_to_native_metadata_value(scope, val);
            unsafe {
                let data_ptr = (native_array as *mut u8).add(8) as *mut f64;
                *data_ptr.add(i as usize) = native_val;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_constants() {
        // Verify our tag constants match expected values
        assert_eq!(TAG_UNDEFINED, 0x7FFC_0000_0000_0001);
        assert_eq!(TAG_NULL, 0x7FFC_0000_0000_0002);
        assert_eq!(TAG_FALSE, 0x7FFC_0000_0000_0003);
        assert_eq!(TAG_TRUE, 0x7FFC_0000_0000_0004);
    }
}
