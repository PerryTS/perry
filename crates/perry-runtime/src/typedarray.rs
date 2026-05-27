//! TypedArray support: Int8Array, Uint8Array, Int16Array, Uint16Array,
//! Int32Array, Uint32Array, Float32Array, Float64Array.
//!
//! Each TypedArrayHeader stores its element kind + size and a contiguous
//! data region. Element-level read/write goes through `js_typed_array_get`
//! and `js_typed_array_set`, which handle the per-kind cast/store. The
//! immutable methods (`toSorted`, `toReversed`, `with`, etc.) materialize
//! a new TypedArrayHeader of the same kind.
//!
//! Pointers are NaN-boxed with POINTER_TAG (0x7FFD) and tracked in
//! TYPED_ARRAY_REGISTRY for `instanceof` and console.log formatting.

use std::alloc::{alloc, Layout};
use std::cell::RefCell;
use std::ptr;

use crate::array::ArrayHeader;
use crate::closure::ClosureHeader;

// Element kind tags. Match the order used by HIR/codegen.
pub const KIND_INT8: u8 = 0;
pub const KIND_UINT8: u8 = 1;
pub const KIND_INT16: u8 = 2;
pub const KIND_UINT16: u8 = 3;
pub const KIND_INT32: u8 = 4;
pub const KIND_UINT32: u8 = 5;
pub const KIND_FLOAT32: u8 = 6;
pub const KIND_FLOAT64: u8 = 7;
// Uint8ClampedArray: same element size as Uint8, but stores clamp to [0,255]
// using ToUint8Clamp (round-half-to-even) instead of truncate-wrap.
pub const KIND_UINT8_CLAMPED: u8 = 8;

// Reserved class IDs for instanceof. Stay in the 0xFFFF00xx reserved range.
pub const CLASS_ID_INT8_ARRAY: u32 = 0xFFFF0030;
pub const CLASS_ID_UINT8_ARRAY: u32 = 0xFFFF0031;
pub const CLASS_ID_INT16_ARRAY: u32 = 0xFFFF0032;
pub const CLASS_ID_UINT16_ARRAY: u32 = 0xFFFF0033;
pub const CLASS_ID_INT32_ARRAY: u32 = 0xFFFF0034;
pub const CLASS_ID_UINT32_ARRAY: u32 = 0xFFFF0035;
pub const CLASS_ID_FLOAT32_ARRAY: u32 = 0xFFFF0036;
pub const CLASS_ID_FLOAT64_ARRAY: u32 = 0xFFFF0037;
pub const CLASS_ID_UINT8_CLAMPED_ARRAY: u32 = 0xFFFF0038;

#[inline]
pub fn elem_size_for_kind(kind: u8) -> usize {
    match kind {
        KIND_INT8 | KIND_UINT8 | KIND_UINT8_CLAMPED => 1,
        KIND_INT16 | KIND_UINT16 => 2,
        KIND_INT32 | KIND_UINT32 | KIND_FLOAT32 => 4,
        KIND_FLOAT64 => 8,
        _ => 8,
    }
}

#[inline]
pub fn class_id_for_kind(kind: u8) -> u32 {
    match kind {
        KIND_INT8 => CLASS_ID_INT8_ARRAY,
        KIND_UINT8 => CLASS_ID_UINT8_ARRAY,
        KIND_INT16 => CLASS_ID_INT16_ARRAY,
        KIND_UINT16 => CLASS_ID_UINT16_ARRAY,
        KIND_INT32 => CLASS_ID_INT32_ARRAY,
        KIND_UINT32 => CLASS_ID_UINT32_ARRAY,
        KIND_FLOAT32 => CLASS_ID_FLOAT32_ARRAY,
        KIND_FLOAT64 => CLASS_ID_FLOAT64_ARRAY,
        KIND_UINT8_CLAMPED => CLASS_ID_UINT8_CLAMPED_ARRAY,
        _ => 0,
    }
}

#[inline]
pub fn name_for_kind(kind: u8) -> &'static str {
    match kind {
        KIND_INT8 => "Int8Array",
        KIND_UINT8 => "Uint8Array",
        KIND_INT16 => "Int16Array",
        KIND_UINT16 => "Uint16Array",
        KIND_INT32 => "Int32Array",
        KIND_UINT32 => "Uint32Array",
        KIND_FLOAT32 => "Float32Array",
        KIND_FLOAT64 => "Float64Array",
        KIND_UINT8_CLAMPED => "Uint8ClampedArray",
        _ => "TypedArray",
    }
}

/// TypedArrayHeader. The data region follows the header inline.
#[repr(C)]
pub struct TypedArrayHeader {
    /// Number of elements.
    pub length: u32,
    /// Capacity in elements.
    pub capacity: u32,
    /// Element kind tag (KIND_*).
    pub kind: u8,
    /// Element size in bytes (1, 2, 4, 8).
    pub elem_size: u8,
    pub _pad: [u8; 6],
}

