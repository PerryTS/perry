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

use std::alloc::{alloc, Layout};
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
const TA_KIND_CACHE_SLOTS: usize = 64;
const TA_CACHE_NEGATIVE: u64 = 0xFF;
static TA_KIND_CACHE: [AtomicU64; TA_KIND_CACHE_SLOTS] =
    [const { AtomicU64::new(0) }; TA_KIND_CACHE_SLOTS];

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
    TA_KIND_CACHE[ta_kind_cache_slot(addr)].store(((addr as u64) << 8) | tag, Ordering::Relaxed);
}

#[inline]
fn ta_kind_cache_store(addr: usize, kind: u8) {
    ta_kind_cache_store_tag(addr, kind as u64);
}

#[inline]
fn ta_kind_cache_invalidate(addr: usize) {
    let slot = ta_kind_cache_slot(addr);
    let entry = TA_KIND_CACHE[slot].load(Ordering::Relaxed);
    if entry != 0 && (entry >> 8) as usize == addr {
        TA_KIND_CACHE[slot].store(0, Ordering::Relaxed);
    }
}

/// Cache probe: `None` = miss (consult the registry), `Some(None)` = cached
/// negative ("not a typed array"), `Some(Some(kind))` = cached typed array.
#[inline]
fn ta_kind_cache_get(addr: usize) -> Option<Option<u8>> {
    let entry = TA_KIND_CACHE[ta_kind_cache_slot(addr)].load(Ordering::Relaxed);
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
fn throw_type_error(message: &[u8]) -> ! {
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
fn typed_array_length_or_throw(val: f64) -> u32 {
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
            // #5067 — surface a catchable `RangeError` (Node's
            // `Array buffer allocation failed`) instead of aborting.
            throw_range_error(b"Array buffer allocation failed");
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
pub(super) unsafe fn store_at(ta: *mut TypedArrayHeader, idx: usize, value: f64) {
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

/// Allocate a typed array of `length` elements, all zero.
#[no_mangle]
pub extern "C" fn js_typed_array_new_empty(kind: i32, length: i32) -> *mut TypedArrayHeader {
    let len = typed_array_length_or_throw(length as f64);
    typed_array_alloc(kind as u8, len)
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
    // `new TA(arg)` with a non-object arg performs ToIndex(arg) = ToNumber(arg)
    // for the length. ToNumber(BigInt) and ToNumber(Symbol) are TypeErrors
    // (§7.1.4), so `new Int8Array(5n)` / `new Int8Array(Symbol())` must throw
    // rather than yielding an empty (BigInt) or garbage-copied (Symbol) array.
    if top16 == 0x7FFA {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    if top16 == 0x7FFD && unsafe { crate::symbol::js_is_symbol(val) } != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
    }
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
        if crate::buffer::is_registered_buffer(raw_addr) {
            if crate::buffer::is_any_array_buffer(raw_addr) {
                let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                return crate::typedarray_view::js_typed_array_view(
                    kind, val, undefined, undefined,
                );
            }
            return bigint::copy_from_uint8_buffer(
                kind as u8,
                raw_addr as *const crate::buffer::BufferHeader,
            );
        }
        // A plain object that is neither a typed array nor a buffer is consumed
        // per the spec's `new TypedArray(object)` path: if it exposes a
        // *callable* `@@iterator` it is iterated (InitializeTypedArrayFromList);
        // a non-callable non-nullish `@@iterator` is a TypeError; otherwise it
        // is read as an array-like (`ToLength(Get(obj, "length"))` then each
        // indexed element). Registered Maps/Sets keep the shared `Array.from`
        // materialization (their `@@iterator` is native, not a stored symbol
        // property). Functions are valid array-like/iterable sources too —
        // previously they were reinterpreted as an `ArrayHeader` (crash).
        if crate::map::is_registered_map(raw_addr)
            || crate::set::is_registered_set(raw_addr)
            || crate::array::is_builtin_iterator_class_id(raw_addr)
            || crate::object::js_util_types_is_generator_object(val).to_bits()
                == crate::value::TAG_TRUE
        {
            // Built-in iterables whose `@@iterator` is native (not a stored
            // symbol property): Maps/Sets, builtin iterator objects, and
            // generator objects (Perry generators carry own `next`/`return`
            // closures and no `@@iterator` symbol prop). The shared
            // `Array.from` materialization drives these correctly.
            let materialized = crate::array::js_array_from_value(val);
            return js_typed_array_new_from_array(kind, materialized);
        }
        if crate::closure::is_closure_ptr(raw_addr) {
            return unsafe { typed_array_from_plain_object(kind as u8, val) };
        }
        if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_hdr = (raw_addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if unsafe { (*gc_hdr).obj_type } == crate::gc::GC_TYPE_OBJECT {
                return unsafe { typed_array_from_plain_object(kind as u8, val) };
            }
        }
        return js_typed_array_new_from_array(kind, arr);
    }
    if top16 == 0x7FFE {
        // INT32_TAG — lower 32 bits are the signed length.
        let n = (bits & 0xFFFF_FFFF) as i32;
        let len = typed_array_length_or_throw(n as f64);
        return typed_array_alloc(kind as u8, len);
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
        // Plain IEEE double (including negative, NaN, ±Inf). Node applies
        // ToIndex: NaN → 0, truncate toward zero, and throw a RangeError on a
        // negative / out-of-range length (#3662).
        let len = typed_array_length_or_throw(val);
        return typed_array_alloc(kind as u8, len);
    }
    // Undefined → ToIndex(undefined) = 0. Null / bool / string run through
    // ToNumber then ToIndex, so `new TA(true)` and `new TA('1')` have length
    // 1 (previously all of these built an empty array).
    if bits == crate::value::TAG_UNDEFINED {
        return typed_array_alloc(kind as u8, 0);
    }
    let len = typed_array_length_or_throw(jsvalue_to_f64(val));
    typed_array_alloc(kind as u8, len)
}

/// `new TA(object)` for a plain object / function source (ES2024 §23.2.5.1
/// step 6.b.iii, InitializeTypedArrayFromList / InitializeTypedArrayFromArrayLike).
///
///   - `GetMethod(obj, @@iterator)`: a non-nullish, non-callable value is a
///     TypeError; a callable one drives the iterator protocol (each `next()`
///     may throw — propagate).
///   - Otherwise array-like: `len = ToLength(? Get(obj, "length"))` (a Symbol
///     length is a TypeError, a `valueOf` runs and may throw), then each
///     indexed element is read and coerced per kind (`ToNumber`/`ToBigInt`,
///     both observable / throwing).
///
/// Element values are fully collected BEFORE coercion begins, mirroring the
/// snapshot rule in `js_typed_array_new_from_array`.
unsafe fn typed_array_from_plain_object(kind: u8, val: f64) -> *mut TypedArrayHeader {
    let raw = typed_array_plain_object_values(val);
    typed_array_from_snapshot(kind, raw)
}

/// Collect the raw (uncoerced) element values of a plain-object / function
/// source per the spec's iterator-or-array-like resolution (see
/// `typed_array_from_plain_object` doc above). Observable: the `@@iterator`
/// validation/iteration, the `ToLength(Get(obj, "length"))` coercion, and
/// each indexed `Get` all run here and may throw.
unsafe fn typed_array_plain_object_values(val: f64) -> Vec<f64> {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let iter_wk = crate::symbol::well_known_symbol("iterator");
    let using_iter = if iter_wk.is_null() {
        undefined
    } else {
        let sym = f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
        crate::symbol::js_object_get_symbol_property(val, sym)
    };
    let ub = using_iter.to_bits();
    if ub != crate::value::TAG_UNDEFINED && ub != crate::value::TAG_NULL {
        let fn_raw = crate::value::js_nanbox_get_pointer(using_iter) as usize;
        if fn_raw < 0x10000 || !crate::closure::is_closure_ptr(fn_raw) {
            throw_type_error(b"object is not iterable");
        }
        let bound = crate::closure::clone_closure_rebind_this(using_iter.to_bits(), val);
        let iter = crate::closure::js_native_call_value(f64::from_bits(bound), ptr::null(), 0);
        let mut raw: Vec<f64> = Vec::new();
        while let Some(v) = crate::collection_iter::iterator_next_value(iter) {
            raw.push(v);
        }
        return raw;
    }
    // Array-like path.
    let len_val = object_like_get(val, "length");
    let n = jsvalue_to_f64(len_val);
    // ToLength: NaN / negative → 0, clamp to 2^53-1.
    let len = if n.is_nan() || n <= 0.0 {
        0.0
    } else {
        n.trunc().min(9_007_199_254_740_991.0)
    };
    // AllocateTypedArrayBuffer implementation limit (Node throws RangeError
    // for lengths past the max typed-array size).
    if len > u32::MAX as f64 {
        throw_range_error(format!("Invalid typed array length: {}", len as u64).as_bytes());
    }
    let len = len as u32;
    let mut raw: Vec<f64> = Vec::with_capacity(len as usize);
    for k in 0..len {
        raw.push(object_like_get(val, &k.to_string()));
    }
    raw
}

/// Collect the raw (uncoerced) source values for `%TypedArray%.from(source)`:
/// plain-object / function sources use the spec iterator-or-array-like
/// resolution (so a throwing `length` getter / `ToLength(Symbol)` / a
/// non-callable `@@iterator` propagate); every other shape (arrays, strings,
/// Maps, Sets, iterators, generators, buffers) goes through the shared
/// `Array.from` materialization.
pub(crate) unsafe fn typed_array_from_source_raw_values(val: f64) -> Vec<f64> {
    let bits = val.to_bits();
    if (bits >> 48) == 0x7FFD {
        let raw_addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        let special = crate::map::is_registered_map(raw_addr)
            || crate::set::is_registered_set(raw_addr)
            || crate::array::is_builtin_iterator_class_id(raw_addr)
            || crate::object::js_util_types_is_generator_object(val).to_bits()
                == crate::value::TAG_TRUE
            || lookup_typed_array_kind(raw_addr).is_some()
            || crate::buffer::is_registered_buffer(raw_addr)
            || crate::symbol::js_is_symbol(val) != 0;
        if !special {
            if crate::closure::is_closure_ptr(raw_addr) {
                return typed_array_plain_object_values(val);
            }
            if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
                let gc_hdr = (raw_addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_hdr).obj_type == crate::gc::GC_TYPE_OBJECT {
                    return typed_array_plain_object_values(val);
                }
            }
        }
    }
    let arr = crate::array::js_array_from_value(val);
    let len = crate::array::js_array_length(arr);
    (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i))
        .collect()
}

/// Coerce a snapshot of raw element values per `kind` (observable, may throw)
/// and store them into a freshly allocated typed array.
unsafe fn typed_array_from_snapshot(kind: u8, raw: Vec<f64>) -> *mut TypedArrayHeader {
    let vals: Vec<f64> = raw
        .into_iter()
        .map(|v| bigint::coerce_for_kind(kind, v))
        .collect();
    let ta = typed_array_alloc(kind, vals.len() as u32);
    for (i, v) in vals.iter().enumerate() {
        store_at(ta, i, *v);
    }
    ta
}

/// `Get(obj, name)` for a plain-object or function source value.
unsafe fn object_like_get(val: f64, name: &str) -> f64 {
    let raw = crate::value::js_nanbox_get_pointer(val) as usize;
    if crate::closure::is_closure_ptr(raw) {
        return crate::closure::closure_get_dynamic_prop(raw, name);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let v =
        crate::object::js_object_get_field_by_name(raw as *const crate::object::ObjectHeader, key);
    f64::from_bits(v.bits())
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
        bigint::validate_copy_kinds(dst_kind, (*src).kind);
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
        // Snapshot the raw source values BEFORE any coercion. Per spec the
        // source list is fully collected first and only THEN are the elements
        // converted (`ToNumber`/`ToBigInt`) and stored. A converting element can
        // run user code (`valueOf`/`Symbol.toPrimitive`) that mutates the source
        // array — `Int32Array.from([0, { valueOf() { src.length = 0; return 100 }}, 2])`
        // must still yield `[0, 100, 2]`, not lose the trailing element. Reading
        // raw values first also keeps the snapshot ahead of the `typed_array_alloc`
        // GC point (#871).
        let raw: Vec<f64> = (0..len)
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect();
        let vals: Vec<f64> = raw
            .into_iter()
            .map(|v| bigint::coerce_for_kind(kind, v))
            .collect();
        let ta = typed_array_alloc(kind, len);
        for (i, v) in vals.iter().enumerate() {
            store_at(ta, i, *v);
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
///   * a numeric (non-string) key → integer-indexed element read.
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

/// Spec default sort order for typed-array Numbers (`%TypedArray%.prototype.
/// sort` without a comparator): ascending, every NaN at the end, and `-0`
/// before `+0`. `partial_cmp` got neither right (NaN compared `Equal` so NaNs
/// stayed in place; `-0 == +0` left zeros in input order).
fn typed_array_default_number_cmp(a: &f64, b: &f64) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.total_cmp(b),
    }
}

/// Default-sort `ta`'s elements in place. BigInt kinds sort the raw 64-bit
/// lanes (signed/unsigned) — `load_at` boxes each element as a fresh BigInt
/// pointer, and sorting those bit patterns scrambled the array.
unsafe fn typed_array_sort_default_in_place(ta: *mut TypedArrayHeader) {
    let len = (*ta).length as usize;
    if len <= 1 {
        return;
    }
    match (*ta).kind {
        KIND_BIGINT64 => {
            let base = data_ptr_mut(ta) as *mut i64;
            std::slice::from_raw_parts_mut(base, len).sort_unstable();
        }
        KIND_BIGUINT64 => {
            let base = data_ptr_mut(ta) as *mut u64;
            std::slice::from_raw_parts_mut(base, len).sort_unstable();
        }
        _ => {
            let mut buf: Vec<f64> = (0..len).map(|i| load_at(ta, i)).collect();
            buf.sort_by(typed_array_default_number_cmp);
            for (i, v) in buf.into_iter().enumerate() {
                store_at(ta, i, v);
            }
        }
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
        typed_array_sort_default_in_place(ta_clean);
        ta_clean
    }
}

/// `ta.sort(cmp)` — in-place sort with comparator. Issue #654.
#[no_mangle]
pub extern "C" fn js_typed_array_sort_with_comparator(
    ta: *mut TypedArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    // #2796: null comparator (validated `undefined`) -> default sort.
    if comparator.is_null() {
        return js_typed_array_sort_default(ta);
    }
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
        // Copy the raw lanes, then reuse the in-place default sort (BigInt
        // kinds sort raw 64-bit lanes; Number kinds use the spec NaN/-0 order).
        let elem = (*ta).elem_size as usize;
        ptr::copy_nonoverlapping(data_ptr(ta), data_ptr_mut(out), len * elem);
        typed_array_sort_default_in_place(out);
        out
    }
}

/// `ta.toSorted(cmp)`.
#[no_mangle]
pub extern "C" fn js_typed_array_to_sorted_with_comparator(
    ta: *const TypedArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut TypedArrayHeader {
    // #2796: null comparator (validated `undefined`) -> default sort.
    if comparator.is_null() {
        return js_typed_array_to_sorted_default(ta);
    }
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
        // ECMA ToIntegerOrInfinity: NaN -> 0, reject non-finite / out-of-range
        // with RangeError("Invalid typed array index") (Node parity, #2792).
        let rel = if index.is_nan() { 0.0 } else { index };
        if !rel.is_finite() {
            throw_range_error(b"Invalid typed array index");
        }
        let resolved = if rel < 0.0 { rel + len as f64 } else { rel };
        if resolved < 0.0 || resolved >= len as f64 {
            throw_range_error(b"Invalid typed array index");
        }
        let idx = resolved as i64;
        let replacement = bigint::coerce_for_kind(kind, value);
        let out = typed_array_alloc(kind, len as u32);
        for i in 0..len {
            if i as i64 == idx {
                store_at(out, i, replacement);
            } else {
                store_at(out, i, load_at(ta, i));
            }
        }
        out
    }
}

/// NaN-box a TypedArray header pointer as the JS `array` receiver value passed
/// as the 3rd/4th callback argument. Per spec the callback observes the
/// original typed-array receiver.
#[inline(always)]
fn ta_receiver_value(ta: *const TypedArrayHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(ta as *const u8).bits())
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
        let recv = ta_receiver_value(ta);
        for i in (0..len).rev() {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
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
        let recv = ta_receiver_value(ta);
        for i in (0..len).rev() {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return i as f64;
            }
        }
        -1.0
    }
}

