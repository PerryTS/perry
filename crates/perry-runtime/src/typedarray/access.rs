//! TypedArray element access FFI: `length`, `get`/`set`, `at`, dynamic-key
//! `[[Get]]`, `set(source, offset)`, `copyWithin`, and the Uint8-specialized
//! get/set. Split out of `typedarray/mod.rs`.

use super::*;

use std::alloc::{alloc, Layout};
use std::cell::RefCell;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::array::ArrayHeader;
use crate::closure::ClosureHeader;
use crate::typedarray_half::{f16_bits_to_f64, f64_to_f16_bits};

/// Element count.
#[no_mangle]
pub extern "C" fn js_typed_array_length(ta: *const TypedArrayHeader) -> i32 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return 0;
    }
    unsafe {
        if crate::native_arena::is_native_typed_view(ta) {
            crate::native_arena::validate_view_alive(
                crate::native_arena::native_view_from_typed_array(ta),
            );
        }
        (*ta).length as i32
    }
}

/// `ta[i]` — returns plain f64 numeric value (NOT NaN-boxed).
#[no_mangle]
pub extern "C" fn js_typed_array_get(ta: *const TypedArrayHeader, index: i32) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return 0.0;
    }
    unsafe {
        if crate::native_arena::is_native_typed_view(ta) {
            crate::native_arena::validate_view_alive(
                crate::native_arena::native_view_from_typed_array(ta),
            );
        }
        if index < 0 || index as u32 >= (*ta).length {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        load_at(ta, index as usize)
    }
}

/// #2063 — dynamic / string-key `[[Get]]` on a TypedArray (`ta[key]`).
///
/// The codegen element-read fast path only fires for statically-proven
/// numeric indices. A string key reaches here instead of being blindly
/// coerced to an integer index (a NaN-boxed string `fptosi`'d to 0, so
/// `ta["copyWithin"]` / `ta[m]` returned element 0 — `typeof` was "number" —
/// and `ta["2"]` returned element 0 instead of element 2). This implements
/// the ECMAScript IntegerIndexedExotic `[[Get]]` dispatch:
///   * canonical numeric index string → integer-indexed element read
///     (bounds-checked; out-of-range → undefined),
///   * any other string → ordinary `[[Get]]` (named / prototype property) via
///     the same `js_object_get_field_by_name_f64` the dotted `ta.copyWithin`
///     PropertyGet path uses (resolves the reified method once #2059 lands;
///     undefined until then — never a stray element value),
///   * a numeric (non-string) key → integer-indexed element read only when it
///     is a valid integer index; fractional numeric keys read `undefined`.
#[no_mangle]
pub extern "C" fn js_typed_array_index_get_dynamic(ta: *const TypedArrayHeader, key: f64) -> f64 {
    unsafe { crate::typedarray_props::typed_array_index_get_dynamic(ta as usize, key) }
}

// #2063: force-keep the dynamic-key getter under LTO / auto-optimize. Like
// `js_dyn_index_get`, this export has zero internal Rust callers — it is only
// invoked from generated LLVM IR (codegen emits the call in
// `perry-codegen/src/expr/index_get.rs`), so a whole-program bitcode link is
// free to internalize and dead-strip it. The `#[used]` anchor pins it.
#[used]
static KEEP_JS_TYPED_ARRAY_INDEX_GET_DYNAMIC: extern "C" fn(*const TypedArrayHeader, f64) -> f64 =
    js_typed_array_index_get_dynamic;

/// `ta.at(i)` with negative-index support.
#[no_mangle]
pub extern "C" fn js_typed_array_at(ta: *const TypedArrayHeader, index: f64) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        if crate::native_arena::is_native_typed_view(ta) {
            crate::native_arena::validate_view_alive(
                crate::native_arena::native_view_from_typed_array(ta),
            );
        }
        let len = (*ta).length as i64;
        let mut idx = index as i64;
        if idx < 0 {
            idx += len;
        }
        if idx < 0 || idx >= len {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        load_at(ta, idx as usize)
    }
}