thread_local! {
    /// Address -> kind, so we can detect typed arrays at format/instanceof time.
    /// PtrHasher (Fibonacci-multiplicative + xorshift): heap pointers don't
    /// need SipHash. Hot on `is_registered_buffer`-adjacent dispatch paths
    /// (~1.0% leaf samples on perf-comprehensive).
    static TYPED_ARRAY_REGISTRY: RefCell<crate::fast_hash::PtrHashMap<usize, u8>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

pub fn register_typed_array(ptr: *const TypedArrayHeader, kind: u8) {
    TYPED_ARRAY_REGISTRY.with(|r| {
        r.borrow_mut().insert(ptr as usize, kind);
    });
}

pub fn unregister_typed_array(ptr: *const TypedArrayHeader) {
    TYPED_ARRAY_REGISTRY.with(|r| {
        r.borrow_mut().remove(&(ptr as usize));
    });
}

/// Returns Some(kind) if the (already-stripped) address is a registered
/// typed array, else None.
pub fn lookup_typed_array_kind(addr: usize) -> Option<u8> {
    TYPED_ARRAY_REGISTRY.with(|r| r.borrow().get(&addr).copied())
}

#[inline]
fn strip_nanbox(p: u64) -> usize {
    let top16 = p >> 48;
    if top16 >= 0x7FF8 {
        (p & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        p as usize
    }
}

#[inline]
pub fn clean_ta_ptr(ptr: *const TypedArrayHeader) -> *const TypedArrayHeader {
    let addr = strip_nanbox(ptr as u64);
    if addr < 0x1000 {
        return ptr::null();
    }
    addr as *const TypedArrayHeader
}

#[inline]
fn data_ptr(ta: *const TypedArrayHeader) -> *const u8 {
    unsafe {
        if crate::native_arena::is_native_typed_view(ta) {
            crate::native_arena::native_view_data_ptr(ta)
        } else {
            (ta as *const u8).add(std::mem::size_of::<TypedArrayHeader>())
        }
    }
}

#[inline]
fn data_ptr_mut(ta: *mut TypedArrayHeader) -> *mut u8 {
    unsafe {
        if crate::native_arena::is_native_typed_view(ta as *const TypedArrayHeader) {
            crate::native_arena::native_view_data_ptr_mut(ta)
        } else {
            (ta as *mut u8).add(std::mem::size_of::<TypedArrayHeader>())
        }
    }
}

/// Return the byte view for a registered typed array.
///
/// Native arena views do not store their bytes after `TypedArrayHeader`; this
/// helper routes through `data_ptr`, which validates disposed native views and
/// returns the external backing pointer.
pub unsafe fn typed_array_bytes<'a>(ta: *const TypedArrayHeader) -> Option<&'a [u8]> {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() || lookup_typed_array_kind(ta as usize).is_none() {
        return None;
    }
    let data = data_ptr(ta);
    let len = ((*ta).length as usize).saturating_mul((*ta).elem_size as usize);
    if len == 0 {
        return Some(std::slice::from_raw_parts(
            ptr::NonNull::<u8>::dangling().as_ptr(),
            0,
        ));
    }
    if data.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts(data, len))
}

/// Return the mutable byte view for a registered typed array.
///
/// See [`typed_array_bytes`] for the native-view layout invariant.
pub unsafe fn typed_array_bytes_mut<'a>(ta: *mut TypedArrayHeader) -> Option<&'a mut [u8]> {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() || lookup_typed_array_kind(ta as usize).is_none() {
        return None;
    }
    let data = data_ptr_mut(ta);
    let len = ((*ta).length as usize).saturating_mul((*ta).elem_size as usize);
    if len == 0 {
        return Some(std::slice::from_raw_parts_mut(
            ptr::NonNull::<u8>::dangling().as_ptr(),
            0,
        ));
    }
    if data.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts_mut(data, len))
}

fn ta_layout(capacity: u32, elem_size: usize) -> Layout {
    let total = std::mem::size_of::<TypedArrayHeader>() + (capacity as usize) * elem_size;
    let total = total.max(std::mem::size_of::<TypedArrayHeader>() + elem_size);
    Layout::from_size_align(total, 8).unwrap()
}

#[inline]
fn typed_array_payload_size(capacity: u32, elem_size: usize) -> usize {
    let total = std::mem::size_of::<TypedArrayHeader>() + (capacity as usize) * elem_size;
    total.max(std::mem::size_of::<TypedArrayHeader>() + elem_size)
}

#[inline]
fn typed_array_gc_total_size(capacity: u32, elem_size: usize) -> usize {
    let payload = typed_array_payload_size(capacity, elem_size);
    (crate::gc::GC_HEADER_SIZE + payload + 7) & !7
}

/// Allocate a zero-filled typed array of `length` elements.
pub fn typed_array_alloc(kind: u8, length: u32) -> *mut TypedArrayHeader {
    let elem_size = elem_size_for_kind(kind);
    let capacity = length.max(1);
    if crate::gc::is_large_object_total_size(typed_array_gc_total_size(capacity, elem_size)) {
        let p = crate::arena::arena_alloc_gc_old(
            typed_array_payload_size(capacity, elem_size),
            8,
            crate::gc::GC_TYPE_TYPED_ARRAY,
        ) as *mut TypedArrayHeader;
        unsafe {
            let header = (p as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*header).gc_flags |= crate::gc::GC_FLAG_TENURED;
            (*p).length = length;
            (*p).capacity = capacity;
            (*p).kind = kind;
            (*p).elem_size = elem_size as u8;
            (*p)._pad = [0; 6];
            let data = data_ptr_mut(p);
            ptr::write_bytes(data, 0, (capacity as usize) * elem_size);
        }
        register_typed_array(p, kind);
        return p;
    }
    let layout = ta_layout(capacity, elem_size);
    unsafe {
        let raw = alloc(layout);
        if raw.is_null() {
            panic!("typed_array_alloc OOM");
        }
        let p = raw as *mut TypedArrayHeader;
        (*p).length = length;
        (*p).capacity = capacity;
        (*p).kind = kind;
        (*p).elem_size = elem_size as u8;
        (*p)._pad = [0; 6];
        // Zero data region
        let data = data_ptr_mut(p);
        ptr::write_bytes(data, 0, (capacity as usize) * elem_size);
        register_typed_array(p, kind);
        p
    }
}