// %TypedArray%.prototype iteration methods. The generic `js_array_*` helpers
// detect a TypedArray receiver via `lookup_typed_array_kind` and delegate
// here (mirroring the existing sort / at / findLast delegation), so these
// read elements through the element-typed `load_at` instead of reinterpreting
// the raw int/float storage as NaN-boxed f64 (which produced garbage values).
// The callback receives `(element, index)` — same 2-arg convention the rest of
// this file and the generic array helpers use.

/// `ta.map(cb)` — returns a NEW TypedArray of the SAME kind (per spec, not a
/// plain Array). Each result is coerced back to the element type via the same
/// `jsvalue_to_f64` path `ta[i] = v` uses.
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
        let recv = ta_receiver_value(ta);
        // 23.2.3.20 step 5: A is TypedArraySpeciesCreate(O, « len ») — BEFORE
        // the callback loop (so a throwing constructor/@@species getter aborts
        // before any callback runs).
        let choice = species::species_constructor(ta as usize, kind);
        let result = species::species_create_length(&choice, kind, len);
        let Some(result_addr) = crate::typedarray_props::typed_array_addr_from_value(result) else {
            return species::result_as_ptr(result);
        };
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            crate::typedarray_props::species_result_store(result_addr, i, r);
        }
        species::result_as_ptr(result)
    }
}

