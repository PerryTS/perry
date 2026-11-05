//! Shared byte-cell admission, representation, numeric coercion and views.
use crate::typedarray::{
    TypedArrayHeader, CLASS_ID_BIGINT64_ARRAY, CLASS_ID_BIGUINT64_ARRAY, CLASS_ID_FLOAT16_ARRAY,
    CLASS_ID_FLOAT32_ARRAY, CLASS_ID_FLOAT64_ARRAY, CLASS_ID_INT16_ARRAY, CLASS_ID_INT32_ARRAY,
    CLASS_ID_INT8_ARRAY, CLASS_ID_UINT16_ARRAY, CLASS_ID_UINT32_ARRAY, CLASS_ID_UINT8_ARRAY,
    CLASS_ID_UINT8_CLAMPED_ARRAY, KIND_BIGINT64, KIND_BIGUINT64, KIND_FLOAT16, KIND_FLOAT32,
    KIND_FLOAT64, KIND_INT16, KIND_INT32, KIND_INT8, KIND_UINT16, KIND_UINT32, KIND_UINT8,
    KIND_UINT8_CLAMPED,
};
use crate::typedarray_half::{f16_bits_to_f64, f64_to_f16_bits};
use std::ptr;
pub(crate) mod shared_access;
const KIND_BRANDS: [u8; 12] = [2, 0, 4, 3, 6, 5, 8, 9, 1, 10, 11, 7];
const BRAND_KINDS: [u8; 12] = [1, 8, 0, 3, 2, 5, 4, 11, 6, 7, 9, 10];
#[cfg(test)]
thread_local! { static TEST_TA_REGISTRY_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }

/// What an element-READ helper (`js_typed_array_get`,
/// `js_typed_array_index_get_dynamic`) was ACTUALLY handed.
///
/// See [`classify_element_read_receiver`].
pub(crate) enum ElementReadReceiver {
    /// A registered %TypedArray% (or a `GC_TYPE_TYPED_ARRAY` /
    /// `GC_TYPE_NATIVE_TYPED_VIEW` allocation the registry does not know)
    /// at this de-NaN-boxed address. Stay on the typed element path.
    TypedArray(usize),
    /// NOT a typed array. Read it through the ordinary `[[Get]]`
    /// ([`crate::value::js_dyn_index_get`]) with this NaN-boxed receiver,
    /// re-tagged the way the value reached codegen before codegen masked
    /// the tag off.
    Ordinary(f64),
    /// Nothing indexable — a masked-away non-pointer tag (`42 as any`), a
    /// null receiver, or an address no managed header backs. `undefined`.
    Absent,
}

#[inline]
pub fn elem_size_for_kind(kind: u8) -> usize {
    match kind {
        KIND_INT8 | KIND_UINT8 | KIND_UINT8_CLAMPED => 1,
        KIND_INT16 | KIND_UINT16 | KIND_FLOAT16 => 2,
        KIND_INT32 | KIND_UINT32 | KIND_FLOAT32 => 4,
        KIND_FLOAT64 | KIND_BIGINT64 | KIND_BIGUINT64 => 8,
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
        KIND_BIGINT64 => CLASS_ID_BIGINT64_ARRAY,
        KIND_BIGUINT64 => CLASS_ID_BIGUINT64_ARRAY,
        KIND_FLOAT16 => CLASS_ID_FLOAT16_ARRAY,
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
        KIND_BIGINT64 => "BigInt64Array",
        KIND_BIGUINT64 => "BigUint64Array",
        KIND_FLOAT16 => "Float16Array",
        _ => "TypedArray",
    }
}

#[inline]
pub fn kind_for_name(name: &str) -> Option<u8> {
    match name {
        "Int8Array" => Some(KIND_INT8),
        "Uint8Array" => Some(KIND_UINT8),
        "Int16Array" => Some(KIND_INT16),
        "Uint16Array" => Some(KIND_UINT16),
        "Int32Array" => Some(KIND_INT32),
        "Uint32Array" => Some(KIND_UINT32),
        "Float32Array" => Some(KIND_FLOAT32),
        "Float64Array" => Some(KIND_FLOAT64),
        "Uint8ClampedArray" => Some(KIND_UINT8_CLAMPED),
        "BigInt64Array" => Some(KIND_BIGINT64),
        "BigUint64Array" => Some(KIND_BIGUINT64),
        "Float16Array" => Some(KIND_FLOAT16),
        _ => None,
    }
}