/// Convert an f64 (NaN-boxed JS value) to the numeric value to store. Strings
/// and undefined become 0/NaN.
fn jsvalue_to_f64(v: f64) -> f64 {
    let bits = v.to_bits();
    let top16 = bits >> 48;
    // Plain double — positive, negative, ±Inf, and all NaN patterns that
    // are NOT NaN-box tags. Tagged values occupy top16 in 0x7FFA..0x7FFF
    // (BIGINT_TAG=0x7FFA, 0x7FFC=undefined/null/bool, POINTER_TAG=0x7FFD,
    // INT32_TAG=0x7FFE, STRING_TAG=0x7FFF). Negative doubles (top16≥0x8000)
    // and non-tag NaN patterns (top16 in 0x7FF8..0x7FF9) return as-is.
    if !(0x7FFA..0x8000).contains(&top16) {
        return v;
    }
    // INT32 tag
    if top16 == 0x7FFE {
        let n = (bits & 0xFFFF_FFFF) as i32;
        return n as f64;
    }
    // TRUE/FALSE
    if bits == 0x7FFC_0000_0000_0004 {
        return 1.0;
    }
    if bits == 0x7FFC_0000_0000_0003 {
        return 0.0;
    }
    if bits == 0x7FFC_0000_0000_0002 {
        return 0.0; // null -> 0
    }
    if bits == 0x7FFC_0000_0000_0001 {
        return f64::NAN; // undefined -> NaN
    }
    // Strings: try to parse, else 0/NaN
    if top16 == 0x7FFF {
        let str_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader;
        if !str_ptr.is_null() && (str_ptr as usize) >= 0x1000 {
            unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data =
                    (str_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                if let Ok(s) = std::str::from_utf8(std::slice::from_raw_parts(data, len)) {
                    if let Ok(n) = s.trim().parse::<f64>() {
                        return n;
                    }
                }
            }
        }
        return f64::NAN;
    }
    f64::NAN
}

/// Store a number into the typed array slot, performing the per-kind cast.
unsafe fn store_at(ta: *mut TypedArrayHeader, idx: usize, value: f64) {
    let kind = (*ta).kind;
    let elem_size = (*ta).elem_size as usize;
    let base = data_ptr_mut(ta);
    let off = idx * elem_size;
    match kind {
        KIND_INT8 => {
            let v = value as i32 as i8;
            *(base.add(off) as *mut i8) = v;
        }
        KIND_UINT8 => {
            let mut v = value as i64;
            v = v.rem_euclid(256);
            *base.add(off) = v as u8;
        }
        KIND_UINT8_CLAMPED => {
            // ToUint8Clamp: NaN → 0, v ≤ 0 → 0, v ≥ 255 → 255,
            // otherwise round-half-to-even then clamp.
            let byte = if value.is_nan() || value <= 0.0 {
                0u8
            } else if value >= 255.0 {
                255u8
            } else {
                let f = value.floor();
                let frac = value - f;
                let rounded = if frac > 0.5 {
                    f + 1.0
                } else if frac < 0.5 {
                    f
                } else if f % 2.0 == 0.0 {
                    f // round half to even
                } else {
                    f + 1.0
                };
                rounded as u8
            };
            *base.add(off) = byte;
        }
        KIND_INT16 => {
            let v = value as i32 as i16;
            *(base.add(off) as *mut i16) = v;
        }
        KIND_UINT16 => {
            let mut v = value as i64;
            v = v.rem_euclid(65536);
            *(base.add(off) as *mut u16) = v as u16;
        }
        KIND_INT32 => {
            let v = value as i32;
            *(base.add(off) as *mut i32) = v;
        }
        KIND_UINT32 => {
            let v = value as i64 as u32;
            *(base.add(off) as *mut u32) = v;
        }
        KIND_FLOAT32 => {
            *(base.add(off) as *mut f32) = value as f32;
        }
        KIND_FLOAT64 => {
            *(base.add(off) as *mut f64) = value;
        }
        _ => {}
    }
}

/// Load a slot, returning a plain f64 (numeric, not NaN-boxed).
unsafe fn load_at(ta: *const TypedArrayHeader, idx: usize) -> f64 {
    let kind = (*ta).kind;
    let elem_size = (*ta).elem_size as usize;
    let base = data_ptr(ta);
    let off = idx * elem_size;
    match kind {
        KIND_INT8 => *(base.add(off) as *const i8) as f64,
        KIND_UINT8 | KIND_UINT8_CLAMPED => *base.add(off) as f64,
        KIND_INT16 => *(base.add(off) as *const i16) as f64,
        KIND_UINT16 => *(base.add(off) as *const u16) as f64,
        KIND_INT32 => *(base.add(off) as *const i32) as f64,
        KIND_UINT32 => *(base.add(off) as *const u32) as f64,
        KIND_FLOAT32 => *(base.add(off) as *const f32) as f64,
        KIND_FLOAT64 => *(base.add(off) as *const f64),
        _ => 0.0,
    }
}

// ---------- FFI ----------

/// Allocate a typed array of `length` elements, all zero.
#[no_mangle]
pub extern "C" fn js_typed_array_new_empty(kind: i32, length: i32) -> *mut TypedArrayHeader {
    typed_array_alloc(kind as u8, length.max(0) as u32)
}