/// `ta.filter(cb)` — returns a NEW TypedArray of the SAME kind holding the
/// elements for which `cb` returned truthy.
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
        let recv = ta_receiver_value(ta);
        // 23.2.3.10: the callback runs for every element FIRST (collecting the
        // kept values), THEN A = TypedArraySpeciesCreate(O, « captured »). The
        // @@species getter is therefore observed after all callbacks.
        let mut kept: Vec<f64> = Vec::new();
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                kept.push(v);
            }
        }
        let choice = species::species_constructor(ta as usize, kind);
        let result = species::species_create_length(&choice, kind, kept.len());
        let Some(result_addr) = crate::typedarray_props::typed_array_addr_from_value(result) else {
            return species::result_as_ptr(result);
        };
        for (i, v) in kept.into_iter().enumerate() {
            crate::typedarray_props::species_result_store(result_addr, i, v);
        }
        species::result_as_ptr(result)
    }
}

/// `ta.every(cb)` — NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_every(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_TRUE);
    }
    unsafe {
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) == 0 {
                return f64::from_bits(crate::value::TAG_FALSE);
            }
        }
        f64::from_bits(crate::value::TAG_TRUE)
    }
}

/// `ta.some(cb)` — NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_typed_array_some(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    unsafe {
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return f64::from_bits(crate::value::TAG_TRUE);
            }
        }
        f64::from_bits(crate::value::TAG_FALSE)
    }
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
            let recv = ta_receiver_value(ta);
            for i in 0..len {
                let v = load_at(ta, i);
                let _ = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `ta.find(cb)` — first element for which `cb` is truthy, else undefined.
#[no_mangle]
pub extern "C" fn js_typed_array_find(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return v;
            }
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// `ta.findIndex(cb)` — first matching index as plain f64, else -1.
#[no_mangle]
pub extern "C" fn js_typed_array_find_index(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return -1.0;
    }
    unsafe {
        let len = (*ta).length as usize;
        let recv = ta_receiver_value(ta);
        for i in 0..len {
            let v = load_at(ta, i);
            let r = crate::closure::js_closure_call3(callback, v, i as f64, recv);
            if crate::value::js_is_truthy(r) != 0 {
                return i as f64;
            }
        }
        -1.0
    }
}