#[inline(always)]
pub(crate) fn type_for_kind(kind: u8) -> u8 {
    crate::codegen_abi::BYTES_TYPE_BASE | KIND_BRANDS[kind as usize]
}

#[inline(always)]
pub(crate) unsafe fn element_kind(ta: *const TypedArrayHeader) -> u8 {
    BRAND_KINDS[((*crate::buffer::store::header(ta as usize)).obj_type & 0x1f) as usize]
}

#[inline(always)]
pub(crate) unsafe fn element_length(ta: *const TypedArrayHeader) -> u32 {
    crate::buffer::store::length(ta as usize) as u32
}

#[cfg(test)]
pub(crate) fn test_typed_array_registry_probe_count() -> u64 {
    TEST_TA_REGISTRY_PROBES.with(|c| c.get())
}

#[inline]
pub fn lookup_typed_array_kind(addr: usize) -> Option<u8> {
    #[cfg(test)]
    TEST_TA_REGISTRY_PROBES.with(|c| c.set(c.get().wrapping_add(1)));
    let ty = crate::buffer::header::byte_cell_type(addr)?;
    crate::gc::is_typed_array_type(ty).then(|| BRAND_KINDS[(ty & 0x1f) as usize])
}

#[inline]
pub fn is_offheap_sidetable_alloc(addr: usize) -> bool {
    crate::buffer::header::byte_cell_type(addr).is_some()
}

/// Retained allocator notification; byte admission has no address entries.
pub(crate) fn invalidate_caches_in_range(_start: usize, _end: usize) {}

pub fn register_typed_array(ptr: *const TypedArrayHeader, expected: u8) {
    debug_assert_eq!(lookup_typed_array_kind(ptr as usize), Some(expected));
}

pub(crate) fn typed_array_has_shared_backing(ptr: *const TypedArrayHeader) -> bool {
    let ptr = clean_ta_ptr(ptr);
    if lookup_typed_array_kind(ptr as usize).is_none() {
        return false;
    }
    unsafe {
        (*crate::buffer::store::header(crate::buffer::store::owner(ptr as usize))).obj_type & !0x20
            == crate::gc::GC_TYPE_BUFFER_SHARED_ARRAY_BUFFER
    }
}

#[inline]
pub(crate) fn strip_nanbox(p: u64) -> usize {
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
    if crate::value::addr_class::is_handle_band(addr) {
        return ptr::null();
    }
    addr as *const TypedArrayHeader
}