/// Allocate a typed array from a NaN-boxed JS value. Dispatches at runtime:
/// - POINTER_TAG (0x7FFD) → create from the pointed-to array's elements
/// - INT32_TAG  (0x7FFE) → use the tagged integer as the element count
/// - plain f64 / NaN    → use the numeric value as the element count
/// - anything else      → empty typed array
///
/// Mirrors `js_uint8array_new` for the generic typed-array constructor path.
/// Used when the codegen cannot determine at compile time whether the single
/// constructor argument is a length or a source array.
#[no_mangle]
pub extern "C" fn js_typed_array_new(kind: i32, val: f64) -> *mut TypedArrayHeader {
    let bits = val.to_bits();
    let top16 = (bits >> 48) as u16;
    if top16 == 0x7FFD {
        // POINTER_TAG — existing array pointer; copy its elements.
        let arr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        // Issue #654: a NaN-boxed pointer can also point at a registered
        // typed array (e.g. when the source flowed through a path that
        // re-applied POINTER_TAG). Detect via the registry and copy
        // through `typed_array_to_typed_array` so element values stay
        // numeric instead of being read as f64-NaN-boxed bits.
        let raw_addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if lookup_typed_array_kind(raw_addr).is_some() {
            return typed_array_copy_from_typed_array(
                kind as u8,
                raw_addr as *const TypedArrayHeader,
            );
        }
        return js_typed_array_new_from_array(kind, arr);
    }
    if top16 == 0x7FFE {
        // INT32_TAG — lower 32 bits are the signed length.
        let n = (bits & 0xFFFF_FFFF) as i32;
        return typed_array_alloc(kind as u8, n.max(0) as u32);
    }
    if !(0x7FFC..=0x7FFF).contains(&top16) {
        // Issue #654: typed-array sources (`new Float64Array(otherTA)`)
        // arrive as raw `i64 → f64` bitcasts (no NaN-box tag) per the
        // typed-array constructor codegen. Without this arm the address
        // was treated as a numeric length and the result was an empty
        // array. Detect via the registry first; only fall back to the
        // numeric-length interpretation for genuine doubles.
        if top16 == 0 && bits >= 0x10000 {
            let addr = bits as usize;
            if lookup_typed_array_kind(addr).is_some() {
                return typed_array_copy_from_typed_array(
                    kind as u8,
                    addr as *const TypedArrayHeader,
                );
            }
        }
        // Plain IEEE double (including negative, NaN, ±Inf).
        let len = if val.is_finite() && val >= 0.0 {
            val as i32
        } else {
            0
        };
        return typed_array_alloc(kind as u8, len.max(0) as u32);
    }
    // Undefined / null / bool / string → empty typed array.
    typed_array_alloc(kind as u8, 0)
}

/// Copy elements from one typed array into a new typed array of `dst_kind`,
/// reading via `load_at` (so source-element semantics stay correct) and
/// writing via `store_at` (which clamps / truncates / sign-extends per
/// `dst_kind`). Used by both `js_typed_array_new` (constructor copy) and
/// `js_typed_array_new_from_array` when it discovers the source is a
/// typed array rather than an `ArrayHeader`.
fn typed_array_copy_from_typed_array(
    dst_kind: u8,
    src: *const TypedArrayHeader,
) -> *mut TypedArrayHeader {
    let src = clean_ta_ptr(src);
    if src.is_null() {
        return typed_array_alloc(dst_kind, 0);
    }
    unsafe {
        let len = (*src).length;
        let out = typed_array_alloc(dst_kind, len);
        for i in 0..len as usize {
            let v = load_at(src, i);
            store_at(out, i, v);
        }
        out
    }
}

/// Allocate a typed array from a Perry array (each element coerced to the
/// per-kind numeric type).
#[no_mangle]
pub extern "C" fn js_typed_array_new_from_array(
    kind: i32,
    arr: *const ArrayHeader,
) -> *mut TypedArrayHeader {
    let kind = kind as u8;
    // Strip NaN-box from the array pointer if needed.
    let arr = {
        let bits = arr as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr
        }
    };
    if arr.is_null() || (arr as usize) < 0x1000 {
        return typed_array_alloc(kind, 0);
    }
    // Issue #654: caller may have handed us a typed-array pointer
    // misaddressed as `*const ArrayHeader`. The two headers differ in
    // layout, so reading element data as raw f64 produces garbage.
    // Detect via the registry and route through the typed-array copy.
    if lookup_typed_array_kind(arr as usize).is_some() {
        return typed_array_copy_from_typed_array(kind, arr as *const TypedArrayHeader);
    }
    unsafe {
        let len = (*arr).length;
        let ta = typed_array_alloc(kind, len);
        let arr_data = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..len as usize {
            let raw = *arr_data.add(i);
            store_at(ta, i, jsvalue_to_f64(raw));
        }
        ta
    }
}

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
        store_at(ta, index as usize, jsvalue_to_f64(value));
    }
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

/// Materialize a typed array as a regular Array of f64s. Each element is
/// loaded via the per-kind accessor (`load_at`) so `Uint8Array([10,20,30,40])`
/// becomes `Array[10.0, 20.0, 30.0, 40.0]` rather than four raw NaN-box-bit
/// reinterpretations of the byte buffer. Issue #578.
///
/// Used by `js_array_clone` (Array.from / for-of materialize), `js_array_concat`
/// (`[...typedArray]` spread + `concat`), and any other path that bridges
/// from typed-array storage into a normal Array.
pub fn typed_array_to_array(ta: *const TypedArrayHeader) -> *mut crate::array::ArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let len = (*ta).length as usize;
        let result = crate::array::js_array_alloc(len as u32);
        if len == 0 {
            return result;
        }
        let dst =
            (result as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut f64;
        for i in 0..len {
            *dst.add(i) = load_at(ta, i);
        }
        (*result).length = len as u32;
        result
    }
}

/// `ta.toReversed()` — new typed array of same kind with reversed elements.
#[no_mangle]
pub extern "C" fn js_typed_array_to_reversed(ta: *const TypedArrayHeader) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let out = typed_array_alloc(kind, len as u32);
        for i in 0..len {
            let v = load_at(ta, len - 1 - i);
            store_at(out, i, v);
        }
        out
    }
}

/// `ta.sort()` — default ascending numeric sort, **in place**. Per the
/// JS spec, the same typed-array reference is returned. Issue #654.
#[no_mangle]
pub extern "C" fn js_typed_array_sort_default(ta: *mut TypedArrayHeader) -> *mut TypedArrayHeader {
    let ta_clean = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta_clean.is_null() {
        return ta_clean;
    }
    unsafe {
        let len = (*ta_clean).length as usize;
        if len <= 1 {
            return ta_clean;
        }
        let mut buf: Vec<f64> = (0..len).map(|i| load_at(ta_clean, i)).collect();
        buf.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for (i, v) in buf.into_iter().enumerate() {
            store_at(ta_clean, i, v);
        }
        ta_clean
    }
}

