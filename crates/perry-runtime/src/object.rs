//! Object representation for Perry
//!
//! Objects are heap-allocated with a header containing:
//! - Class ID (for type checking and vtable lookup)
//! - Field count
//! - Keys array pointer (for Object.keys() support)
//! - Fields array (inline)

use crate::JSValue;
use crate::ArrayHeader;
use crate::arena::arena_alloc_gc;
use std::cell::{Cell, RefCell};
use std::ptr;
use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Overflow field storage for objects that exceed their pre-allocated inline slot count.
/// Keyed by (obj_ptr as usize) -> Vec<JSValue bits> indexed by absolute field_index
/// (inline slots 0..alloc_limit remain `TAG_UNDEFINED` placeholders in the Vec;
/// they're never read since the inline slots are checked first).
///
/// Was a `HashMap<usize, HashMap<usize, u64>>` through v0.5.29 — the inner HashMap
/// dominated the row-decode hot path: a 20-property row object touches the overflow
/// storage on each of its 12 post-8-slot writes, and HashMap ops (hash + probe +
/// mut insert) cost ~40-50ns each. Flat `Vec<u64>` is ~5ns per append + index;
/// removes most of the residual gap after the shape-transition cache landed.
///
/// This handles cases like Object.assign() adding many fields to an object
/// that was allocated with only 8 slots (e.g., @noble/curves Fp field with 21 properties).
thread_local! {
    static OVERFLOW_FIELDS: RefCell<HashMap<usize, Vec<u64>>> = RefCell::new(HashMap::new());
}

/// Last-accessed overflow Vec cache — one entry, keyed by `obj_ptr`.
/// Skips the outer HashMap lookup on consecutive writes to the same
/// object (exactly the row-build pattern: a single object gets its
/// overflow slots filled back-to-back). Refreshed on every slow-path
/// HashMap access; invalidated by `clear_overflow_for_ptr` when GC
/// sweep frees the corresponding object.
///
/// Safety: the cached pointer references the `Vec<u64>` struct stored
/// inside a HashMap bucket. That struct only moves when the HashMap
/// resizes, which only happens on `entry().or_default()` inserting a
/// fresh key. The slow path below does both the potentially-resizing
/// call and the cache refresh inside a single `OVERFLOW_FIELDS.with`
/// closure, so no other thread-local mutation can interleave between
/// obtaining `&mut Vec` and caching its address.
thread_local! {
    static OVERFLOW_LAST: std::cell::UnsafeCell<(usize, *mut Vec<u64>)> =
        std::cell::UnsafeCell::new((0, std::ptr::null_mut()));
}

/// Read the u64 bits stored at `field_index` for `obj`, or `None` if absent.
/// Positions never written are stored as `TAG_UNDEFINED`; this helper reports
/// them as `None` so callers can return JS `undefined` uniformly with the
/// "no Vec entry at all" case.
#[inline]
fn overflow_get(obj_ptr: usize, field_index: usize) -> Option<u64> {
    OVERFLOW_FIELDS.with(|m| {
        m.borrow()
            .get(&obj_ptr)
            .and_then(|v| v.get(field_index).copied())
            .filter(|&bits| bits != crate::value::TAG_UNDEFINED)
    })
}

/// Write `vbits` to the overflow slot `field_index` for `obj`. Grows the
/// per-object `Vec` to `field_index + 1` with `TAG_UNDEFINED` fillers if
/// needed (filler slots correspond to the object's inline region and are
/// never read).
///
/// Fast path skips the outer HashMap when `obj_ptr` matches the last-
/// accessed Vec — the common row-build pattern where an object's
/// overflow slots fill in sequence.
#[inline]
fn overflow_set(obj_ptr: usize, field_index: usize, vbits: u64) {
    let hit = OVERFLOW_LAST.with(|c| unsafe {
        let (cached_obj, cached_vec) = *c.get();
        if cached_obj == obj_ptr && !cached_vec.is_null() {
            let v = &mut *cached_vec;
            if v.len() <= field_index {
                v.resize(field_index + 1, crate::value::TAG_UNDEFINED);
            }
            *v.get_unchecked_mut(field_index) = vbits;
            true
        } else {
            false
        }
    });
    if hit {
        return;
    }
    OVERFLOW_FIELDS.with(|m| {
        let mut map = m.borrow_mut();
        let v = map.entry(obj_ptr).or_default();
        if v.len() <= field_index {
            v.resize(field_index + 1, crate::value::TAG_UNDEFINED);
        }
        v[field_index] = vbits;
        let vec_ptr = v as *mut Vec<u64>;
        OVERFLOW_LAST.with(|c| unsafe {
            *c.get() = (obj_ptr, vec_ptr);
        });
    });
}

/// Per-property attribute flags set by `Object.defineProperty` / `Object.freeze` / `Object.seal`.
/// Tracks the JS PropertyDescriptor attributes (writable, enumerable, configurable) for keys
/// that have been customized away from the default `{ writable: true, enumerable: true, configurable: true }`.
/// Keyed by (obj_ptr as usize, key_string) -> attribute bitmask.
///
/// Bit layout: 0x01 = writable, 0x02 = enumerable, 0x04 = configurable.
/// Default (no entry) is `0x07` (all true). An entry of `0x06` means non-writable but enumerable+configurable.
#[derive(Clone, Copy)]
pub(crate) struct PropertyAttrs {
    pub bits: u8,
}
impl PropertyAttrs {
    const WRITABLE: u8 = 0x01;
    const ENUMERABLE: u8 = 0x02;
    const CONFIGURABLE: u8 = 0x04;
    pub const fn new(writable: bool, enumerable: bool, configurable: bool) -> Self {
        let mut bits = 0u8;
        if writable { bits |= Self::WRITABLE; }
        if enumerable { bits |= Self::ENUMERABLE; }
        if configurable { bits |= Self::CONFIGURABLE; }
        Self { bits }
    }
    pub const fn writable(self) -> bool { (self.bits & Self::WRITABLE) != 0 }
    pub const fn enumerable(self) -> bool { (self.bits & Self::ENUMERABLE) != 0 }
    pub const fn configurable(self) -> bool { (self.bits & Self::CONFIGURABLE) != 0 }
}

thread_local! {
    pub(crate) static PROPERTY_DESCRIPTORS: RefCell<HashMap<(usize, String), PropertyAttrs>> = RefCell::new(HashMap::new());
}

/// Accessor descriptor storage: maps (obj_ptr, key) -> (get_closure_bits, set_closure_bits).
/// A zero bits value means "no getter" or "no setter". Entries here represent properties
/// installed via `Object.defineProperty(obj, key, { get, set })` — those must route reads
/// through the getter closure and writes through the setter closure instead of touching
/// the underlying field slot.
#[derive(Clone, Copy, Default)]
pub(crate) struct AccessorDescriptor {
    pub get: u64, // NaN-boxed closure f64 bits, 0 = absent
    pub set: u64, // NaN-boxed closure f64 bits, 0 = absent
}

thread_local! {
    pub(crate) static ACCESSOR_DESCRIPTORS: RefCell<HashMap<(usize, String), AccessorDescriptor>> = RefCell::new(HashMap::new());
    /// Fast-path gate: `false` when no accessor descriptors have ever been installed
    /// on this thread, so hot `js_object_get_field_by_name` / `set_field_by_name`
    /// can skip the `ACCESSOR_DESCRIPTORS` HashMap lookup entirely.
    pub(crate) static ACCESSORS_IN_USE: Cell<bool> = const { Cell::new(false) };
    /// Fast-path gate for `PROPERTY_DESCRIPTORS` — flipped the first time
    /// `Object.defineProperty` (or freeze/seal via `set_property_attrs`)
    /// installs a per-property descriptor. Lets the hot object-write path
    /// skip the `.to_string()` allocation required to look up a descriptor
    /// that almost never exists.
    pub(crate) static PROPERTY_ATTRS_IN_USE: Cell<bool> = const { Cell::new(false) };
}

/// Global monotonic flag: set once any accessor or property descriptor is
/// installed.  Checked on every dynamic property write via a single
/// `Relaxed` load (no TLS overhead, no fence on aarch64/x86).
static GLOBAL_DESCRIPTORS_IN_USE: AtomicBool = AtomicBool::new(false);

/// Look up the property descriptor for (obj, key). Returns None if no entry exists,
/// in which case the JS default `{ writable: true, enumerable: true, configurable: true }` applies.
pub(crate) fn get_property_attrs(obj: usize, key: &str) -> Option<PropertyAttrs> {
    PROPERTY_DESCRIPTORS.with(|m| m.borrow().get(&(obj, key.to_string())).copied())
}

/// Store a property descriptor for (obj, key).
pub(crate) fn set_property_attrs(obj: usize, key: String, attrs: PropertyAttrs) {
    PROPERTY_ATTRS_IN_USE.with(|c| c.set(true));
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    PROPERTY_DESCRIPTORS.with(|m| { m.borrow_mut().insert((obj, key), attrs); });
}

/// Look up the accessor descriptor (get/set) for (obj, key).
pub(crate) fn get_accessor_descriptor(obj: usize, key: &str) -> Option<AccessorDescriptor> {
    ACCESSOR_DESCRIPTORS.with(|m| m.borrow().get(&(obj, key.to_string())).copied())
}

/// Store an accessor descriptor for (obj, key).
pub(crate) fn set_accessor_descriptor(obj: usize, key: String, acc: AccessorDescriptor) {
    ACCESSORS_IN_USE.with(|c| c.set(true));
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    ACCESSOR_DESCRIPTORS.with(|m| { m.borrow_mut().insert((obj, key), acc); });
}

/// Walk the keys array of `obj` and apply the given attribute mask AND filter to every existing key.
/// Used by `Object.freeze` (drops `writable` + `configurable`) and `Object.seal` (drops `configurable`).
unsafe fn mark_all_keys(obj: *mut ObjectHeader, drop_writable: bool, _drop_enumerable: bool, drop_configurable: bool) {
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return;
    }
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count == 0 || key_count > 65536 {
        return;
    }
    let obj_addr = obj as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let stored_key = key_val.as_string_ptr();
        if stored_key.is_null() {
            continue;
        }
        let name_ptr = (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*stored_key).byte_len as usize;
        let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let key_str = match std::str::from_utf8(name_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => continue,
        };
        // Start from existing attrs (or default `{w:true, e:true, c:true}`) and clear bits.
        let mut attrs = get_property_attrs(obj_addr, &key_str)
            .unwrap_or(PropertyAttrs::new(true, true, true));
        if drop_writable { attrs.bits &= !PropertyAttrs::WRITABLE; }
        if drop_configurable { attrs.bits &= !PropertyAttrs::CONFIGURABLE; }
        set_property_attrs(obj_addr, key_str, attrs);
    }
}

/// Recursion depth guard for js_native_call_method to prevent stack overflow
/// from circular module dependencies during initialization.
thread_local! {
    static CALL_METHOD_DEPTH: Cell<u32> = const { Cell::new(0) };
}
const MAX_CALL_METHOD_DEPTH: u32 = 512;