/// Classify the raw receiver word an element-READ helper was handed, BEFORE
/// anything dereferences it as a `TypedArrayHeader`.
///
/// **Ask this before `clean_ta_ptr`, never after** — the mirror of
/// `array/header.rs`'s [`crate::array::header`] `typed_array_receiver`
/// (#8090), which had to ask the typed-array question before
/// `clean_arr_ptr` rejected the receiver. Here the direction is reversed
/// and the failure is worse, because `clean_ta_ptr` performs **no registry
/// check at all**: it only rejects addresses below `0x1000`. So a plain
/// array or object that arrives through a stale *static* typed-array hint
/// was read AS a `TypedArrayHeader` — `crate::typedarray::element_length(ta)` landed on
/// `ArrayHeader::length` (both at offset 0, so bounds checks "passed"),
/// `crate::typedarray::element_kind(ta)` / `elem_size` on the low bytes of element 0's NaN box, and
/// `data_ptr(ta)` 8 bytes past the real element region. The observable
/// result was a silent `0` for every element read (#8100).
///
/// This is reachable in the shipped default configuration:
/// `perry-codegen`'s `is_width_tracked_typed_array_receiver` (#7494)
/// deliberately keeps a reassigned local's declared typed-array kind — see
/// its comment, which explains that dropping the hint sends a REAL typed
/// array through the plain-array layout instead — and pays for that by
/// promising the runtime helper "re-validates the object's actual GC kind
/// before touching memory". This function is that validation, and it
/// dispatches instead of only rejecting.
///
/// Codegen masks the NaN-box tag off the receiver (`and i64 %bits,
/// 0x0000_FFFF_FFFF_FFFF`) before the call, so the tag cannot simply be
/// read back: it is RECONSTRUCTED from the managed header. That matters for
/// exactly one case — a heap string must be re-boxed with `STRING_TAG`, or
/// `js_dyn_index_get` walks a `StringHeader` as an `ObjectHeader` instead of
/// taking its string arm. Symbols share `GC_TYPE_STRING` and are
/// POINTER-tagged, which is what `js_is_symbol` separates.
///
/// The registry is the authority for "is a typed array", the same gate
/// `js_typed_array_read_int32` and `strict_typed_array_from_raw` already
/// use. A `GC_TYPE_TYPED_ARRAY` / `GC_TYPE_NATIVE_TYPED_VIEW` header still
/// wins on a registry miss, so a lookup failure can only cost the diversion,
/// never the element read.
pub(crate) fn classify_element_read_receiver(raw: u64) -> ElementReadReceiver {
    // No hand-rolled address floor: every probe below is either a side-table
    // lookup (safe for any bit pattern) or `try_read_gc_header`, which
    // magnitude-classifies — handle band included — before it dereferences.
    if let Some((addr, ty)) = crate::buffer::header::byte_cell_of_word(raw) {
        if crate::gc::is_typed_array_type(ty) {
            return ElementReadReceiver::TypedArray(addr);
        }
    }
    let addr = strip_nanbox(raw);
    // Only a POSITIVELY identified receiver is diverted. `try_read_gc_header`
    // magnitude-classifies before it dereferences, so garbage bits that
    // survived the mask never reach a managed-header read.
    let obj_type = match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(header) => header.obj_type,
        None => {
            // Small-buffer slab entries are heap-plausible but carry NO
            // GcHeader, so the registry is the only way to see a Buffer /
            // `Uint8Array`-backed receiver here.
            return if crate::buffer::is_registered_buffer(addr) {
                ElementReadReceiver::Ordinary(crate::value::js_nanbox_pointer(addr as i64))
            } else {
                ElementReadReceiver::Absent
            };
        }
    };
    match obj_type {
        // `lookup_typed_array_kind` above refused it: a typed-array type byte
        // the allocator does not own names no cell.
        ty if crate::gc::is_typed_array_type(ty) => ElementReadReceiver::Absent,
        crate::gc::GC_TYPE_STRING => {
            let boxed = crate::value::js_nanbox_pointer(addr as i64);
            if unsafe { crate::symbol::js_is_symbol(boxed) } != 0 {
                ElementReadReceiver::Ordinary(boxed)
            } else {
                ElementReadReceiver::Ordinary(crate::value::js_nanbox_string(addr as i64))
            }
        }
        _ => ElementReadReceiver::Ordinary(crate::value::js_nanbox_pointer(addr as i64)),
    }
}

#[inline]
pub(crate) fn data_ptr(ta: *const TypedArrayHeader) -> *const u8 {
    unsafe {
        let owner = crate::buffer::store::owner(ta as usize);
        if (*crate::buffer::store::header(owner)).obj_type == crate::gc::GC_TYPE_NATIVE_ARENA_OWNER
            && (*crate::buffer::store::header(owner))._reserved & crate::codegen_abi::BYTES_DETACHED
                != 0
        {
            crate::native_arena::throw_native_arena_disposed();
        }
        #[cfg(test)]
        if crate::buffer::bytes::b4_sabotage("native_resolution")
            && crate::buffer::store::is_view(ta as usize)
        {
            return (ta as *const u8).add(crate::codegen_abi::BYTES_STORE);
        }
        crate::buffer::store::data(ta as usize)
    }
}

