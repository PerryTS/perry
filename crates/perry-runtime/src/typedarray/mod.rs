//! TypedArray support for all JS typed-array element kinds.
//!
//! Each TypedArrayHeader stores its element kind + size and a contiguous
//! data region. Element-level read/write goes through `js_typed_array_get`
//! and `js_typed_array_set`, which handle the per-kind cast/store. The
//! immutable methods (`toSorted`, `toReversed`, `with`, etc.) materialize
//! a new TypedArrayHeader of the same kind.
//!
//! Pointers are NaN-boxed with POINTER_TAG (0x7FFD) and tracked in
//! TYPED_ARRAY_REGISTRY for `instanceof` and console.log formatting.

use std::alloc::Layout;
use std::cell::RefCell;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::array::ArrayHeader;
use crate::closure::ClosureHeader;
use crate::typedarray_half::{f16_bits_to_f64, f64_to_f16_bits};

pub(crate) mod bigint;
mod format;
pub(crate) mod species;
pub use format::format_typed_array;

mod access;
mod construct;
mod iterate;
mod slice_ops;
mod transform;

// `#[no_mangle] pub extern "C"` FFI entry points are compiled regardless (the
// `mod` declarations above pull them in), but Rust code in OTHER modules reaches
// many of them through the `crate::typedarray::js_...` path, so re-export every
// such item by name to keep those paths resolving. Inherent/no-path-referenced
// items need no re-export.
pub use access::{
    js_typed_array_at, js_typed_array_copy_within, js_typed_array_get,
    js_typed_array_index_get_dynamic, js_typed_array_length, js_typed_array_set,
    js_typed_array_set_from, js_uint8array_get, js_uint8array_set,
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
    /// Perry currently materializes typed-array views over ArrayBuffer storage
    /// as owning TypedArrayHeader values. Track which views came from
    /// SharedArrayBuffer so Atomics.wait can apply Node's shared-buffer guard.
    static TYPED_ARRAY_SHARED_BACKING: RefCell<crate::fast_hash::PtrHashSet<usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_set());
}

/// Process-global, lock-free fast cache in front of the thread-local
/// `TYPED_ARRAY_REGISTRY` (#5525). A single untyped `arr[i]` element access on
/// a value whose static type was erased (e.g. a typed array reaching a function
/// through an untyped `Array.<number>` parameter — the shape bcryptjs's
/// Blowfish core uses for its `P`/`S` boxes) funnels through
/// `lookup_typed_array_kind` ~5 times (`js_dyn_index_get`,
/// `typed_array_addr_from_value`, `typed_array_get_numeric_index`,
/// `typed_array_owner_length`, `typed_array_owner_get`). Each call is a
/// thread-local access (`_tlv_get_addr`) plus a `RefCell` borrow + hash probe.
/// At ~600M element reads for one cost-12 `bcrypt.compareSync` that dominated
/// the profile (~45% of samples in `_tlv_get_addr`), turning a ~50ms operation
/// into ~28s and reading as an infinite-loop hang.
///
/// The cache is a small direct-mapped table of `(addr << 8) | tag` words (0 =
/// empty). The low byte is the element kind for a typed array, or the
/// [`TA_CACHE_NEGATIVE`] sentinel meaning "this address is *not* a typed array".
/// Negative entries matter because the same dispatcher serves plain-array
/// element access too (bcryptjs's `_crypt` reads its `lr`/`cdata`/`b`/`salt`
/// plain-array boxes through the identical untyped path), and without them
/// every such read would still fall through to the thread-local registry on a
/// miss. Both populations are small and stable here, so a 64-entry table keeps
/// the hot typed *and* plain arrays resident.
///
/// A hit returns the same answer the registry would: the cache only records
/// facts the registry established (positive on a registry hit, negative on a
/// registry miss). It is process-global (not thread-local) so a hit costs no
/// `_tlv_get_addr`; that is sound because arenas never hand out the same live
/// address to two threads, a typed array's address is stable (off-heap raw
/// alloc or tenured old-gen, never moved), and every registry mutation
/// (`register`/`unregister`) overwrites/clears the matching slot below — so a
/// freed-then-reused address can never read back a stale kind or a stale
/// "not a typed array".
pub const TA_KIND_CACHE_SLOTS: usize = 64;
pub const TA_CACHE_NEGATIVE: u64 = 0xFF;
// #5525 follow-up: exported under a stable link name so the codegen can emit a
// guarded *inline* typed-array element load/store at the access site (it reads a
// cache slot, checks the address tag + element kind, bounds-checks against the
// header `length`, and loads/stores the slot directly), bypassing the
// out-of-line `js_dyn_index_{get,set}` call + `lookup_typed_array_kind` +
// `js_number_coerce` on bcrypt's ~600M hot `S[i]`/`P[i]` Int32Array accesses.
// The inline reader observes exactly the same `(addr << 8) | tag` words this
// module maintains; cache misses / non-typed-array / exotic-key cases fall
// through to the existing runtime slow path, so semantics are unchanged.
#[no_mangle]
pub static PERRY_TA_KIND_CACHE: [AtomicU64; TA_KIND_CACHE_SLOTS] =
    [const { AtomicU64::new(0) }; TA_KIND_CACHE_SLOTS];