struct CallMethodDepthGuard;
impl CallMethodDepthGuard {
    fn enter(method_name: &str) -> Option<Self> {
        CALL_METHOD_DEPTH.with(|d| {
            let v = d.get();
            if v >= MAX_CALL_METHOD_DEPTH {
                // Silently return null object to prevent stack overflow
                None
            } else {
                // Debug logging disabled for production runs
                // if v <= 10 || v % 50 == 0 {
                //     eprintln!("[DEPTH GUARD] depth={} calling method '{}'", v, method_name);
                // }
                d.set(v + 1);
                Some(CallMethodDepthGuard)
            }
        })
    }
}
impl Drop for CallMethodDepthGuard {
    fn drop(&mut self) {
        CALL_METHOD_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

/// Static "null object" used as a safe return value when the depth guard triggers.
/// Instead of returning undefined (which callers may dereference as a null pointer),
/// we return a pointer to this valid-but-empty object so downstream code doesn't crash.
///
/// Uses a raw byte array with matching layout to avoid Sync issues with raw pointers.
#[repr(C, align(8))]
struct NullObjectBytes {
    object_type: u32,   // 1 = OBJECT_TYPE_REGULAR
    class_id: u32,      // 0
    parent_class_id: u32, // 0
    field_count: u32,   // 0
    keys_array: u64,    // 0 (null pointer as u64)
}
// Safety: this is a read-only zero-initialized struct with no interior mutability
unsafe impl Sync for NullObjectBytes {}

static NULL_OBJECT_BYTES: NullObjectBytes = NullObjectBytes {
    object_type: 1,
    class_id: 0,
    parent_class_id: 0,
    field_count: 0,
    keys_array: 0,
};

/// Fast direct-mapped inline cache for class shape keys arrays.
/// Indexed by `shape_id mod CACHE_SIZE`. Each slot stores
/// `(shape_id, keys_array_ptr)`. A 256-entry direct-mapped cache costs
/// 4KB, fits in L1d, and gives ~99% hit rate for typical Perry programs
/// (each class has a unique shape_id, and most programs use <50 classes).
///
/// Misses fall through to the SHAPE_CACHE_OVERFLOW HashMap, which is
/// the original lazy-allocated map for the long tail.
const SHAPE_INLINE_CACHE_SIZE: usize = 256;

#[repr(C)]
#[derive(Clone, Copy)]
struct ShapeCacheEntry {
    shape_id: u32,
    keys_array: *mut ArrayHeader,
}

thread_local! {
    /// Direct-mapped inline cache. Empty entries have shape_id == 0
    /// and keys_array == null.
    static SHAPE_INLINE_CACHE: std::cell::UnsafeCell<[ShapeCacheEntry; SHAPE_INLINE_CACHE_SIZE]> =
        std::cell::UnsafeCell::new([ShapeCacheEntry {
            shape_id: 0,
            keys_array: std::ptr::null_mut(),
        }; SHAPE_INLINE_CACHE_SIZE]);

    /// Overflow map for shape_ids that collide in the inline cache.
    static SHAPE_CACHE_OVERFLOW: RefCell<HashMap<u32, *mut ArrayHeader>> = RefCell::new(HashMap::new());
}

/// Look up a keys_array by shape_id. Returns `null` on miss.
/// Hot-path: ~3 ALU ops + 1 load + 1 cmp + 1 branch (no RefCell, no HashMap).
#[inline(always)]
fn shape_cache_get(shape_id: u32) -> *mut ArrayHeader {
    SHAPE_INLINE_CACHE.with(|cache| {
        let slot = (shape_id as usize) & (SHAPE_INLINE_CACHE_SIZE - 1);
        // Safety: this thread-local is single-threaded by definition;
        // the UnsafeCell allows zero-overhead reads on the hot path.
        let entry = unsafe { (*cache.get())[slot] };
        if entry.shape_id == shape_id {
            return entry.keys_array;
        }
        // Miss — check the overflow map.
        SHAPE_CACHE_OVERFLOW.with(|m| {
            m.borrow().get(&shape_id).copied().unwrap_or(std::ptr::null_mut())
        })
    })
}

/// Insert a keys_array into the cache. Updates the inline slot
/// (evicting any prior entry there) and also writes to the overflow
/// map so misses on the inline cache still find the value.
fn shape_cache_insert(shape_id: u32, keys_array: *mut ArrayHeader) {
    // Mark the array as shape-shared so `js_object_set_field_by_name`
    // knows it must clone before mutating. The clone path was firing
    // every time *any* fresh object literal added a property beyond
    // the first (because `key_count == field_count` with both
    // counting up in lockstep); that's ~19 throwaway clones per
    // 20-property row × 10k rows = 190k clones of growing size on a
    // standard bulk decode. Gating the clone on this flag turns that
    // into zero for locally-owned arrays.
    if !keys_array.is_null() {
        unsafe {
            let gc_header = (keys_array as *const u8)
                .sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*gc_header).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
        }
    }
    SHAPE_INLINE_CACHE.with(|cache| {
        let slot = (shape_id as usize) & (SHAPE_INLINE_CACHE_SIZE - 1);
        unsafe {
            (*cache.get())[slot] = ShapeCacheEntry { shape_id, keys_array };
        }
    });
    SHAPE_CACHE_OVERFLOW.with(|m| {
        m.borrow_mut().insert(shape_id, keys_array);
    });
}

/// Thread-local shape-transition cache for the dynamic-key write path
/// (`obj[name] = value`). One entry per `(prev_keys_array, key_ptr)` edge
/// in the shape lattice.
///
/// When `js_object_set_field_by_name` would otherwise do a linear scan
/// over `keys_array` to locate-or-append a key, it first looks up
/// `(obj.keys_array, key)` here. A hit tells us directly which
/// keys_array to transition the object to and which slot the field
/// lives in — no scan, no clone, no `js_array_push`.
///
/// The cache is populated on the slow (append) path: after the scan
/// confirms the key is new and a new keys_array is built, the
/// transition `(prev_keys, key_ptr) → (new_keys, slot_idx)` is stored
/// here and `new_keys` is stamped `GC_FLAG_SHAPE_SHARED` so any future
/// extension clones before mutating (same invariant as the SHAPE_CACHE
/// for compile-time object literals).
///
/// Direct-mapped, 4096 entries, each a self-describing record (full
/// key included) so a collision just misses instead of returning the
/// wrong slot. The target pointers are GC-rooted via
/// `scan_transition_cache_roots`.
///
/// Two sentinel values: `prev_keys == 0` is the "keys_array is null"
/// edge (first property on a fresh `{}`), which lets a second object
/// building the same shape reuse the first's keys_array from the very
/// first write — no per-row allocation of a 1-entry keys_array.
#[derive(Clone, Copy)]
#[repr(C)]
struct TransitionEntry {
    prev_keys: usize,    // offset 0
    key_ptr: usize,      // offset 8 — interned string pointer (pointer identity)
    next_keys: usize,    // offset 16
    slot_idx: u32,       // offset 24
    _pad: u32,           // offset 28, pad to 32 bytes
}

const TRANSITION_CACHE_SIZE: usize = 16384;
/// Mask for slot computation: TRANSITION_CACHE_SIZE - 1
const TRANSITION_CACHE_MASK: usize = TRANSITION_CACHE_SIZE - 1;

/// Main-thread transition cache — bypasses TLS overhead (user code is
/// single-threaded). `#[no_mangle]` so the LLVM codegen can emit inline
/// lookups against this symbol (write PIC).
#[no_mangle]
static mut TRANSITION_CACHE_GLOBAL: [TransitionEntry; TRANSITION_CACHE_SIZE] =
    [TransitionEntry { prev_keys: 0, key_ptr: 0, next_keys: 0, slot_idx: 0, _pad: 0 }; TRANSITION_CACHE_SIZE];

/// FNV-1a content hash for a property-name string.
/// Exported as `perry_key_content_hash` for the codegen write-PIC to
/// call without going through the full `js_object_set_field_by_name`.
#[no_mangle]
pub extern "C" fn perry_key_content_hash(key: *const crate::StringHeader) -> u64 {
    key_content_hash_impl(key)
}

#[inline(always)]
fn key_content_hash(key: *const crate::StringHeader) -> u64 {
    key_content_hash_impl(key)
}

#[inline(always)]
fn key_content_hash_impl(key: *const crate::StringHeader) -> u64 {
    unsafe {
        let len = (*key).byte_len as usize;
        let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let mut h: u64 = 0xcbf29ce484222325;
        for i in 0..len {
            h ^= *data.add(i) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

#[inline(always)]
fn transition_cache_slot(prev_keys: usize, key_ptr: usize) -> usize {
    let mixed = ((prev_keys >> 3) as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ ((key_ptr >> 3) as u64).wrapping_mul(0xC6BC279692B5C323);
    (mixed as usize) & (TRANSITION_CACHE_SIZE - 1)
}

/// Transition cache lookup using interned string pointer identity.
#[inline(always)]
fn transition_cache_lookup(prev_keys: usize, interned_key: *const crate::StringHeader) -> Option<(usize, u32)> {
    let kp = interned_key as usize;
    let slot = transition_cache_slot(prev_keys, kp);
    let entry = unsafe { TRANSITION_CACHE_GLOBAL[slot] };
    if entry.next_keys != 0 && entry.prev_keys == prev_keys && entry.key_ptr == kp {
        Some((entry.next_keys, entry.slot_idx))
    } else {
        None
    }
}

fn transition_cache_insert(prev_keys: usize, interned_key: *const crate::StringHeader, next_keys: usize, slot_idx: u32) {
    if next_keys == 0 {
        return;
    }
    let kp = interned_key as usize;
    let slot = transition_cache_slot(prev_keys, kp);
    unsafe {
        TRANSITION_CACHE_GLOBAL[slot] = TransitionEntry { prev_keys, key_ptr: kp, next_keys, slot_idx, _pad: 0 };
    }
    // Mark the target as shape-shared so any future extension on the
    // original owning object clones before mutating. Without this flag,
    // the first row's next append would extend `next_keys` in place
    // and every object that picked up `next_keys` via a cache hit
    // would observe the mutation.
    unsafe {
        let gc_header = (next_keys as *const u8)
            .wrapping_sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        if (next_keys) >= crate::gc::GC_HEADER_SIZE
            && (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
        {
            (*gc_header).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
        }
    }
}

/// GC root scanner for the transition cache. Same contract as
/// `scan_shape_cache_roots` — without this the mark phase would free
/// cached target arrays that no live object currently holds directly,
/// and the next cache-hit store would dereference freed memory.
pub fn scan_transition_cache_roots(mark: &mut dyn FnMut(f64)) {
    unsafe {
        for entry in TRANSITION_CACHE_GLOBAL.iter() {
            if entry.next_keys != 0 {
                let jsval = JSValue::pointer(entry.next_keys as *const u8);
                mark(f64::from_bits(jsval.bits()));
            }
        }
    }
}

/// GC root scanner: mark all cached shape keys arrays so they're not freed.
/// The inline cache + overflow map both hold the raw `*mut ArrayHeader`
/// pointers; without this scanner, GC would free those arrays, leaving
/// every object with that shape holding a dangling `keys_array` pointer.
pub fn scan_shape_cache_roots(mark: &mut dyn FnMut(f64)) {
    SHAPE_INLINE_CACHE.with(|cache| {
        let entries = unsafe { *cache.get() };
        for entry in entries.iter() {
            if !entry.keys_array.is_null() {
                let jsval = JSValue::pointer(entry.keys_array as *const u8);
                mark(f64::from_bits(jsval.bits()));
            }
        }
    });
    SHAPE_CACHE_OVERFLOW.with(|cache| {
        let cache = cache.borrow();
        for &arr_ptr in cache.values() {
            if !arr_ptr.is_null() {
                let jsval = JSValue::pointer(arr_ptr as *const u8);
                mark(f64::from_bits(jsval.bits()));
            }
        }
    });
}

/// GC root scanner: mark all JSValues stored in OVERFLOW_FIELDS.
/// OVERFLOW_FIELDS stores extra properties for objects that exceed their pre-allocated inline
/// slot count. The u64 JSValue bits may contain NaN-boxed pointers to heap objects (strings,
/// arrays, other objects) that are ONLY referenced via OVERFLOW_FIELDS. Without this scanner,
/// GC would free those referenced objects.
pub fn scan_overflow_fields_roots(mark: &mut dyn FnMut(f64)) {
    OVERFLOW_FIELDS.with(|m| {
        let m = m.borrow();
        for fields in m.values() {
            for &val_bits in fields.iter() {
                // Mark any NaN-boxed heap pointer (POINTER_TAG, STRING_TAG, BIGINT_TAG)
                let tag = val_bits >> 48;
                if tag == 0x7FFD || tag == 0x7FFF || tag == 0x7FFA {
                    mark(f64::from_bits(val_bits));
                }
            }
        }
    });
}

/// Remove OVERFLOW_FIELDS entry for a freed object pointer.
/// Called from GC sweep when an ObjectHeader is collected, to prevent stale entries
/// from "infecting" new objects allocated at the same address.
pub fn clear_overflow_for_ptr(obj_ptr: usize) {
    OVERFLOW_FIELDS.with(|m| {
        m.borrow_mut().remove(&obj_ptr);
    });
    // If the freed object is the one our last-accessed cache points at,
    // the cached `Vec` pointer is now dangling — clear it.
    OVERFLOW_LAST.with(|c| unsafe {
        if (*c.get()).0 == obj_ptr {
            *c.get() = (0, std::ptr::null_mut());
        }
    });
}

/// Global class registry mapping class_id -> parent_class_id for inheritance chain lookups
static CLASS_REGISTRY: RwLock<Option<HashMap<u32, u32>>> = RwLock::new(None);

/// Global registry of class IDs that extend the built-in Error class
static EXTENDS_ERROR_REGISTRY: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Per-class `Symbol.hasInstance` static hook. Maps class_id → raw function
/// pointer with signature `extern "C" fn(value: f64) -> f64` (NaN-boxed
/// TAG_TRUE / TAG_FALSE result). Populated at module init from
/// `__perry_wk_hasinstance_<class>` top-level functions lifted by the HIR
/// class lowering.
static CLASS_HAS_INSTANCE_REGISTRY: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Per-class `Symbol.toStringTag` getter hook. Maps class_id → raw function
/// pointer with signature `extern "C" fn(this: f64) -> f64` returning a
/// NaN-boxed STRING_TAG value with the user's tag text. Populated at module
/// init from `__perry_wk_tostringtag_<class>` top-level functions lifted by
/// the HIR class lowering. Consulted by `js_object_to_string` so
/// `Object.prototype.toString.call(x)` returns `[object <tag>]`.
static CLASS_TO_STRING_TAG_REGISTRY: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Register a class-level `Symbol.hasInstance` hook.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_has_instance(class_id: u32, func_ptr: i64) {
    let mut registry = CLASS_HAS_INSTANCE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry
        .as_mut()
        .unwrap()
        .insert(class_id, func_ptr as usize);
}

/// Register a class-level `Symbol.toStringTag` getter hook.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_to_string_tag(class_id: u32, func_ptr: i64) {
    let mut registry = CLASS_TO_STRING_TAG_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry
        .as_mut()
        .unwrap()
        .insert(class_id, func_ptr as usize);
}

fn lookup_has_instance_hook(class_id: u32) -> Option<usize> {
    let reg = CLASS_HAS_INSTANCE_REGISTRY.read().unwrap();
    reg.as_ref().and_then(|m| m.get(&class_id).copied())
}

fn lookup_to_string_tag_hook(class_id: u32) -> Option<usize> {
    let reg = CLASS_TO_STRING_TAG_REGISTRY.read().unwrap();
    reg.as_ref().and_then(|m| m.get(&class_id).copied())
}

/// `Object.prototype.toString.call(x)` — returns `[object <tag>]` where
/// `<tag>` is read from the value's class-level `Symbol.toStringTag` getter
/// if registered, otherwise `Object` (matching Node for plain objects).
#[no_mangle]
pub unsafe extern "C" fn js_object_to_string(value: f64) -> f64 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    let bits = value.to_bits();
    let mut tag_str: Option<String> = None;
    if (bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        let obj_ptr = (bits & POINTER_MASK) as *const ObjectHeader;
        if !obj_ptr.is_null() && (obj_ptr as usize) >= 0x1000 {
            let class_id = (*obj_ptr).class_id;
            if let Some(func_ptr) = lookup_to_string_tag_hook(class_id) {
                let getter: extern "C" fn(f64) -> f64 =
                    std::mem::transmute(func_ptr as *const u8);
                let result_f64 = getter(value);
                let rbits = result_f64.to_bits();
                if (rbits & 0xFFFF_0000_0000_0000) == STRING_TAG {
                    let str_ptr =
                        (rbits & POINTER_MASK) as *const crate::string::StringHeader;
                    if !str_ptr.is_null() {
                        let len = (*str_ptr).byte_len as usize;
                        let data = (str_ptr as *const u8)
                            .add(std::mem::size_of::<crate::string::StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        if let Ok(s) = std::str::from_utf8(bytes) {
                            tag_str = Some(s.to_string());
                        }
                    }
                }
            }
        }
    }
    let formatted = match tag_str {
        Some(tag) => format!("[object {}]", tag),
        None => "[object Object]".to_string(),
    };
    let bytes = formatted.as_bytes();
    let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK))
}

/// Mark a user-defined class as extending the built-in Error class.
#[no_mangle]
pub extern "C" fn js_register_class_extends_error(class_id: u32) {
    let mut registry = EXTENDS_ERROR_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(std::collections::HashSet::new());
    }
    registry.as_mut().unwrap().insert(class_id);
}

/// Check if a class id extends the built-in Error class
pub(crate) fn extends_builtin_error(class_id: u32) -> bool {
    let registry = EXTENDS_ERROR_REGISTRY.read().unwrap();
    if let Some(reg) = registry.as_ref() {
        if reg.contains(&class_id) {
            return true;
        }
        let mut current = class_id;
        let parent_reg = CLASS_REGISTRY.read().unwrap();
        if let Some(pr) = parent_reg.as_ref() {
            for _ in 0..32 {
                match pr.get(&current).copied() {
                    Some(parent) if parent != 0 => {
                        if reg.contains(&parent) { return true; }
                        current = parent;
                    }
                    _ => break,
                }
            }
        }
    }
    false
}

// ============================================================================
// Class method vtable registry — enables runtime dispatch for interface-typed
// and dynamically-typed method calls.  Each class registers its methods and
// getters at startup; js_native_call_method / js_dynamic_object_get_property
// look up the vtable by the object's class_id when static dispatch isn't possible.
// ============================================================================

/// Entry in the class method vtable
pub struct VTableMethodEntry {
    pub func_ptr: usize,
    pub param_count: u32,
}

/// Per-class vtable with methods and getters
pub struct ClassVTable {
    pub methods: HashMap<String, VTableMethodEntry>,
    pub getters: HashMap<String, usize>, // getter func_ptr (signature: fn(i64) -> f64)
}

/// Global vtable registry: class_id -> vtable
pub static CLASS_VTABLE_REGISTRY: RwLock<Option<HashMap<u32, ClassVTable>>> = RwLock::new(None);

/// Function pointer type for dispatching method calls on handle-based objects.
/// Handle-based objects use small integer IDs (1, 2, 3...) instead of real heap pointers.
/// This is registered by perry-stdlib to dispatch to Fastify, ioredis, etc.
type HandleMethodDispatchFn = unsafe extern "C" fn(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64;

static mut HANDLE_METHOD_DISPATCH: Option<HandleMethodDispatchFn> = None;

/// Function pointer type for dispatching property access on handle-based objects.
type HandlePropertyDispatchFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64;

pub static mut HANDLE_PROPERTY_DISPATCH: Option<HandlePropertyDispatchFn> = None;

/// Function pointer type for dispatching property set on handle-based objects.
type HandlePropertySetDispatchFn = unsafe extern "C" fn(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    value: f64,
);

pub static mut HANDLE_PROPERTY_SET_DISPATCH: Option<HandlePropertySetDispatchFn> = None;

/// Register a function to handle method calls on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_method_dispatch(f: HandleMethodDispatchFn) {
    HANDLE_METHOD_DISPATCH = Some(f);
}

/// Register a function to handle property access on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_dispatch(f: HandlePropertyDispatchFn) {
    HANDLE_PROPERTY_DISPATCH = Some(f);
}

/// Register a function to handle property set on handle-based objects
#[no_mangle]
pub unsafe extern "C" fn js_register_handle_property_set_dispatch(f: HandlePropertySetDispatchFn) {
    HANDLE_PROPERTY_SET_DISPATCH = Some(f);
}

/// Register a class method in the vtable registry.
/// Called at startup from the init function for every class method/getter.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_method(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
    param_count: i64,
) {
    let name = if name_ptr.is_null() || name_len <= 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
    });
    vtable.methods.insert(name, VTableMethodEntry {
        func_ptr: func_ptr as usize,
        param_count: param_count as u32,
    });
}

/// Register a class getter in the vtable registry.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_getter(
    class_id: i64,
    name_ptr: *const u8,
    name_len: i64,
    func_ptr: i64,
) {
    let name = if name_ptr.is_null() || name_len <= 0 {
        return;
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len as usize)) {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    };
    let mut registry = CLASS_VTABLE_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    let reg = registry.as_mut().unwrap();
    let vtable = reg.entry(class_id as u32).or_insert_with(|| ClassVTable {
        methods: HashMap::new(),
        getters: HashMap::new(),
    });
    vtable.getters.insert(name, func_ptr as usize);
}

/// Call a vtable method with the correct arity.
/// All method params are f64, `this` is i64.
unsafe fn call_vtable_method(
    func_ptr: usize,
    this: i64,
    args_ptr: *const f64,
    args_len: usize,
    param_count: u32,
) -> f64 {
    #[inline(always)]
    unsafe fn arg_or_nan(args_ptr: *const f64, args_len: usize, idx: usize) -> f64 {
        if idx < args_len { *args_ptr.add(idx) } else { f64::NAN }
    }

    // LLVM-generated methods have signature `double(double this, double arg0, ...)`.
    // `this` is NaN-boxed as f64, so we must pass it as f64 — not i64 — to match
    // the calling convention. On ARM64 i64 and f64 share registers, so passing i64
    // works by accident; on Windows x64 ABI they use *different* registers (rcx vs
    // xmm0), causing segfaults when the method reads `this` from the wrong register.
    let this_f64: f64 = f64::from_bits(this as u64);

    match param_count {
        0 => {
            let f: extern "C" fn(f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64)
        }
        1 => {
            let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0))
        }
        2 => {
            let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1))
        }
        3 => {
            let f: extern "C" fn(f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2))
        }
        4 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3))
        }
        5 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4))
        }
        6 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5))
        }
        7 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5), arg_or_nan(args_ptr, args_len, 6))
        }
        8 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5), arg_or_nan(args_ptr, args_len, 6), arg_or_nan(args_ptr, args_len, 7))
        }
        9 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5), arg_or_nan(args_ptr, args_len, 6), arg_or_nan(args_ptr, args_len, 7), arg_or_nan(args_ptr, args_len, 8))
        }
        _ => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(this_f64, arg_or_nan(args_ptr, args_len, 0), arg_or_nan(args_ptr, args_len, 1), arg_or_nan(args_ptr, args_len, 2), arg_or_nan(args_ptr, args_len, 3), arg_or_nan(args_ptr, args_len, 4), arg_or_nan(args_ptr, args_len, 5), arg_or_nan(args_ptr, args_len, 6), arg_or_nan(args_ptr, args_len, 7), arg_or_nan(args_ptr, args_len, 8), arg_or_nan(args_ptr, args_len, 9))
        }
    }
}

/// Register a class with its parent class ID in the global registry
fn register_class(class_id: u32, parent_class_id: u32) {
    let mut registry = CLASS_REGISTRY.write().unwrap();
    if registry.is_none() {
        *registry = Some(HashMap::new());
    }
    registry.as_mut().unwrap().insert(class_id, parent_class_id);
}

/// Public registration entry point used by codegen module init.
///
/// The inline bump allocator (codegen-side `new ClassName()` lowering)
/// writes `parent_class_id` directly into the ObjectHeader and skips
/// the per-alloc `register_class` call that the runtime allocators
/// (`js_object_alloc_with_parent`, `js_object_alloc_class_inline_keys`,
/// etc.) make on every allocation. That breaks multi-level
/// `instanceof` chains: `class Square extends Rectangle extends Shape`
/// — `square instanceof Shape` walks the registry chain
/// `Square → Rectangle → Shape`, but if we never registered the
/// `Square → Rectangle` edge the walk stops immediately and returns
/// false.
///
/// Codegen now emits one call to this function per inheriting class
/// in the entry-block init prelude (after `__perry_init_strings_*`),
/// so the registry chain is fully populated before any user code runs.
#[no_mangle]
pub extern "C" fn js_register_class_parent(class_id: u32, parent_class_id: u32) {
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }
}

/// Look up parent class ID from the registry
fn get_parent_class_id(class_id: u32) -> Option<u32> {
    let registry = CLASS_REGISTRY.read().unwrap();
    registry.as_ref().and_then(|r| r.get(&class_id).copied())
}

/// Check if a pointer is a valid heap object (safe to dereference GcHeader).
/// Values below 0x100000 (1MB) are likely INT32_TAG extracts, small handles,
/// or null. The upper bound filters out NaN-box tag bits that leaked through.
///
/// Issue #73 follow-up: raised the lower bound from 1 MB to 2 TB to reject
/// corrupted NaN-boxes whose 48-bit handle lands in the 1-2 TB window
/// (e.g. `0x00FF_0000_0000` from an `ArrayHeader { length: 0, capacity:
/// 255 }` read as u64). Real Darwin mimalloc + arena allocations all
/// land in the 3-5 TB range; anything below 2 TB is certainly bogus on
/// this platform. The remaining Windows/Linux targets still fit —
/// their heaps are similarly high-address on 64-bit systems — but we
/// haven't captured the same corruption pattern there, so if it ever
/// regresses we'll want platform-specific bounds.
#[inline(always)]
fn is_valid_obj_ptr(ptr: *const u8) -> bool {
    let addr = ptr as u64;
    addr >= 0x200_0000_0000 && addr < 0x8000_0000_0000
}

/// Object header - precedes the fields in memory
#[repr(C)]
pub struct ObjectHeader {
    /// Type tag to distinguish from Error objects (must be first field!)
    /// Uses OBJECT_TYPE_REGULAR (1) for regular objects
    pub object_type: u32,
    /// Class ID for this object (used for instanceof, vtable lookup)
    pub class_id: u32,
    /// Parent class ID for inheritance chain (0 if no parent)
    pub parent_class_id: u32,
    /// Number of fields in this object
    pub field_count: u32,
    /// Pointer to array of key strings (for Object.keys() support)
    /// NULL for class instances (keys are defined by the class)
    pub keys_array: *mut ArrayHeader,
}

/// Allocate a new object with the given class ID and field count
/// Returns a pointer to the object header
#[no_mangle]
pub extern "C" fn js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    js_object_alloc_with_parent(class_id, 0, field_count)
}

/// Allocate a new object with class ID, parent class ID, and field count
/// The parent_class_id is used for instanceof inheritance checks
/// Returns a pointer to the object header
#[no_mangle]
pub extern "C" fn js_object_alloc_with_parent(class_id: u32, parent_class_id: u32, field_count: u32) -> *mut ObjectHeader {
    // Register this class's parent for inheritance lookups
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    // Allocate at least 8 field slots to match js_object_set_field_by_name's alloc_limit
    // assumption (max(field_count, 8)). Without this, empty objects ({}) with field_count=0
    // would have 0 field slots but js_object_set_field_by_name writes up to 8 fields inline,
    // causing heap buffer overflow into adjacent arena objects.
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        (*ptr).keys_array = ptr::null_mut();

        // Initialize ALL allocated field slots to undefined (not just field_count)
        // We allocate max(field_count, 8) slots but must zero all of them to prevent
        // stale data from previously freed GC objects from bleeding through.
        let fields_ptr = (ptr as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        for i in 0..alloc_field_count {
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }

        ptr
    }
}

/// Fast object allocation using bump allocator - NO field initialization
/// This is significantly faster for hot paths where constructor immediately sets all fields
/// Returns a pointer to the object header with UNINITIALIZED fields
#[no_mangle]
pub extern "C" fn js_object_alloc_fast(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header only - fields left uninitialized for constructor to fill
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = 0;
        (*ptr).field_count = field_count;
        (*ptr).keys_array = ptr::null_mut();
    }

    ptr
}

/// Fast object allocation with parent class ID - NO field initialization
#[no_mangle]
pub extern "C" fn js_object_alloc_fast_with_parent(class_id: u32, parent_class_id: u32, field_count: u32) -> *mut ObjectHeader {

    // Only register class if it has a parent (one-time operation per class)
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        (*ptr).keys_array = ptr::null_mut();
    }

    ptr
}

/// Fast class instance allocator that takes a pre-built keys_array
/// pointer directly, skipping the per-call SHAPE_CACHE lookup. The
/// codegen pre-builds the keys_array ONCE at module init time
/// (via `js_build_class_keys_array`) and stores the result in a
/// per-class global, then passes that global to this allocator on
/// every `new ClassName()` call. This eliminates the thread-local
/// + RefCell::borrow_mut + HashMap::get cost from the hot
/// allocation path — for benchmarks like `object_create` (1M
/// `new Point(...)` calls) the SHAPE_CACHE lookup was ~30ns/alloc.
///
/// `#[inline]` lets the bitcode-link path
/// (`PERRY_LLVM_BITCODE_LINK=1`) inline the entire body — including
/// the `arena_alloc_gc` call — into the user's `new ClassName()`
/// site, eliminating function-call overhead from the hot loop.
#[no_mangle]
#[inline]
pub extern "C" fn js_object_alloc_class_inline_keys(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys_array: *mut ArrayHeader,
) -> *mut ObjectHeader {
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }
    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        (*ptr).keys_array = keys_array;
    }
    ptr
}

/// Build (or fetch from SHAPE_CACHE) the keys_array for a class.
/// Called ONCE per class at module init time; the resulting pointer
/// is cached in a per-class global by the codegen and then passed
/// to `js_object_alloc_class_inline_keys` on each `new` call.
///
/// Same packed-keys format as `js_object_alloc_class_with_keys`:
/// null-separated UTF-8 field names.
#[no_mangle]
pub extern "C" fn js_build_class_keys_array(
    class_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ArrayHeader {
    let shape_id = class_id
        .wrapping_mul(10007)
        .wrapping_add(field_count.wrapping_mul(100003))
        .wrapping_add(1000000);
    let cached = shape_cache_get(shape_id);
    if !cached.is_null() {
        return cached;
    }
    let keys_bytes = unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
    let keys: Vec<&[u8]> = keys_bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
    let num_keys = keys.len();
    // Issue #179: the keys_array and its string elements are shape-cache
    // resident for the program's lifetime (anchored by
    // `scan_shape_cache_roots`). Route them through the longlived arena
    // so general-arena block 0 doesn't get pinned by the first `new C()`
    // in a loop, which cascaded via block-persistence into every
    // subsequent iteration's allocations.
    let arr = crate::array::js_array_alloc_with_length_longlived(num_keys as u32);
    let elements_ptr = unsafe { (arr as *mut u8).add(8) as *mut f64 };
    for (i, key_bytes) in keys.iter().enumerate() {
        let str_ptr = crate::string::js_string_from_bytes_longlived(
            key_bytes.as_ptr(),
            key_bytes.len() as u32,
        );
        let nanboxed = f64::from_bits(
            crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK),
        );
        unsafe { *elements_ptr.add(i) = nanboxed; }
    }
    shape_cache_insert(shape_id, arr);
    arr
}

