//! TypedArray support for all JS typed-array element kinds.
//!
//! TypedArrayHeader is the shared 16-byte byte cell. The GC type byte carries
//! element kind and owner/view role. Reads resolve the current owner store;
//! immutable methods allocate a same-kind owner with retained source roots.
//! NaN-boxed pointers identify the cell whose GC header sits at p-8.

use std::ptr;

use crate::typedarray_half::{f16_bits_to_f64, f64_to_f16_bits};

pub(crate) mod bigint;
mod format;
pub(crate) mod species;
pub use format::format_typed_array;

mod access;
mod construct;
#[cfg(perry_gc_instruments)]
mod element_contract;
/// #8100: the element READ helpers against a receiver the static typed-array
/// hint no longer describes — the shape codegen emits for a reassigned local.
#[cfg(test)]
mod element_read_receiver_tests;
mod iterate;
#[cfg(test)]
mod resolved_read_tests;
pub(crate) use crate::byte_cell_support::shared_access;
pub(crate) use shared_access::copy_lane as copy_shared_lane;
mod slice_ops;
#[cfg(test)]
mod thread_exit_tests;
mod transform;

// `#[no_mangle] pub extern "C"` FFI entry points are compiled regardless (the
// `mod` declarations above pull them in), but Rust code in OTHER modules reaches
// many of them through the `crate::typedarray::js_...` path, so re-export every
// such item by name to keep those paths resolving. Inherent/no-path-referenced
// items need no re-export.
pub use access::{
    bigint_lane_bits, js_typed_array_at, js_typed_array_copy_within, js_typed_array_get,
    js_typed_array_index_get_dynamic, js_typed_array_length, js_typed_array_set,
    js_typed_array_set_from, js_uint8array_get, js_uint8array_set, set_bigint_lane_bits,
};
pub(crate) use construct::typed_array_from_source_raw_values;
pub use construct::{js_typed_array_new, js_typed_array_new_empty, js_typed_array_new_from_array};
pub use iterate::{
    js_typed_array_every, js_typed_array_filter, js_typed_array_find, js_typed_array_find_index,
    js_typed_array_for_each, js_typed_array_map, js_typed_array_reduce,
    js_typed_array_reduce_right, js_typed_array_some,
};
pub use slice_ops::{
    js_typed_array_fill, js_typed_array_join, js_typed_array_join_value, js_typed_array_reverse,
    js_typed_array_slice, js_typed_array_subarray,
};
pub use transform::{
    js_typed_array_find_last, js_typed_array_find_last_index, js_typed_array_sort_default,
    js_typed_array_sort_with_comparator, js_typed_array_to_reversed,
    js_typed_array_to_sorted_default, js_typed_array_to_sorted_with_comparator,
    js_typed_array_with, typed_array_to_array,
};

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
pub const KIND_BIGINT64: u8 = 9;
pub const KIND_BIGUINT64: u8 = 10;
/// Float16Array (#2902): IEEE-754 binary16 (half-precision) 2-byte elements.
/// Stored as `u16` bit patterns; converted to/from f64 on read/write.
pub const KIND_FLOAT16: u8 = 11;

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
pub const CLASS_ID_BIGINT64_ARRAY: u32 = 0xFFFF0039;
pub const CLASS_ID_BIGUINT64_ARRAY: u32 = 0xFFFF003A;
pub const CLASS_ID_FLOAT16_ARRAY: u32 = 0xFFFF003B;

/// Buffer, typed arrays and views share the same sixteen-byte cell.
pub type TypedArrayHeader = crate::buffer::BufferHeader;

/// Access uses the common cell header and current owner storage. Any derived
/// data address is consumed without collection, or retained with its owner.
#[no_mangle]
pub extern "C" fn js_typed_array_masked_window_data_ptr(receiver: f64) -> i64 {
    let addr = strip_nanbox(receiver.to_bits());
    if !crate::value::addr_class::is_plausible_heap_addr(addr)
        || lookup_typed_array_kind(addr).is_none()
    {
        return 0;
    }
    data_ptr(addr as *const TypedArrayHeader) as i64
}