/// `ta.sort(cmp)` — in-place sort with comparator. Issue #654.
#[no_mangle]
pub extern "C" fn js_typed_array_sort_with_comparator(
    ta: *mut TypedArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    let ta_clean = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta_clean.is_null() {
        return ta_clean;
    }
    unsafe {
        let len = (*ta_clean).length as usize;
        if len <= 1 {
            return ta_clean;
        }
        let mut buf: Vec<f64> = (0..len).map(|i| load_at(ta_clean, i)).collect();
        buf.sort_by(|a, b| {
            let r = crate::closure::js_closure_call2(comparator, *a, *b);
            if r < 0.0 {
                std::cmp::Ordering::Less
            } else if r > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        for (i, v) in buf.into_iter().enumerate() {
            store_at(ta_clean, i, v);
        }
        ta_clean
    }
}

/// `ta.toSorted()` — default ascending numeric sort.
#[no_mangle]
pub extern "C" fn js_typed_array_to_sorted_default(
    ta: *const TypedArrayHeader,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let out = typed_array_alloc(kind, len as u32);
        // Materialize values, sort, store back.
        let mut buf: Vec<f64> = (0..len).map(|i| load_at(ta, i)).collect();
        buf.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for (i, v) in buf.into_iter().enumerate() {
            store_at(out, i, v);
        }
        out
    }
}

/// `ta.toSorted(cmp)`.
#[no_mangle]
pub extern "C" fn js_typed_array_to_sorted_with_comparator(
    ta: *const TypedArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let mut buf: Vec<f64> = (0..len).map(|i| load_at(ta, i)).collect();
        buf.sort_by(|a, b| {
            let r = crate::closure::js_closure_call2(comparator, *a, *b);
            if r < 0.0 {
                std::cmp::Ordering::Less
            } else if r > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        let out = typed_array_alloc(kind, len as u32);
        for (i, v) in buf.into_iter().enumerate() {
            store_at(out, i, v);
        }
        out
    }
}

/// `ta.with(index, value)` — return new array with single element replaced.
#[no_mangle]
pub extern "C" fn js_typed_array_with(
    ta: *const TypedArrayHeader,
    index: f64,
    value: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let out = typed_array_alloc(kind, len as u32);
        let mut idx = index as i64;
        if idx < 0 {
            idx += len as i64;
        }
        for i in 0..len {
            if i as i64 == idx {
                store_at(out, i, jsvalue_to_f64(value));
            } else {
                store_at(out, i, load_at(ta, i));
            }
        }
        out
    }
}

/// `ta.findLast(cb)`. Returns the matched element as a plain f64
/// (NOT NaN-boxed), or NaN-boxed undefined if none match.
#[no_mangle]
pub extern "C" fn js_typed_array_find_last(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let len = (*ta).length as usize;
        for i in (0..len).rev() {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call2(callback, v, i as f64);
            if crate::value::js_is_truthy(r) != 0 {
                return v;
            }
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// `ta.findLastIndex(cb)`. Returns plain f64 index, or -1.
#[no_mangle]
pub extern "C" fn js_typed_array_find_last_index(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return -1.0;
    }
    unsafe {
        let len = (*ta).length as usize;
        for i in (0..len).rev() {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call2(callback, v, i as f64);
            if crate::value::js_is_truthy(r) != 0 {
                return i as f64;
            }
        }
        -1.0
    }
}

/// True for any name that is a method on `%TypedArray%.prototype`. The
/// field-by-name resolver (#2061) uses this to bind a callable closure for
/// `int8arr.copyWithin` (so `typeof int8arr.copyWithin === "function"` and
/// the call routes through `js_native_call_method`) instead of coercing the
/// key to an element index. Accessor properties (`length`, `byteLength`,
/// `byteOffset`, `BYTES_PER_ELEMENT`, `buffer`, `constructor`) are handled
/// separately by the caller and are intentionally NOT listed here.
pub fn is_typed_array_method_name(name: &str) -> bool {
    matches!(
        name,
        "at" | "copyWithin"
            | "entries"
            | "every"
            | "fill"
            | "filter"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "forEach"
            | "includes"
            | "indexOf"
            | "join"
            | "keys"
            | "lastIndexOf"
            | "map"
            | "reduce"
            | "reduceRight"
            | "reverse"
            | "set"
            | "slice"
            | "some"
            | "sort"
            | "subarray"
            | "toLocaleString"
            | "toReversed"
            | "toSorted"
            | "toString"
            | "values"
    )
}

/// Normalize a relative index argument: `ToIntegerOrInfinity` then clamp to
/// `[0, len]`. `NaN` → 0; negatives count back from the end. Saturating
/// arithmetic keeps ±Infinity and out-of-range finite values in bounds.
#[inline]
fn norm_rel_index(idx: f64, len: usize) -> usize {
    let len_i = len as i64;
    let mut i = if idx.is_nan() {
        0
    } else if idx == f64::INFINITY {
        len_i
    } else if idx == f64::NEG_INFINITY {
        0
    } else {
        idx as i64
    };
    if i < 0 {
        i = i.saturating_add(len_i);
    }
    i.clamp(0, len_i) as usize
}

/// Interpret a search value (for `indexOf`/`lastIndexOf`/`includes`) as the
/// JS number it represents, or `None` if it isn't a number — typed-array
/// elements are always numbers, so a non-number target can never match under
/// strict equality (`indexOf`) / SameValueZero (`includes`).
#[inline]
fn search_as_number(v: f64) -> Option<f64> {
    let bits = v.to_bits();
    let top16 = bits >> 48;
    // INT32-tagged small integers are numbers.
    if top16 == 0x7FFE {
        return Some(((bits & 0xFFFF_FFFF) as i32) as f64);
    }
    // Any other NaN-box tag (undefined/null/bool/string/pointer/bigint and
    // SSO short-strings) is a non-number.
    if (0x7FFA..0x8000).contains(&top16) || top16 == 0x7FF9 {
        return None;
    }
    // Plain double (incl. negatives and canonical NaN).
    Some(v)
}

/// `ta.fill(value, start, end)` — in place, returns `ta`. `start`/`end` are
/// pre-resolved by the caller (absent `start` → 0, absent `end` → length).
#[no_mangle]
pub extern "C" fn js_typed_array_fill(
    ta: *mut TypedArrayHeader,
    value: f64,
    start: f64,
    end: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() {
        return ta;
    }
    unsafe {
        let len = (*ta).length as usize;
        let s = norm_rel_index(start, len);
        let e = norm_rel_index(end, len);
        let v = jsvalue_to_f64(value);
        for i in s..e {
            store_at(ta, i, v);
        }
        ta
    }
}

/// `ta.copyWithin(target, start, end)` — in place, returns `ta`. Copies the
/// sequence `[start, end)` to `target`, handling overlap via a temporary.
#[no_mangle]
pub extern "C" fn js_typed_array_copy_within(
    ta: *mut TypedArrayHeader,
    target: f64,
    start: f64,
    end: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() {
        return ta;
    }
    unsafe {
        let len = (*ta).length as usize;
        let t = norm_rel_index(target, len);
        let s = norm_rel_index(start, len);
        let e = norm_rel_index(end, len);
        let count = e.saturating_sub(s).min(len.saturating_sub(t));
        if count > 0 {
            let tmp: Vec<f64> = (0..count).map(|k| load_at(ta, s + k)).collect();
            for (k, v) in tmp.into_iter().enumerate() {
                store_at(ta, t + k, v);
            }
        }
        ta
    }
}

/// `ta.slice(start, end)` — new typed array of the same kind with the
/// `[start, end)` elements copied out.
#[no_mangle]
pub extern "C" fn js_typed_array_slice(
    ta: *const TypedArrayHeader,
    start: f64,
    end: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let s = norm_rel_index(start, len);
        let e = norm_rel_index(end, len);
        let count = e.saturating_sub(s);
        let out = typed_array_alloc(kind, count as u32);
        for k in 0..count {
            store_at(out, k, load_at(ta, s + k));
        }
        out
    }
}

/// `ta.subarray(start, end)`. Node returns a *view* sharing the backing
/// buffer; Perry typed arrays own their storage, so this returns an
/// independent copy (mutations through the result do not write back to the
/// source). Full shared-buffer view semantics are tracked by the
/// %TypedArray%.prototype reification follow-up (#793).
#[no_mangle]
pub extern "C" fn js_typed_array_subarray(
    ta: *const TypedArrayHeader,
    start: f64,
    end: f64,
) -> *mut TypedArrayHeader {
    js_typed_array_slice(ta, start, end)
}

/// `ta.indexOf(value, fromIndex)` — strict-equality forward search. `from`
/// is pre-resolved (absent → 0). Returns the index or -1.
#[no_mangle]
pub extern "C" fn js_typed_array_index_of(
    ta: *const TypedArrayHeader,
    value: f64,
    from: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return -1.0;
    }
    let Some(target) = search_as_number(value) else {
        return -1.0;
    };
    unsafe {
        let len = (*ta).length as usize;
        let start = norm_rel_index(from, len);
        for i in start..len {
            // `==` never matches NaN, matching indexOf semantics.
            if load_at(ta, i) == target {
                return i as f64;
            }
        }
        -1.0
    }
}