/// Allocate a class instance with a shape-cached keys array for field names.
/// This allows dynamic property access (obj.field1) to work on class instances,
/// not just object literals. Uses class_id as the shape_id for caching.
///
/// Marked `#[inline]` so the LLVM bitcode-link path
/// (`PERRY_LLVM_BITCODE_LINK=1`) can inline the body into hot
/// allocation loops, eliminating the function-call overhead and
/// letting LLVM constant-fold the SHAPE_INLINE_CACHE slot index when
/// `class_id` is a compile-time constant (which it always is at the
/// `new ClassName()` call site).
#[no_mangle]
#[inline]
pub extern "C" fn js_object_alloc_class_with_keys(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ObjectHeader {
    // Register parent class if needed
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
    }

    // Use class_id as shape_id for caching the keys array.
    // Hot path: direct-mapped inline cache lookup (no RefCell, no
    // HashMap). Miss path: lazy-build from packed_keys.
    let shape_id = class_id
        .wrapping_mul(10007)
        .wrapping_add(field_count.wrapping_mul(100003))
        .wrapping_add(1000000);
    let cached = shape_cache_get(shape_id);
    let keys_arr = if !cached.is_null() {
        cached
    } else {
        let keys_bytes = unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = keys_bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
        let num_keys = keys.len();
        // Issue #179: shape-cache keys_array lives in the longlived arena
        // (see `js_build_class_keys_array` for the rationale).
        let arr = crate::array::js_array_alloc_with_length_longlived(num_keys as u32);
        let elements_ptr = unsafe { (arr as *mut u8).add(8) as *mut f64 };
        for (i, key_bytes) in keys.iter().enumerate() {
            let str_ptr = crate::string::js_string_from_bytes_longlived(
                key_bytes.as_ptr(), key_bytes.len() as u32,
            );
            let nanboxed = f64::from_bits(
                crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK)
            );
            unsafe { *elements_ptr.add(i) = nanboxed; }
        }
        shape_cache_insert(shape_id, arr);
        arr
    };

    unsafe { (*ptr).keys_array = keys_arr; }
    ptr
}

/// Allocate an object with a shape-cached keys array.
/// First call per shape_id creates the keys array from packed_keys (null-separated key names);
/// subsequent calls reuse the cached pointer. This eliminates per-object key string allocation
/// and array construction for repeated object literals with the same shape.
#[no_mangle]
pub extern "C" fn js_object_alloc_with_shape(
    shape_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    // Allocate extra field slots for dynamic property growth (plain objects may get new fields)
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * 8;
    let total_size = header_size + fields_size;
    let obj_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*obj_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*obj_ptr).class_id = 0;
        (*obj_ptr).parent_class_id = 0;
        // field_count tracks the logical number of fields; extra allocated slots
        // are available for dynamic property growth via js_object_set_field_by_name
        (*obj_ptr).field_count = field_count;

        // Initialize all allocated field slots to undefined (including extra padding)
        let fields_ptr = (obj_ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
    }

    let cached = shape_cache_get(shape_id);
    let keys_arr = if !cached.is_null() {
        cached
    } else {
        let keys_bytes = unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = keys_bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).collect();
        let num_keys = keys.len();
        // Issue #179: shape-cache keys_array lives in the longlived arena.
        let arr = crate::array::js_array_alloc_with_length_longlived(num_keys as u32);
        let elements_ptr = unsafe { (arr as *mut u8).add(8) as *mut f64 };
        for (i, key_bytes) in keys.iter().enumerate() {
            let str_ptr = crate::string::js_string_from_bytes_longlived(
                key_bytes.as_ptr(), key_bytes.len() as u32,
            );
            let nanboxed = f64::from_bits(
                crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK)
            );
            unsafe { *elements_ptr.add(i) = nanboxed; }
        }
        shape_cache_insert(shape_id, arr);
        arr
    };

    unsafe { (*obj_ptr).keys_array = keys_arr; }

    obj_ptr
}

/// Clone a spread source object and reserve extra physical slot capacity for additional
/// static properties. Used to implement object spread: `{ ...src, key1: val1, key2: val2 }`.
///
/// - `src_f64`: the spread source object as a NaN-boxed f64 (POINTER_TAG or raw pointer)
/// - `extra_count`: number of additional static properties — reserves physical slot capacity
///   for them, but does NOT add their keys to the keys_array upfront. Codegen is expected to
///   call `js_object_set_field_by_name` for each static prop, which correctly overwrites keys
///   that already exist in the spread source (preserving JS "last key wins" semantics) and
///   appends new keys (using the reserved capacity).
/// - `_static_keys_ptr`/`_static_keys_len`: unused (kept for ABI compat). Previously these
///   were used to pre-populate static keys in keys_array, but that created duplicate entries
///   when a static key matched an existing spread key, and the linear-scan lookup returned
///   the first (stale) match instead of the intended last-key value.
///
/// Returns the new *mut ObjectHeader as an i64 raw pointer (NOT NaN-boxed).
/// The returned object's `field_count` equals the source's field_count (NOT src + extra),
/// but the physical allocation reserves enough slots so subsequent
/// `js_object_set_field_by_name` calls have somewhere to append.
#[no_mangle]
pub unsafe extern "C" fn js_object_clone_with_extra(
    src_f64: f64,
    extra_count: u32,
    _static_keys_ptr: *const u8,
    _static_keys_len: u32,
) -> *mut ObjectHeader {
    // Extract raw pointer from NaN-boxed f64
    let src_bits = src_f64.to_bits();
    let top16 = src_bits >> 48;
    let src_raw = if top16 >= 0x7FF8 {
        (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        src_bits as usize
    };

    let header_size = std::mem::size_of::<ObjectHeader>();

    // If source is invalid, create an empty object with enough capacity for the static props.
    // Physical slot count = max(extra_count, 8) to match js_object_set_field_by_name's
    // alloc_limit = max(field_count, 8) expectation.
    if src_raw < 0x10000 {
        let phys_slots = std::cmp::max(extra_count, 8);
        let total_size = header_size + phys_slots as usize * 8;
        let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
        (*new_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*new_ptr).class_id = 0;
        (*new_ptr).parent_class_id = 0;
        (*new_ptr).field_count = 0;
        let fields_ptr = (new_ptr as *mut u8).add(header_size) as *mut u64;
        for i in 0..phys_slots as usize {
            ptr::write(fields_ptr.add(i), crate::value::TAG_UNDEFINED);
        }
        // Empty keys array with capacity reserved for the static props to come.
        let new_keys_arr = crate::array::js_array_alloc(extra_count);
        (*new_ptr).keys_array = new_keys_arr;
        return new_ptr;
    }

    let src_ptr = src_raw as *const ObjectHeader;
    let src_field_count = (*src_ptr).field_count;

    // Physical slot capacity: src_field_count + extra_count, but at least max(fc, 8) to match
    // js_object_set_field's alloc_limit check. Extra slots are scratch space for subsequent
    // js_object_set_field_by_name calls.
    let phys_slots = std::cmp::max(src_field_count + extra_count, 8);
    let total_size = header_size + phys_slots as usize * 8;
    let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
    (*new_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
    (*new_ptr).class_id = 0;
    (*new_ptr).parent_class_id = 0;
    // Logical field count starts at src's count. js_object_set_field_by_name bumps it when
    // appending new keys.
    (*new_ptr).field_count = src_field_count;

    // Copy source fields (as raw f64/u64 words — preserves NaN-boxing)
    let src_fields = (src_ptr as *const u8).add(header_size) as *const u64;
    let dst_fields = (new_ptr as *mut u8).add(header_size) as *mut u64;
    for i in 0..src_field_count as usize {
        let field_val = *src_fields.add(i);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        let cleaned = if field_val == 0x7FFD_0000_0000_0000 {
            eprintln!("[CLONE_NULL_PTR] field {} from src={:p} — replacing with undefined", i, src_ptr);
            crate::value::TAG_UNDEFINED
        } else {
            field_val
        };
        ptr::write(dst_fields.add(i), cleaned);
    }
    // Initialize scratch slots to undefined
    for i in src_field_count as usize..phys_slots as usize {
        ptr::write(dst_fields.add(i), crate::value::TAG_UNDEFINED);
    }

    // Build keys array: copy ONLY src keys. Static keys are NOT added here — codegen uses
    // js_object_set_field_by_name for each static prop, which appends new keys via
    // js_array_push. Pre-size the keys capacity to avoid immediate reallocation on append.
    let src_keys_arr = (*src_ptr).keys_array;
    let new_keys_arr = crate::array::js_array_alloc(src_field_count + extra_count);
    let new_keys_elements = (new_keys_arr as *mut u8).add(8) as *mut f64;

    if !src_keys_arr.is_null() && (src_keys_arr as usize) >= 0x10000 {
        let src_key_len = (*src_keys_arr).length as usize;
        let src_key_elements = (src_keys_arr as *const u8).add(8) as *const f64;
        let copy_count = src_key_len.min(src_field_count as usize);
        for i in 0..copy_count {
            *new_keys_elements.add(i) = *src_key_elements.add(i);
        }
        (*new_keys_arr).length = copy_count as u32;
    } else {
        (*new_keys_arr).length = 0;
    }

    (*new_ptr).keys_array = new_keys_arr;

    new_ptr
}

/// Copy all own enumerable fields from `src` into `dst`, using `js_object_set_field_by_name`
/// semantics (overwrite existing, append new). Used for multi-spread object literals like
/// `{...a, ...b}` to apply each additional spread after the first has been cloned via
/// `js_object_clone_with_extra`.
#[no_mangle]
pub unsafe extern "C" fn js_object_copy_own_fields(dst_i64: i64, src_f64: f64) {
    // Extract dst pointer (may be NaN-boxed or raw)
    let dst_bits = dst_i64 as u64;
    let dst_top16 = dst_bits >> 48;
    let dst_raw = if dst_top16 >= 0x7FF8 {
        (dst_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        dst_bits as usize
    };
    if dst_raw < 0x10000 {
        return;
    }
    let dst = dst_raw as *mut ObjectHeader;

    // Extract src pointer (NaN-boxed f64)
    let src_bits = src_f64.to_bits();
    let src_top16 = src_bits >> 48;
    let src_raw = if src_top16 >= 0x7FF8 {
        (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        src_bits as usize
    };
    if src_raw < 0x10000 {
        return;
    }
    let src = src_raw as *const ObjectHeader;

    // Iterate src's keys and copy each value via set_field_by_name.
    let src_keys = (*src).keys_array;
    if src_keys.is_null() || (src_keys as usize) < 0x10000 {
        return;
    }
    let key_count = crate::array::js_array_length(src_keys) as usize;
    let src_field_count = (*src).field_count as usize;
    let header_size = std::mem::size_of::<ObjectHeader>();
    let src_fields = (src as *const u8).add(header_size) as *const u64;

    for i in 0..key_count.min(src_field_count) {
        let key_val = crate::array::js_array_get(src_keys, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let key_ptr = key_val.as_string_ptr();
        let field_bits = *src_fields.add(i);
        let field_f64 = f64::from_bits(field_bits);
        js_object_set_field_by_name(dst, key_ptr, field_f64);
    }
}

/// Get a field from an object by index
#[no_mangle]
pub extern "C" fn js_object_get_field(obj: *const ObjectHeader, field_index: u32) -> JSValue {
    let obj = { let b = obj as u64; let t = b >> 48; if t >= 0x7FF8 { if t == 0x7FFC || (b & 0x0000_FFFF_FFFF_FFFF) == 0 || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000 { return JSValue::undefined(); } (b & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader } else { obj } };
    if obj.is_null() || (obj as usize) < 0x1000000 { return JSValue::undefined(); }
    unsafe {
        // Bounds check: check inline fields first, then overflow map
        let fc = (*obj).field_count;
        if field_index >= fc {
            // Check overflow map for fields that didn't fit in inline storage
            return match overflow_get(obj as usize, field_index as usize) {
                Some(bits) => JSValue::from_bits(bits),
                None => JSValue::undefined(),
            };
        }
        // Guard: corrupted objects with unreasonably large field_count
        if fc > 10000 {
            return JSValue::undefined();
        }
        let fields_ptr = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const JSValue;
        let val = *fields_ptr.add(field_index as usize);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        if val.bits() == 0x7FFD_0000_0000_0000 {
            eprintln!("[NULL_PTR_FIELD_GET] obj={:p} field_index={} class_id={} field_count={}", obj, field_index, (*obj).class_id, (*obj).field_count);
            return JSValue::undefined();
        }
        val
    }
}

/// Set a field on an object by index
#[no_mangle]
pub extern "C" fn js_object_set_field(obj: *mut ObjectHeader, field_index: u32, value: JSValue) {
    let obj = { let b = obj as u64; let t = b >> 48; if t >= 0x7FF8 { if t == 0x7FFC || (b & 0x0000_FFFF_FFFF_FFFF) == 0 || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000 { return; } (b & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader } else { obj } };
    if obj.is_null() || (obj as usize) < 0x1000000 { return; }
    unsafe {
        // Bounds check: guard against out-of-range field writes that corrupt adjacent
        // arena allocations. js_object_alloc_with_shape uses max(field_count, 8) physical
        // slots, but the stored field_count is the logical count. Class objects from
        // js_object_alloc_class_with_keys use exactly field_count slots.
        // We use a generous limit of max(field_count, 8) to avoid false positives from
        // js_object_alloc_with_shape's extra padding while still catching real overflows.
        let stored_field_count = (*obj).field_count;
        let alloc_limit = std::cmp::max(stored_field_count, 8);
        if field_index >= alloc_limit {
            eprintln!(
                "[PERRY WARN] js_object_set_field: OOB write field_index={} alloc_limit={} (field_count={}) obj={:p} class_id={}",
                field_index, alloc_limit, stored_field_count, obj, (*obj).class_id
            );
            return;
        }
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        let vbits = value.bits();
        let value = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            eprintln!("[WARN_NULL_PTR] js_object_set_field: null POINTER_TAG at obj={:p} field_index={} class_id={} — replacing with undefined", obj, field_index, (*obj).class_id);
            JSValue::undefined()
        } else {
            value
        };
        let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        ptr::write(fields_ptr.add(field_index as usize), value);
    }
}

/// Get the class ID of an object
#[no_mangle]
pub extern "C" fn js_object_get_class_id(obj: *const ObjectHeader) -> u32 {
    if obj.is_null() || (obj as usize) < 0x100000 {
        return 0;
    }
    unsafe { (*obj).class_id }
}

/// Free an object (for manual memory management / testing)
#[no_mangle]
pub extern "C" fn js_object_free(_obj: *mut ObjectHeader) {
    // No-op: GC handles deallocation of arena-allocated objects
}

/// Convert an object pointer to a JSValue
#[no_mangle]
pub extern "C" fn js_object_to_value(obj: *const ObjectHeader) -> JSValue {
    JSValue::pointer(obj as *const u8)
}

/// Extract an object pointer from a JSValue
#[no_mangle]
pub extern "C" fn js_value_to_object(value: JSValue) -> *mut ObjectHeader {
    value.as_pointer::<ObjectHeader>() as *mut ObjectHeader
}

/// Get a field as f64 (returns raw JSValue bits as f64)
/// This preserves NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_f64(obj: *const ObjectHeader, field_index: u32) -> f64 {
    let value = js_object_get_field(obj, field_index);
    f64::from_bits(value.bits())
}

/// Set a field from f64 (interprets raw bits as JSValue)
/// This preserves NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_set_field_f64(obj: *mut ObjectHeader, field_index: u32, value: f64) {
    // Check frozen flag — frozen objects reject all writes
    if !obj.is_null() && (obj as usize) > 0x10000 {
        unsafe {
            let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
                return;
            }
        }
    }
    js_object_set_field(obj, field_index, JSValue::from_bits(value.to_bits()));
}

/// Set a field by index with a raw f64 value (for dynamic object creation)
/// This is a convenience wrapper that takes field_index as u32 and value as f64.
/// Honors `Object.freeze` and per-key `writable: false` descriptors so codegen
/// paths that resolve property writes to a field index still respect the JS
/// invariants set up by `Object.defineProperty`.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_index(obj: *mut ObjectHeader, key: *const crate::string::StringHeader, field_index: u32, value: f64) {
    if obj.is_null() || (obj as usize) < 0x1000000 { return; }
    unsafe {
        // Frozen objects reject all writes.
        let gc = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            return;
        }
        // Per-key writable / accessor check when the key string is provided.
        if !key.is_null() {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            if let Ok(name) = std::str::from_utf8(name_bytes) {
                if ACCESSORS_IN_USE.with(|c| c.get()) {
                    if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                        if acc.set != 0 {
                            let closure = (acc.set & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
                            if !closure.is_null() {
                                crate::closure::js_closure_call1(closure, value);
                            }
                        }
                        return;
                    }
                }
                if let Some(attrs) = get_property_attrs(obj as usize, name) {
                    if !attrs.writable() {
                        return;
                    }
                }
            }
        }
    }
    js_object_set_field(obj, field_index, JSValue::from_bits(value.to_bits()));
}

/// Set the keys array for an object (used for Object.keys() support)
/// The keys_array should be an array of string pointers
#[no_mangle]
pub extern "C" fn js_object_set_keys(obj: *mut ObjectHeader, keys_array: *mut ArrayHeader) {
    unsafe {
        (*obj).keys_array = keys_array;
    }
}

/// Get the keys of an object as an array of strings.
/// If any key has a per-property descriptor with `enumerable: false`, that key is filtered out.
/// Otherwise (the common case), this returns the stored keys array directly.
#[no_mangle]
pub extern "C" fn js_object_keys(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return crate::array::js_array_alloc(0);
        }
        // Fast path: if no descriptors are set for this object, return keys array directly.
        let has_descriptors = PROPERTY_DESCRIPTORS.with(|m| {
            m.borrow().keys().any(|(ptr, _)| *ptr == obj as usize)
        });
        if !has_descriptors {
            return keys;
        }
        // Slow path: filter out non-enumerable keys.
        let len = crate::array::js_array_length(keys) as usize;
        let filtered = crate::array::js_array_alloc(len as u32);
        for i in 0..len {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if !key_val.is_string() { continue; }
            let stored_key = key_val.as_string_ptr();
            if stored_key.is_null() { continue; }
            let name_ptr = (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*stored_key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            let key_str = match std::str::from_utf8(name_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };
            // If a descriptor explicitly marks this key non-enumerable, skip it.
            if let Some(attrs) = get_property_attrs(obj as usize, key_str) {
                if !attrs.enumerable() {
                    continue;
                }
            }
            crate::array::js_array_push_f64(filtered, f64::from_bits(key_val.bits()));
        }
        filtered
    }
}

/// Get the values of an object as an array
/// Returns an array of the object's field values
#[no_mangle]
pub extern "C" fn js_object_values(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let field_count = (*obj).field_count as usize;
        let result = crate::array::js_array_alloc(field_count as u32);

        for i in 0..field_count {
            let value = js_object_get_field(obj as *mut ObjectHeader, i as u32);
            // Store the raw f64 bits (which may be NaN-boxed)
            crate::array::js_array_push_f64(result, f64::from_bits(value.bits()));
        }

        result
    }
}

/// Get the entries of an object as an array of [key, value] pairs
/// Returns an array where each element is a 2-element array [key, value]
#[no_mangle]
pub extern "C" fn js_object_entries(obj: *const ObjectHeader) -> *mut ArrayHeader {
    if obj.is_null() {
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        let keys = (*obj).keys_array;
        let field_count = (*obj).field_count as usize;
        let result = crate::array::js_array_alloc(field_count as u32);

        for i in 0..field_count {
            // Create a pair array [key, value]
            let pair = crate::array::js_array_alloc(2);

            // Get the key (from keys array if available)
            if !keys.is_null() && (i as u32) < crate::array::js_array_length(keys) {
                let key = crate::array::js_array_get_f64(keys, i as u32);
                crate::array::js_array_push_f64(pair, key);
            } else {
                // No key available, use empty string
                crate::array::js_array_push_f64(pair, 0.0);
            }

            // Get the value
            let value = js_object_get_field(obj as *mut ObjectHeader, i as u32);
            crate::array::js_array_push_f64(pair, f64::from_bits(value.bits()));

            // Push the pair to result (NaN-box the array pointer)
            let pair_boxed = crate::value::js_nanbox_pointer(pair as i64);
            crate::array::js_array_push_f64(result, pair_boxed);
        }

        result
    }
}