/// Validate a typed-array constructor length argument with `ToIndex`
/// semantics (#3662). Node truncates toward zero (`NaN` → 0, `2.5` → 2) and
/// throws a plain `RangeError: Invalid typed array length: <n>` when the
/// resulting integer is negative or exceeds `2**53 - 1` (`Infinity` included).
/// Returns the validated element count.
#[inline]
pub(crate) fn typed_array_length_or_throw(val: f64) -> u32 {
    let integer = if val.is_nan() { 0.0 } else { val.trunc() };
    if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
        // Node reports the ORIGINAL argument, not the truncated integer
        // (`new Int32Array(-1.5)` → "Invalid typed array length: -1.5"), with
        // integral values shown without a decimal point (#3146).
        let shown = if val.is_infinite() {
            if val > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
        } else if val.fract() == 0.0 && val.abs() < (i64::MAX as f64) {
            format!("{}", val as i64)
        } else {
            format!("{val}")
        };
        throw_range_error(format!("Invalid typed array length: {shown}").as_bytes());
    }
    // #5067 — Perry stores the element count in a `u32` capacity field, so a
    // length above the cap cannot be represented (and the backing block could
    // never be allocated anyway). Node passes the `<= 2**53-1` length check
    // for these and then fails the actual allocation, so match its
    // `RangeError: Array buffer allocation failed` rather than silently
    // saturating the cast (which produced a wrong-size array or aborted the
    // process in the allocator).
    //
    // RULE 3 (single-path object model): the cap is `i32::MAX`, not
    // `u32::MAX`, because `TypedArrayHeader::capacity` sits at payload `+4` —
    // the word the emitted read path loads as a ShapeId. `new Int8Array(2**31)`
    // wrote `capacity = 0x8000_0000`, which is not merely inside the ShapeId
    // range but is the FIRST id the process ever mints. This lowers no
    // documented maximum: `new Uint8Array(n)`, `new ArrayBuffer(n)` and
    // `Buffer.alloc(n)` already stop at `i32::MAX`
    // (`buffer/from.rs`), so this only makes the remaining element types
    // agree with their siblings.
    if integer > i32::MAX as f64 {
        throw_range_error(b"Array buffer allocation failed");
    }
    integer as u32
}

unsafe fn typed_array_raw_bytes(ta: *const TypedArrayHeader) -> (*const u8, usize) {
    let data = data_ptr(ta);
    let len = (crate::typedarray::element_length(ta) as usize).saturating_mul(
        crate::typedarray::elem_size_for_kind(crate::typedarray::element_kind(ta)) as usize,
    );
    if len == 0 {
        return (ptr::NonNull::<u8>::dangling().as_ptr(), 0);
    }
    if data.is_null() {
        throw_type_error(b"Expected typed array");
    }
    (data, len)
}

unsafe fn typed_array_raw_bytes_mut(ta: *mut TypedArrayHeader) -> (*mut u8, usize) {
    let data = data_ptr_mut(ta);
    let len = (crate::typedarray::element_length(ta) as usize).saturating_mul(
        crate::typedarray::elem_size_for_kind(crate::typedarray::element_kind(ta)) as usize,
    );
    if len == 0 {
        return (ptr::NonNull::<u8>::dangling().as_ptr(), 0);
    }
    if data.is_null() {
        throw_type_error(b"Expected typed array");
    }
    (data, len)
}

unsafe fn native_memory_copy_src_bytes(raw: u64) -> (*const u8, usize) {
    let addr = strip_nanbox(raw);
    if lookup_typed_array_kind(addr).is_some() {
        let ta =
            strict_typed_array_from_raw(raw, None, b"NativeMemory.copy expects typed array views");
        return typed_array_raw_bytes(ta);
    }
    if native_memory_copy_accepts_buffer(addr) {
        let buffer = addr as *const crate::buffer::BufferHeader;
        return (
            crate::buffer::store::data(buffer as usize),
            crate::typedarray::element_length(buffer) as usize,
        );
    }
    throw_type_error(b"NativeMemory.copy expects typed array views");
}