/// `ta[i] = value`.
#[no_mangle]
pub extern "C" fn js_typed_array_set(ta: *mut TypedArrayHeader, index: i32, value: f64) {
    let ta = clean_ta_ptr(ta) as *mut TypedArrayHeader;
    if ta.is_null() {
        return;
    }
    unsafe {
        if crate::native_arena::is_native_typed_view(ta as *const TypedArrayHeader) {
            crate::native_arena::validate_view_alive(
                crate::native_arena::native_view_from_typed_array(ta as *const TypedArrayHeader),
            );
        }
        if index < 0 || index as u32 >= (*ta).length {
            return;
        }
        let kind = (*ta).kind;
        if kind == KIND_BIGINT64 || kind == KIND_BIGUINT64 {
            // IntegerIndexedElementSet on a bigint view performs `ToBigInt` —
            // a Number throws `TypeError`. Pass the NaN-boxed BigInt straight
            // to `store_at` (NOT through `jsvalue_to_f64`, which maps it to NaN).
            store_at(ta, index as usize, bigint::to_bigint_for_store(value));
        } else {
            store_at(ta, index as usize, jsvalue_to_f64(value));
        }
    }
}

/// Classified source for `TypedArray.prototype.set`. A typed-array / Buffer
/// source is coercion-free and is read into a `Vec` up front so an overlapping
/// source copies correctly (#2879). An array-like source is left unmaterialized
/// so the caller can interleave Get + ToNumber/ToBigInt + Set per element
/// (§23.2.3.24.1 SetTypedArrayFromArrayLike), which is observable: a throwing
/// element coercion must leave earlier elements written.
enum SetSource {
    /// Numeric source already read into f64 element values (typed array / Buffer).
    Buffered(Vec<f64>),
    /// Plain JS `Array` source — read+coerce each slot lazily.
    Array(*const ArrayHeader, usize),
    /// Array-like object source — `length` already coerced; read keys lazily.
    ArrayLike(*const crate::object::ObjectHeader, usize),
    /// Recognized but contributes no elements (ArrayBuffer / primitive → len 0).
    Empty,
}

/// `ToLength` clamped to `usize`: NaN/≤0 → 0, else `min(⌊n⌋, 2^53-1)`.
fn to_length_usize(n: f64) -> usize {
    if n.is_nan() || n <= 0.0 {
        0
    } else {
        n.trunc().min(9007199254740991.0) as usize
    }
}