/// `ta.reduce(cb, initial?)` — accumulate left→right. Reads elements through
/// `load_at` (element-typed) and calls the reducer as
/// `(accumulator, currentValue, currentIndex, array)`. Throws
/// `TypeError: Reduce of empty array with no initial value` when the typed
/// array is empty and no initial value was provided. Issue #2799.
#[no_mangle]
pub extern "C" fn js_typed_array_reduce(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        if has_initial != 0 {
            return initial;
        }
        crate::array::throw_reduce_of_empty();
    }
    unsafe {
        let len = (*ta).length as usize;
        if len == 0 {
            if has_initial != 0 {
                return initial;
            }
            crate::array::throw_reduce_of_empty();
        }
        let recv = ta_receiver_value(ta);
        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, 0)
        } else {
            (load_at(ta, 0), 1)
        };
        for i in start_idx..len {
            let v = load_at(ta, i);
            accumulator =
                crate::closure::js_closure_call4(callback, accumulator, v, i as f64, recv);
        }
        accumulator
    }
}

/// `ta.reduceRight(cb, initial?)` — accumulate right→left. Same reducer
/// contract as `js_typed_array_reduce`. Issue #2799.
#[no_mangle]
pub extern "C" fn js_typed_array_reduce_right(
    ta: *const TypedArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        if has_initial != 0 {
            return initial;
        }
        crate::array::throw_reduce_of_empty();
    }
    unsafe {
        let len = (*ta).length as usize;
        if len == 0 {
            if has_initial != 0 {
                return initial;
            }
            crate::array::throw_reduce_of_empty();
        }
        let recv = ta_receiver_value(ta);
        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, len)
        } else {
            (load_at(ta, len - 1), len - 1)
        };
        if start_idx > 0 {
            for i in (0..start_idx).rev() {
                let v = load_at(ta, i);
                accumulator =
                    crate::closure::js_closure_call4(callback, accumulator, v, i as f64, recv);
            }
        }
        accumulator
    }
}