unsafe fn native_memory_copy_dst_bytes(raw: u64) -> (*mut u8, usize) {
    let addr = strip_nanbox(raw);
    if lookup_typed_array_kind(addr).is_some() {
        let ta =
            strict_typed_array_from_raw(raw, None, b"NativeMemory.copy expects typed array views");
        return typed_array_raw_bytes_mut(ta);
    }
    if native_memory_copy_accepts_buffer(addr) {
        let buffer = addr as *mut crate::buffer::BufferHeader;
        return (
            crate::buffer::store::data(buffer as usize),
            crate::typedarray::element_length(buffer) as usize,
        );
    }
    throw_type_error(b"NativeMemory.copy expects typed array views");
}

unsafe fn native_memory_copy_accepts_buffer(addr: usize) -> bool {
    // The brand is the cell's own type byte (#10694), so a Uint8Array answer
    // is already a header proof; there is no registry entry to forge.
    addr >= 0x1000 && crate::buffer::is_uint8array_buffer(addr)
}

/// All typed owners use the common byte-store entry and placement rule.
pub fn typed_array_alloc(kind: u8, length: u32) -> *mut TypedArrayHeader {
    crate::buffer::store::store_alloc(
        type_for_kind(kind),
        length,
        crate::buffer::store::Init::Zero,
    )
}

/// Store a number into the typed array slot, performing the per-kind cast.
pub(crate) unsafe fn store_at(ta: *mut TypedArrayHeader, idx: usize, value: f64) {
    let kind = crate::typedarray::element_kind(ta);
    let elem_size =
        crate::typedarray::elem_size_for_kind(crate::typedarray::element_kind(ta)) as usize;
    let base = data_ptr_mut(ta);
    let off = idx * elem_size;
    if typed_array_has_shared_backing(ta) {
        return shared_access::store(base.add(off), kind, value);
    }
    match kind {
        KIND_INT8 => {
            let v = to_uint32_bits(value) as u8 as i8;
            *(base.add(off) as *mut i8) = v;
        }
        KIND_UINT8 => {
            *base.add(off) = to_uint32_bits(value) as u8;
        }
        KIND_UINT8_CLAMPED => {
            let byte = to_uint8_clamp(value);
            *base.add(off) = byte;
        }
        KIND_INT16 => {
            let v = to_uint32_bits(value) as u16 as i16;
            *(base.add(off) as *mut i16) = v;
        }
        KIND_UINT16 => {
            *(base.add(off) as *mut u16) = to_uint32_bits(value) as u16;
        }
        KIND_INT32 => {
            let v = to_uint32_bits(value) as i32;
            *(base.add(off) as *mut i32) = v;
        }
        KIND_UINT32 => {
            *(base.add(off) as *mut u32) = to_uint32_bits(value);
        }
        KIND_FLOAT16 => {
            *(base.add(off) as *mut u16) = f64_to_f16_bits(value);
        }
        KIND_FLOAT32 => {
            *(base.add(off) as *mut f32) = value as f32;
        }
        KIND_FLOAT64 => {
            *(base.add(off) as *mut f64) = value;
        }
        KIND_BIGINT64 => {
            *(base.add(off) as *mut i64) = bigint::bigint_slot_bits(value) as i64;
        }
        KIND_BIGUINT64 => {
            *(base.add(off) as *mut u64) = bigint::bigint_slot_bits(value);
        }
        _ => {}
    }
}