/// #5525 follow-up: process-global "any exotic typed-array views exist" guard,
/// exported under a stable link name for the codegen inline element path. A
/// non-owning typed array (an `ArrayBuffer`-aliasing view, or a native-arena
/// view) resolves its element-0 pointer through a side table rather than
/// `header + size_of::<TypedArrayHeader>()`, so the inline reader — which
/// assumes inline storage — MUST NOT fire while any such view is live. Both
/// view-registration paths (`typedarray_view::register_view_meta` and
/// `native_arena::register_view`) bump this; the matching unregister paths
/// decrement it. When it reads 0 (the overwhelmingly common case, and always
/// true for bcryptjs's owning `new Int32Array(P_ORIG)` boxes) the inline load
/// of `*(header + 16 + idx*elem_size)` is identical to what `data_ptr` + the
/// per-kind `load_at` slow path computes.
#[no_mangle]
pub static PERRY_TA_VIEW_GUARD: AtomicU64 = AtomicU64::new(0);

#[inline]
pub(crate) fn ta_view_guard_inc() {
    PERRY_TA_VIEW_GUARD.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn ta_view_guard_dec() {
    PERRY_TA_VIEW_GUARD.fetch_sub(1, Ordering::Relaxed);
}

#[inline]
fn ta_kind_cache_slot(addr: usize) -> usize {
    // Addresses are 8-byte aligned; the low 3 bits are always 0. Use the bits
    // above them so distinct live arrays (e.g. `P` and `S`) land in different
    // slots and both stay resident across an alternating access loop.
    (addr >> 3) & (TA_KIND_CACHE_SLOTS - 1)
}

#[inline]
fn ta_kind_cache_store_tag(addr: usize, tag: u64) {
    // `addr` is always > 0x10000, so `(addr << 8) | tag` is never 0 (= empty).
    PERRY_TA_KIND_CACHE[ta_kind_cache_slot(addr)]
        .store(((addr as u64) << 8) | tag, Ordering::Relaxed);
}

#[inline]
fn ta_kind_cache_store(addr: usize, kind: u8) {
    ta_kind_cache_store_tag(addr, kind as u64);
}

#[inline]
fn ta_kind_cache_invalidate(addr: usize) {
    let slot = ta_kind_cache_slot(addr);
    let entry = PERRY_TA_KIND_CACHE[slot].load(Ordering::Relaxed);
    if entry != 0 && (entry >> 8) as usize == addr {
        PERRY_TA_KIND_CACHE[slot].store(0, Ordering::Relaxed);
    }
}

/// Cache probe: `None` = miss (consult the registry), `Some(None)` = cached
/// negative ("not a typed array"), `Some(Some(kind))` = cached typed array.
#[inline]
fn ta_kind_cache_get(addr: usize) -> Option<Option<u8>> {
    let entry = PERRY_TA_KIND_CACHE[ta_kind_cache_slot(addr)].load(Ordering::Relaxed);
    if entry != 0 && (entry >> 8) as usize == addr {
        let tag = entry & 0xff;
        if tag == TA_CACHE_NEGATIVE {
            Some(None)
        } else {
            Some(Some(tag as u8))
        }
    } else {
        None
    }
}

pub fn register_typed_array(ptr: *const TypedArrayHeader, kind: u8) {
    // Keep the cache authoritative: overwrite any colliding/stale slot so a
    // freed-then-reused address never reads back its previous kind.
    ta_kind_cache_store(ptr as usize, kind);
    TYPED_ARRAY_REGISTRY.with(|r| {
        r.borrow_mut().insert(ptr as usize, kind);
    });
}

pub fn unregister_typed_array(ptr: *const TypedArrayHeader) {
    let owner = ptr as usize;
    ta_kind_cache_invalidate(owner);
    TYPED_ARRAY_REGISTRY.with(|r| {
        r.borrow_mut().remove(&owner);
    });
    TYPED_ARRAY_SHARED_BACKING.with(|r| {
        r.borrow_mut().remove(&owner);
    });
    crate::typedarray_view::clear_view_meta(owner);
    crate::typedarray_props::typed_array_clear_own_props(owner);
    crate::typedarray_props::typed_array_clear_no_extend(owner);
}

/// Returns Some(kind) if the (already-stripped) address is a registered
/// typed array, else None.
pub fn lookup_typed_array_kind(addr: usize) -> Option<u8> {
    // #5525 fast path: the process-global cache resolves the hot,
    // repeated-same-address lookups without touching the thread-local
    // registry. A miss (cold address or direct-mapped eviction) falls back to
    // the registry and re-populates the slot.
    if let Some(cached) = ta_kind_cache_get(addr) {
        return cached;
    }
    let kind = TYPED_ARRAY_REGISTRY.with(|r| r.borrow().get(&addr).copied());
    // Record both outcomes: a typed array (positive) or a confirmed non-typed
    // address (negative), so repeated plain-array element access stops hitting
    // the thread-local registry too.
    ta_kind_cache_store_tag(addr, kind.map_or(TA_CACHE_NEGATIVE, |k| k as u64));
    kind
}

/// True for off-GC-heap, header-less allocations — small typed arrays and
/// `Buffer`s, both raw-`alloc`'d with NO 8-byte `GcHeader` prefix and tracked
/// only in side tables. The runtime has many type probes of the form
/// `*(ptr - GC_HEADER_SIZE)` (Promise/Date/Array obj_type checks); each MUST
/// skip these allocations before that back-read, because reading the
/// non-existent header crosses outside the block and segfaults when it sits at
/// the start of a freshly mapped region (#5226). Detection is via the side
/// tables only — never dereferences `addr`.
#[inline]
pub fn is_offheap_sidetable_alloc(addr: usize) -> bool {
    lookup_typed_array_kind(addr).is_some() || crate::buffer::is_registered_buffer(addr)
}

pub(crate) fn mark_typed_array_shared_backing(ptr: *const TypedArrayHeader) {
    TYPED_ARRAY_SHARED_BACKING.with(|r| {
        r.borrow_mut().insert(ptr as usize);
    });
}

pub(crate) fn typed_array_has_shared_backing(ptr: *const TypedArrayHeader) -> bool {
    let ptr = clean_ta_ptr(ptr);
    TYPED_ARRAY_SHARED_BACKING.with(|r| r.borrow().contains(&(ptr as usize)))
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
    if addr < 0x1000 {
        return ptr::null();
    }
    addr as *const TypedArrayHeader
}

#[inline]
pub(crate) fn data_ptr(ta: *const TypedArrayHeader) -> *const u8 {
    unsafe {
        if crate::native_arena::is_native_typed_view(ta) {
            crate::native_arena::native_view_data_ptr(ta)
        } else if let Some(p) = crate::typedarray_view::view_backing_data_ptr(ta as usize) {
            p as *const u8
        } else {
            (ta as *const u8).add(std::mem::size_of::<TypedArrayHeader>())
        }
    }
}

#[inline]
pub(crate) fn data_ptr_mut(ta: *mut TypedArrayHeader) -> *mut u8 {
    unsafe {
        if crate::native_arena::is_native_typed_view(ta as *const TypedArrayHeader) {
            crate::native_arena::native_view_data_ptr_mut(ta)
        } else if let Some(p) = crate::typedarray_view::view_backing_data_ptr(ta as usize) {
            p
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
    let ta = typed_array_for_byte_helper(ta)? as *const TypedArrayHeader;
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
    let ta = typed_array_for_byte_helper(ta as *const TypedArrayHeader)?;
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

pub fn typed_array_to_array_buffer(
    ta: *const TypedArrayHeader,
) -> *mut crate::buffer::BufferHeader {
    let Some(bytes) = (unsafe { typed_array_bytes(ta) }) else {
        return std::ptr::null_mut();
    };
    let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
    if buf.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        (*buf).length = bytes.len() as u32;
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::buffer::buffer_data_mut(buf),
                bytes.len(),
            );
        }
    }
    crate::buffer::mark_as_array_buffer(buf as usize);
    buf
}

unsafe fn typed_array_for_byte_helper(
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
    // length above `u32::MAX` cannot be represented (and the backing block
    // could never be allocated anyway). Node passes the `<= 2**53-1` length
    // check for these and then fails the actual allocation, so match its
    // `RangeError: Array buffer allocation failed` rather than silently
    // saturating the cast to `u32::MAX` (which produced a wrong-size array
    // or aborted the process in the allocator).
    if integer > u32::MAX as f64 {
        throw_range_error(b"Array buffer allocation failed");
    }
    integer as u32
}

#[inline]
fn is_arena_backed_addr(addr: usize) -> bool {
    !matches!(
        crate::arena::classify_heap_space(addr),
        crate::arena::HeapSpace::Unknown
    )
}

#[inline]
unsafe fn arena_payload_has_gc_type(addr: usize, expected_type: u8) -> bool {
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let header_addr = addr - crate::gc::GC_HEADER_SIZE;
    if matches!(
        crate::arena::classify_heap_space(header_addr),
        crate::arena::HeapSpace::Unknown
    ) {
        return false;
    }
    let header = header_addr as *const crate::gc::GcHeader;
    let obj_type = (*header).obj_type;
    if crate::gc::gc_type_info(obj_type).is_none() {
        return false;
    }
    let size = (*header).size as usize;
    if size < crate::gc::GC_HEADER_SIZE || size as u64 > (1u64 << 34) {
        return false;
    }
    if (*header).gc_flags & crate::gc::GC_FLAG_ARENA == 0 {
        return false;
    }
    obj_type == expected_type
}

#[inline]
unsafe fn validate_arena_payload_gc_type(addr: usize, expected_type: u8, message: &[u8]) {
    if is_arena_backed_addr(addr) && !arena_payload_has_gc_type(addr, expected_type) {
        throw_type_error(message);
    }
}

unsafe fn strict_typed_array_from_raw(
    raw: u64,
    expected_kind: Option<u8>,
    message: &[u8],
) -> *mut TypedArrayHeader {
    let addr = strip_nanbox(raw);
    if addr < 0x1000 {
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
        validate_arena_payload_gc_type(addr, crate::gc::GC_TYPE_TYPED_ARRAY, message);
    }
    ta
}

unsafe fn typed_array_raw_bytes(ta: *const TypedArrayHeader) -> (*const u8, usize) {
    let data = data_ptr(ta);
    let len = ((*ta).length as usize).saturating_mul((*ta).elem_size as usize);
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
    let len = ((*ta).length as usize).saturating_mul((*ta).elem_size as usize);
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
            crate::buffer::buffer_data(buffer),
            (*buffer).length as usize,
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
            crate::buffer::buffer_data_mut(buffer),
            (*buffer).length as usize,
        );
    }
    throw_type_error(b"NativeMemory.copy expects typed array views");
}