// #3148: %TypedArray%.prototype join / slice / reverse / fill / subarray.
// (reduce/reduceRight/copyWithin/set_from/findIndex live above — added separately.)
/// `ta.join(sep?)` — Number→String each element (Node formatting), joined by
/// `sep` (default ","). Returns a heap StringHeader.
#[no_mangle]
pub extern "C" fn js_typed_array_join(
    ta: *const TypedArrayHeader,
    separator: *const crate::string::StringHeader,
) -> *mut crate::string::StringHeader {
    use crate::string::{js_string_from_bytes, StringHeader};
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return js_string_from_bytes(b"".as_ptr(), 0);
    }
    unsafe {
        let len = (*ta).length as usize;
        if len == 0 {
            return js_string_from_bytes(ptr::null(), 0);
        }
        let kind = (*ta).kind;
        let sep_str = if separator.is_null() {
            ","
        } else {
            let sep_len = (*separator).byte_len as usize;
            let sep_data = (separator as *const u8).add(std::mem::size_of::<StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(sep_data, sep_len))
        };
        let mut result = String::new();
        for i in 0..len {
            if i > 0 {
                result.push_str(sep_str);
            }
            result.push_str(&format::format_typed_value(kind, load_at(ta, i), false));
        }
        let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
        std::hint::black_box(&result);
        drop(result);
        ret
    }
}