/// Whole-loop admission reads the receiver header as the authority. Views
/// cannot satisfy the owning proof. Inline and Native stores share it.
#[inline]
pub(crate) fn owning_u32_addr(receiver: f64) -> usize {
    let value = crate::value::JSValue::from_bits(receiver.to_bits());
    if !value.is_pointer() {
        return 0;
    }
    let addr = value.as_pointer::<TypedArrayHeader>() as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(addr) {
        return 0;
    }
    #[cfg(test)]
    if crate::buffer::bytes::b4_sabotage("u32_admission") {
        return addr;
    }
    unsafe {
        let h = crate::gc::header_from_trusted_user_ptr(addr as *const u8);
        #[cfg(test)]
        if std::env::var("PERRY_B4_SABOTAGE").ok().as_deref() == Some("inline_admission")
            && (*h)._reserved & crate::codegen_abi::BYTES_OUT_OF_LINE != 0
        {
            return 0;
        }
        if (*h).obj_type == type_for_kind(KIND_UINT32)
            && (*h)._reserved
                & (crate::codegen_abi::BYTES_DETACHED | crate::codegen_abi::BYTES_RESIZABLE)
                == 0
        {
            addr
        } else {
            0
        }
    }
}

#[inline]
pub(crate) fn data_ptr_mut(ta: *mut TypedArrayHeader) -> *mut u8 {
    data_ptr(ta) as *mut u8
}

/// Return the byte view for a registered typed array.
///
/// Native arena views do not store their bytes after `TypedArrayHeader`; this
/// helper routes through `data_ptr`, which validates disposed native views and
/// returns the external backing pointer.
pub unsafe fn typed_array_bytes<'a>(ta: *const TypedArrayHeader) -> Option<&'a [u8]> {
    let ta = typed_array_for_byte_helper(ta)? as *const TypedArrayHeader;
    let data = data_ptr(ta);
    let len = (crate::typedarray::element_length(ta) as usize).saturating_mul(
        crate::typedarray::elem_size_for_kind(crate::typedarray::element_kind(ta)) as usize,
    );
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
    let ta = typed_array_for_byte_helper(ta as *const TypedArrayHeader)?;
    let data = data_ptr_mut(ta);
    let len = (crate::typedarray::element_length(ta) as usize).saturating_mul(
        crate::typedarray::elem_size_for_kind(crate::typedarray::element_kind(ta)) as usize,
    );
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

pub fn typed_array_to_array_buffer(
    ta: *const TypedArrayHeader,
) -> *mut crate::buffer::BufferHeader {
    let admitted = unsafe { typed_array_for_byte_helper(ta) };
    #[cfg(test)]
    let admitted = if crate::buffer::bytes::b4_sabotage("copy_kind") {
        Some(clean_ta_ptr(ta).cast_mut())
    } else {
        admitted
    };
    let Some(ta) = admitted else {
        return std::ptr::null_mut();
    };
    // Destination allocation can collect. Keep the source value rooted and
    // resolve its bytes afterward, rather than retaining an unscoped slice.
    let input = crate::value::js_nanbox_pointer(ta as i64);
    match crate::buffer::bytes::copy_value(crate::buffer::bytes::Brand::ArrayBuffer, input) {
        Ok(value) => crate::value::JSValue::from_bits(value.to_bits())
            .as_pointer::<crate::buffer::BufferHeader>()
            .cast_mut(),
        Err(_) => std::ptr::null_mut(),
    }
}

pub(crate) unsafe fn typed_array_for_byte_helper(
    ta: *const TypedArrayHeader,
) -> Option<*mut TypedArrayHeader> {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() || lookup_typed_array_kind(ta as usize).is_none() {
        return None;
    }
    Some(strict_typed_array_from_raw(
        ta as u64,
        None,
        b"Expected typed array",
    ))
}

#[cold]
pub(crate) fn throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cold]
pub(crate) fn throw_range_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[inline]
pub(crate) fn is_arena_backed_addr(addr: usize) -> bool {
    !matches!(
        crate::arena::classify_heap_space(addr),
        crate::arena::HeapSpace::Unknown
    )
}

