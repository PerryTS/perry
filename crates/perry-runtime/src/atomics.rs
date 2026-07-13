//! Minimal `Atomics` namespace operations for integer TypedArray views.

use crate::closure::ClosureHeader;
use crate::typedarray::{
    clean_ta_ptr, js_typed_array_get, js_typed_array_length, js_typed_array_set,
    lookup_typed_array_kind, TypedArrayHeader, KIND_BIGINT64, KIND_BIGUINT64, KIND_INT16,
    KIND_INT32, KIND_INT8, KIND_UINT16, KIND_UINT32, KIND_UINT8,
};
use crate::value::JSValue;

fn nanbox_bool(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn string_value(bytes: &[u8]) -> f64 {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

fn object_key(bytes: &[u8]) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_range_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn numeric_integer_kind(kind: u8) -> bool {
    matches!(
        kind,
        KIND_INT8 | KIND_UINT8 | KIND_INT16 | KIND_UINT16 | KIND_INT32 | KIND_UINT32
    )
}

fn bigint_integer_kind(kind: u8) -> bool {
    matches!(kind, KIND_BIGINT64 | KIND_BIGUINT64)
}

fn supported_integer_kind(kind: u8) -> bool {
    numeric_integer_kind(kind) || bigint_integer_kind(kind)
}

/// Element size in bytes for the wait/notify-eligible kinds (Int32 / BigInt64).
fn atomic_elem_size(kind: u8) -> usize {
    match kind {
        KIND_BIGINT64 | KIND_BIGUINT64 => 8,
        _ => 4,
    }
}

/// Translate an already-`ToNumber`'d Atomics timeout (milliseconds) into a
/// futex deadline. Per spec: `undefined`/`NaN` and `+Infinity` mean "block
/// forever" (`None`); a non-positive value means "poll, return immediately"
/// (`Some(ZERO)`); otherwise the millisecond duration. Absurdly large finite
/// timeouts that would overflow `Duration` are treated as infinite.
fn timeout_to_deadline(timeout_ms: f64) -> Option<std::time::Duration> {
    if timeout_ms.is_nan() || timeout_ms == f64::INFINITY {
        return None;
    }
    if timeout_ms <= 0.0 {
        return Some(std::time::Duration::ZERO);
    }
    let secs = timeout_ms / 1000.0;
    if secs > 1.0e9 {
        return None;
    }
    Some(std::time::Duration::from_secs_f64(secs))
}

/// `Atomics.notify` count argument → number of waiters to wake. `undefined`
/// means `+Infinity` (wake all). Otherwise `ToIntegerOrInfinity` clamped to
/// `[0, usize::MAX]`. `number_arg` runs the full `ToNumber` (object `valueOf`,
/// BigInt → TypeError) before truncation.
fn notify_count_arg(count: f64) -> usize {
    if JSValue::from_bits(count.to_bits()).is_undefined() {
        return usize::MAX;
    }
    let n = number_arg(count);
    if n.is_nan() {
        0
    } else if n == f64::INFINITY {
        usize::MAX
    } else if n <= 0.0 {
        0
    } else if n >= usize::MAX as f64 {
        usize::MAX
    } else {
        n.trunc() as usize
    }
}

enum AtomicView {
    TypedArray {
        ptr: *mut TypedArrayHeader,
        kind: u8,
    },
    Uint8ArrayBuffer(*mut crate::buffer::BufferHeader),
}

impl AtomicView {
    fn kind(&self) -> u8 {
        match self {
            AtomicView::TypedArray { kind, .. } => *kind,
            AtomicView::Uint8ArrayBuffer(_) => KIND_UINT8,
        }
    }

    fn is_bigint(&self) -> bool {
        bigint_integer_kind(self.kind())
    }

    fn has_shared_backing(&self) -> bool {
        match self {
            AtomicView::TypedArray { ptr, .. } => {
                crate::typedarray::typed_array_has_shared_backing(*ptr)
            }
            AtomicView::Uint8ArrayBuffer(ptr) => {
                crate::buffer::is_shared_array_buffer(*ptr as usize)
            }
        }
    }

    fn length(&self) -> i32 {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_length(*ptr),
            AtomicView::Uint8ArrayBuffer(ptr) => crate::buffer::js_buffer_length(*ptr),
        }
    }

    fn get_numeric(&self, index: i32) -> f64 {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_get(*ptr, index),
            AtomicView::Uint8ArrayBuffer(ptr) => {
                crate::buffer::js_buffer_get(*ptr as *const crate::buffer::BufferHeader, index)
                    as f64
            }
        }
    }

    fn set_numeric(&self, index: i32, value: f64) {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_set(*ptr, index, value),
            AtomicView::Uint8ArrayBuffer(ptr) => {
                crate::buffer::js_buffer_set(*ptr, index, value as i32);
            }
        }
    }

    /// Absolute physical byte address of element `index`'s storage. For a
    /// `SharedArrayBuffer`-backed view this resolves (through the view-meta
    /// aliasing in `typed_array_bytes`) into the process-global backing store,
    /// so the same SAB index yields the same address on every agent — the futex
    /// table key that lets cross-thread `wait`/`notify` rendezvous (#4913).
    /// Returns 0 if the backing pointer can't be resolved.
    fn slot_addr(&self, index: i32) -> usize {
        match self {
            AtomicView::TypedArray { ptr, kind } => unsafe {
                let base = crate::typedarray::typed_array_bytes(*ptr)
                    .map(|b| b.as_ptr() as usize)
                    .unwrap_or(0);
                if base == 0 {
                    return 0;
                }
                base + (index.max(0) as usize) * atomic_elem_size(*kind)
            },
            AtomicView::Uint8ArrayBuffer(ptr) => {
                let base =
                    crate::buffer::buffer_data(*ptr as *const crate::buffer::BufferHeader) as usize;
                if base == 0 {
                    return 0;
                }
                base + index.max(0) as usize
            }
        }
    }

    fn get_bigint_bits(&self, index: i32) -> u64 {
        match self {
            AtomicView::TypedArray { ptr, .. } => typed_array_bigint_bits(*ptr, index),
            AtomicView::Uint8ArrayBuffer(_) => {
                throw_type_error(b"Atomics operation requires a BigInt typed array")
            }
        }
    }

    fn set_bigint_bits(&self, index: i32, value: u64) {
        match self {
            AtomicView::TypedArray { ptr, .. } => typed_array_set_bigint_bits(*ptr, index, value),
            AtomicView::Uint8ArrayBuffer(_) => {
                throw_type_error(b"Atomics operation requires a BigInt typed array")
            }
        }
    }
}