/// `ta.join(sepValue)` — NaN-boxed-separator entry point mirroring
/// `js_array_join_value`.
#[no_mangle]
pub extern "C" fn js_typed_array_join_value(
    ta: *const TypedArrayHeader,
    separator_value: f64,
) -> *mut crate::string::StringHeader {
    let separator = if separator_value.to_bits() == crate::value::TAG_UNDEFINED {
        ptr::null()
    } else {
        // `ToString(separator)`: a Symbol separator is a TypeError (§7.1.17),
        // not a "Symbol(…)" rendering.
        if unsafe { crate::symbol::js_is_symbol(separator_value) } != 0 {
            throw_type_error(b"Cannot convert a Symbol value to a string");
        }
        crate::value::js_jsvalue_to_string(separator_value) as *const crate::string::StringHeader
    };
    js_typed_array_join(ta, separator)
}

/// `ta.slice(start, end?)` — returns a NEW same-kind TypedArray with the
/// selected elements. Mirrors `js_array_slice` index normalization.
#[no_mangle]
pub extern "C" fn js_typed_array_slice(
    ta: *const TypedArrayHeader,
    start: i32,
    end: i32,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as i32;
        let start_idx = if start < 0 {
            (len + start).max(0) as u32
        } else {
            (start as u32).min(len as u32)
        };
        let end_idx = if end == i32::MAX {
            len as u32
        } else if end < 0 {
            (len + end).max(0) as u32
        } else {
            (end as u32).min(len as u32)
        };
        let slice_len = end_idx.saturating_sub(start_idx);
        // 23.2.3.27 step 10: A = TypedArraySpeciesCreate(O, « count »).
        let choice = species::species_constructor(ta as usize, kind);
        let result = species::species_create_length(&choice, kind, slice_len as usize);
        if slice_len > 0 {
            if let species::SpeciesChoice::Default = choice {
                // Fast same-kind path: raw byte-copy preserves exact element
                // bits — e.g. Float NaN payloads (`slice/bit-precision`), which
                // a load→f64→store round-trip would canonicalize.
                let out = species::result_as_ptr(result);
                let esz = elem_size_for_kind(kind);
                let src = (data_ptr(ta) as *const u8).add(start_idx as usize * esz);
                let dst = data_ptr_mut(out);
                ptr::copy_nonoverlapping(src, dst, slice_len as usize * esz);
            } else {
                species::copy_range_into(result, ta, start_idx as usize, slice_len as usize);
            }
        }
        species::result_as_ptr(result)
    }
}