/// Check if a property exists in an object by its string key name
/// Returns NaN-boxed true if the property exists, NaN-boxed false otherwise
/// This implements the JavaScript 'in' operator: "key" in obj
#[no_mangle]
pub extern "C" fn js_object_has_property(obj: f64, key: f64) -> f64 {
    let nanbox_false = f64::from_bits(0x7FFC_0000_0000_0003u64); // TAG_FALSE
    let nanbox_true = f64::from_bits(0x7FFC_0000_0000_0004u64);  // TAG_TRUE

    let obj_val = JSValue::from_bits(obj.to_bits());
    let key_val = JSValue::from_bits(key.to_bits());

    if !obj_val.is_pointer() {
        return nanbox_false;
    }

    let obj_ptr = obj_val.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return nanbox_false;
    }

    if !key_val.is_string() {
        return nanbox_false;
    }

    let key_str = key_val.as_string_ptr();

    unsafe {
        let keys = (*obj_ptr).keys_array;
        if keys.is_null() {
            return nanbox_false;
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        for i in 0..key_count {
            let stored_key_val = crate::array::js_array_get(keys, i as u32);
            if stored_key_val.is_string() {
                let stored_key = stored_key_val.as_string_ptr();
                if crate::string::js_string_equals(key_str, stored_key) != 0 {
                    // Check if the field was deleted (set to undefined by delete operator)
                    let field_val = js_object_get_field(obj_ptr, i as u32);
                    if field_val.is_undefined() {
                        return nanbox_false;
                    }
                    return nanbox_true;
                }
            }
        }

        nanbox_false
    }
}

/// Get a field by its string key name
/// Returns the field value or undefined if the key is not found
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name(obj: *const ObjectHeader, key: *const crate::StringHeader) -> JSValue {
    // SSO property access (v0.5.213 Step 1 gate). The codegen inline
    // `.length` path routes SHORT_STRING_TAG receivers here because
    // it doesn't yet know about the SSO tag. Handle `.length` by
    // reading the length byte directly from the NaN-box payload.
    // Other property accesses on an SSO string (e.g. `.charAt` via
    // `[0]`, `.slice`) aren't yet routed here — handled by the
    // string method dispatch in a future migration step; today they
    // fall through to "undefined" which matches the behavior for
    // string-valued property access on untyped locals in general.
    {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            if !key.is_null() {
                unsafe {
                    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                    if key_bytes == b"length" {
                        let len = (obj_bits & crate::value::SHORT_STRING_LEN_MASK)
                            >> crate::value::SHORT_STRING_LEN_SHIFT;
                        return JSValue::number(len as f64);
                    }
                }
            }
            return JSValue::undefined();
        }
    }
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC || (raw as usize) < 0x10000 {
                // undefined/null tag, null pointer, or small handle — return undefined
                return JSValue::undefined();
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x1000000 {
        return JSValue::undefined();
    }
    unsafe {
        // Buffers: BufferHeader is allocated via raw `alloc()` (no GcHeader)
        // and tracked in BUFFER_REGISTRY. Detect first so the GC header check
        // below doesn't read garbage one word before the BufferHeader.
        // Route `.length` to `js_buffer_length` (matches the codegen path that
        // routes through PropertyGet for chained `Buffer.from(...).length`
        // expressions where the static type isn't recognized as Buffer).
        if crate::buffer::is_registered_buffer(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" || key_bytes == b"byteLength" {
                    let b = obj as *const crate::buffer::BufferHeader;
                    return JSValue::number(crate::buffer::js_buffer_length(b) as f64);
                }
            }
            return JSValue::undefined();
        }
        // Sets: SetHeader is allocated via raw `alloc()` (no GcHeader),
        // so we can't safely read the byte preceding the pointer to
        // determine its type. Detect via the SET_REGISTRY first and
        // route `.size` to `js_set_size`. Other property accesses on a
        // Set return undefined (matching Node behavior — Sets only have
        // a `size` getter property).
        if crate::set::is_registered_set(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"size" {
                    let s = obj as *const crate::set::SetHeader;
                    return JSValue::number(crate::set::js_set_size(s) as f64);
                }
            }
            return JSValue::undefined();
        }
        // Symbols: registered in SYMBOL_POINTERS by symbol.rs. Symbols
        // allocated via Symbol.for(...) are Box-leaked (no GcHeader), so
        // reading the byte before would be UB. Detect via the side table.
        if crate::symbol::is_registered_symbol(obj as usize) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let sym_f64 = f64::from_bits(
                    0x7FFD_0000_0000_0000u64 | (obj as u64 & 0x0000_FFFF_FFFF_FFFF),
                );
                if key_bytes == b"description" {
                    return JSValue::from_bits(
                        crate::symbol::js_symbol_description(sym_f64).to_bits(),
                    );
                }
            }
            return JSValue::undefined();
        }
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return JSValue::undefined();
        }
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if !is_valid_obj_ptr(obj as *const u8) { return JSValue::undefined(); }
        let gc_type = (*gc_header).obj_type;
        // Error objects: route the common instance properties (message,
        // name, stack, cause) through the dedicated error accessors.
        // `js_object_get_field_by_name_f64` is the codegen's default
        // property dispatch for caught exceptions, so this is the only
        // sensible place to wire Error access.
        if gc_type == crate::gc::GC_TYPE_ERROR {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let err_ptr = obj as *mut crate::error::ErrorHeader;
                match key_bytes {
                    b"message" => {
                        let s = crate::error::js_error_get_message(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"name" => {
                        let s = crate::error::js_error_get_name(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"stack" => {
                        let s = crate::error::js_error_get_stack(err_ptr);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"cause" => {
                        let v = crate::error::js_error_get_cause(err_ptr);
                        return JSValue::from_bits(v.to_bits());
                    }
                    b"errors" => {
                        // AggregateError.errors — return the errors array
                        // NaN-boxed with POINTER_TAG so callers can index
                        // into it. (The LLVM backend also has a direct
                        // `js_error_get_errors` fast path in expr.rs but
                        // this covers dynamic dispatch on caught errors.)
                        let errs = crate::error::js_error_get_errors(err_ptr);
                        if errs.is_null() {
                            return JSValue::undefined();
                        }
                        return JSValue::from_bits(crate::js_nanbox_pointer(errs as i64).to_bits());
                    }
                    _ => return JSValue::undefined(),
                }
            }
            return JSValue::undefined();
        }
        // Arrays: handle `.length` so dynamic property access on a
        // typed-Any local returned from `JSON.parse("[1,2,3]")` picks
        // up the real length instead of falling through to object
        // field lookup and returning undefined. The array-length
        // inline fast path in codegen fires only when the type is
        // statically known, so this branch catches the dynamic case.
        if gc_type == crate::gc::GC_TYPE_ARRAY {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let arr = obj as *const crate::array::ArrayHeader;
                    return JSValue::number(crate::array::js_array_length(arr) as f64);
                }
            }
            return JSValue::undefined();
        }
        // Issue #179 Phase 2: lazy array dispatch. `.length` returns
        // cached_length without materializing; any other property
        // access force-materializes (via the call into the generic
        // array path, which goes through `clean_arr_ptr` and hits
        // the lazy branch there).
        if gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let arr = obj as *const crate::array::ArrayHeader;
                    return JSValue::number(crate::array::js_array_length(arr) as f64);
                }
            }
            // Any other property access force-materializes, then
            // re-enters via the materialized ArrayHeader pointer.
            let materialized = crate::json_tape::force_materialize_lazy(
                obj as *mut crate::json_tape::LazyArrayHeader,
            );
            return js_object_get_field_by_name(materialized as *const ObjectHeader, key);
        }
        // Strings: handle `.length` so `(x as string).length` on an
        // unknown-typed local (TypeScript `as` casts are erased in
        // HIR) produces the real codepoint length.
        if gc_type == crate::gc::GC_TYPE_STRING {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"length" {
                    let s = obj as *const crate::StringHeader;
                    return JSValue::number((*s).byte_len as f64);
                }
            }
            return JSValue::undefined();
        }
        // Maps: handle `.size` for `obj.m.size` style access where m is
        // a Map field stored in a plain object literal. Without this
        // the dynamic property dispatch returns undefined.
        if gc_type == crate::gc::GC_TYPE_MAP {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                if key_bytes == b"size" {
                    let m = obj as *const crate::map::MapHeader;
                    return JSValue::number(crate::map::js_map_size(m) as f64);
                }
            }
            return JSValue::undefined();
        }
        // RegExp: RegExpHeader is allocated via GC_TYPE_OBJECT but tracked
        // in REGEX_POINTERS. Detect and route `.source`, `.flags`,
        // `.lastIndex`, `.global`, `.ignoreCase`, `.multiline`, `.sticky`,
        // `.unicode`, `.dotAll` to the regex header fields. Must run
        // before the generic object-field path so the keys_array lookup
        // doesn't try to read the regex header bytes as ObjectHeader.
        if gc_type == crate::gc::GC_TYPE_OBJECT && crate::regex::is_regex_pointer(obj as *const u8) {
            if !key.is_null() {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                let re = obj as *const crate::regex::RegExpHeader;
                match key_bytes {
                    b"source" => {
                        let s = crate::regex::js_regexp_get_source(re);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"flags" => {
                        let s = crate::regex::js_regexp_get_flags(re);
                        return JSValue::from_bits(crate::js_nanbox_string(s as i64).to_bits());
                    }
                    b"lastIndex" => {
                        return JSValue::number((*re).last_index as f64);
                    }
                    b"global" => {
                        return JSValue::bool((*re).global);
                    }
                    b"ignoreCase" => {
                        return JSValue::bool((*re).case_insensitive);
                    }
                    b"multiline" => {
                        return JSValue::bool((*re).multiline);
                    }
                    b"sticky" | b"unicode" | b"dotAll" | b"hasIndices" => {
                        return JSValue::bool(false);
                    }
                    _ => return JSValue::undefined(),
                }
            }
            return JSValue::undefined();
        }
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return JSValue::undefined();
            }
        }

        // Check for CLOSURE_MAGIC at offset 12 (closures may share GC_TYPE_OBJECT arena slot)
        {
            let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
            if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
                return JSValue::undefined();
            }
        }

        let keys = (*obj).keys_array;

        if keys.is_null() {
            return JSValue::undefined();
        }

        // Validate keys_array is a real heap pointer (upper 16 bits must be 0 for ARM64/x86-64 user space).
        // If the object is actually a non-Object type (closure, array, map, etc.), keys_array at offset
        // 16 may contain garbage. An invalid upper 16-bit value catches this case defensively.
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            return JSValue::undefined();
        }

        // Issue #62 phase B: the previous "ASCII-like pointer value" heuristic
        // assumed macOS mmap always returns arena pointers with `top_byte < 0x20`.
        // That stopped holding once strings started arena-allocating (more blocks,
        // mimalloc mapping into higher ranges): valid 0x000_04355_a033_* pointers
        // triggered false positives, the heuristic returned `undefined`, and tests
        // like `Object.defineProperty` flapped. The GcHeader `obj_type ==
        // GC_TYPE_ARRAY` check immediately below is a real content-level validation
        // (can't be faked by an address in any range) and fully supersedes this
        // address-sniffing heuristic.

        // Cross-platform safety: validate keys_array has a valid GcHeader.
        // If the keys_array pointer is corrupt (e.g., due to a stale reference after GC,
        // or a func_addr relocation issue on x86_64), the GcHeader check catches it
        // before we dereference the array contents.
        {
            let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let keys_gc_type = (*keys_gc).obj_type;
            // keys_array must be GC_TYPE_ARRAY (arena-allocated array)
            if keys_gc_type != crate::gc::GC_TYPE_ARRAY {
                return JSValue::undefined();
            }
        }

        // Fast path: check field index cache (keys_array_ptr + key_hash → field_index)
        // Objects with the same shape share the same keys_array, so we cache per-shape lookups.
        let key_bytes = std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            (*key).byte_len as usize,
        );
        let key_hash = {
            let mut h: u32 = 0x811c9dc5;
            for &b in key_bytes {
                h ^= b as u32;
                h = h.wrapping_mul(0x01000193);
            }
            h
        };
        let keys_id = keys as usize;

        // Thread-local inline cache: fixed-size direct-mapped cache (no allocation, no HashMap)
        // Each entry stores (keys_ptr, key_hash, field_index) for collision-safe validation
        const FIELD_CACHE_SIZE: usize = 1024;
        thread_local! {
            static FIELD_CACHE: std::cell::UnsafeCell<[(usize, u32, u32); FIELD_CACHE_SIZE]> =
                std::cell::UnsafeCell::new([(0usize, 0u32, 0u32); FIELD_CACHE_SIZE]);
        }
        let cache_idx = (keys_id.wrapping_add(key_hash as usize)) % FIELD_CACHE_SIZE;
        let cached = FIELD_CACHE.with(|c| {
            let cache = &*c.get();
            let entry = cache[cache_idx];
            if entry.0 == keys_id && entry.1 == key_hash { Some(entry.2) } else { None }
        });
        if let Some(field_idx) = cached {
            // Accessor short-circuit: if this (obj, key) has a getter installed,
            // invoke it instead of reading the slot. The `ACCESSORS_IN_USE`
            // thread-local gate keeps this off the hot path in the common case.
            if ACCESSORS_IN_USE.with(|c| c.get()) {
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                        if acc.get != 0 {
                            let closure = (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
                            if !closure.is_null() {
                                let result_f64 = crate::closure::js_closure_call0(closure);
                                return JSValue::from_bits(result_f64.to_bits());
                            }
                        }
                        // Has accessor but no getter → undefined.
                        return JSValue::undefined();
                    }
                }
            }
            return js_object_get_field(obj, field_idx);
        }

        // Slow path: linear scan through keys array
        let key_count = crate::array::js_array_length(keys) as usize;
        let field_count = (*obj).field_count as usize;

        if key_count > 65536 {
            return JSValue::undefined();
        }

        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;

        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Cache this lookup for next time
                    FIELD_CACHE.with(|c| {
                        let cache = &mut *c.get();
                        cache[cache_idx] = (keys_id, key_hash, i as u32);
                    });
                    // Accessor short-circuit (see fast path above).
                    if ACCESSORS_IN_USE.with(|c| c.get()) {
                        if let Ok(name) = std::str::from_utf8(key_bytes) {
                            if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                                if acc.get != 0 {
                                    let closure = (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
                                    if !closure.is_null() {
                                        let result_f64 = crate::closure::js_closure_call0(closure);
                                        return JSValue::from_bits(result_f64.to_bits());
                                    }
                                }
                                return JSValue::undefined();
                            }
                        }
                    }
                    if i < alloc_limit {
                        return js_object_get_field(obj, i as u32);
                    } else {
                        return match overflow_get(obj as usize, i) {
                            Some(bits) => JSValue::from_bits(bits),
                            None => JSValue::undefined(),
                        };
                    }
                }
            }
        }

        // Key not found
        JSValue::undefined()
    }
}