fn atomics_view_arg(value: f64) -> AtomicView {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        throw_type_error(b"Atomics operation requires an integer typed array");
    }
    let raw = clean_ta_ptr(js.as_pointer::<TypedArrayHeader>()) as usize;
    if raw == 0 {
        throw_type_error(b"Atomics operation requires an integer typed array");
    }
    if let Some(kind) = lookup_typed_array_kind(raw) {
        if !supported_integer_kind(kind) {
            throw_type_error(b"Atomics operation requires an integer typed array");
        }
        return AtomicView::TypedArray {
            ptr: raw as *mut TypedArrayHeader,
            kind,
        };
    }
    if crate::buffer::is_registered_buffer(raw) && crate::buffer::is_uint8array_buffer(raw) {
        return AtomicView::Uint8ArrayBuffer(raw as *mut crate::buffer::BufferHeader);
    }
    throw_type_error(b"Atomics operation requires an integer typed array");
}

fn atomics_wait_notify_view_arg(value: f64) -> AtomicView {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        throw_type_error(b"Atomics wait/notify requires an Int32Array or BigInt64Array");
    }
    let raw = clean_ta_ptr(js.as_pointer::<TypedArrayHeader>()) as usize;
    if raw == 0 {
        throw_type_error(b"Atomics wait/notify requires an Int32Array or BigInt64Array");
    }
    match lookup_typed_array_kind(raw) {
        Some(kind @ (KIND_INT32 | KIND_BIGINT64)) => AtomicView::TypedArray {
            ptr: raw as *mut TypedArrayHeader,
            kind,
        },
        _ => throw_type_error(b"Atomics wait/notify requires an Int32Array or BigInt64Array"),
    }
}