/// `ta.reverse()` — in-place reversal; returns the same typed array.
#[no_mangle]
pub extern "C" fn js_typed_array_reverse(ta: *mut TypedArrayHeader) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() {
        return ta;
    }
    unsafe {
        let len = (*ta).length as usize;
        if len <= 1 {
            return ta;
        }
        let mut i = 0usize;
        let mut j = len - 1;
        while i < j {
            let a = load_at(ta, i);
            let b = load_at(ta, j);
            store_at(ta, i, b);
            store_at(ta, j, a);
            i += 1;
            j -= 1;
        }
        ta
    }
}

/// `ta.fill(value, start?, end?)` — in-place fill; returns the same typed
/// array. `start`/`end` follow Array.prototype.fill index normalization; pass
/// `has_start == 0` to fill the whole array.
#[no_mangle]
pub extern "C" fn js_typed_array_fill(
    ta: *mut TypedArrayHeader,
    value: f64,
    has_start: i32,
    start: f64,
    has_end: i32,
    end: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() {
        return ta;
    }
    unsafe {
        let len = (*ta).length as isize;
        // Spec order: convert `value` first (its `valueOf`/`ToBigInt` runs before
        // the index args are coerced), then `ToIntegerOrInfinity` each index.
        let v = bigint::coerce_for_kind((*ta).kind, value);
        // `ToIntegerOrInfinity` + RelativeIndex clamp. `jsvalue_to_f64` performs
        // `ToNumber` (so `null` → 0, `true` → 1, an object → its `valueOf`, a
        // numeric string → its value); `NaN`/`undefined` → 0, ±Infinity saturate
        // to the array bounds. The previous `x.is_nan() ? default : x as isize`
        // mis-handled every NaN-boxed non-number: `null`/`false`/`undefined` all
        // looked like `NaN` and fell back to the *default* (so a `null` end
        // became `len` instead of 0).
        let rel = |x: f64| -> isize {
            let n = jsvalue_to_f64(x);
            let n = if n.is_nan() { 0.0 } else { n };
            let mut idx = if !n.is_finite() {
                if n > 0.0 {
                    len
                } else {
                    0
                }
            } else {
                n.trunc() as isize
            };
            if idx < 0 {
                idx += len;
            }
            idx.clamp(0, len)
        };
        let is_undef = |x: f64| crate::value::JSValue::from_bits(x.to_bits()).is_undefined();
        let s = if has_start != 0 { rel(start) } else { 0 };
        // An explicit `undefined` end defaults to `len` (spec step 8a), unlike a
        // `null`/absent-coerced end which is `ToIntegerOrInfinity(null)` = 0.
        let e = if has_end != 0 && !is_undef(end) {
            rel(end)
        } else {
            len
        };
        let mut i = s;
        while i < e {
            store_at(ta, i as usize, v);
            i += 1;
        }
        ta
    }
}