#[inline]
pub(crate) unsafe fn arena_payload_has_gc_type(addr: usize, expected_type: u8) -> bool {
    if crate::value::addr_class::is_handle_band(addr) {
        return false;
    }
    let header_addr = addr - crate::gc::GC_HEADER_SIZE;
    if matches!(
        crate::arena::classify_heap_space(header_addr),
        crate::arena::HeapSpace::Unknown
    ) {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    let size = header.size as usize;
    crate::gc::gc_type_info(header.obj_type).is_some()
        && size >= crate::gc::GC_HEADER_SIZE
        && size as u64 <= (1u64 << 34)
        && header.gc_flags & crate::gc::GC_FLAG_ARENA != 0
        && header.obj_type == expected_type
}

#[inline]
pub(crate) unsafe fn validate_arena_payload_gc_type(
    addr: usize,
    expected_type: u8,
    message: &[u8],
) {
    if is_arena_backed_addr(addr) && !arena_payload_has_gc_type(addr, expected_type) {
        throw_type_error(message);
    }
}

pub(crate) unsafe fn strict_typed_array_from_raw(
    raw: u64,
    expected_kind: Option<u8>,
    message: &[u8],
) -> *mut TypedArrayHeader {
    let addr = strip_nanbox(raw);
    if crate::value::addr_class::is_handle_band(addr) {
        throw_type_error(message);
    }
    let Some(kind) = lookup_typed_array_kind(addr) else {
        throw_type_error(message);
    };
    if expected_kind.is_some_and(|expected| kind != expected) {
        throw_type_error(message);
    }
    let ta = addr as *mut TypedArrayHeader;
    if crate::native_arena::is_native_typed_view(ta as *const TypedArrayHeader) {
        crate::native_arena::validate_view_alive(
            crate::native_arena::native_view_from_typed_array(ta as *const TypedArrayHeader),
        );
    } else {
        validate_arena_payload_gc_type(
            addr,
            type_for_kind(kind)
                | (unsafe { (*crate::buffer::store::header(addr)).obj_type }
                    & crate::codegen_abi::BYTES_TYPE_VIEW),
            message,
        );
    }
    ta
}

/// Convert an f64 (NaN-boxed JS value) to the numeric value to store. Strings
/// and undefined become 0/NaN.
pub(crate) fn jsvalue_to_f64(v: f64) -> f64 {
    let bits = v.to_bits();
    let top16 = bits >> 48;
    // Plain double — positive, negative, ±Inf, and all NaN patterns that
    // are NOT NaN-box tags. Tagged values occupy top16 in
    // 0x7FF9..=0x7FFF (SHORT_STRING_TAG=0x7FF9, BIGINT_TAG=0x7FFA,
    // 0x7FFC=undefined/null/bool, POINTER_TAG=0x7FFD, INT32_TAG=0x7FFE,
    // STRING_TAG=0x7FFF). Negative doubles (top16≥0x8000) and the canonical
    // NaN (top16 0x7FF8) return as-is. An inline SSO string must reach the
    // string arm below, not be stored as its raw NaN bits (#11519).
    if crate::value::JSValue::from_bits(bits).is_number() {
        return v;
    }
    // ECMA-262 IntegerIndexedElementSet on a non-bigint view performs
    // ToNumber on the value. ToNumber(Symbol) and ToNumber(BigInt) are both
    // TypeErrors (§7.1.4). Bigint views never reach here (js_typed_array_set
    // routes ToBigInt separately), so a BigInt at this point is being written
    // into a numeric view and must throw. Symbols are POINTER_TAG.
    if top16 == 0x7FFA {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    if top16 == 0x7FFD && unsafe { crate::symbol::js_is_symbol(v) } != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
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
    // Strings (heap or inline SSO, #11519): try to parse, else NaN.
    if top16 == 0x7FFF || top16 == 0x7FF9 {
        return crate::string::with_string_value_bytes(v, |bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .unwrap_or(f64::NAN)
        })
        .unwrap_or(f64::NAN);
    }
    // POINTER_TAG object (Symbols already threw above; BigInt handled above):
    // a non-bigint view performs `ToNumber(value)`, which for an object runs
    // `ToPrimitive(value, "number")` (its `Symbol.toPrimitive` / `valueOf` /
    // `toString`). Previously this fell through to `NaN`, so
    // `new Int32Array([{ valueOf() { return 5 } }])` stored 0 and
    // `TypedArray.from`'s ToNumber side effects never fired. Delegate to the
    // shared ToNumber so the user coercion hook runs.
    if top16 == 0x7FFD {
        return crate::builtins::js_number_coerce(v);
    }
    f64::NAN
}

/// ECMA-262 `ToUint32` (§7.1.7) on a double, returning the raw 32-bit pattern.
/// `NaN`/±`Inf`/±0 → 0; otherwise truncate toward zero, then reduce modulo
/// 2^32. Done in f64 space so values past `i64::MAX` (e.g. `1e21`) wrap
/// correctly instead of saturating — `value as i64` would clamp to `i64::MAX`
/// and produce the wrong low bits. The signed integer kinds (`Int8`/`Int16`/
/// `Int32`) all derive from these bits via a width-narrowing reinterpret, which
/// matches `ToInt8`/`ToInt16`/`ToInt32` (they share `ToUint32`'s modular step
/// and differ only in the final two's-complement reinterpretation).
pub(crate) fn to_uint32_bits(value: f64) -> u32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let m = value.trunc().rem_euclid(4294967296.0); // value mod 2^32, in [0, 2^32)
    m as u32
}