/// `ta.lastIndexOf(value, fromIndex)` — strict-equality backward search.
/// `from` is pre-resolved (absent → +Infinity sentinel). Returns index or -1.
#[no_mangle]
pub extern "C" fn js_typed_array_last_index_of(
    ta: *const TypedArrayHeader,
    value: f64,
    from: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return -1.0;
    }
    let Some(target) = search_as_number(value) else {
        return -1.0;
    };
    unsafe {
        let len = (*ta).length as i64;
        if len == 0 {
            return -1.0;
        }
        let mut start = if from == f64::INFINITY {
            len - 1
        } else if from.is_nan() {
            0
        } else if from == f64::NEG_INFINITY {
            return -1.0;
        } else {
            from as i64
        };
        if start < 0 {
            start += len;
        }
        if start >= len {
            start = len - 1;
        }
        let mut i = start;
        while i >= 0 {
            if load_at(ta, i as usize) == target {
                return i as f64;
            }
            i -= 1;
        }
        -1.0
    }
}

/// `ta.includes(value, fromIndex)` — SameValueZero search (NaN matches NaN).
/// Returns a NaN-boxed JS boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_includes(
    ta: *const TypedArrayHeader,
    value: f64,
    from: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    let mut found = false;
    if !ta.is_null() {
        let target = search_as_number(value);
        unsafe {
            let len = (*ta).length as usize;
            let start = norm_rel_index(from, len);
            for i in start..len {
                let e = load_at(ta, i);
                match target {
                    Some(t) if t.is_nan() => {
                        if e.is_nan() {
                            found = true;
                            break;
                        }
                    }
                    Some(t) => {
                        if e == t {
                            found = true;
                            break;
                        }
                    }
                    None => {}
                }
            }
        }
    }
    f64::from_bits(crate::value::JSValue::bool(found).bits())
}

/// `ta.reverse()` — reverse in place, returns `ta`.
#[no_mangle]
pub extern "C" fn js_typed_array_reverse(ta: *mut TypedArrayHeader) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() {
        return ta;
    }
    unsafe {
        let len = (*ta).length as usize;
        for i in 0..len / 2 {
            let a = load_at(ta, i);
            let b = load_at(ta, len - 1 - i);
            store_at(ta, i, b);
            store_at(ta, len - 1 - i, a);
        }
        ta
    }
}