fn atomics_to_index(index: f64, length: i32) -> i32 {
    // ECMA-262 ToIndex(index): ToNumber(index) → ToIntegerOrInfinity → range
    // check. `js_number_coerce` is the full ToNumber — it runs an object's
    // `Symbol.toPrimitive`/`valueOf`/`toString` (so the harness's
    // `{ valueOf: () => 125 }` indices actually evaluate) and throws TypeError
    // on a Symbol. A BigInt index is a ToNumber TypeError. The truncation must
    // happen BEFORE the negativity check so a fractional in-bounds index like
    // `-0.9` maps to +0 (ToIntegerOrInfinity) instead of wrongly throwing.
    let js = JSValue::from_bits(index.to_bits());
    if js.is_bigint() {
        throw_type_error(b"Cannot convert a BigInt value to a number");
    }
    let num = crate::builtins::js_number_coerce(index);
    let integer = if num.is_nan() { 0.0 } else { num.trunc() };
    if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
        throw_range_error(b"Invalid atomic access index");
    }
    if integer >= length as f64 {
        throw_range_error(b"Invalid atomic access index");
    }
    integer as i32
}

fn number_arg(value: f64) -> f64 {
    // ToNumber for the integer-view value / count / timeout arguments. A BigInt
    // is a ToNumber TypeError; everything else (objects with `valueOf`, strings,
    // booleans, Symbols-throw) is handled by the shared ToNumber.
    let js = JSValue::from_bits(value.to_bits());
    if js.is_bigint() {
        throw_type_error(b"Cannot convert a BigInt value to a number");
    }
    crate::builtins::js_number_coerce(value)
}

fn numeric_arg(value: f64) -> f64 {
    let n = number_arg(value);
    if n.is_finite() {
        // ToIntegerOrInfinity: truncate toward zero and normalize -0 to +0
        // (the `+ 0.0` collapses -0.0). `Atomics.store(i32a, 0, -0)` must
        // therefore return +0 (test262 store/expected-return-value-negative-zero).
        n.trunc() + 0.0
    } else {
        0.0
    }
}

/// Narrow an already-ToInteger'd number to the element kind (the modular
/// reduction in `SetValueInBuffer`/`ToRawBytes`). Takes the integer directly so
/// callers that already coerced the argument (and must observe `valueOf` only
/// once) don't double-coerce.
fn clamp_integer_for_kind(kind: u8, n: f64) -> f64 {
    match kind {
        KIND_INT8 => (n as i32 as i8) as f64,
        KIND_UINT8 => (n as i64).rem_euclid(256) as f64,
        KIND_INT16 => (n as i32 as i16) as f64,
        KIND_UINT16 => (n as i64).rem_euclid(65_536) as f64,
        KIND_INT32 => (n as i32) as f64,
        KIND_UINT32 => (n as i64 as u32) as f64,
        _ => n,
    }
}

fn coerce_for_kind(kind: u8, value: f64) -> f64 {
    clamp_integer_for_kind(kind, numeric_arg(value))
}

fn to_uint32_bits(value: f64) -> u32 {
    numeric_arg(value).rem_euclid(4_294_967_296.0) as u32
}

fn bitwise_result_for_kind(kind: u8, bits: u32) -> f64 {
    match kind {
        KIND_INT8 => (bits as u8 as i8) as f64,
        KIND_UINT8 => (bits as u8) as f64,
        KIND_INT16 => (bits as u16 as i16) as f64,
        KIND_UINT16 => (bits as u16) as f64,
        KIND_INT32 => (bits as i32) as f64,
        KIND_UINT32 => bits as f64,
        _ => bits as f64,
    }
}

/// `ToBigInt(value)` for the BigInt-view Atomics arguments. Delegates to the
/// shared TypedArray store coercion, which runs `Symbol.toPrimitive`/`valueOf`/
/// `toString` on objects and throws the spec TypeErrors (Number/undefined/null/
/// Symbol → "Cannot convert … to a BigInt").
fn bigint_value(value: f64) -> f64 {
    crate::typedarray::bigint::to_bigint_for_store(value)
}

/// Low 64 bits of an already-coerced BigInt value (its limb-0 magnitude bits).
fn bigint_limb0(coerced: f64) -> u64 {
    let ptr = JSValue::from_bits(coerced.to_bits()).as_bigint_ptr();
    let ptr = crate::bigint::clean_bigint_ptr(ptr);
    if ptr.is_null() {
        return 0;
    }
    unsafe { (*ptr).limbs[0] }
}

fn bigint_bits(value: f64) -> u64 {
    bigint_limb0(bigint_value(value))
}