/// Get a field by its string key name, returned as f64 (raw JSValue bits)
/// This preserves the NaN-boxing for strings and other pointer types
#[no_mangle]
pub extern "C" fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const crate::StringHeader) -> f64 {
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

/// Monomorphic inline cache miss handler (issue #51).
///
/// Called when the codegen-emitted shape check (`obj->keys_array == cache[0]`)
/// fails. Performs the full field lookup via `js_object_get_field_by_name`,
/// then populates the per-site cache so subsequent calls with the same shape
/// hit the inline fast path (no function call, direct field load).
///
/// `cache` layout: `[keys_array_ptr: i64, field_slot_index: i64]`
///
/// Only caches when:
/// - obj is a valid ObjectHeader (not null, not handle, not string/array/etc.)
/// - field exists and its slot index < 8 (inline allocation limit)
///
/// Overflow fields (slot >= alloc_limit) are NOT cached and fall through to
/// the slow path — the fast path loads from `obj_ptr + 24 + slot*8` which
/// would read past the inline allocation.
#[no_mangle]
pub extern "C" fn js_object_get_field_ic_miss(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    cache: *mut [i64; 2],
) -> f64 {
    // SSO receiver — never cacheable. Route through the SSO-aware
    // `js_object_get_field_by_name` which handles `.length` inline
    // and returns undefined for other keys.
    if !key.is_null() {
        let obj_bits = obj as u64;
        if (obj_bits & crate::value::TAG_MASK) == crate::value::SHORT_STRING_TAG {
            let v = js_object_get_field_by_name(obj, key);
            return f64::from_bits(v.bits());
        }
    }
    if obj.is_null() || (obj as usize) < 0x10000 || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // When accessors are active anywhere in the program, skip the cache
    // entirely: the PIC fast path does a direct field load that bypasses
    // getter dispatch, so any object that uses defineProperty / get / set
    // would silently return the raw slot value instead of calling the
    // getter. The slow path through js_object_get_field_by_name handles
    // accessors correctly.
    let can_cache = !ACCESSORS_IN_USE.with(|c| c.get());
    unsafe {
        // Issue #72: validate this really is a GC_TYPE_OBJECT before reading
        // (*obj).keys_array — otherwise an Array/String/Buffer/etc. receiver
        // (whose `object_type` byte at offset 0 happens to be 1, matching
        // OBJECT_TYPE_REGULAR for a length-1 array) would be treated as
        // cacheable and seed the per-site PIC with garbage from element[1].
        // The codegen guard funnels non-OBJECT receivers here too, so this
        // belt-and-braces check keeps the cache from being primed with
        // values that would survive into the inline hot path.
        let is_object = (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 && {
            let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
        };
        let keys = (*obj).keys_array;
        let is_regular = is_object
            && (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR;
        if can_cache && is_regular && !keys.is_null() && (keys as usize) > 0x10000 {
            let key_count = *(keys as *const u32) as usize;
            let keys_data = (keys as *const u8).add(8) as *const f64;
            let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
            for i in 0..key_count {
                let k_bits = (*keys_data.add(i)).to_bits();
                let k_ptr = (k_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
                if !k_ptr.is_null() && crate::string::js_string_equals(k_ptr, key) != 0 {
                    if i >= alloc_limit {
                        // Field is in the overflow map — fall through to the
                        // slow path which handles overflow correctly.
                        break;
                    }
                    if i < 8 {
                        (*cache)[0] = keys as i64;
                        (*cache)[1] = i as i64;
                    }
                    let field_ptr = (obj as *const u8).add(
                        std::mem::size_of::<ObjectHeader>() + i * 8,
                    ) as *const f64;
                    return *field_ptr;
                }
            }
        }
    }
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

/// Set a field value by its string key name (dynamic property access)
/// This searches the keys array for a match and sets the corresponding value.
/// If the key doesn't exist, it adds it to the object.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name(obj: *mut ObjectHeader, key: *const crate::StringHeader, value: f64) {
    // Strip NaN-boxing tags if present (defensive: handle POINTER_TAG, UNDEFINED, NULL, etc.)
    let obj = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            // NaN-boxed value — extract lower 48 bits as pointer
            let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
            if raw.is_null() || top16 == 0x7FFC {
                return;
            }
            if (raw as usize) < 0x10000 {
                // Small handle — dispatch to handle property set if registered
                unsafe {
                    if let Some(dispatch) = HANDLE_PROPERTY_SET_DISPATCH {
                        if !key.is_null() {
                            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let name_len = (*key).byte_len as usize;
                            dispatch(raw as i64, name_ptr, name_len, value);
                        }
                    }
                }
                return;
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x1000000 {
        // Small non-null value — could be a stripped handle (after ensure_i64 stripped NaN-box tag)
        if !obj.is_null() && (obj as usize) > 0 {
            unsafe {
                if let Some(dispatch) = HANDLE_PROPERTY_SET_DISPATCH {
                    if !key.is_null() {
                        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let name_len = (*key).byte_len as usize;
                        dispatch(obj as i64, name_ptr, name_len, value);
                    }
                }
            }
        }
        return;
    }
    // Safety: obj is a valid heap pointer (> 0x10000) at this point
    unsafe {
        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return;
        }
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT && gc_type != crate::gc::GC_TYPE_CLOSURE {
        if !is_valid_obj_ptr(obj as *const u8) { return; }
            // Not a heap object/closure — only accept object_type == 1 (OBJECT_TYPE_REGULAR)
            let object_type = (*obj).object_type;
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                return;
            }
        }

        // Check if this is a ClosureHeader — closures support dynamic props via separate storage.
        // ClosureHeader has CLOSURE_MAGIC (0x434C4F53) at offset 12.
        // Without this check, (*obj).keys_array reads capture[0] → corruption/crash.
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    crate::closure::closure_set_dynamic_prop(obj as usize, name_str, value);
                }
            }
            return;
        }

        // Check Object.freeze/seal/preventExtensions flags
        let obj_flags = (*gc_header)._reserved;
        let is_frozen = obj_flags & crate::gc::OBJ_FLAG_FROZEN != 0;
        let is_sealed_or_no_extend = obj_flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0;

        let keys = (*obj).keys_array;

        // Validate keys_array is a real heap pointer or null.
        if !keys.is_null() {
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
                return;
            }
        }

        let prev_keys_usize = keys as usize;

        // Resolve to interned pointer for transition cache (pointer identity).
        // If the key is already interned (GC_FLAG_INTERNED set — e.g. from
        // js_string_concat intern hit), skip the FNV-1a hash entirely.
        let interned_key = if !key.is_null() && (key as usize) > 0x10000 {
            let gc_hdr = (key as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_hdr).gc_flags & crate::gc::GC_FLAG_INTERNED != 0 {
                key // already interned
            } else {
                let kh = key_content_hash(key);
                crate::string::js_string_intern(key, kh)
            }
        } else {
            key
        };

        // FAST PATH: shape-transition cache with interned string pointer identity.
        if !key.is_null()
            && !is_frozen
            && !is_sealed_or_no_extend
            && !GLOBAL_DESCRIPTORS_IN_USE.load(Ordering::Relaxed)
        {
            if let Some((next_keys, slot_idx)) = transition_cache_lookup(prev_keys_usize, interned_key) {
                // Defensive: strip a raw-null POINTER_TAG value the same
                // way the slow overflow path below does, so a bogus
                // 0x7FFD_0000_0000_0000 store doesn't leak into an
                // overflow map.
                let vbits = value.to_bits();
                let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                    crate::value::TAG_UNDEFINED
                } else { vbits };
                (*obj).keys_array = next_keys as *mut ArrayHeader;
                let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
                if (slot_idx as usize) < alloc_limit {
                    // Inline the field write — `obj` has already been
                    // validated (GC header read, type check, closure
                    // check) by the prelude above, and `vbits` has had
                    // the null-POINTER-TAG replacement applied. No
                    // point re-doing it in `js_object_set_field`.
                    let fields_ptr = (obj as *mut u8)
                        .add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
                    ptr::write(fields_ptr.add(slot_idx as usize), JSValue::from_bits(vbits));
                    // Bump field_count only for inline slots — leaving
                    // it at the physical capacity is what steers
                    // `js_object_get_field_by_name`'s reads to the
                    // overflow map for slots ≥ alloc_limit. Bumping it
                    // past capacity would make reads dereference past
                    // the object's inline field array into adjacent
                    // arena data.
                    if slot_idx >= (*obj).field_count {
                        (*obj).field_count = slot_idx + 1;
                    }
                } else {
                    // Cached slot is past the object's inline capacity —
                    // store in the overflow map (same as the slow path's
                    // `new_index >= alloc_limit` branch).
                    overflow_set(obj as usize, slot_idx as usize, vbits);
                    // Deliberately do NOT bump field_count here — see
                    // above.
                }
                return;
            }
        }

        // If no keys array exists, create one (adding new key)
        if keys.is_null() {
            // Frozen or sealed/non-extensible objects reject new keys
            if is_frozen || is_sealed_or_no_extend {
                return;
            }
            // Create a new keys array with the key
            let new_keys = crate::array::js_array_alloc(4);
            let new_keys = crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
            (*obj).keys_array = new_keys;

            // Reallocate fields to hold at least one value
            // Note: We assume the object has enough field slots pre-allocated
            js_object_set_field(obj, 0, JSValue::from_bits(value.to_bits()));
            // Bump field_count so Object.keys()/values()/entries() see the new property.
            if (*obj).field_count == 0 {
                (*obj).field_count = 1;
            }
            // Record the null→single-key transition so the next object
            // that starts with `{}` and sets the same first key hits the
            // fast path above instead of allocating a fresh 4-elem
            // keys_array here.
            transition_cache_insert(0, interned_key, new_keys as usize, 0);
            return;
        }

        // Defer the Rust-String allocation for the incoming key: we only
        // need it if an accessor descriptor or per-property writable
        // attribute has been installed on this object. Both paths are
        // guarded by process-wide flags (`ACCESSORS_IN_USE` and
        // `PROPERTY_ATTRS_IN_USE`) so the common case — plain data
        // properties on a normal object — avoids the `.to_string()`
        // entirely. A 20-property row object written at 10k rows saw
        // 200k of those allocations per query; with this guard the
        // count drops to zero unless userland actually defined a
        // descriptor.
        let needs_descriptor_key = ACCESSORS_IN_USE.with(|c| c.get())
            || PROPERTY_ATTRS_IN_USE.with(|c| c.get());
        let incoming_key_str: Option<String> = if needs_descriptor_key && !key.is_null() {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        } else { None };

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // Keys are stored as string pointers (NaN-boxed)
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Found it - update the field (frozen objects reject writes)
                    if is_frozen {
                        return;
                    }
                    // Accessor short-circuit: if a setter is registered, invoke
                    // it instead of writing the slot. A property with `get` but
                    // no `set` silently ignores the write (non-strict mode).
                    if ACCESSORS_IN_USE.with(|c| c.get()) {
                        if let Some(ref k) = incoming_key_str {
                            if let Some(acc) = get_accessor_descriptor(obj as usize, k) {
                                if acc.set != 0 {
                                    let closure = (acc.set & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
                                    if !closure.is_null() {
                                        crate::closure::js_closure_call1(closure, value);
                                    }
                                }
                                return;
                            }
                        }
                    }
                    // Per-property writable check (set by Object.defineProperty / freeze).
                    if PROPERTY_ATTRS_IN_USE.with(|c| c.get()) {
                        if let Some(ref k) = incoming_key_str {
                            if let Some(attrs) = get_property_attrs(obj as usize, k) {
                                if !attrs.writable() {
                                    return;
                                }
                            }
                        }
                    }
                    if i < alloc_limit {
                        js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
                    } else {
                        // This key was previously stored in the overflow map — update it there
                        let vbits = value.to_bits();
                        let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                            crate::value::TAG_UNDEFINED
                        } else { vbits };
                        overflow_set(obj as usize, i, vbits);
                    }
                    return;
                }
            }
        }

        // Key not found - add it to the object.
        // Frozen/sealed/non-extensible objects reject new keys
        if is_frozen || is_sealed_or_no_extend {
            return;
        }
        // CRITICAL: The keys_array may be SHARED via SHAPE_CACHE (multiple objects with
        // the same shape hash share the same keys array). We must clone it before mutating
        // to avoid corrupting other objects' keys.
        //
        // We detect sharing via the `GC_FLAG_SHAPE_SHARED` bit that
        // `shape_cache_insert` stamps onto the array's GC header —
        // arrays allocated in the `keys.is_null()` branch above are
        // exclusively owned and don't have the flag, so we skip the
        // clone entirely. This saves ~19 clones of growing size per
        // 20-property plain-object literal.
        //
        // Validate the GC header before reading it. `keys_array` has
        // already been range-checked for user address space but may
        // still point at something other than a GC-allocated array
        // in rare cases (static data, buffers re-interpreted as keys
        // arrays). If the header doesn't identify as GC_TYPE_ARRAY,
        // assume shared and clone (the previous, always-safe behaviour).
        let keys_gc_header = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE)
            as *const crate::gc::GcHeader;
        let keys_shared = if (keys as usize) >= crate::gc::GC_HEADER_SIZE
            && (*keys_gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
        {
            (*keys_gc_header).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
        } else {
            // Unknown provenance — take the safe side.
            true
        };
        let owned_keys = if keys_shared {
            let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
            let src_data = (keys as *const u8).add(8) as *const f64;
            let dst_data = (cloned as *mut u8).add(8) as *mut f64;
            for i in 0..key_count {
                *dst_data.add(i) = *src_data.add(i);
            }
            (*cloned).length = key_count as u32;
            (*obj).keys_array = cloned;
            cloned
        } else {
            keys
        };

        // Check if we have a spare physical slot (js_object_alloc_with_shape allocates max(N,8) slots).
        // Class objects (js_object_alloc_class_with_keys) have only exactly field_count slots;
        // attempting to write to new_index = key_count would overflow into the next heap allocation.
        let new_index = key_count;
        if new_index >= alloc_limit {
            // No inline room — store in the overflow HashMap so the value is not lost.
            // Also add the key to keys_array so Object.keys() sees it.
            let vbits = value.to_bits();
            let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                eprintln!("[WARN_NULL_PTR] overflow new store: null POINTER_TAG at obj={:p} new_index={} — replacing with undefined", obj, new_index);
                crate::value::TAG_UNDEFINED
            } else { vbits };
            let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
            (*obj).keys_array = new_keys;
            overflow_set(obj as usize, new_index, vbits);
            // Record the shape transition so the next object sharing
            // `prev_keys` that adds the same key hits the fast path.
            // The cached target is stamped `GC_FLAG_SHAPE_SHARED` by
            // `transition_cache_insert`, which triggers clone-on-extend
            // on either object if someone later appends past this key.
            transition_cache_insert(prev_keys_usize, interned_key, new_keys as usize, new_index as u32);
            return;
        }
        // First, add the key to the keys array (may reallocate)
        let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
        // Update the object's keys_array pointer in case js_array_push reallocated
        (*obj).keys_array = new_keys;

        // Set the field at the new index and update logical field_count
        js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
        // Bump field_count to reflect the newly added property
        if new_index as u32 >= (*obj).field_count {
            (*obj).field_count = new_index as u32 + 1;
        }
        // Record the shape transition — see above for semantics.
        transition_cache_insert(prev_keys_usize, interned_key, new_keys as usize, new_index as u32);
    }
}

/// Delete a field from an object by its string key name
/// Returns 1 if the field was deleted (or didn't exist), 0 otherwise
/// Note: In strict mode, this would return 0 for non-configurable properties,
/// but we don't track configurability, so we always return 1.
#[no_mangle]
pub extern "C" fn js_object_delete_field(obj: *mut ObjectHeader, key: *const crate::StringHeader) -> i32 {
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            // No keys array means no fields to delete, but delete "succeeds" vacuously
            return 1;
        }

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // Keys are stored as string pointers (NaN-boxed)
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Found it - set the field to undefined
                    js_object_set_field(obj, i as u32, JSValue::undefined());
                    return 1;
                }
            }
        }

        // Key not found - delete still "succeeds" for non-existent properties
        1
    }
}

/// Delete a field from an object using a dynamic key (could be string or number index)
/// For arrays, this sets the element to undefined
/// Returns 1 if successful, 0 otherwise
#[no_mangle]
pub extern "C" fn js_object_delete_dynamic(obj: *mut ObjectHeader, key: f64) -> i32 {
    let key_val = JSValue::from_bits(key.to_bits());

    // If the key is a string, use js_object_delete_field
    if key_val.is_string() {
        let key_str = key_val.as_string_ptr();
        return js_object_delete_field(obj, key_str);
    }

    // If the key is a number, treat as array index
    if key_val.is_number() {
        let index = key_val.as_number() as usize;
        // Try to treat it as an array and set the element to undefined
        // This is a simplified implementation - real JS delete on arrays
        // creates a hole (sparse array), but we just set to undefined
        let arr = obj as *mut crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr) as usize;
        if index < len {
            crate::array::js_array_set(arr, index as u32, JSValue::undefined());
            return 1;
        }
    }

    // For other types, delete succeeds vacuously
    1
}

/// Create a rest object from destructuring: copies all properties from src except excluded keys.
/// exclude_keys is an array of NaN-boxed string pointers (the explicitly destructured keys).
/// Returns a pointer to a new object with the remaining key-value pairs.
#[no_mangle]
pub extern "C" fn js_object_rest(src: *const ObjectHeader, exclude_keys: *const ArrayHeader) -> *mut ObjectHeader {
    if src.is_null() {
        return js_object_alloc(0, 0);
    }
    unsafe {
        let keys = (*src).keys_array;
        if keys.is_null() {
            return js_object_alloc(0, 0);
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        let exclude_count = if exclude_keys.is_null() { 0 } else { crate::array::js_array_length(exclude_keys) as usize };

        // Collect indices of keys to include (not in exclude list and not undefined/deleted)
        let mut include_indices: Vec<usize> = Vec::new();
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if !key_val.is_string() { continue; }
            let key_str = key_val.as_string_ptr();

            // Check if field was deleted
            let field_val = js_object_get_field(src, i as u32);
            if field_val.is_undefined() { continue; }

            // Check if this key is in the exclude list
            let mut excluded = false;
            for j in 0..exclude_count {
                let ex_val = crate::array::js_array_get(exclude_keys, j as u32);
                if ex_val.is_string() {
                    let ex_str = ex_val.as_string_ptr();
                    if crate::string::js_string_equals(key_str, ex_str) != 0 {
                        excluded = true;
                        break;
                    }
                }
            }
            if !excluded {
                include_indices.push(i);
            }
        }

        // Allocate new object with the right number of fields
        let rest_count = include_indices.len() as u32;
        let rest_obj = js_object_alloc(0, rest_count);

        // Create keys array for the rest object
        let rest_keys = crate::array::js_array_alloc_with_length(rest_count);
        (*rest_obj).keys_array = rest_keys;

        // Copy included key-value pairs
        for (new_idx, &src_idx) in include_indices.iter().enumerate() {
            let key_val = crate::array::js_array_get(keys, src_idx as u32);
            crate::array::js_array_set(rest_keys, new_idx as u32, key_val);

            let field_val = js_object_get_field(src, src_idx as u32);
            js_object_set_field(rest_obj, new_idx as u32, field_val);
        }

        rest_obj
    }
}