unsafe fn native_memory_copy_accepts_buffer(addr: usize) -> bool {
    if addr < 0x1000
        || !crate::buffer::is_registered_buffer(addr)
        || !crate::buffer::is_uint8array_buffer(addr)
    {
        return false;
    }
    if is_arena_backed_addr(addr) {
        return arena_payload_has_gc_type(addr, crate::gc::GC_TYPE_BUFFER);
    }
    true
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
    // 2026-07-09 audit: small typed arrays were raw-`alloc`'d with NO
    // GcHeader and never freed — invisible to every GC trigger, unbounded
    // RSS on churn. Every typed array now takes the old-arena GC path
    // (non-movable space: raw data pointers are handed out), reclaimed by
    // full-cycle block resets + the post-trace registry pruning below. The
    // bytes now also count toward `arena_total_bytes` trigger pressure.
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
    p
}

/// Post-trace registry pruning (mirrors the #6010 Map/Set pattern and the
/// buffer variant): registered typed arrays whose header is genuinely dead.
/// All GC-heap typed arrays are TENURED old-arena residents — deadness is
/// trustworthy only after a FULL trace. Native-arena views (different
/// obj_type) are filtered out; their own finalizers unregister them.
pub(crate) fn collect_dead_registered_typed_arrays_post_trace(full_trace: bool) -> Vec<usize> {
    if !full_trace {
        return Vec::new();
    }
    TYPED_ARRAY_REGISTRY.with(|r| {
        r.borrow()
            .keys()
            .copied()
            .filter(|&addr| unsafe { registered_typed_array_is_dead_post_trace(addr) })
            .collect()
    })
}