fn bigint_result_for_kind(kind: u8, bits: u64) -> f64 {
    let ptr = match kind {
        KIND_BIGINT64 => crate::bigint::js_bigint_from_i64(bits as i64),
        KIND_BIGUINT64 => crate::bigint::js_bigint_from_u64(bits),
        _ => crate::bigint::js_bigint_from_i64(bits as i64),
    };
    f64::from_bits(JSValue::bigint_ptr(ptr).bits())
}

fn typed_array_bigint_bits(ta: *const TypedArrayHeader, index: i32) -> u64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() || index < 0 {
        return 0;
    }
    unsafe {
        if crate::native_arena::is_native_typed_view(ta) {
            crate::native_arena::validate_view_alive(
                crate::native_arena::native_view_from_typed_array(ta),
            );
        }
        if index as u32 >= (*ta).length {
            return 0;
        }
        let data = crate::typedarray::typed_array_bytes(ta).unwrap_or(&[]);
        let off = (index as usize).saturating_mul((*ta).elem_size as usize);
        let bytes = data.get(off..off + 8).unwrap_or(&[]);
        if bytes.len() != 8 {
            return 0;
        }
        u64::from_ne_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }
}

fn typed_array_set_bigint_bits(ta: *mut TypedArrayHeader, index: i32, value: u64) {
    let ta = clean_ta_ptr(ta) as *mut TypedArrayHeader;
    if ta.is_null() || index < 0 {
        return;
    }
    unsafe {
        if crate::native_arena::is_native_typed_view(ta as *const TypedArrayHeader) {
            crate::native_arena::validate_view_alive(
                crate::native_arena::native_view_from_typed_array(ta as *const TypedArrayHeader),
            );
        }
        if index as u32 >= (*ta).length {
            return;
        }
        let Some(data) = crate::typedarray::typed_array_bytes_mut(ta) else {
            return;
        };
        let off = (index as usize).saturating_mul((*ta).elem_size as usize);
        if let Some(slot) = data.get_mut(off..off + 8) {
            slot.copy_from_slice(&value.to_ne_bytes());
        }
    }
}

fn slot(view: f64, index: f64) -> (AtomicView, i32) {
    let view = atomics_view_arg(view);
    let idx = atomics_to_index(index, view.length());
    (view, idx)
}

fn wait_notify_slot(view: f64, index: f64) -> (AtomicView, i32) {
    let view = atomics_wait_notify_view_arg(view);
    let idx = atomics_to_index(index, view.length());
    (view, idx)
}

fn wait_async_result(async_value: bool, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(0, 2);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let value_handle = scope.root_nanbox_f64(value);

    let async_key = object_key(b"async");
    let async_key_handle = scope.root_string_ptr(async_key);
    crate::object::js_object_set_field_by_name(
        obj_handle.get_raw_mut_ptr(),
        async_key_handle.get_raw_const_ptr(),
        nanbox_bool(async_value),
    );

    let value_key = object_key(b"value");
    let value_key_handle = scope.root_string_ptr(value_key);
    crate::object::js_object_set_field_by_name(
        obj_handle.get_raw_mut_ptr(),
        value_key_handle.get_raw_const_ptr(),
        value_handle.get_nanbox_f64(),
    );

    crate::value::js_nanbox_pointer(
        obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
    )
}

fn atomics_bitwise(view: f64, index: f64, value: f64, op: impl FnOnce(u64, u64) -> u64) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        let result = op(previous, bigint_bits(value));
        view.set_bigint_bits(idx, result);
        return bigint_result_for_kind(kind, previous);
    }
    let kind = view.kind();
    let previous = view.get_numeric(idx);
    let result = op(
        to_uint32_bits(previous) as u64,
        to_uint32_bits(value) as u64,
    );
    view.set_numeric(idx, bitwise_result_for_kind(kind, result as u32));
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_load(_closure: *const ClosureHeader, view: f64, index: f64) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        return bigint_result_for_kind(view.kind(), view.get_bigint_bits(idx));
    }
    view.get_numeric(idx)
}

#[no_mangle]
pub extern "C" fn js_atomics_is_lock_free(_closure: *const ClosureHeader, size: f64) -> f64 {
    // ToNumber(size) (runs `valueOf`/`toString`, so `'4'`→4 behaves like Node),
    // then an EXACT membership test over the lock-free element widths. Node/V8
    // compares the raw number, so `4.9` is NOT floored to 4 — it is simply not a
    // valid element width and returns false.
    let n = number_arg(size);
    nanbox_bool(n == 1.0 || n == 2.0 || n == 4.0 || n == 8.0)
}