/// Load a slot, returning a plain f64 (numeric, not NaN-boxed).
pub(crate) unsafe fn load_at(ta: *const TypedArrayHeader, idx: usize) -> f64 {
    let kind = crate::typedarray::element_kind(ta);
    let elem_size =
        crate::typedarray::elem_size_for_kind(crate::typedarray::element_kind(ta)) as usize;
    let base = data_ptr(ta);
    let off = idx * elem_size;
    if typed_array_has_shared_backing(ta) {
        return shared_access::load(base.add(off), kind);
    }
    match kind {
        KIND_INT8 => *(base.add(off) as *const i8) as f64,
        KIND_UINT8 | KIND_UINT8_CLAMPED => *base.add(off) as f64,
        KIND_INT16 => *(base.add(off) as *const i16) as f64,
        KIND_UINT16 => *(base.add(off) as *const u16) as f64,
        KIND_INT32 => *(base.add(off) as *const i32) as f64,
        KIND_UINT32 => *(base.add(off) as *const u32) as f64,
        // #10779: an ArrayBuffer lane is arbitrary user bytes, so a float
        // kind is the one element kind whose value can land inside Perry's
        // NaN-box tag band. Canonicalise here — this is the single runtime
        // choke point every `ta[i]` read funnels through — so the value that
        // leaves is a number for every consumer that tag-dispatches it.
        // Integer kinds cannot produce a NaN and are left untouched.
        KIND_FLOAT16 => {
            crate::array::canonical_raw_f64(f16_bits_to_f64(*(base.add(off) as *const u16)))
        }
        KIND_FLOAT32 => crate::array::canonical_raw_f64(*(base.add(off) as *const f32) as f64),
        KIND_FLOAT64 => crate::array::canonical_raw_f64(*(base.add(off) as *const f64)),
        // BigInt kinds return a NaN-boxed BigInt (not a plain Number), so
        // `ta[i]` round-trips as a `bigint`. The raw slot bits are the BigInt's
        // low limb; widen via the signed/unsigned constructor for `> i64::MAX`.
        KIND_BIGINT64 => {
            let v = *(base.add(off) as *const i64);
            crate::value::js_nanbox_bigint(crate::bigint_hooks::js_bigint_from_i64(v) as i64)
        }
        KIND_BIGUINT64 => {
            let v = *(base.add(off) as *const u64);
            crate::value::js_nanbox_bigint(crate::bigint_hooks::js_bigint_from_u64(v) as i64)
        }
        _ => 0.0,
    }
}

/// NaN-box a TypedArray header pointer as the JS `array` receiver value passed
/// as the 3rd/4th callback argument. Per spec the callback observes the
/// original typed-array receiver. Shared by the iteration (`map`/`filter`/…)
/// and transform (`findLast`/…) sibling modules.
#[inline(always)]
pub(crate) fn ta_receiver_value(ta: *const TypedArrayHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(ta as *const u8).bits())
}

/// #5525 inline fast read for `obj[i]` when `obj` is dynamically an owning
/// numeric typed array and `i` a canonical non-negative integer index. Lets
/// `js_dyn_index_get` collapse the multi-call dynamic-dispatch chain
/// (`js_typed_array_index_get_dynamic` → `typed_array_index_get_dynamic` →
/// `typed_array_addr_from_value` → `typed_array_get_numeric_index` →
/// `typed_array_owner_get` → `js_typed_array_get`) into a single bounds check +
/// `load_at` on the hot path. Returns `Some(undefined)` for an in-range index
/// past `length` (spec), `Some(value)` for an in-bounds read, and `None` for
/// the cases the full dispatcher must still own: BigInt element kinds (whose
/// read allocates a boxed BigInt) and non-canonical / non-numeric keys
/// (string/symbol expandos, fractional/negative indices). `kind` is the value
/// the caller already resolved via `lookup_typed_array_kind`.
#[inline]
pub fn typed_array_fast_index_get(ptr: usize, kind: u8, index: f64) -> Option<f64> {
    if kind == KIND_BIGINT64 || kind == KIND_BIGUINT64 {
        return None;
    }
    if !(index.is_finite() && index >= 0.0 && index.fract() == 0.0 && index <= u32::MAX as f64) {
        return None;
    }
    let ta = ptr as *const TypedArrayHeader;
    // Native arena views need their liveness validated (the slow path's
    // `validate_view_alive` throws on a disposed owner); defer those. The check
    // is a cheap global-counter gate when no native views exist.
    if crate::native_arena::is_native_typed_view(ta) {
        return None;
    }
    let idx = index as u32;
    unsafe {
        if idx >= crate::typedarray::element_length(ta) {
            return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
        }
        Some(load_at(ta, idx as usize))
    }
}