/// Classify a `TypedArray.prototype.set` source. Returns `None` only for
/// null/undefined (caller throws TypeError). `dst_kind` validates BigInt/Number
/// copy rules up front for typed-array / Buffer sources.
unsafe fn classify_set_source(source_value: f64, dst_kind: u8) -> Option<SetSource> {
    let v = crate::value::JSValue::from_bits(source_value.to_bits());
    if v.is_null() || v.is_undefined() {
        return None;
    }
    // A primitive string source: `ToObject("567")` is an array-like of
    // single-char strings (length 3, "5"/"6"/"7"), each coerced per kind —
    // `ta.set("567")` writes 5, 6, 7 (test262 set/array-arg-primitive-toobject).
    if v.is_any_string() {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some((data, len)) = crate::string::str_bytes_from_jsvalue(source_value, &mut scratch)
        {
            if data.is_null() || len == 0 {
                return Some(SetSource::Empty);
            }
            let bytes = std::slice::from_raw_parts(data, len as usize);
            let Ok(s) = std::str::from_utf8(bytes) else {
                return Some(SetSource::Empty);
            };
            let mut out = Vec::new();
            for ch in s.chars() {
                let mut buf = [0u8; 4];
                let cs = ch.encode_utf8(&mut buf);
                let hdr = crate::string::js_string_from_bytes(cs.as_ptr(), cs.len() as u32);
                let char_value = crate::value::js_nanbox_string(hdr as i64);
                out.push(bigint::coerce_for_kind(dst_kind, char_value));
            }
            return Some(SetSource::Buffered(out));
        }
        return Some(SetSource::Empty);
    }
    let bits = source_value.to_bits();
    let top16 = bits >> 48;
    let addr = if top16 == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && bits >= 0x10000 {
        bits as usize
    } else {
        return Some(SetSource::Empty);
    };

    // Source is another typed array (coercion-free; buffered for overlap safety).
    if lookup_typed_array_kind(addr).is_some() {
        let src = addr as *const TypedArrayHeader;
        bigint::validate_copy_kinds(dst_kind, (*src).kind);
        let len = (*src).length as usize;
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(load_at(src, i));
        }
        return Some(SetSource::Buffered(out));
    }

    // Perry's Uint8Array is Buffer-backed; treat it as a numeric typed-array
    // source instead of reading its bytes as f64 array slots.
    if crate::buffer::is_registered_buffer(addr) {
        if crate::buffer::is_any_array_buffer(addr) {
            return Some(SetSource::Empty);
        }
        if bigint::is_bigint_kind(dst_kind) {
            bigint::throw_bigint_number_mix();
        }
        let src = addr as *const crate::buffer::BufferHeader;
        let len = (*src).length as usize;
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(crate::buffer::js_buffer_get(src, i as i32) as f64);
        }
        return Some(SetSource::Buffered(out));
    }

    if addr >= crate::gc::GC_HEADER_SIZE + 0x1000
        && crate::object::is_valid_obj_ptr(addr as *const u8)
    {
        let header =
            (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let obj_type = (*header).obj_type;
        if obj_type == crate::gc::GC_TYPE_ARRAY {
            let arr = addr as *const ArrayHeader;
            let len = crate::array::js_array_length(arr) as usize;
            return Some(SetSource::Array(arr, len));
        }
        if obj_type == crate::gc::GC_TYPE_OBJECT {
            // Array-like object: LengthOfArrayLike = ToLength(ToNumber(Get(o,"length"))).
            let obj = addr as *const crate::object::ObjectHeader;
            let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
            let len_field = crate::object::js_object_get_field_by_name(obj, len_key);
            let len_num = crate::builtins::js_number_coerce(f64::from_bits(len_field.bits()));
            return Some(SetSource::ArrayLike(obj, to_length_usize(len_num)));
        }
    }

    Some(SetSource::Empty)
}