#[no_mangle]
pub extern "C" fn js_atomics_store(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        // ToBigInt runs the value's coercion hook exactly once; spec returns the
        // coerced BigInt itself.
        let stored = bigint_value(value);
        view.set_bigint_bits(idx, bigint_limb0(stored));
        return stored;
    }
    // ToIntegerOrInfinity once (observes `valueOf` a single time). Atomics.store
    // returns that integer — NOT the element-narrowed read-back — so e.g.
    // `Atomics.store(int8, 0, 300)` returns 300 even though the slot holds 44.
    let n = numeric_arg(value);
    view.set_numeric(idx, clamp_integer_for_kind(view.kind(), n));
    n
}

#[no_mangle]
pub extern "C" fn js_atomics_add(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        view.set_bigint_bits(idx, previous.wrapping_add(bigint_bits(value)));
        return bigint_result_for_kind(kind, previous);
    }
    let previous = view.get_numeric(idx);
    view.set_numeric(
        idx,
        coerce_for_kind(view.kind(), previous + numeric_arg(value)),
    );
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_sub(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        view.set_bigint_bits(idx, previous.wrapping_sub(bigint_bits(value)));
        return bigint_result_for_kind(kind, previous);
    }
    let previous = view.get_numeric(idx);
    view.set_numeric(
        idx,
        coerce_for_kind(view.kind(), previous - numeric_arg(value)),
    );
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_and(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    atomics_bitwise(view, index, value, |a, b| a & b)
}

#[no_mangle]
pub extern "C" fn js_atomics_or(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    atomics_bitwise(view, index, value, |a, b| a | b)
}

#[no_mangle]
pub extern "C" fn js_atomics_xor(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    atomics_bitwise(view, index, value, |a, b| a ^ b)
}

#[no_mangle]
pub extern "C" fn js_atomics_exchange(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        view.set_bigint_bits(idx, bigint_bits(value));
        return bigint_result_for_kind(kind, previous);
    }
    let previous = view.get_numeric(idx);
    view.set_numeric(idx, coerce_for_kind(view.kind(), value));
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_compare_exchange(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    expected: f64,
    replacement: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        let expected = bigint_bits(expected);
        let replacement = bigint_bits(replacement);
        if previous == expected {
            view.set_bigint_bits(idx, replacement);
        }
        return bigint_result_for_kind(kind, previous);
    }
    let previous = view.get_numeric(idx);
    let expected = coerce_for_kind(view.kind(), expected);
    let replacement = coerce_for_kind(view.kind(), replacement);
    if previous == expected {
        view.set_numeric(idx, replacement);
    }
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_notify(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    count: f64,
) -> f64 {
    let (view, idx) = wait_notify_slot(view, index);
    // Coerce the count for its observable side effects even on a non-shared
    // buffer (spec runs ToIntegerOrInfinity before the shared check returns 0).
    let count = notify_count_arg(count);
    if !view.has_shared_backing() {
        // A non-shared buffer can have no parked agents — spec returns 0.
        return 0.0;
    }
    let addr = view.slot_addr(idx);
    if addr == 0 {
        return 0.0;
    }
    crate::atomics_futex::notify(addr, count) as f64
}