/// #5525 inline fast write counterpart to [`typed_array_fast_index_get`].
/// Returns `true` when the write was fully handled here (an in-bounds store, or
/// a silently-dropped out-of-bounds canonical-index write — both spec-correct
/// for integer-indexed exotic objects). Returns `false` to defer to the full
/// dynamic setter for BigInt element kinds (ToBigInt coercion / throw) and
/// non-canonical / non-numeric keys (string/symbol expando writes).
#[inline]
pub fn typed_array_fast_index_set(ptr: usize, kind: u8, index: f64, value: f64) -> bool {
    if kind == KIND_BIGINT64 || kind == KIND_BIGUINT64 {
        return false;
    }
    // `store_at` converts a Number. A NaN-boxed value — string, boolean,
    // null/undefined, object, or an int32 box — needs the slow setter's full
    // ToNumber first; storing its bits here wrote `true` into a Float64Array
    // slot and 0 into every integer kind once the kind cache was warm.
    if (value.to_bits() as i64) >= 0x7FF9_0000_0000_0000 {
        return false;
    }
    if !(index.is_finite() && index >= 0.0 && index.fract() == 0.0 && index <= u32::MAX as f64) {
        return false;
    }
    let ta = ptr as *mut TypedArrayHeader;
    if crate::native_arena::is_native_typed_view(ta as *const TypedArrayHeader) {
        return false;
    }
    let idx = index as u32;
    unsafe {
        if idx < crate::typedarray::element_length(ta) {
            store_at(ta, idx as usize, value);
        }
        // In-bounds → stored; out-of-bounds canonical index → dropped per spec.
        true
    }
}

// ---------- FFI ----------

#[no_mangle]
pub extern "C" fn js_native_memory_fill_u32(view_raw: u64, value: f64) {
    unsafe {
        let view = strict_typed_array_from_raw(
            view_raw,
            Some(KIND_UINT32),
            b"NativeMemory.fillU32 expects a Uint32Array view",
        );
        let (data, len) = typed_array_raw_bytes_mut(view);
        let word_count = len / std::mem::size_of::<u32>();
        let value = jsvalue_to_f64(value) as i64 as u32;
        for i in 0..word_count {
            *(data as *mut u32).add(i) = value;
        }
    }
}