unsafe fn registered_typed_array_is_dead_post_trace(addr: usize) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    if header.obj_type != crate::gc::GC_TYPE_TYPED_ARRAY {
        return false;
    }
    header.gc_flags
        & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED | crate::gc::GC_FLAG_FORWARDED)
        == 0
}

/// Finalize one collected-dead typed array: `unregister_typed_array` clears
/// the registry entry, the global kind-cache slot, and the own-props /
/// no-extend side tables — closing both the leak and the address-reuse
/// (kind-cache ABA) hazard for ordinary typed arrays.
pub(crate) fn finalize_collected_dead_typed_array(addr: usize) {
    unregister_typed_array(addr as *const TypedArrayHeader);
    crate::buffer::view::remove_entries_for_dead_buffer(addr);
}

/// Convert an f64 (NaN-boxed JS value) to the numeric value to store. Strings
/// and undefined become 0/NaN.
pub(crate) fn jsvalue_to_f64(v: f64) -> f64 {
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
fn to_uint32_bits(value: f64) -> u32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let m = value.trunc().rem_euclid(4294967296.0); // value mod 2^32, in [0, 2^32)
    m as u32
}

/// Store a number into the typed array slot, performing the per-kind cast.
pub(crate) unsafe fn store_at(ta: *mut TypedArrayHeader, idx: usize, value: f64) {
    let kind = (*ta).kind;
    let elem_size = (*ta).elem_size as usize;
    let base = data_ptr_mut(ta);
    let off = idx * elem_size;
    match kind {
        KIND_INT8 => {
            let v = to_uint32_bits(value) as u8 as i8;
            *(base.add(off) as *mut i8) = v;
        }
        KIND_UINT8 => {
            *base.add(off) = to_uint32_bits(value) as u8;
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
        KIND_FLOAT16 => f16_bits_to_f64(*(base.add(off) as *const u16)),
        KIND_FLOAT32 => *(base.add(off) as *const f32) as f64,
        KIND_FLOAT64 => *(base.add(off) as *const f64),
        // BigInt kinds return a NaN-boxed BigInt (not a plain Number), so
        // `ta[i]` round-trips as a `bigint`. The raw slot bits are the BigInt's
        // low limb; widen via the signed/unsigned constructor for `> i64::MAX`.
        KIND_BIGINT64 => {
            let v = *(base.add(off) as *const i64);
            crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(v) as i64)
        }
        KIND_BIGUINT64 => {
            let v = *(base.add(off) as *const u64);
            crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_u64(v) as i64)
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
        if idx >= (*ta).length {
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
    if !(index.is_finite() && index >= 0.0 && index.fract() == 0.0 && index <= u32::MAX as f64) {
        return false;
    }
    let ta = ptr as *mut TypedArrayHeader;
    if crate::native_arena::is_native_typed_view(ta as *const TypedArrayHeader) {
        return false;
    }
    let idx = index as u32;
    unsafe {
        if idx < (*ta).length {
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
}