/// `TypedArray.prototype.set(source, offset?)` — bulk-copy/coerce the source
/// elements into the receiver starting at `offset`. Validates the range
/// (throws `RangeError` when `offset + source.length > target.length`) and
/// returns `undefined`. Source reads are buffered into a `Vec` first so an
/// overlapping typed-array source copies correctly (#2879).
#[no_mangle]
pub extern "C" fn js_typed_array_set_from(
    ta: *mut TypedArrayHeader,
    source_value: f64,
    offset_value: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta) as *mut TypedArrayHeader;
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // targetOffset = ToIntegerOrInfinity(offset): ToNumber (valueOf-aware),
    // NaN → 0, ±Infinity preserved so a negative/out-of-range infinite offset
    // still throws RangeError below.
    let offset_num = crate::builtins::js_number_coerce(offset_value);
    let offset = if offset_num.is_nan() {
        0.0
    } else {
        offset_num.trunc()
    };
    unsafe {
        let source = match classify_set_source(source_value, (*ta).kind) {
            Some(s) => s,
            None => throw_type_error(b"Cannot convert undefined or null to object"),
        };
        let target_len = (*ta).length as f64;
        let src_len = match &source {
            SetSource::Buffered(v) => v.len(),
            SetSource::Array(_, n) | SetSource::ArrayLike(_, n) => *n,
            SetSource::Empty => 0,
        };
        // Range validation precedes any element write (RangeError). ±Inf offsets
        // are handled naturally by the f64 comparison.
        if offset < 0.0 || offset + src_len as f64 > target_len {
            throw_range_error(b"offset is out of bounds");
        }
        let base = offset as usize;
        let is_bigint = bigint::is_bigint_kind((*ta).kind);
        match source {
            // Coercion-free numeric source: bulk store (already overlap-buffered).
            SetSource::Buffered(elems) => {
                for (i, v) in elems.into_iter().enumerate() {
                    store_at(ta, base + i, v);
                }
            }
            // SetTypedArrayFromArrayLike: interleave Get + ToNumber/ToBigInt + Set
            // per element so a throwing element coercion leaves earlier elements
            // written ("values are set until exception").
            SetSource::Array(arr, len) => {
                for k in 0..len {
                    let raw = crate::array::js_array_get_f64(arr, k as u32);
                    let v = if is_bigint {
                        bigint::to_bigint_for_store(raw)
                    } else {
                        crate::builtins::js_number_coerce(raw)
                    };
                    store_at(ta, base + k, v);
                }
            }
            SetSource::ArrayLike(obj, len) => {
                for k in 0..len {
                    let key = k.to_string();
                    let key_ptr =
                        crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
                    let raw = f64::from_bits(
                        crate::object::js_object_get_field_by_name(obj, key_ptr).bits(),
                    );
                    let v = if is_bigint {
                        bigint::to_bigint_for_store(raw)
                    } else {
                        crate::builtins::js_number_coerce(raw)
                    };
                    store_at(ta, base + k, v);
                }
            }
            SetSource::Empty => {}
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `TypedArray.prototype.copyWithin(target, start, end?)` — copy the element
/// block `[start, end)` to `target`, mutating the receiver in place and
/// returning it. Uses per-kind `load_at`/`store_at` (NOT boxed Array slots)
/// and buffers the read block so overlapping ranges copy correctly (#2879).
#[no_mangle]
pub extern "C" fn js_typed_array_copy_within(
    ta: *mut TypedArrayHeader,
    target_value: f64,
    start_value: f64,
    end_value: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta) as *mut TypedArrayHeader;
    if ta.is_null() {
        return ta;
    }
    unsafe {
        let len = (*ta).length as i64;
        let rel = |v: f64| -> i64 {
            let n = jsvalue_to_f64(v);
            if n.is_nan() {
                return 0;
            }
            if !n.is_finite() {
                return if n > 0.0 { len } else { 0 };
            }
            let idx = n.trunc() as i64;
            if idx < 0 {
                (len + idx).max(0)
            } else {
                idx.min(len)
            }
        };
        // `end` defaults to len when the argument is undefined.
        let end_is_undefined = crate::value::JSValue::from_bits(end_value.to_bits()).is_undefined();
        let to = rel(target_value);
        let from = rel(start_value);
        let final_ = if end_is_undefined {
            len
        } else {
            rel(end_value)
        };
        let count = (final_ - from).min(len - to);
        if count <= 0 {
            return ta;
        }
        let count = count as usize;
        let from = from as usize;
        let to = to as usize;
        // Buffer the source block first (overlap-safe).
        let block: Vec<f64> = (0..count).map(|i| load_at(ta, from + i)).collect();
        for (i, v) in block.into_iter().enumerate() {
            store_at(ta, to + i, v);
        }
    }
    ta
}

#[no_mangle]
pub extern "C" fn js_uint8array_get(target: *const TypedArrayHeader, index: i32) -> i32 {
    let addr = strip_nanbox(target as u64);
    if addr < 0x1000 || index < 0 {
        return 0;
    }
    if let Some(kind) = lookup_typed_array_kind(addr) {
        if !matches!(kind, KIND_UINT8 | KIND_UINT8_CLAMPED) {
            return 0;
        }
        let value = js_typed_array_get(addr as *const TypedArrayHeader, index);
        if value.to_bits() == crate::value::TAG_UNDEFINED {
            0
        } else {
            value as i32
        }
    } else if crate::buffer::is_registered_buffer(addr) {
        crate::buffer::js_buffer_get(addr as *const crate::buffer::BufferHeader, index)
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_uint8array_set(target: *mut TypedArrayHeader, index: i32, value: i32) {
    let addr = strip_nanbox(target as u64);
    if addr < 0x1000 || index < 0 {
        return;
    }
    if let Some(kind) = lookup_typed_array_kind(addr) {
        if !matches!(kind, KIND_UINT8 | KIND_UINT8_CLAMPED) {
            return;
        }
        js_typed_array_set(addr as *mut TypedArrayHeader, index, value as f64);
    } else if crate::buffer::is_registered_buffer(addr) {
        crate::buffer::js_buffer_set(addr as *mut crate::buffer::BufferHeader, index, value);
    }
}