/// `ta.subarray(begin?, end?)` — returns a NEW same-kind TypedArray that
/// COPIES the selected range. (Perry materializes rather than aliasing the
/// backing store; observationally identical for reads and independent writes
/// of the common cases #3148 targets.)
#[no_mangle]
pub extern "C" fn js_typed_array_subarray(
    ta: *const TypedArrayHeader,
    has_begin: i32,
    begin: f64,
    has_end: i32,
    end: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() || lookup_typed_array_kind(ta as usize).is_none() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as i32;
        // `ToIntegerOrInfinity` + RelativeIndex clamp. `js_number_coerce`
        // performs `ToNumber` (running a `valueOf`/`Symbol.toPrimitive`, which
        // may throw) — done BEFORE the species lookup, per spec order.
        let norm = |has: i32, v: f64, default: i32| -> i32 {
            // Absent OR explicit `undefined` → the default (begin→0, end→len).
            if has == 0 || crate::value::JSValue::from_bits(v.to_bits()).is_undefined() {
                return default;
            }
            let n = crate::builtins::js_number_coerce(v);
            if n.is_nan() {
                return 0;
            }
            let mut x = if !n.is_finite() {
                if n > 0.0 {
                    len
                } else {
                    i32::MIN
                }
            } else {
                n.trunc() as i32
            };
            if x < 0 {
                x = x.saturating_add(len);
            }
            x.clamp(0, len)
        };
        let b = norm(has_begin, begin, 0);
        let e = norm(has_end, end, len);
        let count = (e - b).max(0) as u32;
        // 23.2.3.30: SpeciesCreate(O, « buffer, beginByteOffset, newLength »).
        // A subarray is a VIEW sharing the backing buffer (default and custom).
        let choice = species::species_constructor(ta as usize, kind);
        let elem = elem_size_for_kind(kind) as u32;
        let buffer = crate::typedarray_view::js_typed_array_backing_buffer(ta);
        let byte_offset =
            crate::typedarray_view::js_typed_array_byte_offset(ta) + (b as u32) * elem;
        let buffer_val = crate::value::js_nanbox_pointer(buffer as i64);
        let off_val = byte_offset as f64;
        let len_val = count as f64;
        match choice {
            species::SpeciesChoice::Default => crate::typedarray_view::js_typed_array_view(
                kind as i32,
                buffer_val,
                off_val,
                len_val,
            ),
            species::SpeciesChoice::Custom(c) => {
                let result = species::species_create_args(c, &[buffer_val, off_val, len_val]);
                species::result_as_ptr(result)
            }
        }
    }
}

/// Format a single typed-array element. `bigint_suffix` controls whether a
/// `BigInt64`/`BigUint64` element renders with the trailing `n` (true for the
/// `console.log` inspect form `BigInt64Array(1) [ 5n ]`, false for `join`,
/// which calls plain `ToString` on each element → `"5"`).
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