/// `ta.join(separator)` — `separator` is the raw JS arg (TAG_UNDEFINED →
/// default `","`). Each element is rendered like `String(number)`.
#[no_mangle]
pub extern "C" fn js_typed_array_join(
    ta: *const TypedArrayHeader,
    separator: f64,
) -> *mut crate::string::StringHeader {
    let ta = clean_ta_ptr(ta);
    let sep = if separator.to_bits() == crate::value::TAG_UNDEFINED {
        ",".to_string()
    } else {
        let s_ptr = crate::builtins::js_string_coerce(separator);
        if s_ptr.is_null() {
            String::new()
        } else {
            unsafe {
                let bytes =
                    (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let len = (*s_ptr).byte_len as usize;
                String::from_utf8_lossy(std::slice::from_raw_parts(bytes, len)).into_owned()
            }
        }
    };
    let mut out = String::new();
    if !ta.is_null() {
        unsafe {
            let kind = (*ta).kind;
            let len = (*ta).length as usize;
            for i in 0..len {
                if i > 0 {
                    out.push_str(&sep);
                }
                out.push_str(&format_typed_value(kind, load_at(ta, i)));
            }
        }
    }
    crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32)
}

/// `ta.set(source, offset)` — copy elements from an Array or typed-array
/// `source` into `ta` starting at `offset` (pre-resolved, absent → 0).
/// Out-of-range writes are skipped. Returns undefined.
#[no_mangle]
pub extern "C" fn js_typed_array_set_from(
    ta: *mut TypedArrayHeader,
    source: f64,
    offset: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let off = if offset.is_nan() || offset < 0.0 {
        0usize
    } else {
        offset as usize
    };
    let src_addr = strip_nanbox(source.to_bits());
    if src_addr < 0x1000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let dst_len = (*ta).length as usize;
        if let Some(_k) = lookup_typed_array_kind(src_addr) {
            let src = src_addr as *const TypedArrayHeader;
            let slen = (*src).length as usize;
            for i in 0..slen {
                if off + i < dst_len {
                    store_at(ta, off + i, load_at(src, i));
                }
            }
        } else {
            let arr = src_addr as *const ArrayHeader;
            let slen = crate::array::js_array_length(arr) as usize;
            for i in 0..slen {
                if off + i < dst_len {
                    let v = crate::array::js_array_get_f64(arr, i as u32);
                    store_at(ta, off + i, jsvalue_to_f64(v));
                }
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// NaN-boxed (POINTER_TAG-free) receiver for passing `ta` as the 3rd
/// callback argument `(value, index, array)`. Typed arrays flow as raw
/// f64-bitcast pointers, so this is just `from_bits`.
#[inline]
fn ta_as_callback_arg(ta: *const TypedArrayHeader) -> f64 {
    f64::from_bits(ta as u64)
}

/// `ta.forEach(cb)` — returns undefined.
#[no_mangle]
pub extern "C" fn js_typed_array_for_each(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if !ta.is_null() {
        unsafe {
            let len = (*ta).length as usize;
            let arr = ta_as_callback_arg(ta);
            for i in 0..len {
                crate::closure::js_closure_call3(callback, load_at(ta, i), i as f64, arr);
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `ta.map(cb)` — new typed array of the same kind.
#[no_mangle]
pub extern "C" fn js_typed_array_map(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let arr = ta_as_callback_arg(ta);
        let out = typed_array_alloc(kind, len as u32);
        for i in 0..len {
            let r = crate::closure::js_closure_call3(callback, load_at(ta, i), i as f64, arr);
            store_at(out, i, jsvalue_to_f64(r));
        }
        out
    }
}

/// `ta.filter(cb)` — new typed array of the same kind holding the elements
/// for which `cb` returned truthy.
#[no_mangle]
pub extern "C" fn js_typed_array_filter(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let arr = ta_as_callback_arg(ta);
        let mut kept: Vec<f64> = Vec::new();
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, arr);
            if crate::value::js_is_truthy(r) != 0 {
                kept.push(v);
            }
        }
        let out = typed_array_alloc(kind, kept.len() as u32);
        for (i, v) in kept.into_iter().enumerate() {
            store_at(out, i, v);
        }
        out
    }
}

/// `ta.some(cb)` — NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_some(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    let mut result = false;
    if !ta.is_null() {
        unsafe {
            let len = (*ta).length as usize;
            let arr = ta_as_callback_arg(ta);
            for i in 0..len {
                let r = crate::closure::js_closure_call3(callback, load_at(ta, i), i as f64, arr);
                if crate::value::js_is_truthy(r) != 0 {
                    result = true;
                    break;
                }
            }
        }
    }
    f64::from_bits(crate::value::JSValue::bool(result).bits())
}

/// `ta.every(cb)` — NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_every(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    let mut result = true;
    if !ta.is_null() {
        unsafe {
            let len = (*ta).length as usize;
            let arr = ta_as_callback_arg(ta);
            for i in 0..len {
                let r = crate::closure::js_closure_call3(callback, load_at(ta, i), i as f64, arr);
                if crate::value::js_is_truthy(r) == 0 {
                    result = false;
                    break;
                }
            }
        }
    }
    f64::from_bits(crate::value::JSValue::bool(result).bits())
}

/// `ta.find(cb)` — the matched element (plain f64) or undefined.
#[no_mangle]
pub extern "C" fn js_typed_array_find(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if !ta.is_null() {
        unsafe {
            let len = (*ta).length as usize;
            let arr = ta_as_callback_arg(ta);
            for i in 0..len {
                let v = load_at(ta, i);
                let r = crate::closure::js_closure_call3(callback, v, i as f64, arr);
                if crate::value::js_is_truthy(r) != 0 {
                    return v;
                }
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `ta.findIndex(cb)` — matched index (plain f64) or -1.
#[no_mangle]
pub extern "C" fn js_typed_array_find_index(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if !ta.is_null() {
        unsafe {
            let len = (*ta).length as usize;
            let arr = ta_as_callback_arg(ta);
            for i in 0..len {
                let r = crate::closure::js_closure_call3(callback, load_at(ta, i), i as f64, arr);
                if crate::value::js_is_truthy(r) != 0 {
                    return i as f64;
                }
            }
        }
    }
    -1.0
}

/// `ta.reduce(cb, initialValue?)`. `has_init` is 0 when no initial value was
/// supplied. Empty array with no initial value returns undefined (Node
/// throws a TypeError; this is the documented divergence).
#[no_mangle]
pub extern "C" fn js_typed_array_reduce(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    init: f64,
    has_init: i32,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let len = (*ta).length as usize;
        let arr = ta_as_callback_arg(ta);
        let mut acc;
        let mut start;
        if has_init != 0 {
            acc = init;
            start = 0;
        } else {
            if len == 0 {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            acc = load_at(ta, 0);
            start = 1;
        }
        while start < len {
            acc = crate::closure::js_closure_call4(
                callback,
                acc,
                load_at(ta, start),
                start as f64,
                arr,
            );
            start += 1;
        }
        acc
    }
}

/// `ta.reduceRight(cb, initialValue?)`.
#[no_mangle]
pub extern "C" fn js_typed_array_reduce_right(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    init: f64,
    has_init: i32,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let len = (*ta).length as usize;
        let arr = ta_as_callback_arg(ta);
        let mut acc;
        let mut i: i64;
        if has_init != 0 {
            acc = init;
            i = len as i64 - 1;
        } else {
            if len == 0 {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            acc = load_at(ta, len - 1);
            i = len as i64 - 2;
        }
        while i >= 0 {
            acc = crate::closure::js_closure_call4(
                callback,
                acc,
                load_at(ta, i as usize),
                i as f64,
                arr,
            );
            i -= 1;
        }
        acc
    }
}

/// `ta[key]` where `key` has an unknown static type (#2061). Codegen routes
/// the ambiguous-index case here so a string method-name key resolves a
/// prototype method/accessor (via `js_object_get_field_by_name_f64`) while a
/// numeric key still reads an element. Without this, codegen `fptosi`'d the
/// NaN-boxed string key to 0 and returned element 0 (a number), so
/// `int8arr[m]` where `m` held `"copyWithin"` reported `typeof === "number"`.
#[no_mangle]
pub extern "C" fn js_typed_array_member_get(ta: *const TypedArrayHeader, index: f64) -> f64 {
    let idx_bits = index.to_bits();
    let top16 = idx_bits >> 48;
    // STRING_TAG (0x7FFF) or SSO short-string (0x7FF9) → property/method.
    if top16 == 0x7FFF || top16 == 0x7FF9 {
        let key_ptr = crate::value::js_get_string_pointer_unified(index)
            as *const crate::string::StringHeader;
        if key_ptr.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        return crate::object::js_object_get_field_by_name_f64(
            ta as *const crate::object::ObjectHeader,
            key_ptr,
        );
    }
    // Numeric key → element read (per-kind; OOB → undefined).
    if index.is_nan() || index.is_infinite() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    js_typed_array_get(ta, index as i32)
}

/// Format a typed array Node-style: `Int32Array(N) [ a, b, c ]`. Used by
/// `format_jsvalue` in builtins.rs.
pub fn format_typed_array(ta: *const TypedArrayHeader) -> String {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return "TypedArray(0) []".to_string();
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let name = name_for_kind(kind);
        if len == 0 {
            return format!("{}(0) []", name);
        }
        let mut s = format!("{}({}) [", name, len);
        for i in 0..len {
            if i == 0 {
                s.push(' ');
            } else {
                s.push_str(", ");
            }
            let v = load_at(ta, i);
            s.push_str(&format_typed_value(kind, v));
        }
        s.push_str(" ]");
        s
    }
}

fn format_typed_value(kind: u8, v: f64) -> String {
    match kind {
        KIND_FLOAT32 | KIND_FLOAT64 => {
            // Match Node: integer-valued floats render with no decimal,
            // others render via Rust's default Debug for f64.
            if v.is_nan() {
                "NaN".to_string()
            } else if v.is_infinite() {
                if v > 0.0 {
                    "Infinity".to_string()
                } else {
                    "-Infinity".to_string()
                }
            } else if v == v.trunc() && v.abs() < 1e16 {
                format!("{}", v as i64)
            } else {
                // Use Rust's default short formatting.
                let s = format!("{}", v);
                s
            }
        }
        _ => {
            // Integer types
            format!("{}", v as i64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_object_typed_array_alloc_uses_old_gc_header_and_stays_usable() {
        let ta = typed_array_alloc(KIND_UINT8, crate::gc::LARGE_OBJECT_THRESHOLD_BYTES as u32);
        assert!(!ta.is_null());
        assert_eq!(lookup_typed_array_kind(ta as usize), Some(KIND_UINT8));
        assert!(crate::arena::pointer_in_old_gen(ta as usize));
        unsafe {
            let header =
                (ta as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            assert_eq!((*header).obj_type, crate::gc::GC_TYPE_TYPED_ARRAY);
            assert_ne!((*header).gc_flags & crate::gc::GC_FLAG_TENURED, 0);
        }

        js_typed_array_set(ta, 0, 17.0);
        js_typed_array_set(ta, crate::gc::LARGE_OBJECT_THRESHOLD_BYTES as i32 - 1, 99.0);
        assert_eq!(js_typed_array_get(ta, 0), 17.0);
        assert_eq!(
            js_typed_array_get(ta, crate::gc::LARGE_OBJECT_THRESHOLD_BYTES as i32 - 1),
            99.0
        );
    }
}