#[no_mangle]
pub extern "C" fn js_atomics_wait(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    expected: f64,
    timeout: f64,
) -> f64 {
    // Spec order: validate the view, then the shared-buffer check throws BEFORE
    // ValidateAtomicAccess coerces the index — so a poisoned `valueOf` index on
    // a non-shared view must not run (test262 wait/non-shared-bufferdata-throws).
    let view = atomics_wait_notify_view_arg(view);
    if !view.has_shared_backing() {
        throw_type_error(b"Atomics.wait requires a shared typed array");
    }
    let idx = atomics_to_index(index, view.length());
    let kind = view.kind();
    // Coerce expected (and timeout) before parking, observing `valueOf` once.
    let expected_bits = if kind == KIND_BIGINT64 {
        bigint_bits(expected)
    } else {
        0
    };
    let expected_num = if kind == KIND_BIGINT64 {
        0.0
    } else {
        coerce_for_kind(KIND_INT32, expected)
    };
    let deadline = timeout_to_deadline(number_arg(timeout));
    let addr = view.slot_addr(idx);
    if addr == 0 {
        return string_value(b"not-equal");
    }

    // Real futex park (#4913). The value re-check runs under the wait table's
    // lock (atomic with the enqueue), so a `notify` from another agent that
    // races this call is never lost: the agent either sees the changed value
    // and returns "not-equal", or parks and is woken. A `timeout === 0` poll
    // enqueues then immediately times out → "timed-out". An `undefined`/
    // Infinity timeout blocks until notified.
    let still_equal = || {
        if kind == KIND_BIGINT64 {
            view.get_bigint_bits(idx) == expected_bits
        } else {
            view.get_numeric(idx) == expected_num
        }
    };
    match crate::atomics_futex::wait(addr, deadline, still_equal) {
        crate::atomics_futex::WaitOutcome::NotEqual => string_value(b"not-equal"),
        crate::atomics_futex::WaitOutcome::Ok => string_value(b"ok"),
        crate::atomics_futex::WaitOutcome::TimedOut => string_value(b"timed-out"),
    }
}

#[no_mangle]
pub extern "C" fn js_atomics_wait_async(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    expected: f64,
    timeout: f64,
) -> f64 {
    let view = atomics_wait_notify_view_arg(view);
    if !view.has_shared_backing() {
        throw_type_error(b"Atomics.waitAsync requires a shared typed array");
    }
    let idx = atomics_to_index(index, view.length());
    let kind = view.kind();
    let expected_bigint_bits = if kind == KIND_BIGINT64 {
        bigint_bits(expected)
    } else {
        0
    };
    let expected_i32 = if kind == KIND_BIGINT64 {
        0.0
    } else {
        coerce_for_kind(KIND_INT32, expected)
    };
    let timeout = number_arg(timeout);
    let mismatched = if kind == KIND_BIGINT64 {
        view.get_bigint_bits(idx) != expected_bigint_bits
    } else {
        view.get_numeric(idx) != expected_i32
    };
    if mismatched {
        // Value already differs → synchronous, non-async "not-equal".
        return wait_async_result(false, string_value(b"not-equal"));
    }
    if timeout <= 0.0 {
        // A zero/negative timeout returns synchronously (no promise).
        return wait_async_result(false, string_value(b"timed-out"));
    }

    // Matching value + positive (possibly Infinity/NaN) timeout: enqueue a
    // waiter synchronously (atomic with the value check, so a racing `notify`
    // is not lost), then resolve the promise from a background thread when a
    // matching `notify` arrives or the timeout elapses (#4913, Stage 2).
    let still_equal = || {
        if kind == KIND_BIGINT64 {
            view.get_bigint_bits(idx) == expected_bigint_bits
        } else {
            view.get_numeric(idx) == expected_i32
        }
    };
    let addr = view.slot_addr(idx);
    let handle = if addr == 0 {
        None
    } else {
        crate::atomics_futex::enqueue(addr, still_equal)
    };
    let Some(handle) = handle else {
        // Value changed at the atomic enqueue point → synchronous "not-equal".
        return wait_async_result(false, string_value(b"not-equal"));
    };

    let deadline = timeout_to_deadline(timeout);
    // Cross-thread variant: referenced only by a raw usize in the pending
    // results queue until drained — must not live in the copying nursery
    // (the from-space flip ignores pins that no scanner reaches).
    let promise = crate::promise::js_promise_new_cross_thread();
    // Pin the promise + keep the event loop alive until the async result lands.
    unsafe {
        crate::thread::pin_promise(promise);
    }
    crate::thread::thread_job_begin();
    let promise_usize = promise as usize;
    // #6185: the promise belongs to the agent calling `waitAsync`. The futex
    // waiter below owns no heap (it never runs JS), so it cannot infer the
    // right agent from itself — capture it here, on the calling thread.
    let owner_agent = crate::agent::current_agent();
    std::thread::spawn(move || {
        let outcome = crate::atomics_futex::block(handle, deadline);
        let result = match outcome {
            crate::atomics_futex::WaitOutcome::Ok => "ok",
            _ => "timed-out",
        };
        crate::thread::queue_promise_string_result(owner_agent, promise_usize, result);
    });
    wait_async_result(true, crate::value::js_nanbox_pointer(promise as i64))
}