/// Check if a value is an instance of a class with the given class_id
/// Walks the inheritance chain to check parent classes
/// Returns NaN-boxed TAG_TRUE / TAG_FALSE so the result identifies as a boolean.
#[no_mangle]
pub extern "C" fn js_instanceof(value: f64, class_id: u32) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let true_val = f64::from_bits(TAG_TRUE);
    let false_val = f64::from_bits(TAG_FALSE);

    // User-defined `Symbol.hasInstance` takes precedence over the built-in
    // prototype-chain walk. The HIR lifts `static [Symbol.hasInstance](v)`
    // to a top-level function `__perry_wk_hasinstance_<class>` and the
    // LLVM backend registers a pointer to it against the class's id at
    // module init. If a hook is present, call it with the candidate value
    // and return the boolean-shaped result directly.
    if let Some(func_ptr) = lookup_has_instance_hook(class_id) {
        let hook: extern "C" fn(f64) -> f64 =
            unsafe { std::mem::transmute(func_ptr as *const u8) };
        let result = hook(value);
        // Normalize: any truthy NaN-boxed bool stays as the TAG_TRUE/FALSE
        // sentinel. User-written `return typeof v === "number" && ...`
        // already returns a NaN-boxed bool, so this is usually a no-op.
        let rbits = result.to_bits();
        if rbits == TAG_TRUE || rbits == TAG_FALSE {
            return result;
        }
        // Fallback: treat as truthy → TRUE, zero/undefined → FALSE.
        if result.is_nan() && rbits & 0xFFFF_0000_0000_0000 == 0x7FFC_0000_0000_0000 {
            return false_val;
        }
        if result == 0.0 || result.is_nan() {
            return false_val;
        }
        return true_val;
    }

    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);

    // Special handling for Uint8Array/Buffer (class_id 0xFFFF0004)
    // Perry buffers are raw BufferHeader pointers bitcast to f64 (not NaN-boxed),
    // so the normal POINTER_TAG check doesn't work for them.
    // We use a thread-local buffer registry to identify buffer pointers.
    if class_id == crate::buffer::BUFFER_TYPE_ID {
        // Check if NaN-boxed pointer
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::buffer::is_registered_buffer(addr) {
                return true_val;
            }
        }
        // Check if raw pointer (buffer values are bitcast, not NaN-boxed)
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 {
            if crate::buffer::is_registered_buffer(bits as usize) {
                return true_val;
            }
        }
        return false_val;
    }

    // Built-in JS types Map / Set / RegExp / Date — Perry doesn't define
    // user classes for these, so we use reserved class IDs and detect via
    // the per-type registries (MAP_REGISTRY / SET_REGISTRY / REGEX_POINTERS)
    // or, for Date, by checking that the value is a finite f64 timestamp.
    const CLASS_ID_DATE: u32 = 0xFFFF0020;
    const CLASS_ID_REGEXP: u32 = 0xFFFF0021;
    const CLASS_ID_MAP: u32 = 0xFFFF0022;
    const CLASS_ID_SET: u32 = 0xFFFF0023;
    if class_id == CLASS_ID_DATE {
        // A Perry Date is a raw f64 timestamp (no NaN-box tag, real f64).
        // Distinguishing it from a regular number requires a side-channel:
        // `js_date_new(...)` registers the f64 bits in DATE_REGISTRY, and
        // here we consult that registry. Without the registry, every finite
        // number would match (the prior "approximate" rule), which made
        // `100 instanceof Date` true and broke the BSON encoder's typed
        // dispatch (`if (value instanceof Date) … else if (typeof v === 'number') …`).
        if !value.is_nan() && value.is_finite() {
            if crate::date::is_registered_date_bits(value.to_bits()) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_MAP {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::map::is_registered_map(addr) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_SET {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::set::is_registered_set(addr) {
                return true_val;
            }
        }
        return false_val;
    }
    if class_id == CLASS_ID_REGEXP {
        if jsval.is_pointer() {
            let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::regex::is_regex_pointer(addr as *const u8) {
                return true_val;
            }
        }
        return false_val;
    }

    // Array — Perry arrays are heap allocations with `GC_TYPE_ARRAY` in
    // their gc_header (one byte at obj-8). Pointer can arrive NaN-boxed
    // (POINTER_TAG) or as a raw bitcast f64; handle both. Lazy arrays
    // (Phase 5 JSON.parse result) are also arrays from the user's
    // perspective — must return true without force-materializing.
    const CLASS_ID_ARRAY: u32 = 0xFFFF0024;
    if class_id == CLASS_ID_ARRAY {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 { bits as usize } else { 0 }
        };
        if addr != 0 && addr >= crate::gc::GC_HEADER_SIZE {
            let gc_header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            unsafe {
                let obj_type = (*gc_header).obj_type;
                if obj_type == crate::gc::GC_TYPE_ARRAY
                    || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
                {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // Typed arrays — Int8Array..Float64Array reserved IDs (0xFFFF0030..37).
    // The pointer can arrive as either a NaN-boxed POINTER_TAG value or a
    // raw bitcast f64, so handle both forms.
    if (0xFFFF0030..=0xFFFF0037).contains(&class_id) {
        let addr = if jsval.is_pointer() {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            let top16 = (bits >> 48) as u16;
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0 {
            if let Some(actual_kind) = crate::typedarray::lookup_typed_array_kind(addr) {
                let want_id = crate::typedarray::class_id_for_kind(actual_kind);
                if want_id == class_id {
                    return true_val;
                }
            }
        }
        return false_val;
    }

    // Only objects (pointers) can be instances of classes
    if !jsval.is_pointer() {
        return false_val;
    }

    // Get the object pointer
    let obj_ptr = jsval.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return false_val;
    }

    unsafe {
        // Special handling for built-in Error and its subclasses (TypeError, RangeError, etc.).
        // ErrorHeader uses GC_TYPE_ERROR; we match by error_kind against the requested CLASS_ID_*.
        let gc_header = (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ERROR {
            let err_ptr = obj_ptr as *const crate::error::ErrorHeader;
            let kind = (*err_ptr).error_kind;
            return match class_id {
                crate::error::CLASS_ID_ERROR => true_val,
                crate::error::CLASS_ID_TYPE_ERROR => {
                    if kind == crate::error::ERROR_KIND_TYPE_ERROR { true_val } else { false_val }
                }
                crate::error::CLASS_ID_RANGE_ERROR => {
                    if kind == crate::error::ERROR_KIND_RANGE_ERROR { true_val } else { false_val }
                }
                crate::error::CLASS_ID_REFERENCE_ERROR => {
                    if kind == crate::error::ERROR_KIND_REFERENCE_ERROR { true_val } else { false_val }
                }
                crate::error::CLASS_ID_SYNTAX_ERROR => {
                    if kind == crate::error::ERROR_KIND_SYNTAX_ERROR { true_val } else { false_val }
                }
                crate::error::CLASS_ID_AGGREGATE_ERROR => {
                    if kind == crate::error::ERROR_KIND_AGGREGATE_ERROR { true_val } else { false_val }
                }
                _ => false_val,
            };
        }

        // For user-defined classes that extend Error: `myErr instanceof Error` should be true.
        if class_id == crate::error::CLASS_ID_ERROR {
            let obj_class_id = (*obj_ptr).class_id;
            if extends_builtin_error(obj_class_id) {
                return true_val;
            }
        }

        // Check if the object's class_id matches directly
        let obj_class_id = (*obj_ptr).class_id;
        if obj_class_id == class_id {
            return true_val;
        }

        // Walk up the inheritance chain using the class registry
        let mut current_class = obj_class_id;
        while let Some(parent_id) = get_parent_class_id(current_class) {
            if parent_id == 0 {
                break;
            }
            if parent_id == class_id {
                return true_val;
            }
            current_class = parent_id;
        }

        false_val
    }
}

/// Call a method on an object with dynamic dispatch
/// This is used for runtime method calls when the method cannot be resolved statically.
/// object: NaN-boxed f64 containing an object pointer
/// method_name_ptr: pointer to the method name string (raw bytes, not StringHeader)
/// method_name_len: length of the method name
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as f64
///
/// NOTE: This function is named js_native_call_method to avoid symbol collision
/// with js_call_method in perry-jsruntime which handles V8 JavaScript values.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Get the method name (parsed early for depth guard logging)
    let method_name = if method_name_ptr.is_null() || method_name_len == 0 {
        ""
    } else {
        let bytes = std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len);
        std::str::from_utf8(bytes).unwrap_or("")
    };
    // RAII recursion depth guard: prevent stack overflow from circular module deps.
    // The guard auto-decrements on drop, covering all ~20 return points in this function.
    // When max depth is hit, return a pointer to a static empty object instead of undefined.
    // This prevents crashes when callers NaN-unbox the result and dereference it as a pointer.
    let _depth_guard = match CallMethodDepthGuard::enter(method_name) {
        Some(g) => g,
        None => {
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }
    };

    // Check if this is a JS handle (V8 object from JS runtime)
    if crate::value::is_js_handle(object) {
        let func_ptr = crate::value::JS_HANDLE_CALL_METHOD.load(std::sync::atomic::Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: unsafe extern "C" fn(f64, *const i8, usize, *const f64, usize) -> f64 =
                std::mem::transmute(func_ptr);
            let result = func(object, method_name_ptr, method_name_len, args_ptr, args_len);
            return result;
        }
        return f64::from_bits(0x7FF8_0000_0000_0001); // undefined
    }

    let jsval = JSValue::from_bits(object.to_bits());

    // Symbols: Symbol.for() pointers are Box-leaked (no GcHeader), so the
    // ObjectHeader path below would dereference garbage. Detect symbols
    // up front via the side-table.
    if jsval.is_pointer() {
        let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::symbol::is_registered_symbol(raw_ptr) {
            let sym_f64 = object;
            return match method_name {
                "toString" => {
                    let s = crate::symbol::js_symbol_to_string(sym_f64);
                    f64::from_bits(JSValue::string_ptr(s as *mut crate::StringHeader).bits())
                }
                "valueOf" => sym_f64,
                "description" => {
                    f64::from_bits(crate::symbol::js_symbol_description(sym_f64).to_bits())
                }
                _ => f64::from_bits(crate::value::TAG_UNDEFINED),
            };
        }
    }

    // Handle BigInt method calls (NaN-boxed with BIGINT_TAG 0x7FFA)
    if jsval.is_bigint() {
        let bigint_ptr = crate::bigint::clean_bigint_ptr(
            (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader
        );
        match method_name {
            "isZero" => {
                let result = crate::bigint::js_bigint_is_zero(bigint_ptr);
                return f64::from_bits(JSValue::bool(result != 0).bits());
            }
            "isNeg" | "isNegative" => {
                let result = crate::bigint::js_bigint_is_negative(bigint_ptr);
                return f64::from_bits(JSValue::bool(result != 0).bits());
            }
            "toNumber" => {
                return crate::bigint::js_bigint_to_f64(bigint_ptr);
            }
            "toString" => {
                let result_ptr = if args_len > 0 && !args_ptr.is_null() {
                    let radix_f64 = *args_ptr;
                    let radix = radix_f64 as i32;
                    crate::bigint::js_bigint_to_string_radix(bigint_ptr, radix)
                } else {
                    crate::bigint::js_bigint_to_string(bigint_ptr)
                };
                return f64::from_bits(JSValue::string_ptr(result_ptr).bits());
            }
            "add" | "sub" | "mul" | "div" | "mod" | "umod" | "pow"
            | "and" | "or" | "xor" | "shln" | "shrn" | "maskn"
            | "eq" | "lt" | "lte" | "gt" | "gte" | "cmp"
            | "fromTwos" | "toTwos" => {
                return dispatch_bigint_binary_method(bigint_ptr, method_name, args_ptr, args_len);
            }
            _ => {
                // Unknown BigInt method - fall through to general dispatch
            }
        }
    }

    // Check for raw handle integer: Perry may bit-cast an i64 handle directly to f64,
    // producing a subnormal float (bits == handle_id, no NaN-box tag). Values 0 < bits < 0x100000
    // with no tag are raw handle IDs from Perry's integer-typed handle parameters.
    let raw_bits = object.to_bits();
    if raw_bits > 0 && raw_bits < 0x100000 {
        if let Some(dispatch) = HANDLE_METHOD_DISPATCH {
            return dispatch(
                raw_bits as i64,
                method_name.as_ptr(),
                method_name.len(),
                args_ptr,
                args_len,
            );
        }
        return f64::from_bits(0x7FF8_0000_0000_0001); // undefined
    }

    // Check if this is a handle-based object (small integer, not a real heap pointer)
    // Handles are used by Fastify, ioredis, and other native modules that store
    // objects in a registry and use integer IDs to reference them.
    if jsval.is_pointer() {
        let raw_ptr = jsval.as_pointer::<u8>() as usize;
        if raw_ptr > 0 && raw_ptr < 0x100000 {
            // This is a handle, not a real memory pointer - dispatch to stdlib
            if let Some(dispatch) = HANDLE_METHOD_DISPATCH {
                return dispatch(
                    raw_ptr as i64,
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                );
            }
            // No dispatcher registered, return undefined
            return f64::from_bits(0x7FF8_0000_0000_0001);
        }

        // Guard: null pointer (raw_ptr == 0) means null POINTER_TAG (0x7FFD_0000_0000_0000)
        // Produced by codegen bugs (uninitialized I64 NaN-boxed). Return undefined instead of crashing.
        if raw_ptr == 0 {
            eprintln!("[NULL_PTR_METHOD_CALL] js_native_call_method: null pointer object for method '{}'", method_name);
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Buffer / Uint8Array dispatch — buffers are allocated raw without
        // a GcHeader, so the GC type check below would read random bytes
        // before the buffer storage and may accidentally match GC_TYPE_OBJECT.
        // Detect buffers via the BUFFER_REGISTRY first and route through the
        // dedicated dispatcher.
        if crate::buffer::is_registered_buffer(raw_ptr) {
            return dispatch_buffer_method(raw_ptr, method_name, args_ptr, args_len);
        }

        // Array method dispatch: when the object is a real or lazy array at runtime,
        // dispatch callback-bearing array methods directly to the array runtime helpers.
        // This covers the `anyTypedVar.map(fn)` / `anyTypedVar.filter(fn)` pattern where
        // the HIR lowering conservatively skipped Expr::ArrayMap/Filter because the
        // receiver's static type was `any` and the method name overlaps with user-class
        // method names — see the `is_class_overlapping_method` guard in expr_call.rs
        // (issue #267). The GC type check here ensures we only intercept when the
        // value is actually an array; user-class instances with a `.map` closure field
        // fall through to the object-field scan below unchanged.
        if raw_ptr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let arr_gc_hdr = (raw_ptr as *const u8)
                .sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let arr_obj_type = (*arr_gc_hdr).obj_type;
            if arr_obj_type == crate::gc::GC_TYPE_ARRAY
                || arr_obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            {
                match method_name {
                    "map" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_map(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "filter" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_filter(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    _ => {} // not a handled array method — fall through to object dispatch
                }
            }
        }

        // Check if this is a native module namespace object (e.g., fs, os, path)
        let obj = jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader to confirm this is actually an object before reading class_id
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
        if !is_valid_obj_ptr(obj as *const u8) { return 0.0; }
            }

            // Scan object fields for a callable property (closure stored via IndexSet)
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if (keys_ptr as u64) >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_key = crate::string::js_string_from_bytes(
                            method_name.as_ptr(),
                            method_name.len() as u32,
                        );
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if key_val.is_string() {
                                let stored_key = key_val.as_string_ptr();
                                if crate::string::js_string_equals(method_key, stored_key) != 0 {
                                    let field_val = js_object_get_field(obj as *mut _, i as u32);
                                    // Always try the field as a callable —
                                    // `js_native_call_value` validates
                                    // CLOSURE_MAGIC internally and safely
                                    // returns undefined for non-callables.
                                    // The previous `is_pointer()` gate bailed
                                    // on raw-pointer-bit values (e.g. the
                                    // Promise executor's resolve/reject
                                    // closures — stored as
                                    // `transmute(ptr → f64)` without a
                                    // POINTER_TAG). That turned
                                    // `box.resolve(val)` into a no-op that
                                    // returned the raw pointer bits instead
                                    // of invoking `js_promise_resolve`, so
                                    // the outer `await` hung forever
                                    // (issue #87).
                                    return crate::closure::js_native_call_value(
                                        f64::from_bits(field_val.bits()),
                                        args_ptr,
                                        args_len,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup for class instances
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        if let Some(vtable) = reg.get(&class_id) {
                            if let Some(entry) = vtable.methods.get(method_name) {
                                let this_i64 = jsval.as_pointer::<u8>() as i64;
                                return call_vtable_method(
                                    entry.func_ptr, this_i64,
                                    args_ptr, args_len, entry.param_count,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Check Map/Set registries for raw or NaN-boxed pointers.
    // Maps/Sets are allocated with plain alloc (no GcHeader), so they can't be
    // dispatched through the ObjectHeader path below.
    {
        let check_ptr = if jsval.is_pointer() {
            (raw_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if !object.is_nan() && raw_bits >= 0x100000 && (raw_bits >> 48) == 0 {
            raw_bits as usize
        } else {
            0
        };
        if check_ptr >= 0x10000 {
            if crate::map::is_registered_map(check_ptr) {
                let map = check_ptr as *mut crate::map::MapHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return match method_name {
                    "get" if !args.is_empty() => crate::map::js_map_get(map, args[0]),
                    "set" if args.len() >= 2 => {
                        let result = crate::map::js_map_set(map, args[0], args[1]);
                        f64::from_bits(JSValue::pointer(result as *mut u8).bits())
                    }
                    "has" if !args.is_empty() => {
                        let r = crate::map::js_map_has(map, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "delete" if !args.is_empty() => {
                        let r = crate::map::js_map_delete(map, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "clear" => { crate::map::js_map_clear(map); f64::from_bits(crate::value::TAG_UNDEFINED) }
                    "size" => crate::map::js_map_size(map) as f64,
                    "entries" => f64::from_bits(JSValue::pointer(crate::map::js_map_entries(map) as *mut u8).bits()),
                    "keys" => f64::from_bits(JSValue::pointer(crate::map::js_map_keys(map) as *mut u8).bits()),
                    "values" => f64::from_bits(JSValue::pointer(crate::map::js_map_values(map) as *mut u8).bits()),
                    "forEach" if !args.is_empty() => { crate::map::js_map_foreach(map, args[0]); f64::from_bits(crate::value::TAG_UNDEFINED) }
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
            }
            if crate::set::is_registered_set(check_ptr) {
                let set = check_ptr as *mut crate::set::SetHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return match method_name {
                    "add" if !args.is_empty() => {
                        let result = crate::set::js_set_add(set, args[0]);
                        f64::from_bits(JSValue::pointer(result as *mut u8).bits())
                    }
                    "has" if !args.is_empty() => {
                        let r = crate::set::js_set_has(set, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "delete" if !args.is_empty() => {
                        let r = crate::set::js_set_delete(set, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "clear" => { crate::set::js_set_clear(set); f64::from_bits(crate::value::TAG_UNDEFINED) }
                    "size" => crate::set::js_set_size(set) as f64,
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
            }
            // Buffer / Uint8Array dispatch — allocated raw, not behind a
            // GcHeader, so it can't be discovered through the ObjectHeader
            // path below. Tracked in BUFFER_REGISTRY. Routes Node-style
            // numeric read/write/search/swap method family through
            // `crate::buffer` helpers.
            if crate::buffer::is_registered_buffer(check_ptr) {
                return dispatch_buffer_method(check_ptr, method_name, args_ptr, args_len);
            }
        }
    }

    // Handle raw pointer values without NaN-box tags.
    // Perry sometimes bitcasts I64 pointers to F64 without NaN-boxing (POINTER_TAG).
    // These appear as subnormal floats with bits in the valid heap address range
    // (0x100000 .. 0x0000_FFFF_FFFF_FFFF, upper 16 bits = 0).
    if !jsval.is_pointer() && !object.is_nan() && raw_bits >= 0x100000 && (raw_bits >> 48) == 0 {
        // Looks like a raw heap pointer — re-wrap as POINTER_TAG and retry
        let reboxed = f64::from_bits(0x7FFD_0000_0000_0000u64 | raw_bits);
        let reboxed_jsval = JSValue::from_bits(reboxed.to_bits());
        let obj = reboxed_jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader before accessing
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            // Check for native module namespace
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
        if !is_valid_obj_ptr(obj as *const u8) { return 0.0; }
            }

            // Field name scan on this object
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if (keys_ptr as u64) >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_key = crate::string::js_string_from_bytes(
                            method_name.as_ptr(),
                            method_name.len() as u32,
                        );
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if key_val.is_string() {
                                let stored_key = key_val.as_string_ptr();
                                if crate::string::js_string_equals(method_key, stored_key) != 0 {
                                    let field_val = js_object_get_field(obj as *mut _, i as u32);
                                    if field_val.is_pointer() {
                                        return crate::closure::js_native_call_value(
                                            f64::from_bits(field_val.bits()),
                                            args_ptr,
                                            args_len,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        if let Some(vtable) = reg.get(&class_id) {
                            if let Some(entry) = vtable.methods.get(method_name) {
                                let this_i64 = raw_bits as i64;
                                return call_vtable_method(
                                    entry.func_ptr, this_i64,
                                    args_ptr, args_len, entry.param_count,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Handle common method calls
    match method_name {
        // Function.prototype.bind - returns the same function for native closures
        // This is a simplification - real bind() creates a new function with bound 'this'
        "bind" => {
            // For native closures, we return the function as-is
            // The 'this' binding is handled at the call site
            return object;
        }

        // Common string methods on string values
        "toString" => {
            if jsval.is_string() {
                return object;
            } else if jsval.is_bigint() {
                let ptr = jsval.as_bigint_ptr();
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_number() {
                let n = jsval.as_number();
                let s = if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                    (n as i64).to_string()
                } else {
                    n.to_string()
                };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_bool() {
                let s = if jsval.as_bool() { "true" } else { "false" };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_undefined() {
                let s = "undefined";
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_null() {
                let s = "null";
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            }
        }

        // Array methods - delegate to array runtime
        "push" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            if args_len > 0 && !args_ptr.is_null() {
                let val = *args_ptr;
                crate::array::js_array_push_f64(arr, val);
            }
            return crate::array::js_array_length(arr) as f64;
        }
        "pop" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            return crate::array::js_array_pop_f64(arr);
        }
        "length" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>();
            return crate::array::js_array_length(arr) as f64;
        }

        _ => {}
    }

    // If it's an object with a method stored as a closure in a field,
    // try to find and call it
    if jsval.is_pointer() {
        let obj = jsval.as_pointer::<ObjectHeader>();

        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return 0.0;
        }
        let gc_header = (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT {
            // Only accept object_type == 1 (OBJECT_TYPE_REGULAR)
            let object_type = (*obj).object_type;
        if !is_valid_obj_ptr(obj as *const u8) { return 0.0; }
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }

        // Check for CLOSURE_MAGIC at offset 12 — closures have different layout
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }

        let keys = (*obj).keys_array;

        if !keys.is_null() {
            // Validate keys_array pointer before dereferencing
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            // Issue #62 phase B: removed macOS "ASCII-like pointer" heuristic —
            // mimalloc + arena strings produce valid heap pointers with bytes
            // 32-39 in the 0x20-0x7E range, causing false positives. The call
            // into `js_object_get_field_by_name` below performs its own
            // GcHeader-based validation.

            // Search for the method in the object's fields
            let key_count = crate::array::js_array_length(keys) as usize;
            // Sanity check key_count
            if key_count > 65536 {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            let method_key = crate::string::js_string_from_bytes(
                method_name.as_ptr(),
                method_name.len() as u32,
            );

            for i in 0..key_count {
                let key_val = crate::array::js_array_get(keys, i as u32);
                if key_val.is_string() {
                    let stored_key = key_val.as_string_ptr();
                    if crate::string::js_string_equals(method_key, stored_key) != 0 {
                        // Found the method — delegate to `js_native_call_value`
                        // which handles both NaN-boxed pointers (POINTER_TAG)
                        // and raw-pointer-bits (e.g. the resolve/reject
                        // closures from `js_promise_new_with_executor`,
                        // transmuted `i64 → f64` so their bits live outside
                        // the NaN range). The earlier `is_pointer()` gate
                        // bailed on the raw-pointer case: `{ resolve }` on a
                        // plain object caused `box.resolve(x)` to land here,
                        // the tag check failed, we fell through to vtable
                        // lookup, and returned NULL_OBJECT_BYTES without
                        // invoking `js_promise_resolve` → the awaiter hung
                        // forever (issue #87). `js_native_call_value`
                        // validates CLOSURE_MAGIC before calling the func
                        // pointer, so non-callable field values (numbers,
                        // strings, booleans) safely return undefined.
                        let field_val = js_object_get_field(obj as *mut _, i as u32);
                        return crate::closure::js_native_call_value(
                            f64::from_bits(field_val.bits()),
                            args_ptr,
                            args_len,
                        );
                    }
                }
            }
        }

        // Vtable lookup: check if this class has a registered method in the vtable
        let class_id = (*obj).class_id;
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    if let Some(vtable) = reg.get(&class_id) {
                        if let Some(entry) = vtable.methods.get(method_name) {
                            let this_i64 = jsval.as_pointer::<u8>() as i64;
                            return call_vtable_method(
                                entry.func_ptr, this_i64,
                                args_ptr, args_len, entry.param_count,
                            );
                        }
                    }
                }
            }
        }
    }

    // Method not found — return a safe "null object" pointer instead of undefined.
    // The generated code often NaN-unboxes the result as a pointer and dereferences it
    // without null-checking. Returning undefined (0x7FFC000000000001) unboxes to null,
    // causing a SIGSEGV. A null object with field_count=0 is safe to dereference.
    let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
    f64::from_bits(JSValue::pointer(null_obj_ptr).bits())
}

/// Dispatch a Buffer / Uint8Array instance method call. Receiver address
/// is the raw heap pointer (already stripped of NaN-box tags). Routes
/// the Node-style numeric read/write/search/swap method family through
/// `crate::buffer` helpers; unknown methods return undefined.
pub unsafe fn dispatch_buffer_method(
    addr: usize,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let buf_f64 = f64::from_bits(JSValue::pointer(addr as *mut u8).bits());
    let buf_ptr = addr as *mut crate::buffer::BufferHeader;
    let args = if !args_ptr.is_null() && args_len > 0 {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    let arg_i32 = |i: usize| -> i32 {
        if i < args.len() { args[i] as i32 } else { 0 }
    };
    let arg_or_zero = |i: usize| -> f64 {
        if i < args.len() { args[i] } else { 0.0 }
    };
    let i32_bool = |b: i32| f64::from_bits(JSValue::bool(b != 0).bits());
    let i32_num = |n: i32| n as f64;

    match method_name {
        "length" => crate::buffer::js_buffer_length(buf_ptr) as f64,
        "toString" => {
            let enc = if !args.is_empty() {
                crate::buffer::js_encoding_tag_from_value(args[0])
            } else { 0 };
            let str_ptr = if args.len() >= 2 {
                let len = (*buf_ptr).length as i32;
                let start = arg_i32(1);
                let end = if args.len() >= 3 { arg_i32(2) } else { len };
                crate::buffer::js_buffer_to_string_range(buf_ptr, enc, start, end)
            } else {
                crate::buffer::js_buffer_to_string(buf_ptr, enc)
            };
            f64::from_bits(JSValue::string_ptr(str_ptr).bits())
        }
        "slice" | "subarray" => {
            let len = (*buf_ptr).length as i32;
            let start = arg_i32(0);
            let end = if args.len() >= 2 { arg_i32(1) } else { len };
            let result = crate::buffer::js_buffer_slice(buf_ptr, start, end);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        // `src.copy(dst, targetStart?, sourceStart?, sourceEnd?)` — mirrors
        // Node's Buffer.prototype.copy. Returns the number of bytes copied.
        "copy" if !args.is_empty() => {
            let dst_bits = args[0].to_bits();
            let dst_addr = if (dst_bits >> 48) >= 0x7FF8 {
                dst_bits & 0x0000_FFFF_FFFF_FFFF
            } else { dst_bits };
            let dst_ptr = dst_addr as *mut crate::buffer::BufferHeader;
            let target_start = if args.len() >= 2 { arg_i32(1) } else { 0 };
            let source_start = if args.len() >= 3 { arg_i32(2) } else { 0 };
            let source_end = if args.len() >= 4 { arg_i32(3) } else { (*buf_ptr).length as i32 };
            crate::buffer::js_buffer_copy(buf_ptr, dst_ptr, target_start, source_start, source_end) as f64
        }
        // `buf.write(string, offset?, length?, encoding?)` — writes the
        // utf8/hex/base64 encoding of `string` into `buf` at `offset`.
        // Returns the number of bytes written.
        "write" if !args.is_empty() => {
            let str_bits = args[0].to_bits();
            let str_addr = if (str_bits >> 48) >= 0x7FF8 {
                str_bits & 0x0000_FFFF_FFFF_FFFF
            } else { str_bits };
            let str_ptr = str_addr as *const crate::string::StringHeader;
            let offset = if args.len() >= 2 { arg_i32(1) } else { 0 };
            // Detect trailing encoding arg (string) vs length arg (number).
            // Common forms: write(str), write(str, offset), write(str, offset, enc),
            // write(str, offset, length, enc).
            let enc = if args.len() >= 4 {
                crate::buffer::js_encoding_tag_from_value(args[3])
            } else if args.len() >= 3 {
                crate::buffer::js_encoding_tag_from_value(args[2])
            } else { 0 };
            crate::buffer::js_buffer_write(buf_ptr, str_ptr, offset, enc) as f64
        }
        "fill" => {
            let result = crate::buffer::js_buffer_fill(buf_ptr, arg_i32(0));
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "equals" => {
            if args.is_empty() { return i32_bool(0); }
            let other_bits = args[0].to_bits();
            let other_addr = if (other_bits >> 48) >= 0x7FF8 {
                other_bits & 0x0000_FFFF_FFFF_FFFF
            } else { other_bits };
            let other = other_addr as *const crate::buffer::BufferHeader;
            i32_bool(crate::buffer::js_buffer_equals(buf_ptr, other))
        }
        "compare" => {
            if args.is_empty() { return 0.0; }
            let other_bits = args[0].to_bits();
            let other_addr = if (other_bits >> 48) >= 0x7FF8 {
                other_bits & 0x0000_FFFF_FFFF_FFFF
            } else { other_bits };
            let other = other_addr as *const crate::buffer::BufferHeader;
            i32_num(crate::buffer::js_buffer_compare(buf_ptr, other))
        }
        "indexOf" => i32_num(crate::buffer::js_buffer_index_of(buf_f64, arg_or_zero(0), arg_i32(1))),
        "lastIndexOf" => i32_num(crate::buffer::js_buffer_index_of(buf_f64, arg_or_zero(0), arg_i32(1))),
        "includes" => i32_bool(crate::buffer::js_buffer_includes(buf_f64, arg_or_zero(0), arg_i32(1))),
        // `buf.at(i)` — supports negative indices like Array.prototype.at.
        "at" => {
            let len = (*buf_ptr).length as i32;
            let mut idx = arg_i32(0);
            if idx < 0 { idx += len; }
            if idx < 0 || idx >= len {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            crate::buffer::js_buffer_get(buf_ptr, idx) as f64
        }
        "swap16" => { crate::buffer::js_buffer_swap16(buf_f64); buf_f64 }
        "swap32" => { crate::buffer::js_buffer_swap32(buf_f64); buf_f64 }
        "swap64" => { crate::buffer::js_buffer_swap64(buf_f64); buf_f64 }
        // Synthetic method emitted by lower.rs for `crypto.getRandomValues(buf)`.
        "$$cryptoFillRandom" => crate::buffer::js_buffer_fill_random(buf_f64),
        "readUInt8" | "readUint8" => crate::buffer::js_buffer_read_uint8(buf_f64, arg_i32(0)),
        "readInt8" => crate::buffer::js_buffer_read_int8(buf_f64, arg_i32(0)),
        "readUInt16BE" | "readUint16BE" => crate::buffer::js_buffer_read_uint16_be(buf_f64, arg_i32(0)),
        "readUInt16LE" | "readUint16LE" => crate::buffer::js_buffer_read_uint16_le(buf_f64, arg_i32(0)),
        "readInt16BE" => crate::buffer::js_buffer_read_int16_be(buf_f64, arg_i32(0)),
        "readInt16LE" => crate::buffer::js_buffer_read_int16_le(buf_f64, arg_i32(0)),
        "readUInt32BE" | "readUint32BE" => crate::buffer::js_buffer_read_uint32_be(buf_f64, arg_i32(0)),
        "readUInt32LE" | "readUint32LE" => crate::buffer::js_buffer_read_uint32_le(buf_f64, arg_i32(0)),
        "readInt32BE" => crate::buffer::js_buffer_read_int32_be(buf_f64, arg_i32(0)),
        "readInt32LE" => crate::buffer::js_buffer_read_int32_le(buf_f64, arg_i32(0)),
        "readFloatBE" => crate::buffer::js_buffer_read_float_be(buf_f64, arg_i32(0)),
        "readFloatLE" => crate::buffer::js_buffer_read_float_le(buf_f64, arg_i32(0)),
        "readDoubleBE" => crate::buffer::js_buffer_read_double_be(buf_f64, arg_i32(0)),
        "readDoubleLE" => crate::buffer::js_buffer_read_double_le(buf_f64, arg_i32(0)),
        "readBigInt64BE" => crate::buffer::js_buffer_read_bigint64_be(buf_f64, arg_i32(0)),
        "readBigInt64LE" => crate::buffer::js_buffer_read_bigint64_le(buf_f64, arg_i32(0)),
        "readBigUInt64BE" | "readBigUint64BE" => crate::buffer::js_buffer_read_biguint64_be(buf_f64, arg_i32(0)),
        "readBigUInt64LE" | "readBigUint64LE" => crate::buffer::js_buffer_read_biguint64_le(buf_f64, arg_i32(0)),
        "writeUInt8" | "writeUint8" => {
            crate::buffer::js_buffer_write_uint8(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 1) as f64
        }
        "writeInt8" => {
            crate::buffer::js_buffer_write_int8(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 1) as f64
        }
        "writeUInt16BE" | "writeUint16BE" => {
            crate::buffer::js_buffer_write_uint16_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 2) as f64
        }
        "writeUInt16LE" | "writeUint16LE" => {
            crate::buffer::js_buffer_write_uint16_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 2) as f64
        }
        "writeInt16BE" => {
            crate::buffer::js_buffer_write_int16_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 2) as f64
        }
        "writeInt16LE" => {
            crate::buffer::js_buffer_write_int16_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 2) as f64
        }
        "writeUInt32BE" | "writeUint32BE" => {
            crate::buffer::js_buffer_write_uint32_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeUInt32LE" | "writeUint32LE" => {
            crate::buffer::js_buffer_write_uint32_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeInt32BE" => {
            crate::buffer::js_buffer_write_int32_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeInt32LE" => {
            crate::buffer::js_buffer_write_int32_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeFloatBE" => {
            crate::buffer::js_buffer_write_float_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeFloatLE" => {
            crate::buffer::js_buffer_write_float_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 4) as f64
        }
        "writeDoubleBE" => {
            crate::buffer::js_buffer_write_double_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeDoubleLE" => {
            crate::buffer::js_buffer_write_double_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeBigInt64BE" => {
            crate::buffer::js_buffer_write_bigint64_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeBigInt64LE" => {
            crate::buffer::js_buffer_write_bigint64_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeBigUInt64BE" | "writeBigUint64BE" => {
            crate::buffer::js_buffer_write_biguint64_be(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        "writeBigUInt64LE" | "writeBigUint64LE" => {
            crate::buffer::js_buffer_write_biguint64_le(buf_f64, arg_or_zero(0), arg_i32(1));
            (arg_i32(1) + 8) as f64
        }
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// Dispatch a method call on a native module namespace object.
/// Extracts the module name from the object and dispatches to the appropriate
/// runtime function based on (module_name, method_name).
unsafe fn dispatch_native_module_method(
    obj: *const ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Extract the module name from field 0 of the namespace object
    let module_field = js_object_get_field(obj as *mut _, 0);
    let module_name = if module_field.is_string() {
        let str_ptr = module_field.as_string_ptr();
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
    } else {
        ""
    };

    // Helper: get arg N as f64
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() { *args_ptr.add(n) } else { f64::from_bits(JSValue::undefined().bits()) }
    };

    // Helper: extract raw string pointer from a NaN-boxed f64 value
    let arg_str_ptr = |n: usize| -> *const crate::StringHeader {
        let v = arg(n);
        let jsv = JSValue::from_bits(v.to_bits());
        if jsv.is_string() {
            jsv.as_string_ptr()
        } else {
            std::ptr::null()
        }
    };

    // Helper: convert i32 boolean to NaN-boxed TAG_TRUE / TAG_FALSE
    let bool_to_f64 = |v: i32| -> f64 {
        if v != 0 {
            f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
        } else {
            f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
        }
    };

    // Helper: convert *mut StringHeader to NaN-boxed string f64
    let str_to_f64 = |ptr: *mut crate::StringHeader| -> f64 {
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    };

    match (module_name, method_name) {
        // ── fs module (args are NaN-boxed f64, booleans return as i32→f64) ──
        ("fs", "existsSync") => bool_to_f64(crate::fs::js_fs_exists_sync(arg(0))),
        ("fs", "readFileSync") => str_to_f64(crate::fs::js_fs_read_file_sync(arg(0))),
        ("fs", "writeFileSync") => bool_to_f64(crate::fs::js_fs_write_file_sync(arg(0), arg(1))),
        ("fs", "appendFileSync") => bool_to_f64(crate::fs::js_fs_append_file_sync(arg(0), arg(1))),
        ("fs", "mkdirSync") => bool_to_f64(crate::fs::js_fs_mkdir_sync(arg(0))),
        ("fs", "unlinkSync") => bool_to_f64(crate::fs::js_fs_unlink_sync(arg(0))),
        ("fs", "readdirSync") => crate::fs::js_fs_readdir_sync(arg(0)),
        ("fs", "isDirectory") => bool_to_f64(crate::fs::js_fs_is_directory(arg(0))),

        // ── os module (no args, return string or f64) ──
        ("os", "tmpdir") => str_to_f64(crate::os::js_os_tmpdir()),
        ("os", "homedir") => str_to_f64(crate::os::js_os_homedir()),
        ("os", "platform") => str_to_f64(crate::os::js_os_platform()),
        ("os", "arch") => str_to_f64(crate::os::js_os_arch()),
        ("os", "hostname") => str_to_f64(crate::os::js_os_hostname()),
        ("os", "type") => str_to_f64(crate::os::js_os_type()),
        ("os", "release") => str_to_f64(crate::os::js_os_release()),
        ("os", "eol") => str_to_f64(crate::os::js_os_eol()),
        ("os", "totalmem") => crate::os::js_os_totalmem(),
        ("os", "freemem") => crate::os::js_os_freemem(),
        ("os", "uptime") => crate::os::js_os_uptime(),

        // ── path module (args are NaN-boxed strings → extract raw StringHeader ptr) ──
        ("path", "dirname") => str_to_f64(crate::path::js_path_dirname(arg_str_ptr(0))),
        ("path", "basename") => str_to_f64(crate::path::js_path_basename(arg_str_ptr(0))),
        ("path", "extname") => str_to_f64(crate::path::js_path_extname(arg_str_ptr(0))),
        ("path", "resolve") => str_to_f64(crate::path::js_path_resolve(arg_str_ptr(0))),
        ("path", "join") => str_to_f64(crate::path::js_path_join(arg_str_ptr(0), arg_str_ptr(1))),
        ("path", "isAbsolute") => bool_to_f64(crate::path::js_path_is_absolute(arg_str_ptr(0))),

        _ => {
            // Method not found on native module — return undefined
            f64::from_bits(JSValue::undefined().bits())
        }
    }
}

/// Special class ID for native module namespace objects
/// This is used to identify objects that represent native module namespaces
pub const NATIVE_MODULE_CLASS_ID: u32 = 0xFFFFFFFE;

/// Create a native module namespace object
/// This is used for `import * as X from 'module'` patterns
/// The returned object identifies itself as an object (typeof returns "object")
/// and stores the module name for debugging purposes
///
/// module_name_ptr: pointer to the module name string bytes
/// module_name_len: length of the module name
/// Returns the object as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_create_native_module_namespace(module_name_ptr: *const u8, module_name_len: usize) -> f64 {
    // Create an object with one field to store the module name
    let obj = js_object_alloc(NATIVE_MODULE_CLASS_ID, 1);

    unsafe {
        // Create a string from the module name
        let module_name = crate::string::js_string_from_bytes(module_name_ptr, module_name_len as u32);

        // Store the module name in the first field
        js_object_set_field(obj, 0, JSValue::string_ptr(module_name));

        // Create a keys array with one key: "__module__"
        let keys_array = crate::array::js_array_alloc(1);
        let key_bytes = b"__module__";
        let key_str = crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        crate::array::js_array_push(keys_array, JSValue::string_ptr(key_str));
        js_object_set_keys(obj, keys_array);
    }

    // Return as NaN-boxed pointer
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Access a property on a native module namespace object.
/// For method references (e.g., `fs.existsSync`), creates a bound method closure.
/// For constant properties (e.g., `path.sep`, `fs.constants`), returns the value directly.
#[no_mangle]
pub extern "C" fn js_native_module_bind_method(
    namespace_obj: f64,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    let property_name = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(property_name_ptr, property_name_len))
    };

    // Extract module name from the namespace object's first field
    let module_name = unsafe { get_module_name_from_namespace(namespace_obj) };

    // Check for known constant properties first
    if let Some(val) = unsafe { get_native_module_constant(module_name, property_name, namespace_obj) } {
        return val;
    }

    // Try V8 JS runtime fallback for unknown properties (e.g., ethers.Contract)
    let js_val = crate::value::native_module_try_js_property(module_name, property_name);
    if js_val.to_bits() != crate::value::TAG_UNDEFINED {
        return js_val;
    }

    // Not a constant — create a bound method closure
    let heap_name = unsafe {
        let layout = std::alloc::Layout::from_size_align(property_name_len, 1).unwrap();
        let ptr = std::alloc::alloc(layout);
        std::ptr::copy_nonoverlapping(property_name_ptr, ptr, property_name_len);
        ptr
    };

    let closure = crate::closure::js_closure_alloc(
        crate::closure::BOUND_METHOD_FUNC_PTR,
        3,
    );
    crate::closure::js_closure_set_capture_f64(closure, 0, namespace_obj);
    crate::closure::js_closure_set_capture_ptr(closure, 1, heap_name as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, property_name_len as i64);

    crate::value::js_nanbox_pointer(closure as i64)
}

/// Extract the module name string from a native module namespace object.
unsafe fn get_module_name_from_namespace(namespace_obj: f64) -> &'static str {
    let jsval = JSValue::from_bits(namespace_obj.to_bits());
    if !jsval.is_pointer() {
        return "";
    }
    let obj = jsval.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x100000 {
        return "";
    }
    let module_field = js_object_get_field(obj as *mut _, 0);
    if module_field.is_string() {
        let str_ptr = module_field.as_string_ptr();
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
    } else {
        ""
    }
}

/// Return constant (non-method) property values for native modules.
/// Returns None for method names, which should create bound closures instead.
unsafe fn get_native_module_constant(
    module_name: &str,
    property: &str,
    namespace_obj: f64,
) -> Option<f64> {
    let str_val = |s: &str| -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    };

    let o_nofollow: f64 = {
        #[cfg(target_os = "macos")]
        { 0x0100 as f64 }
        #[cfg(target_os = "linux")]
        { 0x20000 as f64 }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        { 0x0100 as f64 }
    };

    // Helper for fs constants — shared between "fs" and "fs.constants" modules.
    // Using a nested match (module first, then property) instead of OR patterns
    // on tuples, because rustc's match optimizer can miscompile tuple OR patterns
    // by absorbing one alternative's entries into the other branch's decision tree.
    let fs_const = |prop: &str| -> Option<f64> {
        match prop {
            "F_OK" => Some(0.0),
            "R_OK" => Some(4.0),
            "W_OK" => Some(2.0),
            "X_OK" => Some(1.0),
            "O_RDONLY" => Some(0.0),
            "O_WRONLY" => Some(1.0),
            "O_RDWR" => Some(2.0),
            "O_NOFOLLOW" => Some(o_nofollow),
            "O_CREAT" => Some(0x200 as f64),
            "O_TRUNC" => Some(0x400 as f64),
            "O_APPEND" => Some(0x8 as f64),
            "O_EXCL" => Some(0x800 as f64),
            "COPYFILE_EXCL" => Some(1.0),
            "COPYFILE_FICLONE" => Some(2.0),
            "COPYFILE_FICLONE_FORCE" => Some(4.0),
            "S_IRUSR" => Some(0o400 as f64),
            "S_IWUSR" => Some(0o200 as f64),
            "S_IXUSR" => Some(0o100 as f64),
            "S_IRGRP" => Some(0o040 as f64),
            "S_IWGRP" => Some(0o020 as f64),
            "S_IXGRP" => Some(0o010 as f64),
            "S_IROTH" => Some(0o004 as f64),
            "S_IWOTH" => Some(0o002 as f64),
            "S_IXOTH" => Some(0o001 as f64),
            _ => None,
        }
    };

    match module_name {
        "path" => match property {
            "sep" => if cfg!(windows) { Some(str_val("\\")) } else { Some(str_val("/")) },
            "delimiter" => if cfg!(windows) { Some(str_val(";")) } else { Some(str_val(":")) },
            "posix" => Some(create_sub_namespace("path.posix")),
            "win32" => Some(create_sub_namespace("path.win32")),
            _ => None,
        },
        "path.posix" => match property {
            "sep" => Some(str_val("/")),
            "delimiter" => Some(str_val(":")),
            _ => None,
        },
        "path.win32" => match property {
            "sep" => Some(str_val("\\")),
            "delimiter" => Some(str_val(";")),
            _ => None,
        },
        "fs" => match property {
            "constants" => Some(create_sub_namespace("fs.constants")),
            _ => fs_const(property),
        },
        "fs.constants" => fs_const(property),
        "os" => match property {
            "EOL" => if cfg!(windows) { Some(str_val("\r\n")) } else { Some(str_val("\n")) },
            "constants" => Some(create_sub_namespace("os.constants")),
            _ => None,
        },
        _ => None,
    }
}

/// Create a NativeModuleRef sub-namespace (e.g. "fs.constants", "path.posix").
/// The compiled code treats the result as another NativeModuleRef, so chained
/// property accesses like `fs.constants.O_RDONLY` work through the dispatch table.
fn create_sub_namespace(name: &str) -> f64 {
    js_create_native_module_namespace(name.as_ptr(), name.len())
}

/// Create (and cache) the fs.constants object with POSIX file system constants.
unsafe fn create_fs_constants_object() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CACHED: AtomicU64 = AtomicU64::new(0);
    let cached = CACHED.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }

    // POSIX file-access constants
    let field_names: &[&str] = &[
        "F_OK", "R_OK", "W_OK", "X_OK",
        "O_RDONLY", "O_WRONLY", "O_RDWR",
        "O_NOFOLLOW", "COPYFILE_EXCL",
    ];
    let o_nofollow: f64 = {
        #[cfg(target_os = "macos")]
        { 0x0100 as f64 }
        #[cfg(target_os = "linux")]
        { 0x20000 as f64 }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        { 0x0100 as f64 }
    };
    let field_values: &[f64] = &[
        0.0, 4.0, 2.0, 1.0,   // F_OK, R_OK, W_OK, X_OK
        0.0, 1.0, 2.0,        // O_RDONLY, O_WRONLY, O_RDWR
        o_nofollow,            // O_NOFOLLOW
        1.0,                   // COPYFILE_EXCL
    ];

    // Build null-separated packed keys: "F_OK\0R_OK\0..."
    let packed = field_names.join("\0");
    let obj = js_object_alloc_with_shape(
        0x7FFF_FF01, // unique shape_id for fs.constants
        field_names.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );

    for (i, &val) in field_values.iter().enumerate() {
        js_object_set_field(obj, i as u32, JSValue::number(val));
    }

    let result = crate::value::js_nanbox_pointer(obj as i64);
    CACHED.store(result.to_bits(), Ordering::Relaxed);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_alloc_and_fields() {
        let obj = js_object_alloc(1, 3);

        // Check header
        assert_eq!(js_object_get_class_id(obj), 1);

        // Fields should be undefined initially
        let f0 = js_object_get_field(obj, 0);
        assert!(f0.is_undefined());

        // Set and get a field
        js_object_set_field(obj, 0, JSValue::number(42.0));
        let f0 = js_object_get_field(obj, 0);
        assert!(f0.is_number());
        assert_eq!(f0.as_number(), 42.0);

        // Set another field
        js_object_set_field(obj, 2, JSValue::bool(true));
        let f2 = js_object_get_field(obj, 2);
        assert!(f2.is_bool());
        assert!(f2.as_bool());

        // Clean up
        js_object_free(obj);
    }

    #[test]
    fn test_object_to_value_roundtrip() {
        let obj = js_object_alloc(5, 2);
        js_object_set_field(obj, 0, JSValue::number(123.0));

        let value = js_object_to_value(obj);
        assert!(value.is_pointer());

        let obj2 = js_value_to_object(value);
        assert_eq!(js_object_get_class_id(obj2), 5);

        let f0 = js_object_get_field(obj2, 0);
        assert_eq!(f0.as_number(), 123.0);

        js_object_free(obj);
    }
}

/// Dispatch BigInt binary methods (add, sub, mul, div, mod, etc.)
/// Called from js_native_call_method when object is BIGINT_TAG.
unsafe fn dispatch_bigint_binary_method(
    a: *const crate::bigint::BigIntHeader,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Extract second operand from args (if any)
    let b = if args_len > 0 && !args_ptr.is_null() {
        let arg_f64 = *args_ptr;
        let arg_jsval = JSValue::from_bits(arg_f64.to_bits());
        if arg_jsval.is_bigint() {
            crate::bigint::clean_bigint_ptr(
                (arg_f64.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader
            )
        } else {
            // Try to convert number to BigInt
            crate::bigint::js_bigint_from_f64(arg_f64)
        }
    } else {
        std::ptr::null()
    };

    match method {
        // Binary arithmetic → returns BigInt
        "add" => {
            let result = crate::bigint::js_bigint_add(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "sub" => {
            let result = crate::bigint::js_bigint_sub(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "mul" => {
            let result = crate::bigint::js_bigint_mul(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "div" => {
            let result = crate::bigint::js_bigint_div(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "mod" | "umod" => {
            let result = crate::bigint::js_bigint_mod(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "pow" => {
            let result = crate::bigint::js_bigint_pow(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "and" => {
            let result = crate::bigint::js_bigint_and(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "or" => {
            let result = crate::bigint::js_bigint_or(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "xor" => {
            let result = crate::bigint::js_bigint_xor(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "shln" => {
            let result = crate::bigint::js_bigint_shl(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "shrn" => {
            let result = crate::bigint::js_bigint_shr(a, b);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "maskn" => {
            // maskn(bits) — mask to lowest N bits
            let result = crate::bigint::js_bigint_and(a, b); // approximate
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        // Comparison → returns boolean/number
        "eq" => {
            let result = crate::bigint::js_bigint_eq(a, b);
            return f64::from_bits(JSValue::bool(result != 0).bits());
        }
        "lt" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return f64::from_bits(JSValue::bool(result < 0).bits());
        }
        "lte" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return f64::from_bits(JSValue::bool(result <= 0).bits());
        }
        "gt" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return f64::from_bits(JSValue::bool(result > 0).bits());
        }
        "gte" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return f64::from_bits(JSValue::bool(result >= 0).bits());
        }
        "cmp" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            return result as f64;
        }
        "fromTwos" => {
            // bn.js: interpret `a` as the unsigned encoding of a signed
            // `width`-bit integer in two's complement. If bit (width-1) of
            // `a` is set the result is `a - 2^width`; otherwise return `a`.
            // `width` arrives in `b` (already a BigInt — see top of fn).
            let width = if b.is_null() { 0u64 } else { (*b).limbs[0] };
            let max_bits = (crate::bigint::BIGINT_LIMBS * 64) as u64;
            if width == 0 || width > max_bits {
                return f64::from_bits(JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits());
            }
            let bit = (width - 1) as usize;
            let high_bit_set = ((*a).limbs[bit / 64] >> (bit % 64)) & 1 == 1;
            if !high_bit_set {
                return f64::from_bits(JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits());
            }
            let one = crate::bigint::js_bigint_from_u64(1);
            let two_pow = crate::bigint::js_bigint_shl(one, b);
            let result = crate::bigint::js_bigint_sub(a, two_pow);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        "toTwos" => {
            // bn.js: convert to `width`-bit two's complement encoding. If `a`
            // is negative the result is `a + 2^width` (mod 2^width);
            // otherwise return `a` unchanged. bn.js does not mask
            // non-negative inputs to `width` bits, so neither do we.
            let width = if b.is_null() { 0u64 } else { (*b).limbs[0] };
            let max_bits = (crate::bigint::BIGINT_LIMBS * 64) as u64;
            if width == 0 || width > max_bits {
                return f64::from_bits(JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits());
            }
            if crate::bigint::js_bigint_is_negative(a) == 0 {
                return f64::from_bits(JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits());
            }
            let one = crate::bigint::js_bigint_from_u64(1);
            let two_pow = crate::bigint::js_bigint_shl(one, b);
            let result = crate::bigint::js_bigint_add(a, two_pow);
            return f64::from_bits(JSValue::bigint_ptr(result).bits());
        }
        _ => {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
    }
}

/// Object.fromEntries(entries) — build an object from an array of [key, value] pairs or a Map.
/// `entries` is an array of arrays, or a Map. Returns a NaN-boxed pointer to a new object.
#[no_mangle]
pub extern "C" fn js_object_from_entries(entries_value: f64) -> f64 {
    // Extract pointer from NaN-boxed value
    let bits = entries_value.to_bits();
    let raw_ptr = if (bits & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
    } else if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF {
        bits as *const u8
    } else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if raw_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    unsafe {
        // Check GcHeader to see if this is a Map
        let gc_header = (raw_ptr).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_MAP {
            // It's a Map — convert via js_map_entries first
            let map_ptr = raw_ptr as *const crate::map::MapHeader;
            let entries_arr = crate::map::js_map_entries(map_ptr);
            // Recursively call ourselves with the entries array (NaN-boxed pointer)
            let arr_boxed = crate::value::js_nanbox_pointer(entries_arr as i64);
            return js_object_from_entries(arr_boxed);
        }

        // It's an array of [key, value] pairs
        let arr_ptr = raw_ptr as *const ArrayHeader;
        let length = (*arr_ptr).length as usize;
        // Allocate empty object — class_id 0 = generic object
        let obj = js_object_alloc(0, length as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Iterate entries: each entry is itself an array [key, value]
        let entries_data = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..length {
            let entry_val = *entries_data.add(i);
            // Get the inner entry array
            let entry_bits = entry_val.to_bits();
            let entry_arr = if (entry_bits & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
                (entry_bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
            } else if entry_bits != 0 && entry_bits <= 0x0000_FFFF_FFFF_FFFF {
                entry_bits as *const ArrayHeader
            } else {
                continue;
            };
            if entry_arr.is_null() || (*entry_arr).length < 2 {
                continue;
            }
            let entry_data = (entry_arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let key_val = *entry_data;
            let val_val = *entry_data.add(1);
            // Convert key to string
            let key_str = crate::builtins::js_string_coerce(key_val);
            if key_str.is_null() { continue; }
            js_object_set_field_by_name(obj, key_str, val_val);
        }
        // Return as NaN-boxed pointer
        let bits = (obj as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }
}

/// `Object.groupBy(items, callback)` — Node 22+ static method.
/// Walks `items` (an array), calls `callback(item, index)` to compute a
/// string key per item, and returns a new object whose keys are the
/// distinct callback results and whose values are arrays of the items
/// that produced each key.
///
/// `items_value` is the NaN-boxed array pointer; `callback` is the
/// closure to invoke per element. Returns the result object as a
/// NaN-boxed POINTER_TAG f64 so codegen can pass it through the normal
/// f64 plumbing.
#[no_mangle]
pub extern "C" fn js_object_group_by(
    items_value: f64,
    callback: *const crate::closure::ClosureHeader,
) -> f64 {
    // Strip NaN-box and validate the array pointer.
    let bits = items_value.to_bits();
    let raw = if (bits >> 48) == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
    } else if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF {
        bits as *const ArrayHeader
    } else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    if raw.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    unsafe {
        let length = (*raw).length as usize;
        let elements = (raw as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Build a side table: key (UTF-8 String) -> Vec<f64> of group elements.
        // We materialize the result object only at the end so we don't have to
        // worry about per-push reallocation invalidating an array stored
        // inside the object's field slot.
        use std::collections::BTreeMap;
        let mut groups: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        // Preserve insertion order for the keys array (Node iterates the
        // result object in insertion order, not sorted order).
        let mut order: Vec<String> = Vec::new();

        for i in 0..length {
            let item = *elements.add(i);
            let key_val = crate::closure::js_closure_call2(callback, item, i as f64);
            // Coerce the key to a UTF-8 String.
            let key_ptr = crate::builtins::js_string_coerce(key_val);
            let key_string = if key_ptr.is_null() {
                "undefined".to_string()
            } else {
                let len = (*key_ptr).byte_len as usize;
                let data = (key_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(bytes).unwrap_or("").to_string()
            };

            if !groups.contains_key(&key_string) {
                order.push(key_string.clone());
            }
            groups.entry(key_string).or_insert_with(Vec::new).push(item);
        }

        // Materialize the result object. Allocate with the right field count
        // up front so the keys_array is sized correctly.
        let obj = js_object_alloc(0, order.len() as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        for key in &order {
            // Build the JS string for the key.
            let key_str_ptr = crate::string::js_string_from_bytes(
                key.as_ptr(),
                key.len() as u32,
            );
            // Build the per-group Array<f64> from the materialized Vec.
            let items_for_key = groups.get(key).unwrap();
            let arr = crate::array::js_array_alloc(items_for_key.len() as u32);
            (*arr).length = items_for_key.len() as u32;
            let arr_data = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for (i, v) in items_for_key.iter().enumerate() {
                std::ptr::write(arr_data.add(i), *v);
            }
            // NaN-box the array pointer with POINTER_TAG before storing.
            let arr_boxed = f64::from_bits((arr as u64) | 0x7FFD_0000_0000_0000);
            js_object_set_field_by_name(obj, key_str_ptr, arr_boxed);
        }
        // Return the result object NaN-boxed.
        f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Object.is(a, b) — SameValue algorithm
/// Like ===, except: NaN === NaN (true) and +0 !== -0 (false).
/// Returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is(a: f64, b: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    // Handle NaN: SameValue treats NaN as equal to NaN
    let a_jsval = crate::JSValue::from_bits(a_bits);
    let b_jsval = crate::JSValue::from_bits(b_bits);

    if a_jsval.is_number() && b_jsval.is_number() {
        let an = a_jsval.as_number();
        let bn = b_jsval.as_number();
        if an.is_nan() && bn.is_nan() {
            return f64::from_bits(TAG_TRUE);
        }
        // Distinguish +0 / -0 by bit pattern
        if an == 0.0 && bn == 0.0 {
            if a_bits == b_bits { return f64::from_bits(TAG_TRUE); }
            return f64::from_bits(TAG_FALSE);
        }
        if an == bn { return f64::from_bits(TAG_TRUE); }
        return f64::from_bits(TAG_FALSE);
    }

    // For strings, do content comparison
    if a_jsval.is_string() && b_jsval.is_string() {
        let result = crate::string::js_string_equals(
            a_jsval.as_string_ptr() as *const crate::StringHeader,
            b_jsval.as_string_ptr() as *const crate::StringHeader,
        );
        if result != 0 { return f64::from_bits(TAG_TRUE); }
        return f64::from_bits(TAG_FALSE);
    }

    // For everything else, bit-pattern equality
    if a_bits == b_bits { f64::from_bits(TAG_TRUE) } else { f64::from_bits(TAG_FALSE) }
}

/// Object.hasOwn(obj, key) — check if obj has its own property `key`.
/// Returns NaN-boxed boolean. Checks via `keys_array` membership (not via
/// "value != undefined") so properties that legitimately hold `undefined` and
/// accessor descriptors with no backing slot still report true.
#[no_mangle]
pub extern "C" fn js_object_has_own(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x1000000 {
            return f64::from_bits(TAG_FALSE);
        }
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }
        if own_key_present(obj, key_str) {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Helper: extract object pointer from NaN-boxed f64. Returns null on failure.
unsafe fn extract_obj_ptr(value: f64) -> *mut ObjectHeader {
    let jsval = crate::JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() {
        jsval.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        let bits = value.to_bits();
        if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
            bits as *mut ObjectHeader
        } else {
            ptr::null_mut()
        }
    }
}

/// Helper: get GcHeader for an object pointer
unsafe fn gc_header_for(obj: *const ObjectHeader) -> *mut crate::gc::GcHeader {
    (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
}

/// Object.defineProperty(obj, key, descriptor) — set the value AND record the
/// `writable` / `enumerable` / `configurable` attribute flags in the side table.
/// Returns the object (NaN-boxed pointer).
///
/// IMPORTANT: writes the value via `js_object_set_field_by_name` BEFORE recording
/// the descriptor — otherwise a `writable: false` descriptor would block its own
/// initial value from being stored.
#[no_mangle]
pub extern "C" fn js_object_define_property(obj_value: f64, key_value: f64, descriptor_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        // Extract the key as a Rust string for the descriptor side-table lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };
        // Extract descriptor object
        let desc_ptr = extract_obj_ptr(descriptor_value);
        if desc_ptr.is_null() {
            return obj_value;
        }

        // Detect accessor descriptor (has `get` and/or `set`) vs. data descriptor (has `value`).
        // JS disallows mixing them, but we only check for `get`/`set` presence.
        let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
        let get_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
        let set_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
        let has_accessor = !get_field.is_undefined() || !set_field.is_undefined();

        if has_accessor {
            // Store the accessor closures in the side table. Ensure the key is present
            // in the object's keys_array so lookups (hasOwn, getOwnPropertyDescriptor,
            // keys) can see it.
            ensure_key_in_keys_array(obj, key_str);
            if let Some(k) = key_rust.clone() {
                let get_bits = if get_field.is_undefined() { 0u64 } else { get_field.bits() };
                let set_bits = if set_field.is_undefined() { 0u64 } else { set_field.bits() };
                set_accessor_descriptor(obj as usize, k, AccessorDescriptor { get: get_bits, set: set_bits });
            }
        } else {
            // Data descriptor: look for "value" field and store it.
            let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
            let value_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
            // Clear any existing accessor for this key so the write doesn't fire the setter.
            if let Some(ref k) = key_rust {
                ACCESSOR_DESCRIPTORS.with(|m| { m.borrow_mut().remove(&(obj as usize, k.clone())); });
            }
            // Ensure the key exists even if the descriptor's value is undefined —
            // the property still "exists" per JS semantics.
            if value_field.is_undefined() {
                ensure_key_in_keys_array(obj, key_str);
            } else {
                // Store via runtime path. Any existing descriptor attrs are NOT yet set,
                // so writability defaults to true and the write goes through.
                js_object_set_field_by_name(obj, key_str, f64::from_bits(value_field.bits()));
            }
        }

        // Read attribute flags from descriptor. JS defaults when omitted in
        // `Object.defineProperty` are `false` (NOT `true` like for direct assignment).
        let read_bool = |name: &[u8]| -> Option<bool> {
            let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
            if v.is_undefined() {
                None
            } else {
                Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
            }
        };
        // Accessor descriptors don't have `writable`; we leave it true so data
        // lookups that happen before the accessor override don't accidentally
        // reject a legitimate fallthrough write. Attrs default to false when
        // omitted (JS spec).
        let writable = read_bool(b"writable").unwrap_or(has_accessor);
        let enumerable = read_bool(b"enumerable").unwrap_or(false);
        let configurable = read_bool(b"configurable").unwrap_or(false);

        if let Some(k) = key_rust {
            set_property_attrs(obj as usize, k, PropertyAttrs::new(writable, enumerable, configurable));
        }
        // Return the object
        obj_value
    }
}

/// Ensure a key appears in the object's keys_array. Used by `Object.defineProperty`
/// so the property is enumerable-filterable and discoverable by `getOwnPropertyNames`
/// even when the value is undefined or the property is an accessor (no underlying slot).
unsafe fn ensure_key_in_keys_array(obj: *mut ObjectHeader, key: *const crate::StringHeader) {
    if obj.is_null() || (obj as usize) < 0x1000000 || key.is_null() {
        return;
    }
    // If no keys array exists, create one with this key.
    let keys = (*obj).keys_array;
    if keys.is_null() {
        let new_keys = crate::array::js_array_alloc(4);
        let new_keys = crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
        (*obj).keys_array = new_keys;
        if (*obj).field_count == 0 {
            (*obj).field_count = 1;
        }
        return;
    }
    // Validate keys array pointer
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return;
    }
    // Check if key already exists
    let key_count = crate::array::js_array_length(keys) as usize;
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        if stored.is_string() {
            let stored_key = stored.as_string_ptr();
            if crate::string::js_string_equals(key, stored_key) != 0 {
                return; // already present
            }
        }
    }
    // Clone shared keys array if needed, then append.
    let owned_keys = if key_count == (*obj).field_count as usize {
        let cloned = crate::array::js_array_alloc(key_count as u32 + 4);
        let src_data = (keys as *const u8).add(8) as *const f64;
        let dst_data = (cloned as *mut u8).add(8) as *mut f64;
        for i in 0..key_count {
            *dst_data.add(i) = *src_data.add(i);
        }
        (*cloned).length = key_count as u32;
        (*obj).keys_array = cloned;
        cloned
    } else {
        keys
    };
    let new_keys = crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
    (*obj).keys_array = new_keys;
    let new_index = key_count as u32;
    if new_index >= (*obj).field_count {
        (*obj).field_count = new_index + 1;
    }
}

/// Object.getOwnPropertyDescriptor(obj, key) — returns a data descriptor
/// `{ value, writable, enumerable, configurable }` for data properties, or an
/// accessor descriptor `{ get, set, enumerable, configurable }` for properties
/// installed via `Object.defineProperty(obj, key, { get, set })`. Returns
/// TAG_UNDEFINED if the property doesn't exist.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_descriptor(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        // Extract key as a Rust string for descriptor lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };

        // Check whether the key is actually present on the object. A property can
        // legitimately hold `undefined`, and accessor descriptors have no value slot,
        // so we check the keys_array directly instead of relying on "value != undefined".
        let present = own_key_present(obj, key_str);
        if !present {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Look up descriptor flags (default: all true).
        let attrs = key_rust
            .as_ref()
            .and_then(|k| get_property_attrs(obj as usize, k))
            .unwrap_or(PropertyAttrs::new(true, true, true));
        let bool_to_f64 = |b: bool| f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });

        // Accessor descriptor path.
        if let Some(acc) = key_rust
            .as_ref()
            .and_then(|k| get_accessor_descriptor(obj as usize, k))
        {
            let packed = b"get\0set\0enumerable\0configurable";
            let desc = js_object_alloc_with_shape(
                0x0D_E5_C1,
                4,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let header_size = std::mem::size_of::<ObjectHeader>();
            let fields = (desc as *mut u8).add(header_size) as *mut f64;
            *fields = if acc.get != 0 {
                f64::from_bits(acc.get)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            *fields.add(1) = if acc.set != 0 {
                f64::from_bits(acc.set)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            *fields.add(2) = bool_to_f64(attrs.enumerable());
            *fields.add(3) = bool_to_f64(attrs.configurable());
            return f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000);
        }

        // Data descriptor path.
        let value = js_object_get_field_by_name(obj, key_str);
        let packed = b"value\0writable\0enumerable\0configurable";
        let desc = js_object_alloc_with_shape(
            0x0D_E5_C0, // unique shape_id for property descriptors
            4,
            packed.as_ptr(),
            packed.len() as u32,
        );
        let header_size = std::mem::size_of::<ObjectHeader>();
        let fields = (desc as *mut u8).add(header_size) as *mut f64;
        *fields = f64::from_bits(value.bits());                  // value
        *fields.add(1) = bool_to_f64(attrs.writable());          // writable
        *fields.add(2) = bool_to_f64(attrs.enumerable());        // enumerable
        *fields.add(3) = bool_to_f64(attrs.configurable());      // configurable
        f64::from_bits((desc as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Helper: does `key` appear in `obj.keys_array`?
unsafe fn own_key_present(obj: *mut ObjectHeader, key: *const crate::StringHeader) -> bool {
    if obj.is_null() || (obj as usize) < 0x1000000 || key.is_null() {
        return false;
    }
    let keys = (*obj).keys_array;
    if keys.is_null() {
        return false;
    }
    let keys_ptr = keys as usize;
    if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return false;
    }
    // Validate keys_array GC header
    let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
        return false;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65536 {
        return false;
    }
    for i in 0..key_count {
        let stored = crate::array::js_array_get(keys, i as u32);
        if stored.is_string() {
            let stored_key = stored.as_string_ptr();
            if !stored_key.is_null() && crate::string::js_string_equals(key, stored_key) != 0 {
                return true;
            }
        }
    }
    false
}

/// Object.getOwnPropertyNames(obj) — returns all own property names (including non-enumerable).
/// Takes a NaN-boxed f64 object pointer, returns a NaN-boxed f64 array pointer.
#[no_mangle]
pub extern "C" fn js_object_get_own_property_names(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            let empty = crate::array::js_array_alloc(0);
            return f64::from_bits((empty as u64) | 0x7FFD_0000_0000_0000);
        }
        // Clone the keys array — Object.getOwnPropertyNames includes ALL keys (even non-enumerable).
        let len = crate::array::js_array_length(keys) as usize;
        let result = crate::array::js_array_alloc(len as u32);
        for i in 0..len {
            let key_val = crate::array::js_array_get(keys, i as u32);
            crate::array::js_array_push_f64(result, f64::from_bits(key_val.bits()));
        }
        f64::from_bits((result as u64) | 0x7FFD_0000_0000_0000)
    }
}

/// Object.create(proto) — create empty object. Perry ignores prototype; Object.create(null) returns {}.
#[no_mangle]
pub extern "C" fn js_object_create(_proto_value: f64) -> f64 {
    let obj = js_object_alloc(0, 0);
    // Return NaN-boxed pointer
    f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000)
}

/// Object.freeze(obj) — sets the frozen flag and drops `writable` +
/// `configurable` on every existing key so per-key descriptor lookups report
/// the post-freeze state. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_freeze(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
            // Drop writable + configurable for every existing key.
            mark_all_keys(obj, /*drop_writable=*/true, false, /*drop_configurable=*/true);
        }
    }
    obj_value
}

/// Object.seal(obj) — sets the sealed flag and drops `configurable` on every
/// existing key. Writable is preserved (sealed ≠ frozen). Returns the object.
#[no_mangle]
pub extern "C" fn js_object_seal(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
            // Drop configurable for every existing key (but leave writable intact).
            mark_all_keys(obj, /*drop_writable=*/false, false, /*drop_configurable=*/true);
        }
    }
    obj_value
}

/// Object.preventExtensions(obj) — sets the no-extend flag. Returns the object.
#[no_mangle]
pub extern "C" fn js_object_prevent_extensions(obj_value: f64) -> f64 {
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if !obj.is_null() && (obj as usize) > 0x10000 {
            let gc = gc_header_for(obj);
            (*gc)._reserved |= crate::gc::OBJ_FLAG_NO_EXTEND;
        }
    }
    obj_value
}

/// Object.isFrozen(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_frozen(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously frozen
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Object.isSealed(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_sealed(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_TRUE); // non-objects are vacuously sealed
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_SEALED != 0 {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        }
    }
}

/// Object.isExtensible(obj) — returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is_extensible(obj_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return f64::from_bits(TAG_FALSE); // non-objects are not extensible
        }
        let gc = gc_header_for(obj);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            f64::from_bits(TAG_FALSE)
        } else {
            f64::from_bits(TAG_TRUE)
        }
    }
}

/// Object.getPrototypeOf(obj) — Perry doesn't have a prototype chain, returns null.
#[no_mangle]
pub extern "C" fn js_object_get_prototype_of(_obj_value: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    f64::from_bits(TAG_NULL)
}