/// Coerce a NaN-boxed JS value for a Uint8Array/Buffer element store.
///
/// Perry represents `Uint8Array` with `BufferHeader`, outside the generic
/// `TypedArrayHeader` registry. Dynamic stores still receive a full JS value,
/// so they must perform the same ToNumber + modulo narrowing as the generic
/// typed-array path before calling the integer-only buffer accessor.
pub(crate) fn jsvalue_to_uint8(value: f64) -> u8 {
    to_uint32_bits(jsvalue_to_f64(value)) as u8
}

pub(crate) fn to_uint8_clamp(value: f64) -> u8 {
    // ToUint8Clamp: NaN → 0, v ≤ 0 → 0, v ≥ 255 → 255,
    // otherwise round-half-to-even then clamp.
    if value.is_nan() || value <= 0.0 {
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
    }
}

/// Extract the low 64 bits to write into a `BigInt64`/`BigUint64` slot. The
/// element-set path hands us an already-`ToBigInt`-coerced, NaN-boxed BigInt
/// (so reads round-trip the raw bits); internal `load_at`→`store_at` copies do
/// the same. A bare finite Number falls back to a truncating `as i64` cast so
/// any legacy numeric caller keeps its old behavior rather than reading garbage.
pub(crate) fn bigint_slot_bits(value: f64) -> u64 {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader;
        let cleaned = crate::bigint::clean_bigint_ptr(ptr);
        if cleaned.is_null() {
            return 0;
        }
        unsafe { (*cleaned).limbs[0] }
    } else if value.is_finite() {
        value as i64 as u64
    } else {
        0
    }
}

pub(crate) fn is_bigint_kind(kind: u8) -> bool {
    matches!(kind, KIND_BIGINT64 | KIND_BIGUINT64)
}

#[cold]
pub(crate) fn throw_bigint_number_mix() -> ! {
    // PERRY_BIGINT_MIX_DIAG=1: print a native backtrace before throwing so a
    // compiled bundle's stackless "Cannot mix BigInt" error can be traced to
    // its JS frame (same diagnostic hook as dynamic_arith's mixed-operand
    // gate; used to root-cause #6649).
    if std::env::var_os("PERRY_BIGINT_MIX_DIAG").is_some() {
        eprintln!("[bigint-mix-diag] typedarray mix gate");
        eprintln!("{}", std::backtrace::Backtrace::force_capture());
    }
    throw_type_error(b"Cannot mix BigInt and other types, use explicit conversions")
}

/// All typed owners use the common byte-store entry and placement rule.
pub fn typed_array_alloc(kind: u8, length: u32) -> *mut TypedArrayHeader {
    crate::buffer::store::store_alloc(
        type_for_kind(kind),
        length,
        crate::buffer::store::Init::Zero,
    )
}