#[no_mangle]
pub extern "C" fn js_native_memory_copy(dst_raw: u64, src_raw: u64) {
    unsafe {
        let (dst_data, dst_len) = native_memory_copy_dst_bytes(dst_raw);
        let (src_data, src_len) = native_memory_copy_src_bytes(src_raw);
        ptr::copy(src_data, dst_data, dst_len.min(src_len));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owning_u32_admission_reads_current_header() {
        for len in [16, 65536] {
            let ta = typed_array_alloc(KIND_UINT32, len);
            let boxed = crate::value::js_nanbox_pointer(ta as i64);
            assert_eq!(
                crate::buffer::header::has_owned_backing(ta as usize),
                len > 1024
            );
            assert_eq!(
                owning_u32_addr(boxed),
                ta as usize,
                "Native owners must admit"
            );
            unsafe {
                let h = crate::buffer::store::header(ta as usize);
                (*h)._reserved |= crate::codegen_abi::BYTES_DETACHED;
                assert_eq!(owning_u32_addr(boxed), 0, "detached owners must decline");
                (*h)._reserved &= !crate::codegen_abi::BYTES_DETACHED;
                (*h).obj_type = type_for_kind(KIND_INT32);
                assert_eq!(owning_u32_addr(boxed), 0, "wrong brands must decline");
                (*h).obj_type = type_for_kind(KIND_UINT32) | crate::codegen_abi::BYTES_TYPE_VIEW;
                assert_eq!(owning_u32_addr(boxed), 0, "views must decline");
                (*h).obj_type = type_for_kind(KIND_UINT32);
                assert_eq!(owning_u32_addr(boxed), ta as usize);
            }
        }
    }

    #[test]
    fn inline_only_admission_turns_the_native_u32_witness_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "typedarray::tests::owning_u32_admission_reads_current_header",
                "--nocapture",
            ])
            .env("PERRY_B4_SABOTAGE", "inline_admission")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(
            !child.status.success(),
            "Native admission must be exercised"
        );
    }

    #[test]
    fn large_object_typed_array_alloc_uses_old_gc_header_and_stays_usable() {
        let ta = typed_array_alloc(KIND_UINT8, crate::gc::LARGE_OBJECT_THRESHOLD_BYTES as u32);
        assert!(!ta.is_null());
        assert_eq!(lookup_typed_array_kind(ta as usize), Some(KIND_UINT8));
        assert!(crate::arena::pointer_in_old_gen(ta as usize));
        unsafe {
            let header =
                (ta as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            assert_eq!((*header).obj_type, type_for_kind(KIND_UINT8));
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

    /// The #5525 fast store converts only Numbers: every NaN-boxed value must
    /// decline so the slow setter can apply ToNumber, while plain doubles —
    /// negative values and NaN included — still store here.
    #[test]
    fn fast_index_set_declines_nan_boxed_values() {
        unsafe {
            let ta = typed_array_alloc(KIND_FLOAT64, 2);
            let ptr = ta as usize;
            for boxed in [
                crate::value::TAG_TRUE,
                crate::value::TAG_FALSE,
                crate::value::TAG_NULL,
                crate::value::TAG_UNDEFINED,
                crate::value::JSValue::int32(5).bits(),
                0x7FF9_0100_0000_0035, // short string "5"
            ] {
                assert!(
                    !typed_array_fast_index_set(ptr, KIND_FLOAT64, 0.0, f64::from_bits(boxed)),
                    "boxed value {boxed:#x} must take the slow setter"
                );
            }
            assert!(typed_array_fast_index_set(ptr, KIND_FLOAT64, 1.0, -2.5));
            assert_eq!(load_at(ta, 1), -2.5);
            assert!(typed_array_fast_index_set(ptr, KIND_FLOAT64, 1.0, f64::NAN));
            assert!(load_at(ta, 1).is_nan());
        }
    }

    #[test]
    fn to_uint32_bits_wraps_modularly_not_saturating() {
        // NaN / ±Inf / ±0 → 0.
        assert_eq!(to_uint32_bits(f64::NAN), 0);
        assert_eq!(to_uint32_bits(f64::INFINITY), 0);
        assert_eq!(to_uint32_bits(f64::NEG_INFINITY), 0);
        assert_eq!(to_uint32_bits(0.0), 0);
        assert_eq!(to_uint32_bits(-0.0), 0);
        // Truncate toward zero, then mod 2^32.
        assert_eq!(to_uint32_bits(7.9), 7);
        assert_eq!(to_uint32_bits(-1.0), 0xFFFF_FFFF);
        assert_eq!(to_uint32_bits(4294967296.0 + 7.0), 7); // 2^32 + 7
                                                           // 1e21 is exactly representable; ToUint32 wraps (NOT i32::MAX saturate).
        assert_eq!(to_uint32_bits(1e21) as i32, -559939584);
    }

    #[test]
    fn store_at_integer_kinds_wrap_per_spec() {
        unsafe {
            let check = |kind: u8, v: f64| -> f64 {
                let ta = typed_array_alloc(kind, 1);
                store_at(ta, 0, v);
                load_at(ta, 0)
            };
            assert_eq!(check(KIND_INT8, 300.0), 44.0);
            assert_eq!(check(KIND_UINT8, 261.0), 5.0);
            assert_eq!(check(KIND_INT16, 105536.0), -25536.0);
            assert_eq!(check(KIND_UINT16, 1e21), 0.0);
            assert_eq!(check(KIND_INT32, 4294967303.0), 7.0);
            assert_eq!(check(KIND_INT32, 1e21), -559939584.0);
            assert_eq!(check(KIND_UINT32, -1.0), 4294967295.0);
            assert_eq!(check(KIND_INT8, 1e21), 0.0);
        }
    }

    #[test]
    fn bigint_lane_snapshot_helpers_preserve_all_64_bits() {
        for kind in [KIND_BIGINT64, KIND_BIGUINT64] {
            let typed = typed_array_alloc(kind, 2);
            assert!(set_bigint_lane_bits(typed, 0, u64::MAX));
            assert!(set_bigint_lane_bits(typed, 1, 9_007_199_254_740_993));
            assert_eq!(bigint_lane_bits(typed, 0), Some(u64::MAX));
            assert_eq!(bigint_lane_bits(typed, 1), Some(9_007_199_254_740_993));
        }
    }
}

pub use crate::byte_cell_support::*;
