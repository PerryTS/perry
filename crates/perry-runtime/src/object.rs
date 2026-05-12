//! Object representation for Perry
//!
//! Objects are heap-allocated with a header containing:
//! - Class ID (for type checking and vtable lookup)
//! - Field count
//! - Keys array pointer (for Object.keys() support)
//! - Fields array (inline)

use crate::arena::arena_alloc_gc;
use crate::ArrayHeader;
use crate::JSValue;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;

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
    /// Heap-pointer keyed; PtrHasher avoids the per-call SipHash on
    /// every overflow read/write. `clear_overflow_for_ptr` was 0.7%
    /// leaf samples on perf-comprehensive (called from object dispatch
    /// + arena_walk_objects in the GC path).
    static OVERFLOW_FIELDS: RefCell<crate::fast_hash::PtrHashMap<usize, Vec<u64>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Sidecar hash index for object key lookup. The on-object
    /// `keys_array` only supports O(N) linear scan; for objects that
    /// grow beyond `KEYS_INDEX_THRESHOLD` keys, the linear scan
    /// becomes O(N²) total work for the build-then-fill pattern (e.g.
    /// `for (i=0..N) obj["k_"+i] = i`). Without this index, building
    /// a 10k-key dictionary takes ~9 s (Bun: 4 ms — 2200× slower).
    ///
    /// Keyed on the keys_array heap pointer. Each entry maps
    /// FNV-1a content hash of the key bytes → slot index in the
    /// keys_array. Built lazily on first lookup at threshold; rebuilt
    /// on miss after a reallocation (`js_array_push` returns a new
    /// pointer when the backing storage grew). Incremental updates
    /// happen when the array stays in place.
    ///
    /// Stale entries (keys_array address recycled by GC into an
    /// unrelated array) are tolerated: lookup just misses, content
    /// validation against the actual stored key on the linear-scan
    /// fallback ensures correctness.
    static KEYS_INDEX: RefCell<crate::fast_hash::PtrHashMap<usize, (u32, std::collections::HashMap<u64, Vec<u32>>)>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// When keys_array length exceeds this, build the sidecar hash index
/// on the next lookup. Below this threshold, the linear scan is
/// faster than the hash overhead (memory access, cache footprint).
const KEYS_INDEX_THRESHOLD: u32 = 32;

/// FNV-1a hash of the bytes behind a string header. Same hash function
/// as `key_content_hash_impl` so callers can mix paths.
#[inline(always)]
fn key_bytes_hash(name_ptr: *const u8, name_len: usize) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    unsafe {
        for i in 0..name_len {
            h ^= *name_ptr.add(i) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Look up the slot index for `key` in `obj`'s keys array via the
/// sidecar hash index. Returns `Some(slot)` on hit, `None` on miss
/// (the caller must then fall through to append/grow).
///
/// Keyed on the OBJECT pointer (not the keys_array pointer) because
/// shape-sharing means the keys_array gets cloned on every insert,
/// which would invalidate a keys-keyed sidecar after each call. The
/// object pointer is stable within its lifetime (until GC moves it —
/// at which point any sidecar entry just becomes a harmless stale
/// reference; the next lookup misses and rebuilds).
#[inline]
unsafe fn keys_index_lookup(
    obj: *const ObjectHeader,
    keys: *const crate::array::ArrayHeader,
    key_bytes: &[u8],
    key_hash: u64,
) -> Option<u32> {
    let key_count = crate::array::js_array_length(keys);
    if key_count < KEYS_INDEX_THRESHOLD {
        return None;
    }
    let obj_addr = obj as usize;
    // Look up the cached index. If absent OR stale (length doesn't
    // match — caller appended without going through `keys_index_insert`),
    // rebuild.
    let needs_rebuild = KEYS_INDEX.with(|m| {
        let m = m.borrow();
        match m.get(&obj_addr) {
            Some((cached_len, _)) => *cached_len != key_count,
            None => true,
        }
    });
    if needs_rebuild {
        let mut map: std::collections::HashMap<u64, Vec<u32>> =
            std::collections::HashMap::with_capacity(key_count as usize);
        for i in 0..key_count {
            let v = crate::array::js_array_get(keys, i);
            if !v.is_string() {
                continue;
            }
            let sp = v.as_string_ptr();
            if sp.is_null() {
                continue;
            }
            let sname_ptr = (sp as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let sname_len = (*sp).byte_len as usize;
            let h = key_bytes_hash(sname_ptr, sname_len);
            map.entry(h).or_default().push(i);
        }
        KEYS_INDEX.with(|m| {
            m.borrow_mut().insert(obj_addr, (key_count, map));
        });
    }
    KEYS_INDEX.with(|m| {
        let m = m.borrow();
        let (_, map) = m.get(&obj_addr)?;
        let candidates = map.get(&key_hash)?;
        for &i in candidates {
            let v = crate::array::js_array_get(keys, i);
            if !v.is_string() {
                continue;
            }
            let sp = v.as_string_ptr();
            if sp.is_null() {
                continue;
            }
            let sname_ptr = (sp as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let sname_len = (*sp).byte_len as usize;
            if sname_len != key_bytes.len() {
                continue;
            }
            let stored_bytes = std::slice::from_raw_parts(sname_ptr, sname_len);
            if stored_bytes == key_bytes {
                return Some(i);
            }
        }
        None
    })
}

/// Record a new (key_hash → slot) entry in the sidecar after a key
/// has been appended to `obj`. Caller ensures `new_count` equals the
/// new keys_array length right after the append.
#[inline]
fn keys_index_insert(obj_addr: usize, new_count: u32, key_hash: u64, slot: u32) {
    if new_count < KEYS_INDEX_THRESHOLD {
        return;
    }
    KEYS_INDEX.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(entry) = m.get_mut(&obj_addr) {
            if entry.0 + 1 == new_count {
                entry.0 = new_count;
                entry.1.entry(key_hash).or_default().push(slot);
            }
        }
    });
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
        const { std::cell::UnsafeCell::new((0, std::ptr::null_mut())) };
}

/// Implicit `this` for closure-typed class fields invoked method-style.
///
/// Issue #519: when `obj.fn(args)` calls a closure stored as a class field,
/// the field-scan dispatch in `js_native_call_method` can't bind `this`
/// through the closure ABI (closures take `(closure_ptr, arg0, …)` — no
/// `this` slot). Hono's RegExpRouter does this with `match = match` (the
/// imported function from matcher.js), and the function body's
/// `this.buildAllMatchers()` reads `this = 0` and TypeErrors out.
///
/// Codegen for `Expr::This` (perry-codegen/src/expr.rs) reads from this
/// thread-local when the lexical `this_stack` is empty (i.e. inside a
/// non-arrow function body or top-level closure body). The field-scan
/// dispatch saves the previous value, sets it to the receiver, calls the
/// closure, then restores. Direct function calls (`fn(args)`) don't touch
/// this slot, so non-method invocations don't pollute it across calls.
///
/// Defaults to `TAG_UNDEFINED`. JS spec says top-level `this` is undefined
/// in strict mode, which matches.
thread_local! {
    static IMPLICIT_THIS: Cell<u64> = const { Cell::new(crate::value::TAG_UNDEFINED) };
}

/// Read the current implicit `this` (issue #519).
#[no_mangle]
pub extern "C" fn js_implicit_this_get() -> f64 {
    IMPLICIT_THIS.with(|c| f64::from_bits(c.get()))
}

/// Set the implicit `this` and return the previous value.
/// Callers must restore the previous value to scope the binding to the
/// duration of a single method-style call.
#[no_mangle]
pub extern "C" fn js_implicit_this_set(value: f64) -> f64 {
    IMPLICIT_THIS.with(|c| f64::from_bits(c.replace(value.to_bits())))
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
        if writable {
            bits |= Self::WRITABLE;
        }
        if enumerable {
            bits |= Self::ENUMERABLE;
        }
        if configurable {
            bits |= Self::CONFIGURABLE;
        }
        Self { bits }
    }
    pub const fn writable(self) -> bool {
        (self.bits & Self::WRITABLE) != 0
    }
    pub const fn enumerable(self) -> bool {
        (self.bits & Self::ENUMERABLE) != 0
    }
    pub const fn configurable(self) -> bool {
        (self.bits & Self::CONFIGURABLE) != 0
    }
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
    PROPERTY_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), attrs);
    });
}

/// Look up the accessor descriptor (get/set) for (obj, key).
pub(crate) fn get_accessor_descriptor(obj: usize, key: &str) -> Option<AccessorDescriptor> {
    ACCESSOR_DESCRIPTORS.with(|m| m.borrow().get(&(obj, key.to_string())).copied())
}

/// Store an accessor descriptor for (obj, key).
pub(crate) fn set_accessor_descriptor(obj: usize, key: String, acc: AccessorDescriptor) {
    ACCESSORS_IN_USE.with(|c| c.set(true));
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    ACCESSOR_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), acc);
    });
}

/// Walk the keys array of `obj` and apply the given attribute mask AND filter to every existing key.
/// Used by `Object.freeze` (drops `writable` + `configurable`) and `Object.seal` (drops `configurable`).
unsafe fn mark_all_keys(
    obj: *mut ObjectHeader,
    drop_writable: bool,
    _drop_enumerable: bool,
    drop_configurable: bool,
) {
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
        let mut attrs =
            get_property_attrs(obj_addr, &key_str).unwrap_or(PropertyAttrs::new(true, true, true));
        if drop_writable {
            attrs.bits &= !PropertyAttrs::WRITABLE;
        }
        if drop_configurable {
            attrs.bits &= !PropertyAttrs::CONFIGURABLE;
        }
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
    fn enter(_method_name: &str) -> Option<Self> {
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
    object_type: u32,     // 1 = OBJECT_TYPE_REGULAR
    class_id: u32,        // 0
    parent_class_id: u32, // 0
    field_count: u32,     // 0
    keys_array: u64,      // 0 (null pointer as u64)
}
// Safety: this is a read-only zero-initialized struct with no interior mutability
unsafe impl Sync for NullObjectBytes {}

/// Issue #629: namespace imports for unresolved modules
/// (`import * as fsp from "node:fs/promises"` when the module isn't
/// implemented) used to fall back to `TAG_TRUE` at the codegen
/// catch-all, which made `typeof fsp === "boolean"` and every
/// `fsp.method` access return undefined silently — confusing because
/// the user sees `(boolean).method is not a function`. Returning a
/// stable empty-object stub makes `typeof === "object"` (matches
/// Node's module-namespace shape) and property access cleanly returns
/// undefined via the existing object-field path.
#[no_mangle]
pub extern "C" fn js_unresolved_namespace_stub() -> f64 {
    let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
    f64::from_bits(crate::JSValue::pointer(null_obj_ptr).bits())
}

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
    /// Issue #618-followup / drizzle SQL.Aliased: dynamic properties added
    /// via the IIFE pattern `((SQL2) => { SQL2.Aliased = Aliased; })(SQL)`
    /// to imported classes (which Perry stores as INT32-tagged class ids).
    /// Pre-fix `js_object_set_field_by_name` saw the receiver as an INT32
    /// "small handle" and silently dropped the assignment. Now route through
    /// this side-table keyed by class_id.
    pub(crate) static CLASS_DYNAMIC_PROPS: std::cell::RefCell<std::collections::HashMap<u32, std::collections::HashMap<String, f64>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

thread_local! {
    /// Direct-mapped inline cache. Empty entries have shape_id == 0
    /// and keys_array == null.
    static SHAPE_INLINE_CACHE: std::cell::UnsafeCell<[ShapeCacheEntry; SHAPE_INLINE_CACHE_SIZE]> =
        const { std::cell::UnsafeCell::new([ShapeCacheEntry {
            shape_id: 0,
            keys_array: std::ptr::null_mut(),
        }; SHAPE_INLINE_CACHE_SIZE]) };

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
            m.borrow()
                .get(&shape_id)
                .copied()
                .unwrap_or(std::ptr::null_mut())
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
            let gc_header = (keys_array as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *mut crate::gc::GcHeader;
            (*gc_header).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
        }
    }
    SHAPE_INLINE_CACHE.with(|cache| {
        let slot = (shape_id as usize) & (SHAPE_INLINE_CACHE_SIZE - 1);
        unsafe {
            (*cache.get())[slot] = ShapeCacheEntry {
                shape_id,
                keys_array,
            };
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
    prev_keys: usize, // offset 0
    key_ptr: usize,   // offset 8 — interned string pointer (pointer identity)
    next_keys: usize, // offset 16
    slot_idx: u32,    // offset 24
    _pad: u32,        // offset 28, pad to 32 bytes
}

const TRANSITION_CACHE_SIZE: usize = 16384;
/// Mask for slot computation: TRANSITION_CACHE_SIZE - 1
const TRANSITION_CACHE_MASK: usize = TRANSITION_CACHE_SIZE - 1;

/// Main-thread transition cache — bypasses TLS overhead (user code is
/// single-threaded). `#[no_mangle]` so the LLVM codegen can emit inline
/// lookups against this symbol (write PIC).
#[no_mangle]
static mut TRANSITION_CACHE_GLOBAL: [TransitionEntry; TRANSITION_CACHE_SIZE] = [TransitionEntry {
    prev_keys: 0,
    key_ptr: 0,
    next_keys: 0,
    slot_idx: 0,
    _pad: 0,
};
    TRANSITION_CACHE_SIZE];

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
///
/// On HIT we stamp the returned keys_array with `GC_FLAG_SHAPE_SHARED`
/// because the caller is about to reuse it for a SECOND object — any
/// future extension on either object must now clone-before-mutate. The
/// stamping happens here (lazily, on the first second-user lookup)
/// instead of in `transition_cache_insert` (eagerly, on every new
/// shape), which was the source of an O(N²) build-then-fill cost:
/// when a single object builds N unique keys, the old eager-stamp
/// forced a full clone of the keys_array on EVERY insert (10k inserts
/// → 50M total array entries copied). With lazy stamping, single-owner
/// shapes stay un-stamped and the per-insert clone is avoided —
/// 10k-key build drops from 20 s to milliseconds.
#[inline(always)]
fn transition_cache_lookup(
    prev_keys: usize,
    interned_key: *const crate::StringHeader,
) -> Option<(usize, u32)> {
    let kp = interned_key as usize;
    let slot = transition_cache_slot(prev_keys, kp);
    let entry = unsafe { TRANSITION_CACHE_GLOBAL[slot] };
    if entry.next_keys != 0 && entry.prev_keys == prev_keys && entry.key_ptr == kp {
        // Stamp SHAPE_SHARED on the returned keys_array — this is the
        // moment we observe that a SECOND object is reusing the
        // pre-existing shape. Both this caller and the original
        // owner (whose keys_array points at the same memory) must
        // now treat the array as shared.
        unsafe {
            let gc_header = (entry.next_keys as *const u8).wrapping_sub(crate::gc::GC_HEADER_SIZE)
                as *mut crate::gc::GcHeader;
            if entry.next_keys >= crate::gc::GC_HEADER_SIZE
                && (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
            {
                (*gc_header).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
            }
        }
        Some((entry.next_keys, entry.slot_idx))
    } else {
        None
    }
}

fn transition_cache_insert(
    prev_keys: usize,
    interned_key: *const crate::StringHeader,
    next_keys: usize,
    slot_idx: u32,
) {
    if next_keys == 0 {
        return;
    }
    let kp = interned_key as usize;
    let slot = transition_cache_slot(prev_keys, kp);
    unsafe {
        TRANSITION_CACHE_GLOBAL[slot] = TransitionEntry {
            prev_keys,
            key_ptr: kp,
            next_keys,
            slot_idx,
            _pad: 0,
        };
    }
    // NOTE: we deliberately do NOT stamp `GC_FLAG_SHAPE_SHARED` here.
    // The stamp moves to `transition_cache_lookup` so it only fires
    // when a second object actually reuses the shape — single-owner
    // build-then-fill (the common dict-construction pattern) stays
    // unshared and avoids the per-insert clone of the keys_array.
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

/// Cheap check used by the GC sweep to short-circuit per-object
/// `clear_overflow_for_ptr` calls. Most workloads never exceed the 8
/// inline slots and OVERFLOW_FIELDS stays empty for the entire run; on
/// those, paying a TLS access + RefCell borrow + HashMap remove on
/// every dead arena object is pure waste (~1.4 % leaf samples on
/// perf-comprehensive's sweep walk over ~1.6 M dead headers per cycle).
/// When this returns true, the sweep skips both `clear_overflow_for_ptr`
/// AND the `OVERFLOW_LAST` cache invalidation: with no entries in the
/// HashMap, the cached `Vec` pointer is either already null (initial
/// state) or was nulled by the most recent `clear_overflow_for_ptr` /
/// `overflow_set` cycle that emptied the map. Either way it can't
/// alias a freed pointer because no allocation can have produced a
/// matching obj_ptr without first writing to OVERFLOW_FIELDS.
#[inline]
pub fn overflow_fields_is_empty() -> bool {
    OVERFLOW_FIELDS.with(|m| m.borrow().is_empty())
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
                let getter: extern "C" fn(f64) -> f64 = std::mem::transmute(func_ptr as *const u8);
                let result_f64 = getter(value);
                let rbits = result_f64.to_bits();
                if (rbits & 0xFFFF_0000_0000_0000) == STRING_TAG {
                    let str_ptr = (rbits & POINTER_MASK) as *const crate::string::StringHeader;
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
                        if reg.contains(&parent) {
                            return true;
                        }
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

/// Per-class vtable with methods, getters, and setters
pub struct ClassVTable {
    pub methods: HashMap<String, VTableMethodEntry>,
    pub getters: HashMap<String, usize>, // getter func_ptr (signature: fn(this_f64) -> f64)
    pub setters: HashMap<String, usize>, // setter func_ptr (signature: fn(this_f64, value_f64) -> f64)
}

/// Global vtable registry: class_id -> vtable
pub static CLASS_VTABLE_REGISTRY: RwLock<Option<HashMap<u32, ClassVTable>>> = RwLock::new(None);

/// Set of all registered class ids. Populated at module init by codegen
/// emitting `js_register_class_id(cid)` for every user class — even
/// classes without any methods. Refs #618 / #420 followup.
pub static REGISTERED_CLASS_IDS: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Register a class id so `js_value_typeof` can distinguish class refs
/// (INT32-tagged with class_id payload) from real int32 numeric values.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = REGISTERED_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashSet::new());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// Register a static field value on a class so `Cls.field` (when `Cls` is
/// accessed via dynamic dispatch — e.g. through an Any-typed local) finds
/// the value via the runtime path. Codegen calls this at module init for
/// every static field initializer in addition to writing the value to the
/// per-field module global. Refs #420 / #618 followup. Static-field values
/// stored in CLASS_DYNAMIC_PROPS keyed by class_id.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_field(
    class_id: u32,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    CLASS_DYNAMIC_PROPS.with(|m| {
        m.borrow_mut()
            .entry(class_id)
            .or_insert_with(std::collections::HashMap::new)
            .insert(name, value);
    });
}

/// Returns true if `class_id` corresponds to a registered class. Used by
/// `js_value_typeof` (refs #618 / #420 followup) to distinguish a class
/// reference (NaN-boxed INT32 with class_id payload) from a regular int32
/// numeric value — JS spec says `typeof <class>` is "function", but
/// Perry's INT32_TAG storage shape is shared with numeric int32, so the
/// runtime needs an explicit registry check. Consults both
/// REGISTERED_CLASS_IDS (every class) and CLASS_VTABLE_REGISTRY (classes
/// with methods) so even classes registered before the explicit-id call
/// runs still detect via the vtable.
pub fn is_class_id_registered(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if let Ok(guard) = REGISTERED_CLASS_IDS.read() {
        if let Some(set) = guard.as_ref() {
            if set.contains(&class_id) {
                return true;
            }
        }
    }
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    registry
        .as_ref()
        .map(|m| m.contains_key(&class_id))
        .unwrap_or(false)
}

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
        setters: HashMap::new(),
    });
    vtable.methods.insert(
        name,
        VTableMethodEntry {
            func_ptr: func_ptr as usize,
            param_count: param_count as u32,
        },
    );
    VTABLE_GEN.fetch_add(1, Ordering::Release);
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
        setters: HashMap::new(),
    });
    vtable.getters.insert(name, func_ptr as usize);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

/// Register a class setter in the vtable registry.
///
/// Refs #486 (hono): hono's Context has `set res(_res) { ...; this.#res = _res;
/// this.finalized = true; }`. Without setter dispatch in `js_object_set_field_by_name`,
/// `c.res = response` from inside compose's `await handler(c, next)` chain stored
/// the response into a regular field slot but never ran the setter body — so
/// `this.finalized = true` never executed, `c.finalized` stayed false, and
/// hono-base's `if (!context.finalized) throw …` fired.
///
/// Setter signature: `fn(this_f64, value_f64) -> f64` (returns ignored, but
/// codegen emits a return so the LLVM signature matches a regular method body).
#[no_mangle]
pub unsafe extern "C" fn js_register_class_setter(
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
        setters: HashMap::new(),
    });
    vtable.setters.insert(name, func_ptr as usize);
    VTABLE_GEN.fetch_add(1, Ordering::Release);
}

// ============================================================================
// Per-callsite-keyed inline cache for vtable method dispatch.
//
// `js_native_call_method` is the hot dispatch tower for cross-module class
// instance method calls (e.g. `archetype.set(...)` from CommandBuffer.execute
// in the ECS workloads). Per profile, ~12% of perf-comprehensive samples land
// in `core::hash::BuildHasher` from the per-call `HashMap.get(method_name)`
// SipHash on the vtable lookup.
//
// Cache key: `(class_id, method_name_ptr)` where `method_name_ptr` is the
// rodata byte-pointer perry-codegen passes for the interned method name. The
// pointer is stable across calls within a module, so its address acts as a
// faster identity than re-hashing the bytes. Different modules may produce
// different rodata copies of the same name — the cache simply gets one entry
// per (class_id, name_pointer) pair, no correctness impact.
//
// Invalidation: a global `VTABLE_GEN` atomic is bumped on every
// `js_register_class_method` / `js_register_class_getter`. Each cache entry
// records the gen at populate time; lookups skip stale entries. Registration
// is one-shot at init in practice, so steady-state lookups never miss on
// gen.
// ============================================================================

static VTABLE_GEN: AtomicU64 = AtomicU64::new(1);

const VTABLE_IC_SIZE: usize = 4096;
const VTABLE_IC_MASK: usize = VTABLE_IC_SIZE - 1;

#[repr(C)]
#[derive(Copy, Clone)]
struct VTableICEntry {
    gen: u64,
    class_id: u32,
    _pad: u32,
    method_name_ptr: usize,
    func_ptr: usize,
    param_count: u32,
    _pad2: u32,
}

const EMPTY_VTABLE_IC_ENTRY: VTableICEntry = VTableICEntry {
    gen: 0,
    class_id: 0,
    _pad: 0,
    method_name_ptr: 0,
    func_ptr: 0,
    param_count: 0,
    _pad2: 0,
};

thread_local! {
    static VTABLE_IC: UnsafeCell<[VTableICEntry; VTABLE_IC_SIZE]> = const {
        UnsafeCell::new([EMPTY_VTABLE_IC_ENTRY; VTABLE_IC_SIZE])
    };
}

#[inline(always)]
fn vtable_ic_slot(class_id: u32, method_name_ptr: usize) -> usize {
    // Mix class_id into the upper bits of the pointer to spread (class, name)
    // pairs across slots. method_name_ptr is at least 1-byte aligned but
    // typically 8+ for rodata strings, so shift by 3 to drop the alignment
    // zeros before masking.
    let key = method_name_ptr
        .rotate_left(13)
        .wrapping_add((class_id as usize).wrapping_mul(0x9E37_79B9));
    (key >> 3) & VTABLE_IC_MASK
}

#[inline(always)]
unsafe fn vtable_ic_lookup(class_id: u32, method_name_ptr: usize) -> Option<(usize, u32)> {
    if method_name_ptr == 0 {
        return None;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = vtable_ic_slot(class_id, method_name_ptr);
    VTABLE_IC.with(|cell| {
        let cache = &*cell.get();
        let entry = &cache[slot];
        if entry.gen == cur_gen
            && entry.class_id == class_id
            && entry.method_name_ptr == method_name_ptr
        {
            Some((entry.func_ptr, entry.param_count))
        } else {
            None
        }
    })
}

#[inline(always)]
unsafe fn vtable_ic_insert(
    class_id: u32,
    method_name_ptr: usize,
    func_ptr: usize,
    param_count: u32,
) {
    if method_name_ptr == 0 {
        return;
    }
    let cur_gen = VTABLE_GEN.load(Ordering::Relaxed);
    let slot = vtable_ic_slot(class_id, method_name_ptr);
    VTABLE_IC.with(|cell| {
        let cache = &mut *cell.get();
        cache[slot] = VTableICEntry {
            gen: cur_gen,
            class_id,
            _pad: 0,
            method_name_ptr,
            func_ptr,
            param_count,
            _pad2: 0,
        };
    });
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
        if idx < args_len {
            *args_ptr.add(idx)
        } else {
            f64::NAN
        }
    }

    // LLVM-generated methods have signature `double(double this, double arg0, ...)`.
    // `this` is NaN-boxed as f64, so we must pass it as f64 — not i64 — to match
    // the calling convention. On ARM64 i64 and f64 share registers, so passing i64
    // works by accident; on Windows x64 ABI they use *different* registers (rcx vs
    // xmm0), causing segfaults when the method reads `this` from the wrong register.
    //
    // Issue #519: all call sites pass `this` as a RAW POINTER (the bottom-48-bit
    // address from `jsval.as_pointer()`). Bit-casting raw pointer bits to f64
    // produces a subnormal float (no NaN-box tag), which the method body
    // interprets as a number — every nested method call inside the body sees
    // `(number).<method>` and either returns garbage or throws TypeError via
    // the issue #510 catch-all (e.g. RegExpRouter.match → `this.buildAllMatchers()`
    // → "(number).buildAllMatchers is not a function" inside SmartRouter's
    // dispatch chain). NaN-box with POINTER_TAG before passing so the body
    // sees a real instance pointer.
    let this_f64: f64 = {
        let bits = this as u64;
        const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
        if bits != 0 && bits <= PTR_MASK {
            // Raw pointer (no NaN-box tag) — wrap with POINTER_TAG so the
            // method body's `this` arrives as a real instance pointer.
            f64::from_bits(JSValue::pointer(bits as *mut u8).bits())
        } else {
            // Already NaN-boxed (top bits set) or null — pass through.
            f64::from_bits(bits)
        }
    };

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
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
            )
        }
        3 => {
            let f: extern "C" fn(f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
            )
        }
        4 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64) -> f64 = std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
            )
        }
        5 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
            )
        }
        6 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
            )
        }
        7 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
                arg_or_nan(args_ptr, args_len, 6),
            )
        }
        8 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
                arg_or_nan(args_ptr, args_len, 6),
                arg_or_nan(args_ptr, args_len, 7),
            )
        }
        9 => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
                arg_or_nan(args_ptr, args_len, 6),
                arg_or_nan(args_ptr, args_len, 7),
                arg_or_nan(args_ptr, args_len, 8),
            )
        }
        _ => {
            let f: extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64 =
                std::mem::transmute(func_ptr);
            f(
                this_f64,
                arg_or_nan(args_ptr, args_len, 0),
                arg_or_nan(args_ptr, args_len, 1),
                arg_or_nan(args_ptr, args_len, 2),
                arg_or_nan(args_ptr, args_len, 3),
                arg_or_nan(args_ptr, args_len, 4),
                arg_or_nan(args_ptr, args_len, 5),
                arg_or_nan(args_ptr, args_len, 6),
                arg_or_nan(args_ptr, args_len, 7),
                arg_or_nan(args_ptr, args_len, 8),
                arg_or_nan(args_ptr, args_len, 9),
            )
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

/// Look up a method by name in the class vtable, walking the parent chain.
/// Returns `Some((func_ptr, param_count))` if found, `None` otherwise.
/// Used by `js_assimilate_thenable` (refs #586) and other runtime callers
/// that need to probe a class for a method without invoking it.
pub fn lookup_class_method_in_chain(class_id: u32, name: &str) -> Option<(usize, u32)> {
    let registry = CLASS_VTABLE_REGISTRY.read().unwrap();
    let reg = registry.as_ref()?;
    let mut cur = class_id;
    for _ in 0..32 {
        if let Some(vt) = reg.get(&cur) {
            if let Some(entry) = vt.methods.get(name) {
                return Some((entry.func_ptr, entry.param_count));
            }
        }
        match get_parent_class_id(cur) {
            Some(pid) if pid != 0 => cur = pid,
            _ => return None,
        }
    }
    None
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
/// that platform. Linux glibc and Windows mimalloc allocate well below
/// 2 TB though (often in the GB-to-tens-of-GB range), so the Darwin floor
/// silently rejects every legitimate object pointer there — issues
/// #385/#386/#387 traced back to this exact filter on Windows.
#[inline(always)]
fn is_valid_obj_ptr(ptr: *const u8) -> bool {
    let addr = ptr as u64;
    #[cfg(any(target_os = "android", target_os = "linux", target_os = "windows"))]
    const HEAP_MIN: u64 = 0x1000;
    #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "windows")))]
    const HEAP_MIN: u64 = 0x200_0000_0000;
    (HEAP_MIN..0x8000_0000_0000).contains(&addr)
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
pub extern "C" fn js_object_alloc_with_parent(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
) -> *mut ObjectHeader {
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
pub extern "C" fn js_object_alloc_fast_with_parent(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
) -> *mut ObjectHeader {
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
    let keys: Vec<&[u8]> = keys_bytes
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
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
        unsafe {
            *elements_ptr.add(i) = nanboxed;
        }
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
        let keys_bytes =
            unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = keys_bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .collect();
        let num_keys = keys.len();
        // Issue #179: shape-cache keys_array lives in the longlived arena
        // (see `js_build_class_keys_array` for the rationale).
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
            unsafe {
                *elements_ptr.add(i) = nanboxed;
            }
        }
        shape_cache_insert(shape_id, arr);
        arr
    };

    unsafe {
        (*ptr).keys_array = keys_arr;
    }
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
        let keys_bytes =
            unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = keys_bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .collect();
        let num_keys = keys.len();
        // Issue #179: shape-cache keys_array lives in the longlived arena.
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
            unsafe {
                *elements_ptr.add(i) = nanboxed;
            }
        }
        shape_cache_insert(shape_id, arr);
        arr
    };

    unsafe {
        (*obj_ptr).keys_array = keys_arr;
    }

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
            eprintln!(
                "[CLONE_NULL_PTR] field {} from src={:p} — replacing with undefined",
                i, src_ptr
            );
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
    let alloc_limit = std::cmp::max(src_field_count, 8);
    let header_size = std::mem::size_of::<ObjectHeader>();
    let src_fields = (src as *const u8).add(header_size) as *const u64;

    // Iterate up to `key_count`, not `min(key_count, src_field_count)`.
    // For objects with overflow fields (≥9 keys) `src_field_count` caps
    // at the inline alloc_limit (8) and the values for slots ≥ 8 live
    // in OVERFLOW_FIELDS — without iterating to `key_count` and routing
    // slots ≥ alloc_limit through `js_object_get_field`, the copy
    // silently dropped 9th..Nth properties.
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(src_keys, i as u32);
        if !key_val.is_string() {
            continue;
        }
        let key_ptr = key_val.as_string_ptr();
        let field_f64 = if i < alloc_limit {
            let field_bits = *src_fields.add(i);
            f64::from_bits(field_bits)
        } else {
            let v = js_object_get_field(src, i as u32);
            f64::from_bits(v.bits())
        };
        js_object_set_field_by_name(dst, key_ptr, field_f64);
    }
}

/// `Object.assign(target, source)` for a single source: mutate `target` by
/// copying every own enumerable string-keyed AND symbol-keyed property from
/// `source`, returning `target`. Both args are NaN-boxed JSValues; the return
/// is `target` unchanged so the caller can chain successive sources and the
/// final returned value is the same pointer the user passed in (preserving
/// object identity, class_id, and the existing entries in the SYMBOL_PROPERTIES
/// side table — the bug from #590 was that the previous lowering allocated a
/// fresh object, breaking `result === target` and orphaning target's
/// symbol-keyed properties since the side table is keyed by raw pointer).
///
/// Per spec, undefined/null target throws TypeError; here we silently no-op
/// to match the rest of perry-runtime's permissive style. Non-object sources
/// are skipped (matching `Object.assign(t, null)` / `Object.assign(t, 5)`
/// which are spec-allowed).
#[no_mangle]
pub unsafe extern "C" fn js_object_assign_one(target_f64: f64, source_f64: f64) -> f64 {
    const POINTER_TAG_LOCAL: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK_LOCAL: u64 = 0x0000_FFFF_FFFF_FFFF;

    // Decode target pointer. Accept either NaN-boxed POINTER_TAG or a raw
    // pointer value (defensive: callers occasionally pass i64-typed handles).
    let tgt_bits = target_f64.to_bits();
    let tgt_top16 = tgt_bits >> 48;
    let tgt_raw = if tgt_top16 >= 0x7FF8 {
        if tgt_top16 == 0x7FFC {
            // undefined/null/bool — spec says throw TypeError; silently return.
            return target_f64;
        }
        (tgt_bits & POINTER_MASK_LOCAL) as usize
    } else {
        tgt_bits as usize
    };
    if tgt_raw < 0x10000 {
        return target_f64;
    }

    // Decode source pointer. Skip null/undefined/non-pointer sources.
    let src_bits = source_f64.to_bits();
    let src_top16 = src_bits >> 48;
    let src_raw = if src_top16 >= 0x7FF8 {
        if src_top16 == 0x7FFC {
            return target_f64;
        }
        (src_bits & POINTER_MASK_LOCAL) as usize
    } else {
        src_bits as usize
    };
    if src_raw < 0x10000 || src_raw == tgt_raw {
        return target_f64;
    }

    let target = tgt_raw as *mut ObjectHeader;
    let src = src_raw as *const ObjectHeader;

    // 1) Copy own string-keyed enumerable properties from source to target,
    //    in source insertion order. Mirrors `js_object_copy_own_fields`.
    let src_keys = (*src).keys_array;
    if !src_keys.is_null() && (src_keys as usize) >= 0x10000 {
        let key_count = crate::array::js_array_length(src_keys) as usize;
        let src_field_count = (*src).field_count as usize;
        let alloc_limit = std::cmp::max(src_field_count, 8);
        let header_size = std::mem::size_of::<ObjectHeader>();
        let src_fields = (src as *const u8).add(header_size) as *const u64;
        // Same overflow-aware iteration as `js_object_copy_own_fields`.
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(src_keys, i as u32);
            if !key_val.is_string() {
                continue;
            }
            let key_ptr = key_val.as_string_ptr();
            let field_f64 = if i < alloc_limit {
                let field_bits = *src_fields.add(i);
                f64::from_bits(field_bits)
            } else {
                let v = js_object_get_field(src, i as u32);
                f64::from_bits(v.bits())
            };
            js_object_set_field_by_name(target, key_ptr, field_f64);
        }
    }

    // 2) Copy own symbol-keyed enumerable properties from source to target.
    //    The clone-then-iterate dance is non-negotiable — the inner
    //    `js_object_set_symbol_property` re-acquires SYMBOL_PROPERTIES'
    //    Mutex; holding the lock across the iteration would deadlock.
    let entries = crate::symbol::clone_symbol_entries_for_obj_ptr(src_raw);
    for (sym_ptr, value_bits) in entries {
        let sym_f64 = f64::from_bits(POINTER_TAG_LOCAL | (sym_ptr as u64 & POINTER_MASK_LOCAL));
        let value_f64 = f64::from_bits(value_bits);
        crate::symbol::js_object_set_symbol_property(target_f64, sym_f64, value_f64);
    }

    target_f64
}

/// Get a field from an object by index
#[no_mangle]
pub extern "C" fn js_object_get_field(obj: *const ObjectHeader, field_index: u32) -> JSValue {
    let obj = {
        let b = obj as u64;
        let t = b >> 48;
        if t >= 0x7FF8 {
            if t == 0x7FFC
                || (b & 0x0000_FFFF_FFFF_FFFF) == 0
                || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000
            {
                return JSValue::undefined();
            }
            (b & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x1000000 {
        return JSValue::undefined();
    }
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
        let fields_ptr =
            (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const JSValue;
        let val = *fields_ptr.add(field_index as usize);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        if val.bits() == 0x7FFD_0000_0000_0000 {
            eprintln!(
                "[NULL_PTR_FIELD_GET] obj={:p} field_index={} class_id={} field_count={}",
                obj,
                field_index,
                (*obj).class_id,
                (*obj).field_count
            );
            return JSValue::undefined();
        }
        val
    }
}

/// Set a field on an object by index
#[no_mangle]
pub extern "C" fn js_object_set_field(obj: *mut ObjectHeader, field_index: u32, value: JSValue) {
    let obj = {
        let b = obj as u64;
        let t = b >> 48;
        if t >= 0x7FF8 {
            if t == 0x7FFC
                || (b & 0x0000_FFFF_FFFF_FFFF) == 0
                || (b & 0x0000_FFFF_FFFF_FFFF) < 0x10000
            {
                return;
            }
            (b & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
        } else {
            obj
        }
    };
    if obj.is_null() || (obj as usize) < 0x1000000 {
        return;
    }
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

/// Get the class ID of an object.
///
/// Returns 0 unless `obj` is a real GC-arena-allocated class instance.
/// Issue #350 (round 2): the codegen's `idispatch` tower for unknown-receiver
/// method calls (e.g. `set.has(c)` when the static type is `ReadonlySet<T>`,
/// or `a.componentTypeSet.has(c)` where `a` is `Archetype | undefined`) uses
/// this function to compare the receiver's class id against every user
/// class implementing the same method name. Without the GC-type guard we
/// blindly read 4 bytes at offset 4 of the receiver — which for a
/// `SetHeader` (allocated via std::alloc, no GcHeader, layout
/// `{ size: u32, capacity: u32, elements: *mut f64 }`) is its `capacity`
/// field. `js_set_alloc(0)` defaults capacity to 4, which collides with
/// whichever user class lands at id 4, routing the call into the wrong
/// method body and crashing on the bogus `this` pointer.
#[no_mangle]
pub extern "C" fn js_object_get_class_id(obj: *const ObjectHeader) -> u32 {
    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return 0;
    }
    let addr = obj as usize;
    // Built-in headers (Set / Map / Regex) live in their own per-type
    // registries — they're never user class instances. Reject them first
    // so we never try to read a GcHeader at obj-8, which doesn't exist
    // for these std::alloc'd headers.
    if crate::set::is_registered_set(addr)
        || crate::map::is_registered_map(addr)
        || crate::regex::is_regex_pointer(obj as *const u8)
    {
        return 0;
    }
    unsafe {
        if !is_valid_obj_ptr(obj as *const u8) {
            return 0;
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return 0;
        }
        (*obj).class_id
    }
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
            let gc =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
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
pub extern "C" fn js_object_set_field_by_index(
    obj: *mut ObjectHeader,
    key: *const crate::string::StringHeader,
    field_index: u32,
    value: f64,
) {
    if obj.is_null() || (obj as usize) < 0x1000000 {
        return;
    }
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
                            let closure = (acc.set & crate::value::POINTER_MASK)
                                as *const crate::closure::ClosureHeader;
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
    // Issue #323: arrays land here too (the codegen routes every `Object.keys`
    // call through this entry point, regardless of receiver type). Treating an
    // ArrayHeader as an ObjectHeader read garbage from the slot-0 element bits
    // — `obj_type=length`, `keys_array=elements[1]` — which happened to look
    // null when slots were zero-filled. After the issue #323 init-to-HOLE fix,
    // slot[1] reads as TAG_HOLE which is non-null and segfaulted downstream.
    // Detect arrays by GC type byte and emit string indices for non-HOLE slots.
    let stripped = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    if !stripped.is_null() && (stripped as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header = (stripped as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = stripped as *const crate::array::ArrayHeader;
                let length = (*arr).length;
                if length > 100_000 {
                    return crate::array::js_array_alloc(0);
                }
                let elements = (arr as *const u8)
                    .add(std::mem::size_of::<crate::array::ArrayHeader>())
                    as *const u64;
                let result = crate::array::js_array_alloc(length);
                for i in 0..length {
                    if std::ptr::read(elements.add(i as usize)) == crate::value::TAG_HOLE {
                        continue;
                    }
                    // Format `i` as decimal into a stack buffer; SSO covers
                    // 0..=99999 (≤5 bytes), and a length-100k array hits the
                    // sanity-cap above so we never need a heap StringHeader.
                    let s = i.to_string();
                    let key_box = crate::string::js_string_new_sso(s.as_ptr(), s.len() as u32);
                    crate::array::js_array_push_f64(result, key_box);
                }
                return result;
            }
        }
    }
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return crate::array::js_array_alloc(0);
        }
        // Per JS spec, `Object.keys` must return a fresh array — callers
        // can `.sort()`, `.push()`, etc. without mutating the receiver.
        // Pre-fix this fast path returned the object's own internal
        // `keys_array` pointer, so `Object.keys(o).sort()` reordered
        // `o`'s key→slot mapping and subsequent `o.foo` reads returned
        // the wrong slot's value. The slow path below already builds a
        // fresh array; the fast path now mirrors it, just without the
        // per-key descriptor check.
        let has_descriptors =
            PROPERTY_DESCRIPTORS.with(|m| m.borrow().keys().any(|(ptr, _)| *ptr == obj as usize));
        let len = crate::array::js_array_length(keys) as usize;
        if !has_descriptors {
            let out = crate::array::js_array_alloc(len as u32);
            for i in 0..len {
                let key_val = crate::array::js_array_get(keys, i as u32);
                crate::array::js_array_push_f64(out, f64::from_bits(key_val.bits()));
            }
            return out;
        }
        // Slow path: filter out non-enumerable keys.
        let filtered = crate::array::js_array_alloc(len as u32);
        for i in 0..len {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if !key_val.is_string() {
                continue;
            }
            let stored_key = key_val.as_string_ptr();
            if stored_key.is_null() {
                continue;
            }
            let name_ptr =
                (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
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
        // Iterate up to keys_len (logical property count), not
        // field_count — same fix as Object.entries above. Without
        // this, objects with overflow fields silently returned only
        // their first 8 values.
        let keys = (*obj).keys_array;
        let count = if !keys.is_null() {
            crate::array::js_array_length(keys) as usize
        } else {
            (*obj).field_count as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        for i in 0..count {
            let value = js_object_get_field(obj as *mut ObjectHeader, i as u32);
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
        // Iterate up to keys_len (the logical property count), not
        // field_count. Parser-built and dict-built objects with ≥9
        // fields cap field_count at the inline alloc_limit (8) and
        // store overflow values in OVERFLOW_FIELDS — for those,
        // field_count under-counts the actual property count by N-8.
        // Without this fix, `Object.entries(obj)` on a 50-key dict
        // returned only the first 8 entries (silent data loss).
        // Mirrors the same fix in `js_object_keys` and the
        // `actual_fields = keys_len` line in `json.rs::stringify_object`.
        let count = if !keys.is_null() {
            crate::array::js_array_length(keys) as usize
        } else {
            (*obj).field_count as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        for i in 0..count {
            // Create a pair array [key, value]
            let pair = crate::array::js_array_alloc(2);

            // Get the key (from keys array — already validated non-null
            // when count came from there).
            if !keys.is_null() && (i as u32) < crate::array::js_array_length(keys) {
                let key = crate::array::js_array_get_f64(keys, i as u32);
                crate::array::js_array_push_f64(pair, key);
            } else {
                crate::array::js_array_push_f64(pair, 0.0);
            }

            // Read the value. `js_object_get_field` handles the
            // inline-vs-overflow split internally (inline if
            // i < field_count, overflow_get otherwise).
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
    let nanbox_true = f64::from_bits(0x7FFC_0000_0000_0004u64); // TAG_TRUE

    let obj_val = JSValue::from_bits(obj.to_bits());
    let key_val = JSValue::from_bits(key.to_bits());

    // Refs #420 / #618: `Symbol in ClassRef` — drizzle's `entityKind in cls`.
    // Class refs are INT32-tagged. Check CLASS_STATIC_SYMBOLS for symbol
    // keys and CLASS_DYNAMIC_PROPS for string keys.
    {
        let bits = obj.to_bits();
        if (bits >> 48) == 0x7FFE {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            // Symbol key path.
            if let Some(_) = crate::symbol::class_static_symbol_lookup(class_id, key) {
                return nanbox_true;
            }
            // String key path: check CLASS_DYNAMIC_PROPS via the get-by-name fn.
            if !key_val.is_pointer() && key_val.is_string() {
                // is_string covers heap StringHeader. Route through the
                // CLASS_DYNAMIC_PROPS-aware get fn.
            }
            // Fallback: emit false for class refs that aren't in either table.
            return nanbox_false;
        }
    }

    if !obj_val.is_pointer() {
        return nanbox_false;
    }

    let obj_ptr = obj_val.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return nanbox_false;
    }

    // Issue #323: array fast path. `n in arr` with a numeric key was always
    // returning false because the receiver was treated as ObjectHeader and
    // the key-is-string guard below rejected the numeric key. Detect an
    // ArrayHeader by GC type byte; for numeric keys check `index < length`
    // and slot != TAG_HOLE (distinguishes a hole from an explicit
    // `arr[i] = undefined` write, the latter overwrites HOLE with UNDEFINED).
    if (obj_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header =
                (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = obj_ptr as *const crate::array::ArrayHeader;
                let length = (*arr).length;
                if length > 100_000 {
                    return nanbox_false;
                }
                // Numeric key: extract the index. Accept both NaN-boxed i32
                // and plain f64 (e.g. literal `1`) provided it's a
                // non-negative integer in range.
                let idx: Option<u32> = if key_val.is_int32() {
                    let i = key_val.as_int32();
                    if i >= 0 {
                        Some(i as u32)
                    } else {
                        None
                    }
                } else if key_val.is_number() {
                    let f = f64::from_bits(key_val.bits());
                    if f >= 0.0 && f.fract() == 0.0 && f < u32::MAX as f64 {
                        Some(f as u32)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(idx) = idx {
                    if idx >= length {
                        return nanbox_false;
                    }
                    let elements = (arr as *const u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *const u64;
                    if std::ptr::read(elements.add(idx as usize)) == crate::value::TAG_HOLE {
                        return nanbox_false;
                    }
                    return nanbox_true;
                }
                // Non-numeric key on an array: only `length` and inherited
                // prototype methods would return true. Conservatively return
                // false for now — out of scope for #323.
                return nanbox_false;
            }
        }
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
pub extern "C" fn js_object_get_field_by_name(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> JSValue {
    // Issue #618-followup: read INT32-tagged class ref's dynamic property
    // from the side-table (mirror of the set-side intercept). For drizzle's
    // `SQL.Aliased` lookup pattern.
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFE && !key.is_null() {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("");
                // v0.5.752: class_ref.constructor synthesizes back to the
                // same class ref so drizzle's
                // `Object.getPrototypeOf(value).constructor === Class` chain
                // collapses correctly (with v0.5.751's getPrototypeOf
                // returning the class ref for instance receivers). Refs
                // #420 / #618 followup.
                if name == "constructor" && class_id != 0 && is_class_id_registered(class_id) {
                    return JSValue::from_bits(bits);
                }
                if !name.is_empty() {
                    let result = CLASS_DYNAMIC_PROPS.with(|m| {
                        m.borrow()
                            .get(&class_id)
                            .and_then(|props| props.get(name).copied())
                    });
                    if let Some(v) = result {
                        return JSValue::from_bits(v.to_bits());
                    }
                }
            }
            return JSValue::undefined();
        }
    }
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
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
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
            if raw.is_null() || top16 == 0x7FFC {
                // undefined/null tag or null pointer — return undefined
                return JSValue::undefined();
            }
            // Issue #340: small-handle receivers (raw < 0x100000) come
            // from native modules (axios, fastify, ioredis, ...) that
            // store objects in registries and expose integer ids. The
            // handle property dispatcher (registered by stdlib via
            // `js_register_handle_property_dispatch`) routes the
            // property name to the per-module accessor (e.g. axios
            // status/data, fastify req query/params/...). Without
            // this, every property access on those handles silently
            // returned undefined.
            if (raw as usize) > 0 && (raw as usize) < 0x100000 {
                if !key.is_null() {
                    // Drizzle-sqlite blocker: synth `data.constructor` for
                    // small-handle native instances so drizzle's
                    // `isConfig(data)` duck-type via
                    // `data.constructor.name !== "Object"` doesn't crash on
                    // `(undefined).name` under #648's strict catch-all.
                    // Returning the existing NULL_OBJECT_BYTES stub (a real
                    // ObjectHeader-shape with no fields) makes `(stub).name`
                    // return undefined safely, and `undefined !== "Object"`
                    // makes isConfig return false at the first gate. Refs
                    // #645 deeper followup.
                    unsafe {
                        let key_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let key_len = (*key).byte_len as usize;
                        let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
                        if key_bytes == b"constructor" {
                            let null_obj_ptr =
                                &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                            return JSValue::from_bits(JSValue::pointer(null_obj_ptr).bits());
                        }
                    }
                    let dispatch = unsafe { HANDLE_PROPERTY_DISPATCH };
                    if let Some(dispatch) = dispatch {
                        unsafe {
                            let key_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let key_len = (*key).byte_len as usize;
                            let bits = dispatch(raw as i64, key_ptr, key_len);
                            return JSValue::from_bits(bits.to_bits());
                        }
                    }
                }
                return JSValue::undefined();
            }
            raw
        } else {
            obj
        }
    };
    if obj.is_null() {
        return JSValue::undefined();
    }
    // Same handle-receiver path for already-stripped pointers — happens
    // when the codegen passes a raw i64 handle through the slow path.
    if (obj as usize) < 0x100000 {
        if !key.is_null() {
            let dispatch = unsafe { HANDLE_PROPERTY_DISPATCH };
            if let Some(dispatch) = dispatch {
                unsafe {
                    let key_ptr =
                        (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let key_len = (*key).byte_len as usize;
                    let bits = dispatch(obj as i64, key_ptr, key_len);
                    return JSValue::from_bits(bits.to_bits());
                }
            }
        }
        return JSValue::undefined();
    }
    if (obj as usize) < 0x1000000 {
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
                // Issue #639 followup: method-as-value reads on a Buffer
                // (e.g. duck-type tests like `typeof v.readUInt8 === "function"`
                // in @perryts/mysql's `isBufferLike`) need to return a
                // bound-method closure so `typeof` reports `"function"` and
                // a subsequent call routes through `js_native_call_method`'s
                // existing `dispatch_buffer_method` arm. Pre-fix every
                // non-length read returned undefined, so duck tests failed
                // and the encoder fell through to its `String(buf)` fallback —
                // BLOB params got encoded as VAR_STRING and the INSERT
                // silently corrupted the binary column.
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if is_buffer_method_name(name) {
                        let heap_name = {
                            let layout =
                                std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1)
                                    .unwrap();
                            let ptr = std::alloc::alloc(layout);
                            std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                            ptr
                        };
                        // Buffers are stored as raw f64-bitcast pointers
                        // (NOT NaN-boxed) per CLAUDE.md "Module-level
                        // variables" — but `js_native_call_method`'s
                        // buffer arm at line ~5031 strips both raw and
                        // NaN-boxed payloads via `(bits >> 48) >= 0x7FF8`,
                        // so wrapping in POINTER_TAG here is equally
                        // valid and matches `js_class_method_bind`.
                        let this_f64 =
                            f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                        let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                        return JSValue::from_bits(result.to_bits());
                    }
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
                let sym_f64 =
                    f64::from_bits(0x7FFD_0000_0000_0000u64 | (obj as u64 & 0x0000_FFFF_FFFF_FFFF));
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
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if !is_valid_obj_ptr(obj as *const u8) {
            return JSValue::undefined();
        }
        let gc_type = (*gc_header).obj_type;
        // Issue #618: closures have their own GC type (GC_TYPE_CLOSURE=4)
        // distinct from GC_TYPE_OBJECT, but support dynamic-property storage
        // via the `CLOSURE_DYNAMIC_PROPS` side-table. `js_object_set_field_by_name`
        // routes writes there for the IIFE-namespace pattern
        // (`((sql2) => { sql2.identifier = ...; })(sql)`); mirror the read
        // path here so the companion get fires. Pre-fix the
        // `gc_type != GC_TYPE_OBJECT` arm below would early-return undefined
        // for any closure receiver, masking the dynamic-prop side-table.
        if gc_type == crate::gc::GC_TYPE_CLOSURE {
            if !key.is_null() {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                if let Ok(name_str) = std::str::from_utf8(name_bytes) {
                    let val = crate::closure::closure_get_dynamic_prop(obj as usize, name_str);
                    return JSValue::from_bits(val.to_bits());
                }
            }
            return JSValue::undefined();
        }
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
        if gc_type == crate::gc::GC_TYPE_OBJECT && crate::regex::is_regex_pointer(obj as *const u8)
        {
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

        // Issue #649: native-module sub-namespace property access.
        // `fs.constants.F_OK` lowers to `PropertyGet { PropertyGet { fs,
        // "constants" }, "F_OK" }` — the inner expression's runtime value
        // is a NATIVE_MODULE_CLASS_ID-tagged ObjectHeader produced by
        // `js_create_native_module_namespace`; the outer PropertyGet then
        // arrives here with the sub-namespace as receiver. Pre-fix the
        // lookup fell through to the field-bag scan (which only stores
        // `__module__`) and returned undefined. Now we route through
        // `get_native_module_constant` directly.
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID && !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let nb_ptr = crate::value::js_nanbox_pointer(obj as i64);
            let module_name = get_module_name_from_namespace(nb_ptr);
            if !module_name.is_empty() {
                let property_name =
                    std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
                if let Some(val) = get_native_module_constant(module_name, property_name, nb_ptr) {
                    return JSValue::from_bits(val.to_bits());
                }
                return JSValue::undefined();
            }
        }

        // Refs #420 / #618 followup: `instance.constructor` returns the
        // class ref. Pre-fix this fell through to the keys_array lookup
        // which never finds "constructor" (the class itself isn't stored
        // as a field on the instance), and the chain returned undefined.
        // Drizzle's `is(value, type)` walks `value.constructor[entityKind]`
        // which depends on this. Spec: every instance's `__proto__.constructor`
        // points back to the class function. We materialize that lookup
        // by reading the ObjectHeader's class_id and returning the
        // INT32-tagged class ref if registered. Unregistered class_id
        // (e.g. `class C {}` with no methods) still returns undefined
        // here; pure object literals have class_id=0 and also return
        // undefined (matches Node behavior — bare object literals don't
        // get a custom constructor; their .constructor would be Object).
        if !key.is_null() {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"constructor" {
                let class_id = (*obj).class_id;
                if class_id != 0 && is_class_id_registered(class_id) {
                    let bits = 0x7FFE_0000_0000_0000u64 | (class_id as u64);
                    return JSValue::from_bits(bits);
                }
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
            let keys_gc =
                (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
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
                const { std::cell::UnsafeCell::new([(0usize, 0u32, 0u32); FIELD_CACHE_SIZE]) };
        }
        let cache_idx = (keys_id.wrapping_add(key_hash as usize)) % FIELD_CACHE_SIZE;
        let cached = FIELD_CACHE.with(|c| {
            let cache = &*c.get();
            let entry = cache[cache_idx];
            if entry.0 == keys_id && entry.1 == key_hash {
                Some(entry.2)
            } else {
                None
            }
        });
        if let Some(field_idx) = cached {
            // Accessor short-circuit: if this (obj, key) has a getter installed,
            // invoke it instead of reading the slot. The `ACCESSORS_IN_USE`
            // thread-local gate keeps this off the hot path in the common case.
            if ACCESSORS_IN_USE.with(|c| c.get()) {
                if let Ok(name) = std::str::from_utf8(key_bytes) {
                    if let Some(acc) = get_accessor_descriptor(obj as usize, name) {
                        if acc.get != 0 {
                            let closure = (acc.get & crate::value::POINTER_MASK)
                                as *const crate::closure::ClosureHeader;
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
        let _field_count = (*obj).field_count as usize;

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
                                    let closure = (acc.get & crate::value::POINTER_MASK)
                                        as *const crate::closure::ClosureHeader;
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

        // Key not found in the keys_array — fall back to the class
        // vtable's getter map. Refs #486 (hono): cross-module class
        // getters (e.g. hono Context's `get req()` defined in
        // `hono/dist/context.js` and read from a user `c.req.url`
        // expression in main.ts) reach this point because the field
        // dispatcher only looks for stored fields, not getter accessors.
        // The getter is registered in `CLASS_VTABLE_REGISTRY` via
        // `js_register_class_getter` at module init by codegen — invoke
        // it with the same NaN-boxed `this` the codegen passes for
        // method dispatch.
        let class_id = (*obj).class_id;
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    // Walk the class -> parent chain so a getter declared
                    // on a base class is also found when the receiver is
                    // a subclass instance. `get_parent_class_id` reads
                    // CLASS_REGISTRY (populated by `js_register_class_parent`).
                    let mut cid = class_id;
                    let mut depth = 0usize;
                    while depth < 32 {
                        if let Some(vtable) = reg.get(&cid) {
                            if let Ok(name) = std::str::from_utf8(key_bytes) {
                                if let Some(&getter_ptr) = vtable.getters.get(name) {
                                    // Getters take `this` as f64 (NaN-boxed
                                    // POINTER_TAG), matching the codegen
                                    // calling convention for class methods.
                                    let this_f64: f64 = f64::from_bits(
                                        crate::value::js_nanbox_pointer(obj as i64).to_bits(),
                                    );
                                    let f: extern "C" fn(f64) -> f64 =
                                        std::mem::transmute(getter_ptr);
                                    return JSValue::from_bits(f(this_f64).to_bits());
                                }
                            }
                        }
                        match get_parent_class_id(cid) {
                            Some(p) if p != 0 && p != cid => {
                                cid = p;
                                depth += 1;
                            }
                            _ => break,
                        }
                    }
                }
            }

            // v0.5.756: method-as-value fallback. If `obj.method` reads via
            // the runtime path (Any-typed receiver, so the codegen #446
            // arm at expr.rs:3596 didn't fire), look up the method in the
            // class vtable chain and return a bound-method closure
            // (BOUND_METHOD_FUNC_PTR sentinel + (this, name_ptr, name_len)
            // captures). This makes both `typeof obj.method === "function"`
            // and `obj.method(args)` work for class methods on Any-typed
            // receivers — the closure-call dispatch routes through
            // `js_native_call_method` which walks the same vtable chain.
            // Refs #446 / drizzle's `(ins as any)._prepare()` chain.
            if let Ok(name) = std::str::from_utf8(key_bytes) {
                if lookup_class_method_in_chain(class_id, name).is_some() {
                    // Allocate a fresh i8 buffer for the method name owned
                    // by the closure. The keys_array's StringHeader bytes
                    // could in theory be GC'd if the keys_array is not
                    // pinned for the closure's lifetime.
                    let heap_name = {
                        let layout =
                            std::alloc::Layout::from_size_align(key_bytes.len().max(1), 1).unwrap();
                        let ptr = std::alloc::alloc(layout);
                        std::ptr::copy_nonoverlapping(key_bytes.as_ptr(), ptr, key_bytes.len());
                        ptr
                    };
                    let this_f64 =
                        f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
                    let result = js_class_method_bind(this_f64, heap_name, key_bytes.len());
                    return JSValue::from_bits(result.to_bits());
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
pub extern "C" fn js_object_get_field_by_name_f64(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
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
    if obj.is_null() || key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Issue #340: small-handle receivers (axios, fastify, ioredis,
    // ...) are passed here from the codegen IC miss path with the
    // lower-48 of the NaN-box stripped — `obj as usize` is the
    // raw handle id (1, 2, 3, ...). Route to HANDLE_PROPERTY_DISPATCH
    // (registered by stdlib via js_register_handle_property_dispatch)
    // so `r.status` / `r.data` and similar handle-property accesses
    // dispatch to the per-module accessor instead of silently
    // returning undefined.
    if (obj as usize) > 0 && (obj as usize) < 0x100000 {
        // Drizzle-sqlite blocker: synth `data.constructor` for small-handle
        // receivers — IC-miss path mirror of the constructor intercept in
        // `js_object_get_field_by_name`. Refs #645 deeper followup.
        unsafe {
            let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let key_len = (*key).byte_len as usize;
            let key_bytes = std::slice::from_raw_parts(key_ptr, key_len);
            if key_bytes == b"constructor" {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }
        let dispatch = unsafe { HANDLE_PROPERTY_DISPATCH };
        if let Some(dispatch) = dispatch {
            unsafe {
                let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let key_len = (*key).byte_len as usize;
                return dispatch(obj as i64, key_ptr, key_len);
            }
        }
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if (obj as usize) < 0x10000 {
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
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT
        };
        let keys = (*obj).keys_array;
        let is_regular = is_object && (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR;
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
                    // The codegen IC fast path computes `obj + 24 + slot*8`
                    // and does a direct load. Any inline slot (`i <
                    // alloc_limit`) is reachable via that path, so cache
                    // every inline slot — including the ones at index >= 8
                    // for classes whose `field_count` exceeds the
                    // MIN_FIELD_SLOTS=8 baseline (e.g. World.commandBuffer
                    // sits at slot 12). Pre-fix this branch capped the cache
                    // at `i < 8` which left every >8-slot field permanently
                    // missing the cache: every access fell through to a
                    // fresh keys_array walk + js_string_equals chain. On
                    // perf-comprehensive's hot loops that path was hit
                    // ~900k times per run (40% inclusive samples per
                    // perfcomp.profile).
                    (*cache)[0] = keys as i64;
                    (*cache)[1] = i as i64;
                    let field_ptr = (obj as *const u8)
                        .add(std::mem::size_of::<ObjectHeader>() + i * 8)
                        as *const f64;
                    return *field_ptr;
                }
            }
        }
    }
    let value = js_object_get_field_by_name(obj, key);
    f64::from_bits(value.bits())
}

/// Polymorphic numeric-key get: companion of `js_object_set_index_polymorphic`.
/// Reads `obj[idx]` where `idx` is a number and the receiver type isn't
/// statically narrowed. Dispatches by GC type:
///
/// - `GC_TYPE_ARRAY` (and forwarded / lazy variants) → `js_array_get_f64`,
///   which routes through `clean_arr_ptr` for forwarding-chain follow.
/// - `GC_TYPE_OBJECT` / `GC_TYPE_CLOSURE`            → stringify `idx` and
///   delegate to `js_object_get_field_by_name_f64`. JS treats `obj[0]` as
///   `obj["0"]`, so the stringification matches spec semantics.
///
/// Closes #471 (read side): paired with the IndexSet polymorphic fix so
/// `Record<number, T>` stores and reads through the same path. Without
/// this, `constMap[i] = v; constMap[i]` would set via the object setter
/// but read from `obj+8+i*8` (stale ObjectHeader fields), returning
/// garbage f64 values.
#[no_mangle]
pub extern "C" fn js_object_get_index_polymorphic(obj_handle: i64, idx: f64) -> f64 {
    let raw = if (obj_handle as u64) >> 48 >= 0x7FF8 {
        (obj_handle as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        obj_handle as u64
    };
    if raw < 0x1000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let idx_i32 = idx as i32;
    if idx_i32 < 0 {
        // Negative numeric keys → string keys on the object path.
        let s = idx_i32.to_string();
        unsafe {
            let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            let v = js_object_get_field_by_name(raw as *mut ObjectHeader, key);
            return f64::from_bits(v.bits());
        }
    }

    let gc_type = unsafe {
        let gc_header_addr = raw.wrapping_sub(crate::gc::GC_HEADER_SIZE as u64) as usize;
        if gc_header_addr < 0x1000 {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        *(gc_header_addr as *const u8)
    };

    if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
        return crate::array::js_array_get_f64(
            raw as *mut crate::array::ArrayHeader,
            idx_i32 as u32,
        );
    }
    if gc_type == crate::gc::GC_TYPE_OBJECT || gc_type == crate::gc::GC_TYPE_CLOSURE {
        let s = if idx == (idx_i32 as f64) {
            idx_i32.to_string()
        } else {
            format!("{}", idx)
        };
        unsafe {
            let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            let v = js_object_get_field_by_name(raw as *mut ObjectHeader, key);
            return f64::from_bits(v.bits());
        }
    }
    // Buffer / Map / Set / typed-array / unknown — try the array getter
    // (which handles registered buffers + typed arrays via per-kind reads).
    crate::array::js_array_get_f64(raw as *mut crate::array::ArrayHeader, idx_i32 as u32)
}

/// Polymorphic numeric-key set: `obj[idx] = value` where `idx` is a number
/// and the receiver type isn't statically known. Dispatches by GC type:
///
/// - `GC_TYPE_ARRAY` / buffer / typed-array → `js_array_set_f64_extend`,
///   which preserves the array fast-path (forwarding chain follow + grow).
/// - `GC_TYPE_OBJECT` / `GC_TYPE_CLOSURE`   → stringify `idx` and delegate
///   to `js_object_set_field_by_name`. JS treats `obj[0] = v` as `obj["0"] = v`,
///   so the stringification matches spec semantics.
///
/// Closes #471: codegen's previous IndexSet numeric-key fallback emitted
/// an inline `obj+8+idx*8` store. That layout assumes an `ArrayHeader`
/// (8-byte header) but `ObjectHeader` is 24 bytes followed by `max(field_count, 8)`
/// inline slots, so any `idMap[i] = v` on an object with i ≥ 7 wrote past
/// the object's allocation, corrupting whatever heap memory followed.
/// In the @perryts/mongodb repro, that memory happened to be doc[0]'s
/// `keys_array` pointer — Object.keys returned a stale string pointer
/// the BSON encoder read as an empty array, emitting empty BSON docs
/// over the wire.
///
/// Receiver layout other than array/object (e.g. raw pointer below the heap
/// or a small handle) silently no-ops, matching the existing tolerant-on-
/// bad-args contract of `js_array_set_f64` / `js_object_set_field_by_name`.
#[no_mangle]
pub extern "C" fn js_object_set_index_polymorphic(obj_handle: i64, idx: f64, value: f64) {
    // Strip NaN-box tags defensively. Codegen calls this with the lower-48
    // bits already extracted via `unbox_to_i64`, but match the convention
    // of every other entry-point so a stray un-stripped caller (or a JIT
    // that forgets the mask) still works.
    let raw = if (obj_handle as u64) >> 48 >= 0x7FF8 {
        (obj_handle as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        obj_handle as u64
    };
    if raw < 0x1000 {
        return;
    }
    let idx_i32 = idx as i32;
    if idx_i32 < 0 {
        // Negative indices on objects coerce to e.g. "-1" string keys; on
        // arrays, JS spec gates them to no-ops. Stringify and delegate so
        // the object case (rare but possible) still routes correctly.
        let s = idx_i32.to_string();
        unsafe {
            let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            js_object_set_field_by_name(raw as *mut ObjectHeader, key, value);
        }
        return;
    }

    // Read GC type byte (offset 0 of GcHeader, which lives at obj-8).
    let gc_type = unsafe {
        let gc_header_addr = raw.wrapping_sub(crate::gc::GC_HEADER_SIZE as u64) as usize;
        if gc_header_addr < 0x1000 {
            return;
        }
        *(gc_header_addr as *const u8)
    };

    if gc_type == crate::gc::GC_TYPE_ARRAY {
        // Includes lazy/forwarded — js_array_set_f64_extend's clean_arr_ptr_mut
        // walks the forwarding chain and routes buffers/typed-arrays through
        // their per-kind setter.
        crate::array::js_array_set_f64_extend(
            raw as *mut crate::array::ArrayHeader,
            idx_i32 as u32,
            value,
        );
        return;
    }
    if gc_type == crate::gc::GC_TYPE_OBJECT || gc_type == crate::gc::GC_TYPE_CLOSURE {
        // Stringify the index and route through the object field setter,
        // which handles shape transitions, frozen/sealed/extensible checks,
        // overflow into out-of-line storage, and accessor descriptors.
        let s = if idx == (idx_i32 as f64) {
            // Common integer case — avoid the Display path's allocator hit
            // and just format an i32 directly.
            idx_i32.to_string()
        } else {
            format!("{}", idx)
        };
        unsafe {
            let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            js_object_set_field_by_name(raw as *mut ObjectHeader, key, value);
        }
        return;
    }
    // Buffer / Map / Set / other GC types — fall through to the array
    // setter, which has its own per-kind dispatch (registered buffer →
    // byte write, registered typed-array → typed setter). Anything not
    // recognized is a no-op via clean_arr_ptr_mut returning null.
    crate::array::js_array_set_f64_extend(
        raw as *mut crate::array::ArrayHeader,
        idx_i32 as u32,
        value,
    );
}

/// Issue #615 helper — read a `*const StringHeader` as a Rust `String`
/// for inclusion in TypeError diagnostic messages. Returns `"<unknown>"`
/// for null / non-UTF-8 / corrupt headers so the throw still fires
/// rather than panicking on the slow-path edge case.
unsafe fn key_to_str_for_diag(key: *const crate::StringHeader) -> String {
    if key.is_null() {
        return "<unknown>".to_string();
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    if name_len == 0 {
        return String::new();
    }
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
    std::str::from_utf8(name_bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "<unknown>".to_string())
}

/// Set a field value by its string key name (dynamic property access)
/// This searches the keys array for a match and sets the corresponding value.
/// If the key doesn't exist, it adds it to the object.
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    // Issue #618-followup: detect INT32-tagged class ref (top16 == 0x7FFE).
    // Drizzle's `((SQL2) => { SQL2.Aliased = Aliased; })(SQL)` pattern sets
    // a static property on an imported class — Perry stores classes as
    // INT32-tagged class ids, so the receiver here is e.g. 0x7FFE_0000_0000_002A
    // not a real ObjectHeader. Route to the CLASS_DYNAMIC_PROPS side-table
    // so a later `SQL.Aliased` read can find it.
    {
        let bits = obj as u64;
        if (bits >> 48) == 0x7FFE && !key.is_null() {
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            unsafe {
                let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key).byte_len as usize;
                let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    CLASS_DYNAMIC_PROPS.with(|m| {
                        m.borrow_mut()
                            .entry(class_id)
                            .or_insert_with(std::collections::HashMap::new)
                            .insert(name, value);
                    });
                }
            }
            return;
        }
    }
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
                            let name_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
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
                        let name_ptr =
                            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
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
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type != crate::gc::GC_TYPE_OBJECT && gc_type != crate::gc::GC_TYPE_CLOSURE {
            if !is_valid_obj_ptr(obj as *const u8) {
                return;
            }
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

        // Refs #486 (hono): class setter dispatch. JS spec: a `set X(...)`
        // accessor on the prototype intercepts `obj.X = value` writes
        // before they hit the instance's data slots. Hono's `set res(_res)
        // { …; this.#res = _res; this.finalized = true; }` is the canonical
        // example — without setter dispatch, `c.res = response` from inside
        // compose stored the response into a regular field slot but never
        // ran the body, so `this.finalized = true` never executed and
        // hono-base's `if (!context.finalized) throw` fired on every
        // request. Walk the class -> parent chain mirroring the getter
        // dispatch in `js_object_get_field_by_name`.
        if !key.is_null() && (key as usize) > 0x10000 {
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        let key_bytes = {
                            let name_ptr =
                                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                            let name_len = (*key).byte_len as usize;
                            std::slice::from_raw_parts(name_ptr, name_len)
                        };
                        let mut cid = class_id;
                        let mut depth = 0usize;
                        while depth < 32 {
                            if let Some(vtable) = reg.get(&cid) {
                                if let Ok(name) = std::str::from_utf8(key_bytes) {
                                    if let Some(&setter_ptr) = vtable.setters.get(name) {
                                        // Setters take `(this_f64, value_f64)`
                                        // matching the codegen calling
                                        // convention for class methods (this
                                        // = NaN-boxed POINTER_TAG of the
                                        // receiver).
                                        let this_f64: f64 = f64::from_bits(
                                            crate::value::js_nanbox_pointer(obj as i64).to_bits(),
                                        );
                                        let f: extern "C" fn(f64, f64) -> f64 =
                                            std::mem::transmute(setter_ptr);
                                        let _ = f(this_f64, value);
                                        return;
                                    }
                                }
                            }
                            match get_parent_class_id(cid) {
                                Some(p) if p != 0 && p != cid => {
                                    cid = p;
                                    depth += 1;
                                }
                                _ => break,
                            }
                        }
                    }
                }
            }
        }

        // Check Object.freeze/seal/preventExtensions flags
        let obj_flags = (*gc_header)._reserved;
        let is_frozen = obj_flags & crate::gc::OBJ_FLAG_FROZEN != 0;
        let is_sealed_or_no_extend =
            obj_flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0;

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
            let gc_hdr =
                (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
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
            if let Some((next_keys, slot_idx)) =
                transition_cache_lookup(prev_keys_usize, interned_key)
            {
                // Defensive: strip a raw-null POINTER_TAG value the same
                // way the slow overflow path below does, so a bogus
                // 0x7FFD_0000_0000_0000 store doesn't leak into an
                // overflow map.
                let vbits = value.to_bits();
                let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                    crate::value::TAG_UNDEFINED
                } else {
                    vbits
                };
                (*obj).keys_array = next_keys as *mut ArrayHeader;
                let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
                if (slot_idx as usize) < alloc_limit {
                    // Inline the field write — `obj` has already been
                    // validated (GC header read, type check, closure
                    // check) by the prelude above, and `vbits` has had
                    // the null-POINTER-TAG replacement applied. No
                    // point re-doing it in `js_object_set_field`.
                    let fields_ptr =
                        (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
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
            // Frozen or sealed/non-extensible objects reject new keys.
            // Issue #615 — strict-mode throw instead of silent return.
            if is_frozen || is_sealed_or_no_extend {
                let key_str = key_to_str_for_diag(key);
                crate::error::throw_immutable_write(1, &key_str);
            }
            // Create a new keys array with the key
            let new_keys = crate::array::js_array_alloc(4);
            let new_keys =
                crate::array::js_array_push(new_keys, JSValue::string_ptr(key as *mut _));
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
        let needs_descriptor_key =
            ACCESSORS_IN_USE.with(|c| c.get()) || PROPERTY_ATTRS_IN_USE.with(|c| c.get());
        let incoming_key_str: Option<String> = if needs_descriptor_key && !key.is_null() {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        } else {
            None
        };

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;

        // Sidecar O(1) lookup when keys_array has grown past the
        // linear-scan break-even. Without this, the build-then-fill
        // pattern (`for i in 0..N { obj["k_"+i] = i; }`) is O(N²)
        // because every insert does a linear scan that grows by one
        // each iteration. With the sidecar, the per-insert cost is
        // O(1) amortized (rebuild after a `js_array_push` realloc is
        // bounded by the doubling growth pattern).
        if !key.is_null() && (key as usize) > 0x10000 && key_count >= KEYS_INDEX_THRESHOLD as usize
        {
            let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            let key_hash = key_bytes_hash(name_ptr, name_len);
            if let Some(i) = keys_index_lookup(obj, keys, name_bytes, key_hash) {
                let i = i as usize;
                if is_frozen {
                    let key_str = key_to_str_for_diag(key);
                    crate::error::throw_immutable_write(0, &key_str);
                }
                if i < alloc_limit {
                    js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
                } else {
                    let vbits = value.to_bits();
                    let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                        crate::value::TAG_UNDEFINED
                    } else {
                        vbits
                    };
                    overflow_set(obj as usize, i, vbits);
                }
                return;
            }
            // Miss path: the linear scan below will confirm and then
            // append. We skip the scan entirely and just append the
            // key (the sidecar would have found it if it existed).
            // Same effect as scanning all N entries with no match.
            if is_frozen || is_sealed_or_no_extend {
                let key_str = key_to_str_for_diag(key);
                crate::error::throw_immutable_write(1, &key_str);
            }
            // Skip the linear-scan loop by jumping past it via a
            // labeled-block break. The append code that follows the
            // scan is shared.
            // We achieve this by setting a marker, then the linear
            // scan checks it and skips.
            let keys_gc_header =
                (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let keys_shared = if (keys as usize) >= crate::gc::GC_HEADER_SIZE
                && (*keys_gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
            {
                (*keys_gc_header).gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
            } else {
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
            let new_index = key_count;
            if new_index >= alloc_limit {
                let vbits = value.to_bits();
                let vbits = if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                    crate::value::TAG_UNDEFINED
                } else {
                    vbits
                };
                let new_keys =
                    crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
                (*obj).keys_array = new_keys;
                overflow_set(obj as usize, new_index, vbits);
                transition_cache_insert(
                    prev_keys_usize,
                    interned_key,
                    new_keys as usize,
                    new_index as u32,
                );
                keys_index_insert(
                    obj as usize,
                    (new_index + 1) as u32,
                    key_hash,
                    new_index as u32,
                );
                return;
            }
            let new_keys =
                crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
            (*obj).keys_array = new_keys;
            js_object_set_field(obj, new_index as u32, JSValue::from_bits(value.to_bits()));
            if new_index as u32 >= (*obj).field_count {
                (*obj).field_count = new_index as u32 + 1;
            }
            transition_cache_insert(
                prev_keys_usize,
                interned_key,
                new_keys as usize,
                new_index as u32,
            );
            keys_index_insert(
                new_keys as usize,
                (new_index + 1) as u32,
                key_hash,
                new_index as u32,
            );
            return;
        }

        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            // Keys are stored as string pointers (NaN-boxed)
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    // Found it - update the field. Frozen objects must
                    // throw a TypeError on writes to existing keys
                    // (issue #615 — strict-mode behavior, default for TS).
                    if is_frozen {
                        let key_str = key_to_str_for_diag(key);
                        crate::error::throw_immutable_write(0, &key_str);
                    }
                    // Accessor short-circuit: if a setter is registered, invoke
                    // it instead of writing the slot. A property with `get` but
                    // no `set` silently ignores the write (non-strict mode).
                    if ACCESSORS_IN_USE.with(|c| c.get()) {
                        if let Some(ref k) = incoming_key_str {
                            if let Some(acc) = get_accessor_descriptor(obj as usize, k) {
                                if acc.set != 0 {
                                    let closure = (acc.set & crate::value::POINTER_MASK)
                                        as *const crate::closure::ClosureHeader;
                                    if !closure.is_null() {
                                        crate::closure::js_closure_call1(closure, value);
                                    }
                                }
                                return;
                            }
                        }
                    }
                    // Per-property writable check (set by Object.defineProperty / freeze).
                    // Issue #615 — strict-mode throw on read-only assign.
                    if PROPERTY_ATTRS_IN_USE.with(|c| c.get()) {
                        if let Some(ref k) = incoming_key_str {
                            if let Some(attrs) = get_property_attrs(obj as usize, k) {
                                if !attrs.writable() {
                                    crate::error::throw_immutable_write(0, k);
                                }
                            }
                        }
                    }
                    if i < alloc_limit {
                        js_object_set_field(obj, i as u32, JSValue::from_bits(value.to_bits()));
                    } else {
                        // This key was previously stored in the overflow map — update it there
                        let vbits = value.to_bits();
                        let vbits =
                            if (vbits >> 48) == 0x7FFD && (vbits & 0x0000_FFFF_FFFF_FFFF) == 0 {
                                crate::value::TAG_UNDEFINED
                            } else {
                                vbits
                            };
                        overflow_set(obj as usize, i, vbits);
                    }
                    return;
                }
            }
        }

        // Key not found - add it to the object.
        // Frozen/sealed/non-extensible objects reject new keys.
        // Issue #615 — strict-mode throw.
        if is_frozen || is_sealed_or_no_extend {
            let key_str = key_to_str_for_diag(key);
            crate::error::throw_immutable_write(1, &key_str);
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
        let keys_gc_header =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
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
            } else {
                vbits
            };
            let new_keys =
                crate::array::js_array_push(owned_keys, JSValue::string_ptr(key as *mut _));
            (*obj).keys_array = new_keys;
            overflow_set(obj as usize, new_index, vbits);
            // Record the shape transition so the next object sharing
            // `prev_keys` that adds the same key hits the fast path.
            // The cached target is stamped `GC_FLAG_SHAPE_SHARED` by
            // `transition_cache_insert`, which triggers clone-on-extend
            // on either object if someone later appends past this key.
            transition_cache_insert(
                prev_keys_usize,
                interned_key,
                new_keys as usize,
                new_index as u32,
            );
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
        transition_cache_insert(
            prev_keys_usize,
            interned_key,
            new_keys as usize,
            new_index as u32,
        );
    }
}

/// Delete a field from an object by its string key name
/// Returns 1 if the field was deleted (or didn't exist), 0 otherwise
/// Note: In strict mode, this would return 0 for non-configurable properties,
/// but we don't track configurability, so we always return 1.
#[no_mangle]
pub extern "C" fn js_object_delete_field(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> i32 {
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            // No keys array means no fields to delete, but delete "succeeds" vacuously
            return 1;
        }

        // Search through the keys array for a match
        let key_count = crate::array::js_array_length(keys) as usize;
        let mut found_idx: Option<usize> = None;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if crate::string::js_string_equals(key, stored_key) != 0 {
                    found_idx = Some(i);
                    break;
                }
            }
        }

        let i = match found_idx {
            Some(i) => i,
            None => return 1, // Not found — delete succeeds vacuously
        };

        // Proper delete: shift remaining keys + values down by one, then
        // shorten keys_array. Pre-fix this just set the value to
        // undefined and left the key in place, so `Object.keys`,
        // `Object.entries`, `for-in` etc. all still saw the deleted
        // property. Bun and Node remove the property entirely; we
        // match that.
        let field_count = (*obj).field_count;
        let alloc_limit = std::cmp::max(field_count as usize, 8);
        let new_count = key_count - 1;

        // CRITICAL: clone the keys_array before mutating it. The same
        // keys_array is shared across all objects that built the same
        // shape via `transition_cache_lookup`-hit fast paths. Without
        // cloning, mutating its length / contents to remove the deleted
        // key would corrupt every other object that picks up this
        // shape — they'd silently lose entries they never deleted.
        let keys_cloned = crate::array::js_array_alloc(new_count.max(1) as u32 + 4);
        let src_elements =
            (keys as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
        let dst_elements =
            (keys_cloned as *mut u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *mut f64;
        // Copy keys [0..i) ++ [i+1..N) into [0..new_count).
        for j in 0..i {
            *dst_elements.add(j) = *src_elements.add(j);
        }
        for j in i..new_count {
            *dst_elements.add(j) = *src_elements.add(j + 1);
        }
        (*keys_cloned).length = new_count as u32;
        (*obj).keys_array = keys_cloned;

        // 1) Shift values down: for slot j in i..new_count, copy slot j+1
        //    into slot j. Inline reads/writes for j < alloc_limit;
        //    overflow_get/set otherwise.
        for j in i..new_count {
            let next = js_object_get_field(obj, (j + 1) as u32);
            // Inline write if target slot < alloc_limit, else overflow.
            if j < alloc_limit {
                let fields_ptr =
                    (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
                ptr::write(fields_ptr.add(j), next);
            } else {
                overflow_set(obj as usize, j, next.bits());
            }
        }
        // Clear the now-tail slot so reads past keys_array.length see undefined.
        if new_count < alloc_limit {
            let fields_ptr =
                (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
            ptr::write(fields_ptr.add(new_count), JSValue::undefined());
        } else {
            overflow_set(obj as usize, new_count, crate::value::TAG_UNDEFINED);
        }

        // 2) (Keys already shifted into the cloned keys_array above —
        //    we built the new keys directly with the deleted entry
        //    omitted, so no in-place shift is needed.)

        // 3) Adjust field_count: keep within bounds. If the original
        //    field_count counted this slot, drop by one.
        if (i as u32) < field_count {
            (*obj).field_count = field_count - 1;
        }

        // 4) Invalidate the keys-index sidecar for this object — the
        //    slot map is now stale (entries past `i` have shifted).
        //    The next lookup at threshold will rebuild from current
        //    keys_array.
        KEYS_INDEX.with(|m| {
            m.borrow_mut().remove(&(obj as usize));
        });

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
pub extern "C" fn js_object_rest(
    src: *const ObjectHeader,
    exclude_keys: *const ArrayHeader,
) -> *mut ObjectHeader {
    if src.is_null() {
        return js_object_alloc(0, 0);
    }
    unsafe {
        let keys = (*src).keys_array;
        if keys.is_null() {
            return js_object_alloc(0, 0);
        }

        let key_count = crate::array::js_array_length(keys) as usize;
        let exclude_count = if exclude_keys.is_null() {
            0
        } else {
            crate::array::js_array_length(exclude_keys) as usize
        };

        // Collect indices of keys to include (not in exclude list and not undefined/deleted)
        let mut include_indices: Vec<usize> = Vec::new();
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if !key_val.is_string() {
                continue;
            }
            let key_str = key_val.as_string_ptr();

            // Check if field was deleted
            let field_val = js_object_get_field(src, i as u32);
            if field_val.is_undefined() {
                continue;
            }

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

/// v0.5.749: dynamic instanceof — `value instanceof type` where the
/// type is a runtime value (function arg holding a class ref). Extracts
/// the class_id from the INT32 NaN-tag (top16=0x7FFE) and dispatches to
/// `js_instanceof`. Returns FALSE for non-class-ref type values (matches
/// JS spec: `1 instanceof 2` throws, but Perry returns false defensively).
/// Refs #420 / #618 followup.
#[no_mangle]
pub extern "C" fn js_instanceof_dynamic(value: f64, type_ref: f64) -> f64 {
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let bits = type_ref.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 {
            return js_instanceof(value, class_id);
        }
    }
    f64::from_bits(TAG_FALSE)
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
        let hook: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(func_ptr as *const u8) };
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
        if top16 == 0 && bits >= 0x1000 && crate::buffer::is_registered_buffer(bits as usize) {
            return true_val;
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
        if !value.is_nan()
            && value.is_finite()
            && crate::date::is_registered_date_bits(value.to_bits())
        {
            return true_val;
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

    // `Object` — ECMAScript spec: `x instanceof Object` is true for any
    // non-primitive (every object/array/function/Map/Set/Buffer/RegExp/
    // Date/typed-array/Promise/etc.). The codegen maps `Object` to this
    // reserved id (#585 follow-up: pre-#585 fix this case worked by
    // accident because the codegen produced `class_id = 0` and the
    // runtime returned true via `0 == 0` on the obj_class_id check).
    const CLASS_ID_OBJECT: u32 = 0xFFFF0050;
    if class_id == CLASS_ID_OBJECT {
        if jsval.is_pointer() {
            return true_val;
        }
        if !value.is_nan()
            && value.is_finite()
            && crate::date::is_registered_date_bits(value.to_bits())
        {
            return true_val;
        }
        let top16 = (bits >> 48) as u16;
        if top16 == 0 && bits >= 0x1000 {
            let addr = bits as usize;
            if crate::buffer::is_registered_buffer(addr)
                || crate::set::is_registered_set(addr)
                || crate::map::is_registered_map(addr)
                || crate::typedarray::lookup_typed_array_kind(addr).is_some()
            {
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
            if top16 == 0 && bits >= 0x1000 {
                bits as usize
            } else {
                0
            }
        };
        if addr != 0 && addr >= crate::gc::GC_HEADER_SIZE {
            let gc_header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            unsafe {
                let obj_type = (*gc_header).obj_type;
                if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
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

    // Refs #421: NaN-boxed POINTER_TAG values whose unboxed payload is a
    // small registry id (Web Fetch handles, sockets, DB connections, etc.)
    // are NOT real ObjectHeader pointers — reading the GC header at
    // `obj_ptr - 8` would SIGSEGV on unmapped memory. They aren't instances
    // of any user-defined class either, so return false unconditionally.
    if (obj_ptr as usize) < 0x100000 {
        return false_val;
    }

    unsafe {
        // Special handling for built-in Error and its subclasses (TypeError, RangeError, etc.).
        // ErrorHeader uses GC_TYPE_ERROR; we match by error_kind against the requested CLASS_ID_*.
        let gc_header =
            (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ERROR {
            let err_ptr = obj_ptr as *const crate::error::ErrorHeader;
            let kind = (*err_ptr).error_kind;
            return match class_id {
                crate::error::CLASS_ID_ERROR => true_val,
                crate::error::CLASS_ID_TYPE_ERROR => {
                    if kind == crate::error::ERROR_KIND_TYPE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_RANGE_ERROR => {
                    if kind == crate::error::ERROR_KIND_RANGE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_REFERENCE_ERROR => {
                    if kind == crate::error::ERROR_KIND_REFERENCE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_SYNTAX_ERROR => {
                    if kind == crate::error::ERROR_KIND_SYNTAX_ERROR {
                        true_val
                    } else {
                        false_val
                    }
                }
                crate::error::CLASS_ID_AGGREGATE_ERROR => {
                    if kind == crate::error::ERROR_KIND_AGGREGATE_ERROR {
                        true_val
                    } else {
                        false_val
                    }
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

/// Apply form for method calls with spread arguments on dynamically-typed
/// receivers (refs #421). Reads `args_array_handle` (a JS array containing
/// v0.5.754: dispatch `obj[strKey](args)` — computed-key method call.
/// `name_handle` is a StringHeader pointer (already-unboxed). Extracts
/// the bytes/length from the header and forwards to
/// `js_native_call_method`. Refs #420 / drizzle's
/// `this.session[isOneTimeQuery ? "prepareOneTimeQuery" :
/// "prepareQuery"](...)` chain.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_str_key(
    object: f64,
    name_handle: i64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if name_handle == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let str_ptr = name_handle as *const crate::StringHeader;
    let bytes_ptr = (str_ptr as *const i8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes_len = (*str_ptr).byte_len as usize;
    js_native_call_method(object, bytes_ptr, bytes_len, args_ptr, args_len)
}

/// every regular + spread arg already concatenated by codegen), materialises
/// the f64 elements into a temporary `Vec<f64>`, and forwards to
/// `js_native_call_method`. Lets the caller use a single uniform shape for
/// `recv.method(...args)` without exposing array layout to the dispatcher.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_apply(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_array_handle: i64,
) -> f64 {
    let arr = args_array_handle as *const crate::array::ArrayHeader;
    let len = if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr) as usize
    };
    let buf: Vec<f64> = (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i as u32))
        .collect();
    let (args_ptr, args_len) = if buf.is_empty() {
        (std::ptr::null::<f64>(), 0_usize)
    } else {
        (buf.as_ptr(), buf.len())
    };
    js_native_call_method(object, method_name_ptr, method_name_len, args_ptr, args_len)
}

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
        let func_ptr =
            crate::value::JS_HANDLE_CALL_METHOD.load(std::sync::atomic::Ordering::SeqCst);
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
            (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader,
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
            "add" | "sub" | "mul" | "div" | "mod" | "umod" | "pow" | "and" | "or" | "xor"
            | "shln" | "shrn" | "maskn" | "eq" | "lt" | "lte" | "gt" | "gte" | "cmp"
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

    // Issue #654: typed-array method dispatch. The codegen for
    // `new Float64Array(...)` (and the other typed-array constructors)
    // returns the raw heap pointer bitcast to f64 — no POINTER_TAG —
    // so neither `is_pointer()` nor the handle dispatch above catches
    // it. Detect via the `TYPED_ARRAY_REGISTRY` side table and route
    // common methods (`sort`, `at`, `toSorted`, `toReversed`, `with`,
    // `findLast`, `findLastIndex`) to their `js_typed_array_*` runtime
    // helpers. Without this arm `(a: Float64Array).sort()` reached the
    // `(number).sort is not a function` catch-all because raw pointer
    // bits classify as `is_number()` (top16 outside the tagged range).
    {
        let top16 = raw_bits >> 48;
        if top16 == 0 && raw_bits >= 0x10000 {
            let addr = raw_bits as usize;
            if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
                let ta = addr as *mut crate::typedarray::TypedArrayHeader;
                let arg0 = || -> f64 {
                    if args_len >= 1 && !args_ptr.is_null() {
                        unsafe { *args_ptr }
                    } else {
                        f64::NAN
                    }
                };
                let arg_closure = |i: usize| -> *const crate::closure::ClosureHeader {
                    if i < args_len && !args_ptr.is_null() {
                        let v = unsafe { *args_ptr.add(i) };
                        let bits = v.to_bits();
                        let tag = (bits >> 48) as u16;
                        if tag == 0x7FFD {
                            (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::closure::ClosureHeader
                        } else {
                            std::ptr::null()
                        }
                    } else {
                        std::ptr::null()
                    }
                };
                match method_name {
                    "length" => {
                        return crate::typedarray::js_typed_array_length(ta) as f64;
                    }
                    "at" => {
                        return crate::typedarray::js_typed_array_at(ta, arg0());
                    }
                    "sort" => {
                        let cmp = arg_closure(0);
                        let result = if cmp.is_null() {
                            crate::typedarray::js_typed_array_sort_default(ta)
                        } else {
                            crate::typedarray::js_typed_array_sort_with_comparator(ta, cmp)
                        };
                        return f64::from_bits(result as u64);
                    }
                    "toSorted" => {
                        let cmp = arg_closure(0);
                        let result = if cmp.is_null() {
                            crate::typedarray::js_typed_array_to_sorted_default(ta)
                        } else {
                            crate::typedarray::js_typed_array_to_sorted_with_comparator(ta, cmp)
                        };
                        return f64::from_bits(result as u64);
                    }
                    "toReversed" => {
                        let result = crate::typedarray::js_typed_array_to_reversed(ta);
                        return f64::from_bits(result as u64);
                    }
                    "with" => {
                        let idx = arg0();
                        let val = if args_len >= 2 && !args_ptr.is_null() {
                            unsafe { *args_ptr.add(1) }
                        } else {
                            f64::NAN
                        };
                        let result = crate::typedarray::js_typed_array_with(ta, idx, val);
                        return f64::from_bits(result as u64);
                    }
                    "findLast" => {
                        let cb = arg_closure(0);
                        if cb.is_null() {
                            return f64::from_bits(crate::value::TAG_UNDEFINED);
                        }
                        return crate::typedarray::js_typed_array_find_last(ta, cb);
                    }
                    "findLastIndex" => {
                        let cb = arg_closure(0);
                        if cb.is_null() {
                            return -1.0;
                        }
                        return crate::typedarray::js_typed_array_find_last_index(ta, cb);
                    }
                    _ => {
                        // Fall through. Other methods aren't handled here
                        // yet; they hit the primitive-method catch-all
                        // below — better than silent no-op.
                    }
                }
            }
        }
    }

    // Issue #514 followup: string method dispatch on any-typed receivers.
    // When `(s: any).at(-1)` / `.slice(1)` / etc. lower through the
    // dispatch tower and `s` actually holds a string, we need to route
    // to the matching `js_string_*` runtime helper. Without this, the
    // primitive-method TypeError catch-all (issue #510 fix below) fires
    // for every legitimate string method call on a `(s: any)` parameter,
    // breaking hono's `mergePath` template-literal logic that mixes
    // `s?.[0]` (handled by `js_dyn_index_get`, issue #514) with
    // `s?.at(-1)` and `s?.slice(1)`. Static call sites for typed string
    // receivers continue to use the inline `js_string_*` paths in
    // `lower_string_method.rs`; this dispatch only catches fallthroughs
    // where codegen couldn't statically prove the type.
    if jsval.is_string() || jsval.is_short_string() {
        let s_ptr =
            crate::value::js_get_string_pointer_unified(object) as *const crate::StringHeader;
        if !s_ptr.is_null() {
            let arg_i32 = |i: usize| -> i32 {
                if i < args_len && !args_ptr.is_null() {
                    let v = unsafe { *args_ptr.add(i) };
                    if v.is_nan() || v.is_infinite() {
                        0
                    } else {
                        v as i32
                    }
                } else {
                    0
                }
            };
            match method_name {
                "at" => {
                    return crate::string::js_string_at(s_ptr, arg_i32(0));
                }
                "charAt" => {
                    let result = crate::string::js_string_char_at(s_ptr, arg_i32(0));
                    if result.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(result).bits());
                }
                "charCodeAt" => {
                    return crate::string::js_string_char_code_at(s_ptr, arg_i32(0));
                }
                "slice" => {
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let len_i32 = unsafe { (*s_ptr).byte_len } as i32;
                    let end = if args_len >= 2 { arg_i32(1) } else { len_i32 };
                    let result = crate::string::js_string_slice(s_ptr, start, end);
                    if result.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(result).bits());
                }
                "toString" | "valueOf" => return object,
                // Issue #519 follow-up: hono's matcher.js does
                // `path2.match(matcher[0])` where `path2` is a string and
                // `matcher[0]` is a regex. The HIR optimistic
                // `Expr::StringMatch` lowering only fires when the regex
                // arg is a literal or a static `RegExp`-typed Ident — for
                // a `Member` or `Element` access (matcher[0]) it falls
                // through to the dynamic dispatch, which then ended up at
                // the issue #510 catch-all (`(string).match is not a
                // function`) because no runtime arm handled `match`.
                "match" | "matchAll" => {
                    if args_len >= 1 && !args_ptr.is_null() {
                        let regex_val = unsafe { *args_ptr };
                        // Extract regex handle from the arg value. RegExp
                        // values are NaN-boxed pointers; pass through the
                        // pointer extraction the same way the HIR-level
                        // StringMatch path does.
                        let regex_jsval = JSValue::from_bits(regex_val.to_bits());
                        if !regex_jsval.is_pointer() {
                            return f64::from_bits(JSValue::null().bits());
                        }
                        let regex_ptr = regex_jsval.as_pointer::<crate::regex::RegExpHeader>();
                        let result_ptr = if method_name == "match" {
                            crate::regex::js_string_match(s_ptr, regex_ptr)
                        } else {
                            crate::regex::js_string_match_all(s_ptr, regex_ptr)
                        };
                        if result_ptr.is_null() {
                            return f64::from_bits(JSValue::null().bits());
                        }
                        return f64::from_bits(JSValue::pointer(result_ptr as *mut u8).bits());
                    }
                    return f64::from_bits(JSValue::null().bits());
                }
                "search" => {
                    if args_len >= 1 && !args_ptr.is_null() {
                        let regex_val = unsafe { *args_ptr };
                        let regex_jsval = JSValue::from_bits(regex_val.to_bits());
                        if !regex_jsval.is_pointer() {
                            return f64::from_bits(JSValue::int32(-1).bits());
                        }
                        let regex_ptr = regex_jsval.as_pointer::<crate::regex::RegExpHeader>();
                        let i32_v = crate::regex::js_string_search_regex(s_ptr, regex_ptr);
                        return f64::from_bits(JSValue::int32(i32_v).bits());
                    }
                    return f64::from_bits(JSValue::int32(-1).bits());
                }
                // Refs #421 — common string methods on any-typed receivers.
                // Hono's compiled JS (and most npm packages with stripped TS
                // types) does `request.url.indexOf("/")` where `url` is in
                // any-typed position because the type annotation on
                // `(request) =>` was erased at bundle time. Without these
                // arms, the v0.5.593 catch-all throws `(string).indexOf is
                // not a function`. Each arm extracts the search-string
                // argument and calls the existing `js_string_*` runtime
                // helper. Static call sites for typed string receivers keep
                // their inline paths in `lower_string_method.rs` and don't
                // come through this dispatcher.
                "indexOf" | "includes" | "lastIndexOf" | "startsWith" | "endsWith" | "concat" => {
                    let arg_str = |i: usize| -> *const crate::StringHeader {
                        if i < args_len && !args_ptr.is_null() {
                            let v = unsafe { *args_ptr.add(i) };
                            crate::value::js_get_string_pointer_unified(v)
                                as *const crate::StringHeader
                        } else {
                            std::ptr::null()
                        }
                    };
                    let needle = arg_str(0);
                    // Integer-returning methods MUST return raw `i as f64` (not
                    // NaN-boxed INT32_TAG) — otherwise downstream comparisons
                    // like `idx < url.length` fail because NaN-boxed values
                    // are NaN and any comparison with NaN returns false. The
                    // typed string-method path in `lower_string_method.rs`
                    // uses `sitofp` (signed-int-to-float) for the same reason.
                    // Boolean-returning methods stay as TAG_TRUE/FALSE since
                    // codegen's `js_is_truthy` and explicit `=== true/false`
                    // checks both unbox these tags correctly (and Node's
                    // `Array.prototype.includes` etc. on plain values
                    // already use this representation).
                    if needle.is_null() {
                        // Match Node: `s.indexOf(undefined)` → -1, includes → false.
                        return match method_name {
                            "indexOf" | "lastIndexOf" => -1.0_f64,
                            "includes" | "startsWith" | "endsWith" => {
                                f64::from_bits(JSValue::bool(false).bits())
                            }
                            "concat" => f64::from_bits(
                                JSValue::string_ptr(s_ptr as *mut crate::StringHeader).bits(),
                            ),
                            _ => f64::from_bits(JSValue::undefined().bits()),
                        };
                    }
                    return match method_name {
                        "indexOf" => {
                            let from = if args_len >= 2 { arg_i32(1) } else { 0 };
                            crate::string::js_string_index_of_from(s_ptr, needle, from) as f64
                        }
                        "includes" => {
                            let from = if args_len >= 2 { arg_i32(1) } else { 0 };
                            let i = crate::string::js_string_index_of_from(s_ptr, needle, from);
                            f64::from_bits(JSValue::bool(i >= 0).bits())
                        }
                        "lastIndexOf" => {
                            crate::string::js_string_last_index_of(s_ptr, needle) as f64
                        }
                        "startsWith" => {
                            let at = if args_len >= 2 { arg_i32(1) } else { 0 };
                            let b = crate::string::js_string_starts_with_at(s_ptr, needle, at);
                            f64::from_bits(JSValue::bool(b != 0).bits())
                        }
                        "endsWith" => {
                            let len_i32 = unsafe { (*s_ptr).byte_len } as i32;
                            let at = if args_len >= 2 { arg_i32(1) } else { len_i32 };
                            let b = crate::string::js_string_ends_with_at(s_ptr, needle, at);
                            f64::from_bits(JSValue::bool(b != 0).bits())
                        }
                        "concat" => {
                            let r = crate::string::js_string_concat(s_ptr, needle);
                            f64::from_bits(JSValue::string_ptr(r).bits())
                        }
                        _ => f64::from_bits(JSValue::undefined().bits()),
                    };
                }
                "toUpperCase" => {
                    let r = crate::string::js_string_to_upper_case(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "toLowerCase" => {
                    let r = crate::string::js_string_to_lower_case(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trim" => {
                    let r = crate::string::js_string_trim(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trimStart" | "trimLeft" => {
                    let r = crate::string::js_string_trim_start(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trimEnd" | "trimRight" => {
                    let r = crate::string::js_string_trim_end(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "substring" => {
                    let len_i32 = unsafe { (*s_ptr).byte_len } as i32;
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let end = if args_len >= 2 { arg_i32(1) } else { len_i32 };
                    let r = crate::string::js_string_substring(s_ptr, start, end);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "repeat" => {
                    let n = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let r = crate::string::js_string_repeat(s_ptr, n);
                    if r.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "split" => {
                    let sep = if args_len >= 1 && !args_ptr.is_null() {
                        let v = unsafe { *args_ptr };
                        crate::value::js_get_string_pointer_unified(v) as *const crate::StringHeader
                    } else {
                        std::ptr::null()
                    };
                    // Issue #567: optional 2nd arg `limit`.
                    let limit = if args_len >= 2 && !args_ptr.is_null() {
                        let v = unsafe { *args_ptr.add(1) };
                        let jsv = JSValue::from_bits(v.to_bits());
                        if jsv.is_undefined() || jsv.is_null() {
                            -1
                        } else {
                            let n = crate::builtins::js_number_coerce(v);
                            if n.is_nan() || n < 0.0 {
                                0
                            } else if n > i32::MAX as f64 {
                                i32::MAX
                            } else {
                                n as i32
                            }
                        }
                    } else {
                        -1
                    };
                    let arr = crate::string::js_string_split_n(s_ptr, sep, limit);
                    return f64::from_bits(JSValue::pointer(arr as *mut u8).bits());
                }
                "replace" | "replaceAll" => {
                    // Two-arg shape: (pattern, replacement). pattern can be a
                    // string OR a RegExp; replacement is a string. Function
                    // replacements aren't supported here yet — they need
                    // closure dispatch and aren't on hono's hot path.
                    let pat_str = if args_len >= 1 && !args_ptr.is_null() {
                        let v = unsafe { *args_ptr };
                        crate::value::js_get_string_pointer_unified(v) as *const crate::StringHeader
                    } else {
                        std::ptr::null()
                    };
                    let repl_str = if args_len >= 2 && !args_ptr.is_null() {
                        let v = unsafe { *args_ptr.add(1) };
                        crate::value::js_get_string_pointer_unified(v) as *const crate::StringHeader
                    } else {
                        std::ptr::null()
                    };
                    // Detect RegExp pattern: NaN-boxed pointer to a RegExpHeader.
                    if args_len >= 1 && !args_ptr.is_null() {
                        let v = unsafe { *args_ptr };
                        let jsv = JSValue::from_bits(v.to_bits());
                        if jsv.is_pointer() {
                            // Probe whether the pointer is a RegExpHeader by
                            // checking the GC type tag the regex helpers
                            // already validate; if it's not, the regex helper
                            // returns the original string unchanged. The
                            // global flag on the RegExp determines whether
                            // it replaces all or just the first.
                            let regex_ptr = jsv.as_pointer::<crate::regex::RegExpHeader>();
                            // Heuristic: a non-null POINTER_TAG that's not a
                            // string/array (those have different GC type tags)
                            // is treated as a RegExp here. The runtime helper
                            // already validates internally and falls back
                            // safely on mismatch.
                            if !regex_ptr.is_null() {
                                let r = crate::regex::js_string_replace_regex(
                                    s_ptr, regex_ptr, repl_str,
                                );
                                return f64::from_bits(JSValue::string_ptr(r).bits());
                            }
                        }
                    }
                    let r = if method_name == "replaceAll" {
                        crate::regex::js_string_replace_all_string(s_ptr, pat_str, repl_str)
                    } else {
                        crate::regex::js_string_replace_string(s_ptr, pat_str, repl_str)
                    };
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                _ => {} // not a handled string method — fall through to TypeError catch-all
            }
        }
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
            eprintln!(
                "[NULL_PTR_METHOD_CALL] js_native_call_method: null pointer object for method '{}'",
                method_name
            );
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
            let arr_gc_hdr =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
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
                    // Issue #493 followup: dispatch `forEach` on any-typed
                    // arrays the same way as map/filter. Codegen's HIR-level
                    // `Expr::ArrayForEach` only fires for receivers it can
                    // statically prove are arrays — rest params and other
                    // dynamically-typed receivers fall through to the runtime
                    // dispatch tower, where this arm now intercepts. Without
                    // it, `args.forEach(cb)` (where `args` is a closure rest
                    // param threaded across module boundaries) silently
                    // no-op'd, breaking hono's route-registration loop and
                    // any other code that does the same arrow-rest-forEach
                    // pattern.
                    "forEach" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        crate::array::js_array_forEach(arr, cb_ptr);
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    }
                    // Issue #291: defensive `slice` arm for arrays that
                    // reach the generic dispatch tower (e.g. when the
                    // receiver is `Expr::Logical` / `Expr::Conditional` /
                    // `any`-typed `Expr::Call` and codegen's
                    // `is_array_expr` returned false). Without this arm
                    // the fallthrough returned the static `NULL_OBJECT_BYTES`
                    // sentinel and the next chained operation segfaulted.
                    "slice" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let arg_i32 = |i: usize| -> i32 {
                            if i < args_len && !args_ptr.is_null() {
                                let v = *args_ptr.add(i);
                                if v.is_nan() || v.is_infinite() {
                                    0
                                } else {
                                    v as i32
                                }
                            } else {
                                0
                            }
                        };
                        let len = crate::array::js_array_length(arr) as i32;
                        let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                        let end = if args_len >= 2 { arg_i32(1) } else { len };
                        let result = crate::array::js_array_slice(arr, start, end);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    // Issue #515 followup: defensive `with` arm for arrays that
                    // reach the generic dispatch tower because the HIR fold
                    // bailed (untyped receiver, chained call returning Array,
                    // etc.). Without this arm, tightening the HIR fold to
                    // ignore unknown-type receivers would silently break
                    // legitimate `(arr: any).with(idx, val)` callers.
                    "with" if args_len >= 2 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let index = *args_ptr;
                        let value = *args_ptr.add(1);
                        let result = crate::array::js_array_with(arr, index, value);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    // Issue #546 followup: defensive `some` / `every` /
                    // `find` / `findIndex` / `findLast` / `findLastIndex`
                    // arms for any-typed receivers that escape the HIR
                    // fast-path. The `is_class_overlapping_method` guard
                    // (expr_call.rs ~2621) bails on Any-typed locals — so
                    // a destructured `const { arr } = entry; arr.some(cb)`
                    // (where `arr` lost its `EntityId<any>[]` type through
                    // destructuring) silently fell through to the object
                    // field-scan and returned the array itself, producing
                    // `typeof = object` instead of a boolean. The hooks
                    // module in @codehz/ecs hits this exact pattern in
                    // `triggerMultiComponentHooks`, so on_set never fired.
                    "some" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_some(arr, cb_ptr);
                    }
                    "every" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_every(arr, cb_ptr);
                    }
                    "find" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_find(arr, cb_ptr);
                    }
                    "findIndex" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let idx = crate::array::js_array_findIndex(arr, cb_ptr);
                        return idx as f64;
                    }
                    "findLast" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_find_last(arr, cb_ptr);
                    }
                    "findLastIndex" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let idx = crate::array::js_array_find_last_index(arr, cb_ptr);
                        return idx as f64;
                    }
                    // Issue #587: `str.split(sep).map(fn).sort()` returned ""
                    // because chained `.sort()` falls through HIR's array-fold
                    // (the `"sort" if !args.is_empty()` arm in expr_call.rs
                    // requires a comparator) and lands here. Without these
                    // arms the very-end fallthrough returns NULL_OBJECT_BYTES,
                    // which JSON.stringify renders as "". The s3-lite-client
                    // SigV4 canonical-query-string builder
                    // (`.split("&").map(...).sort().join("&")`) was the
                    // load-bearing user impact. Same gap for `.reverse()` —
                    // tracked by issue #587's regressions list. Adding
                    // `reduce` / `reduceRight` / `flat` / `flatMap` / `concat`
                    // / `indexOf` / `includes` / `at` / `fill` while we're
                    // here defensively, since they have the same shape and
                    // share the HIR-fold escape risk for chained-call
                    // receivers.
                    "sort" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let result = if args_len >= 1 && !args_ptr.is_null() {
                            let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                            let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                            crate::array::js_array_sort_with_comparator(arr, cb_ptr)
                        } else {
                            crate::array::js_array_sort_default(arr)
                        };
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "reverse" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let result = crate::array::js_array_reverse(arr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "reduce" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let (has_init, init) = if args_len >= 2 {
                            (1i32, *args_ptr.add(1))
                        } else {
                            (0i32, f64::from_bits(crate::value::TAG_UNDEFINED))
                        };
                        return crate::array::js_array_reduce(arr, cb_ptr, has_init, init);
                    }
                    "reduceRight" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let (has_init, init) = if args_len >= 2 {
                            (1i32, *args_ptr.add(1))
                        } else {
                            (0i32, f64::from_bits(crate::value::TAG_UNDEFINED))
                        };
                        return crate::array::js_array_reduce_right(arr, cb_ptr, has_init, init);
                    }
                    "flat" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let result = crate::array::js_array_flat(arr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "flatMap" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_flatMap(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "concat" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let other_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let other_ptr = other_bits as *mut crate::array::ArrayHeader;
                        let result = crate::array::js_array_concat(arr, other_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "indexOf" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        return crate::array::js_array_indexOf_jsvalue(arr, value) as f64;
                    }
                    "includes" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        let r = crate::array::js_array_includes_jsvalue(arr, value);
                        return f64::from_bits(JSValue::bool(r != 0).bits());
                    }
                    "at" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        return crate::array::js_array_at(arr, *args_ptr);
                    }
                    "fill" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let result = crate::array::js_array_fill(arr, *args_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "join" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let sep_ptr = if args_len >= 1 && !args_ptr.is_null() {
                            let bits = (*args_ptr).to_bits();
                            let tag = bits >> 48;
                            if tag == 0x7FFF || tag == 0x7FFE {
                                // STRING_TAG or SHORT_STRING tag
                                (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader
                            } else {
                                std::ptr::null()
                            }
                        } else {
                            std::ptr::null()
                        };
                        let s = crate::array::js_array_join(arr, sep_ptr);
                        return f64::from_bits(JSValue::string_ptr(s).bits());
                    }
                    _ => {} // not a handled array method — fall through to object dispatch
                }
            }
        }

        // Check if this is a native module namespace object (e.g., fs, os, path)
        let obj = jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader to confirm this is actually an object before reading class_id
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
                if !is_valid_obj_ptr(obj as *const u8) {
                    return 0.0;
                }
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
                                    //
                                    // Issue #519: bind `this` to the receiver
                                    // for the duration of the call. Non-arrow
                                    // function bodies read `this` from
                                    // IMPLICIT_THIS (codegen Expr::This
                                    // fallback when this_stack is empty);
                                    // without this save/set/restore, the
                                    // body sees `this = undefined` and any
                                    // `this.foo()` call falls through to the
                                    // issue #510 catch-all "(undefined).foo
                                    // is not a function" TypeError. Hono's
                                    // RegExpRouter.match (imported function
                                    // assigned as a class field) hit this.
                                    let recv_bits = jsval.bits();
                                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                                    let result = crate::closure::js_native_call_value(
                                        f64::from_bits(field_val.bits()),
                                        args_ptr,
                                        args_len,
                                    );
                                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                                    return result;
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup for class instances — fast path via per-callsite IC
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some((func_ptr, param_count)) =
                    vtable_ic_lookup(class_id, method_name_ptr as usize)
                {
                    let this_i64 = jsval.as_pointer::<u8>() as i64;
                    return call_vtable_method(func_ptr, this_i64, args_ptr, args_len, param_count);
                }
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        // Refs #420: walk the parent chain via the class
                        // registry. Per JS spec, `subInstance.method()` for
                        // a method defined on a parent dispatches to the
                        // parent's implementation — drizzle's
                        // `serial("id").primaryKey()` where primaryKey is on
                        // ColumnBuilder (grandparent) but the receiver is a
                        // PgSerialBuilder (grandchild). The codegen-side
                        // dispatch tower in `lower_call.rs` only registers
                        // classes the importing module knows about; for
                        // not-by-name-imported subclasses (return values of
                        // imported functions) we depend on this runtime walk.
                        let mut cur_cid = class_id;
                        let mut depth = 0u32;
                        while depth < 32 {
                            if let Some(vtable) = reg.get(&cur_cid) {
                                if let Some(entry) = vtable.methods.get(method_name) {
                                    vtable_ic_insert(
                                        class_id,
                                        method_name_ptr as usize,
                                        entry.func_ptr,
                                        entry.param_count,
                                    );
                                    let this_i64 = jsval.as_pointer::<u8>() as i64;
                                    return call_vtable_method(
                                        entry.func_ptr,
                                        this_i64,
                                        args_ptr,
                                        args_len,
                                        entry.param_count,
                                    );
                                }
                            }
                            match get_parent_class_id(cur_cid) {
                                Some(pid) if pid != 0 => {
                                    cur_cid = pid;
                                    depth += 1;
                                }
                                _ => break,
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
                    "clear" => {
                        crate::map::js_map_clear(map);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    "size" => crate::map::js_map_size(map) as f64,
                    "entries" => f64::from_bits(
                        JSValue::pointer(crate::map::js_map_entries(map) as *mut u8).bits(),
                    ),
                    "keys" => f64::from_bits(
                        JSValue::pointer(crate::map::js_map_keys(map) as *mut u8).bits(),
                    ),
                    "values" => f64::from_bits(
                        JSValue::pointer(crate::map::js_map_values(map) as *mut u8).bits(),
                    ),
                    "forEach" if !args.is_empty() => {
                        crate::map::js_map_foreach(map, args[0]);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
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
                    "clear" => {
                        crate::set::js_set_clear(set);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
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
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            // Check for native module namespace
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
                if !is_valid_obj_ptr(obj as *const u8) {
                    return 0.0;
                }
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

            // Vtable lookup — fast path via per-callsite IC
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some((func_ptr, param_count)) =
                    vtable_ic_lookup(class_id, method_name_ptr as usize)
                {
                    let this_i64 = raw_bits as i64;
                    return call_vtable_method(func_ptr, this_i64, args_ptr, args_len, param_count);
                }
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        // Refs #420: parent-chain walk (mirror of the path
                        // above for raw pointer instances).
                        let mut cur_cid = class_id;
                        let mut depth = 0u32;
                        while depth < 32 {
                            if let Some(vtable) = reg.get(&cur_cid) {
                                if let Some(entry) = vtable.methods.get(method_name) {
                                    vtable_ic_insert(
                                        class_id,
                                        method_name_ptr as usize,
                                        entry.func_ptr,
                                        entry.param_count,
                                    );
                                    let this_i64 = raw_bits as i64;
                                    return call_vtable_method(
                                        entry.func_ptr,
                                        this_i64,
                                        args_ptr,
                                        args_len,
                                        entry.param_count,
                                    );
                                }
                            }
                            match get_parent_class_id(cur_cid) {
                                Some(pid) if pid != 0 => {
                                    cur_cid = pid;
                                    depth += 1;
                                }
                                _ => break,
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
            let arr =
                jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            if args_len > 0 && !args_ptr.is_null() {
                let val = *args_ptr;
                crate::array::js_array_push_f64(arr, val);
            }
            return crate::array::js_array_length(arr) as f64;
        }
        "pop" if jsval.is_pointer() => {
            let arr =
                jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
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
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;

        // Issue #618: closure receivers (GC_TYPE_CLOSURE=4 OR
        // CLOSURE_MAGIC-marked GC_TYPE_OBJECT slot) — look up the method
        // name in the closure's dynamic-prop side-table. If a callable
        // closure is stored there (via the IIFE-namespace pattern
        // `((sql2) => { sql2.identifier = ...; })(sql)`), dispatch
        // through `js_native_call_value`. Pre-fix this path returned the
        // NULL_OBJECT_BYTES stub for any method call on a closure, so
        // the call result was an empty object stub instead of the
        // dynamic-prop closure's return value.
        let is_closure = gc_type == crate::gc::GC_TYPE_CLOSURE
            || *((obj as *const u8).add(12) as *const u32) == crate::closure::CLOSURE_MAGIC;
        if is_closure {
            let dyn_val = crate::closure::closure_get_dynamic_prop(obj as usize, method_name);
            if dyn_val.to_bits() != crate::value::TAG_UNDEFINED {
                let recv_bits = jsval.bits();
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                let result = crate::closure::js_native_call_value(dyn_val, args_ptr, args_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return result;
            }
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }

        if gc_type != crate::gc::GC_TYPE_OBJECT {
            // Only accept object_type == 1 (OBJECT_TYPE_REGULAR)
            let object_type = (*obj).object_type;
            // Closes #645: when a method falls through every dispatcher
            // and returns NULL_OBJECT_BYTES (e.g. drizzle's
            // `this.client.prepare(...)` where `this.client` resolved to
            // a heap-object that doesn't dispatch any method named
            // "prepare"), the result gets stored as `this.stmt` and the
            // chained `this.stmt.raw().all(...)` re-enters this function
            // with `obj` pointing at NULL_OBJECT_BYTES — a static stub in
            // the binary's data segment, NOT the macOS userspace heap
            // range that `is_valid_obj_ptr` requires (HEAP_MIN ==
            // 0x200_0000_0000). Pre-fix this returned a literal `0.0`,
            // which the codegen interprets as the IEEE-754 number zero,
            // so the next chained method saw a number receiver and
            // threw `(number).<method> is not a function`. Returning the
            // null-object stub matches every other catch-all in this
            // function and keeps `typeof === "object"` so chained
            // operations propagate consistently instead of mid-chain
            // numeric arithmetic on bit patterns. Truly garbage pointers
            // benefit too — chained calls hit a stable null stub instead
            // of mysterious numeric values.
            if !is_valid_obj_ptr(obj as *const u8) {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
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
            // Compare method_name bytes directly against each stored key
            // instead of allocating a transient StringHeader via
            // js_string_from_bytes — that allocation showed up as ~10% of
            // perf-comprehensive's hot-path samples (one alloc per
            // dynamic-dispatch method call × N keys-array lookups).
            let method_bytes = method_name.as_bytes();
            for i in 0..key_count {
                let key_val = crate::array::js_array_get(keys, i as u32);
                if key_val.is_string() {
                    let stored_key = key_val.as_string_ptr();
                    let matches = if !crate::string::is_valid_string_ptr(stored_key) {
                        false
                    } else {
                        let blen = (*stored_key).byte_len as usize;
                        if blen != method_bytes.len() {
                            false
                        } else {
                            let stored_data = crate::string::string_data(stored_key);
                            let stored = std::slice::from_raw_parts(stored_data, blen);
                            stored == method_bytes
                        }
                    };
                    if matches {
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
                                entry.func_ptr,
                                this_i64,
                                args_ptr,
                                args_len,
                                entry.param_count,
                            );
                        }
                    }
                }
            }
        }
    }

    // Issue #510: throw `TypeError: <expr> is not a function` when
    // the receiver is a non-string primitive (number / int32 / bool /
    // bigint) and dispatch above didn't fire. Node auto-boxes
    // primitives via Number/Boolean/BigInt prototypes; when the
    // prototype lookup yields undefined, the call site throws.
    // Without primitive auto-boxing, Perry must surface the same
    // diagnostic at dispatch time — silently returning the
    // null-object sentinel (the historical fall-through below) lets
    // typo'd method calls run as no-ops, masking real bugs.
    //
    // Strings don't reach this catch-all in the typical case —
    // codegen's `lower_string_method` intercepts string-typed
    // receivers and throws there directly (matching ABI). The string
    // arm is left in here for the rare path where a string flows
    // through dynamic dispatch (e.g. raw NaN-boxed receiver from a
    // Map.get() result the user typed as `any`).
    //
    // Real-object receivers keep the `NULL_OBJECT_BYTES`
    // fall-through. Many existing call paths use this dispatcher as
    // a generic shortcut and rely on the silent null-object return
    // for unknown methods; tightening that is tracked separately.
    //
    // Issue #511: `undefined` / `null` receivers must throw a node-shaped
    // `TypeError: Cannot read properties of <kind> (reading '<method>')`
    // and exit 1. Codegen's `Expr::PropertyGet` lowering already throws
    // on the bare property read (`obj.foo`, issue #462), but the
    // `Call { callee: PropertyGet }` shortcut in `lower_call.rs`
    // routes `obj.foo()` straight to `js_native_call_method` without
    // re-evaluating the receiver through PropertyGet — so the codegen
    // gate never fires for the call form. Without this arm, `x.foo()`
    // on `undefined` silently returned `NULL_OBJECT_BYTES` and the
    // process exited 0, breaking CI gates that rely on non-zero exit
    // for uncaught errors. Earlier toString/bind/push/pop/length match
    // arms intentionally short-circuit before this point so existing
    // Perry code that calls those on `undefined`/`null` keeps working
    // (Perry-ism — Node throws there too, but tightening that breaks
    // unrelated callers; the typo case below is what we want to surface).
    if jsval.is_undefined() || jsval.is_null() {
        let is_null_u32 = if jsval.is_null() { 1u32 } else { 0u32 };
        crate::error::js_throw_type_error_property_access(
            is_null_u32,
            method_name.as_ptr(),
            method_name.len(),
        );
    }
    // Issue #687: INT32-NaN-boxed value whose payload is a registered
    // class id — i.e. a `ClassRef` produced by `Expr::ClassRef` codegen.
    // Effect's `Schema.NonNegative.pipe(int()).annotations({...})` chains
    // produce a ClassRef out of the first `.pipe()` (via the codegen-side
    // defensive no-op in `lower_call.rs::Expr::ClassRef`) and the chained
    // `.annotations(...)` reaches us with that ClassRef as the receiver.
    // Treat it as a chainable no-op: return the receiver so further
    // `.method(...)` calls stay typed-class-shaped during module init.
    // The result isn't semantically equivalent to Effect's transformed
    // schema, but it advances Schema.ts__init past sites that previously
    // threw `(number).<method> is not a function`. Paired with the
    // codegen-side fix in `lower_call.rs` for the simpler
    // `ClassRef.method()` shape.
    if jsval.is_int32() {
        let payload = jsval.as_int32() as u32;
        if payload != 0 {
            let guard = REGISTERED_CLASS_IDS.read().unwrap();
            if let Some(set) = guard.as_ref() {
                if set.contains(&payload) {
                    return object;
                }
            }
        }
    }
    let primitive_kind: Option<&'static str> = if jsval.is_any_string() {
        Some("string")
    } else if jsval.is_int32() || jsval.is_number() {
        Some("number")
    } else if jsval.is_bool() {
        Some("boolean")
    } else if jsval.is_bigint() {
        Some("bigint")
    } else {
        None
    };
    if let Some(kind) = primitive_kind {
        crate::error::js_throw_type_error_not_a_function(
            kind.as_ptr(),
            kind.len(),
            method_name.as_ptr(),
            method_name.len(),
        );
    }

    // Issue #648: real-object receivers also throw when the method
    // doesn't exist anywhere in the dispatch chain (no field-stored
    // closure, no class vtable entry, no prototype walk hit). Pre-fix
    // this catch-all returned `NULL_OBJECT_BYTES` so codegen wouldn't
    // SIGSEGV when it NaN-unboxed the result and dereferenced it as a
    // pointer — but that masked typo'd method calls as silent no-ops
    // and was the single largest source of cascading parity failures
    // (`test_parity_timers` hung waiting on `timers.setTimeout` which
    // silently no-op'd; many other parity tests truncated mid-script
    // when an unimplemented binding's method silently no-op'd inside
    // the surrounding async path). Now we throw the standard `<prop>
    // is not a function` TypeError, which `try`/`catch` catches (per
    // #596's exception-routing fix).
    crate::error::js_throw_type_error_not_a_function(
        std::ptr::null(),
        0,
        method_name.as_ptr(),
        method_name.len(),
    );
}

/// Dispatch a Buffer / Uint8Array instance method call. Receiver address
/// is the raw heap pointer (already stripped of NaN-box tags). Routes
/// the Node-style numeric read/write/search/swap method family through
/// `crate::buffer` helpers; unknown methods return undefined.
/// Issue #639 followup: list of method names recognized by `dispatch_buffer_method`.
/// Used by `js_object_get_field_by_name`'s Buffer arm to decide whether a
/// non-length property read should synthesize a bound-method closure (so
/// duck-type tests like `typeof v.readUInt8 === "function"` pass and a
/// subsequent call dispatches through `js_native_call_method`).
///
/// Keep this list aligned with the `match method_name` arms below — every
/// arm there should be reachable from a method-as-value read.
pub fn is_buffer_method_name(name: &str) -> bool {
    matches!(
        name,
        "toString"
            | "slice"
            | "subarray"
            | "copy"
            | "write"
            | "fill"
            | "equals"
            | "compare"
            | "indexOf"
            | "lastIndexOf"
            | "includes"
            | "at"
            | "swap16"
            | "swap32"
            | "swap64"
            | "readUInt8"
            | "readUint8"
            | "readInt8"
            | "readUInt16BE"
            | "readUint16BE"
            | "readUInt16LE"
            | "readUint16LE"
            | "readInt16BE"
            | "readInt16LE"
            | "readUInt32BE"
            | "readUint32BE"
            | "readUInt32LE"
            | "readUint32LE"
            | "readInt32BE"
            | "readInt32LE"
            | "readFloatBE"
            | "readFloatLE"
            | "readDoubleBE"
            | "readDoubleLE"
            | "readBigInt64BE"
            | "readBigInt64LE"
            | "readBigUInt64BE"
            | "readBigUint64BE"
            | "readBigUInt64LE"
            | "readBigUint64LE"
            | "readUIntBE"
            | "readUintBE"
            | "readUIntLE"
            | "readUintLE"
            | "readIntBE"
            | "readIntLE"
            | "writeUInt8"
            | "writeUint8"
            | "writeInt8"
            | "writeUInt16BE"
            | "writeUint16BE"
            | "writeUInt16LE"
            | "writeUint16LE"
            | "writeInt16BE"
            | "writeInt16LE"
            | "writeUInt32BE"
            | "writeUint32BE"
            | "writeUInt32LE"
            | "writeUint32LE"
            | "writeInt32BE"
            | "writeInt32LE"
            | "writeFloatBE"
            | "writeFloatLE"
            | "writeDoubleBE"
            | "writeDoubleLE"
            | "writeBigInt64BE"
            | "writeBigInt64LE"
            | "writeBigUInt64BE"
            | "writeBigUint64BE"
            | "writeBigUInt64LE"
            | "writeBigUint64LE"
            | "writeUIntBE"
            | "writeUintBE"
            | "writeUIntLE"
            | "writeUintLE"
            | "writeIntBE"
            | "writeIntLE"
    )
}

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
        if i < args.len() {
            args[i] as i32
        } else {
            0
        }
    };
    let arg_or_zero = |i: usize| -> f64 {
        if i < args.len() {
            args[i]
        } else {
            0.0
        }
    };
    let i32_bool = |b: i32| f64::from_bits(JSValue::bool(b != 0).bits());
    let i32_num = |n: i32| n as f64;

    match method_name {
        "length" => crate::buffer::js_buffer_length(buf_ptr) as f64,
        "toString" => {
            let enc = if !args.is_empty() {
                crate::buffer::js_encoding_tag_from_value(args[0])
            } else {
                0
            };
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
            } else {
                dst_bits
            };
            let dst_ptr = dst_addr as *mut crate::buffer::BufferHeader;
            let target_start = if args.len() >= 2 { arg_i32(1) } else { 0 };
            let source_start = if args.len() >= 3 { arg_i32(2) } else { 0 };
            let source_end = if args.len() >= 4 {
                arg_i32(3)
            } else {
                (*buf_ptr).length as i32
            };
            crate::buffer::js_buffer_copy(buf_ptr, dst_ptr, target_start, source_start, source_end)
                as f64
        }
        // `buf.write(string, offset?, length?, encoding?)` — writes the
        // utf8/hex/base64 encoding of `string` into `buf` at `offset`.
        // Returns the number of bytes written.
        "write" if !args.is_empty() => {
            let str_bits = args[0].to_bits();
            let str_addr = if (str_bits >> 48) >= 0x7FF8 {
                str_bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                str_bits
            };
            let str_ptr = str_addr as *const crate::string::StringHeader;
            let offset = if args.len() >= 2 { arg_i32(1) } else { 0 };
            // Detect trailing encoding arg (string) vs length arg (number).
            // Common forms: write(str), write(str, offset), write(str, offset, enc),
            // write(str, offset, length, enc).
            let enc = if args.len() >= 4 {
                crate::buffer::js_encoding_tag_from_value(args[3])
            } else if args.len() >= 3 {
                crate::buffer::js_encoding_tag_from_value(args[2])
            } else {
                0
            };
            crate::buffer::js_buffer_write(buf_ptr, str_ptr, offset, enc) as f64
        }
        "fill" => {
            let result = crate::buffer::js_buffer_fill(buf_ptr, arg_i32(0));
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "equals" => {
            if args.is_empty() {
                return i32_bool(0);
            }
            let other_bits = args[0].to_bits();
            let other_addr = if (other_bits >> 48) >= 0x7FF8 {
                other_bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                other_bits
            };
            let other = other_addr as *const crate::buffer::BufferHeader;
            i32_bool(crate::buffer::js_buffer_equals(buf_ptr, other))
        }
        "compare" => {
            if args.is_empty() {
                return 0.0;
            }
            let other_bits = args[0].to_bits();
            let other_addr = if (other_bits >> 48) >= 0x7FF8 {
                other_bits & 0x0000_FFFF_FFFF_FFFF
            } else {
                other_bits
            };
            let other = other_addr as *const crate::buffer::BufferHeader;
            i32_num(crate::buffer::js_buffer_compare(buf_ptr, other))
        }
        "indexOf" => i32_num(crate::buffer::js_buffer_index_of(
            buf_f64,
            arg_or_zero(0),
            arg_i32(1),
        )),
        "lastIndexOf" => i32_num(crate::buffer::js_buffer_index_of(
            buf_f64,
            arg_or_zero(0),
            arg_i32(1),
        )),
        "includes" => i32_bool(crate::buffer::js_buffer_includes(
            buf_f64,
            arg_or_zero(0),
            arg_i32(1),
        )),
        // `buf.at(i)` — supports negative indices like Array.prototype.at.
        "at" => {
            let len = (*buf_ptr).length as i32;
            let mut idx = arg_i32(0);
            if idx < 0 {
                idx += len;
            }
            if idx < 0 || idx >= len {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            crate::buffer::js_buffer_get(buf_ptr, idx) as f64
        }
        "swap16" => {
            crate::buffer::js_buffer_swap16(buf_f64);
            buf_f64
        }
        "swap32" => {
            crate::buffer::js_buffer_swap32(buf_f64);
            buf_f64
        }
        "swap64" => {
            crate::buffer::js_buffer_swap64(buf_f64);
            buf_f64
        }
        // Synthetic method emitted by lower.rs for `crypto.getRandomValues(buf)`.
        "$$cryptoFillRandom" => crate::buffer::js_buffer_fill_random(buf_f64),
        "readUInt8" | "readUint8" => crate::buffer::js_buffer_read_uint8(buf_f64, arg_i32(0)),
        "readInt8" => crate::buffer::js_buffer_read_int8(buf_f64, arg_i32(0)),
        "readUInt16BE" | "readUint16BE" => {
            crate::buffer::js_buffer_read_uint16_be(buf_f64, arg_i32(0))
        }
        "readUInt16LE" | "readUint16LE" => {
            crate::buffer::js_buffer_read_uint16_le(buf_f64, arg_i32(0))
        }
        "readInt16BE" => crate::buffer::js_buffer_read_int16_be(buf_f64, arg_i32(0)),
        "readInt16LE" => crate::buffer::js_buffer_read_int16_le(buf_f64, arg_i32(0)),
        "readUInt32BE" | "readUint32BE" => {
            crate::buffer::js_buffer_read_uint32_be(buf_f64, arg_i32(0))
        }
        "readUInt32LE" | "readUint32LE" => {
            crate::buffer::js_buffer_read_uint32_le(buf_f64, arg_i32(0))
        }
        "readInt32BE" => crate::buffer::js_buffer_read_int32_be(buf_f64, arg_i32(0)),
        "readInt32LE" => crate::buffer::js_buffer_read_int32_le(buf_f64, arg_i32(0)),
        "readFloatBE" => crate::buffer::js_buffer_read_float_be(buf_f64, arg_i32(0)),
        "readFloatLE" => crate::buffer::js_buffer_read_float_le(buf_f64, arg_i32(0)),
        "readDoubleBE" => crate::buffer::js_buffer_read_double_be(buf_f64, arg_i32(0)),
        "readDoubleLE" => crate::buffer::js_buffer_read_double_le(buf_f64, arg_i32(0)),
        "readBigInt64BE" => crate::buffer::js_buffer_read_bigint64_be(buf_f64, arg_i32(0)),
        "readBigInt64LE" => crate::buffer::js_buffer_read_bigint64_le(buf_f64, arg_i32(0)),
        "readBigUInt64BE" | "readBigUint64BE" => {
            crate::buffer::js_buffer_read_biguint64_be(buf_f64, arg_i32(0))
        }
        "readBigUInt64LE" | "readBigUint64LE" => {
            crate::buffer::js_buffer_read_biguint64_le(buf_f64, arg_i32(0))
        }
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
        // Variable byteLength forms (Node-spec: byteLength 1..=6).
        // ObjectId / BSON drivers rely on these for the 3-byte counter.
        "readUIntBE" | "readUintBE" => {
            crate::buffer::js_buffer_read_uint_be(buf_f64, arg_i32(0), arg_i32(1))
        }
        "readUIntLE" | "readUintLE" => {
            crate::buffer::js_buffer_read_uint_le(buf_f64, arg_i32(0), arg_i32(1))
        }
        "readIntBE" => crate::buffer::js_buffer_read_int_be(buf_f64, arg_i32(0), arg_i32(1)),
        "readIntLE" => crate::buffer::js_buffer_read_int_le(buf_f64, arg_i32(0), arg_i32(1)),
        "writeUIntBE" | "writeUintBE" => {
            crate::buffer::js_buffer_write_uint_be(buf_f64, arg_or_zero(0), arg_i32(1), arg_i32(2));
            (arg_i32(1) + arg_i32(2)) as f64
        }
        "writeUIntLE" | "writeUintLE" => {
            crate::buffer::js_buffer_write_uint_le(buf_f64, arg_or_zero(0), arg_i32(1), arg_i32(2));
            (arg_i32(1) + arg_i32(2)) as f64
        }
        "writeIntBE" => {
            crate::buffer::js_buffer_write_int_be(buf_f64, arg_or_zero(0), arg_i32(1), arg_i32(2));
            (arg_i32(1) + arg_i32(2)) as f64
        }
        "writeIntLE" => {
            crate::buffer::js_buffer_write_int_le(buf_f64, arg_or_zero(0), arg_i32(1), arg_i32(2));
            (arg_i32(1) + arg_i32(2)) as f64
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
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            f64::from_bits(JSValue::undefined().bits())
        }
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
    let str_to_f64 =
        |ptr: *mut crate::StringHeader| -> f64 { f64::from_bits(JSValue::string_ptr(ptr).bits()) };

    match (module_name, method_name) {
        // ── fs module (args are NaN-boxed f64, booleans return as i32→f64) ──
        ("fs", "existsSync") => bool_to_f64(crate::fs::js_fs_exists_sync(arg(0))),
        ("fs", "readFileSync") => str_to_f64(crate::fs::js_fs_read_file_sync(arg(0))),
        ("fs", "writeFileSync") => bool_to_f64(crate::fs::js_fs_write_file_sync(arg(0), arg(1))),
        ("fs", "appendFileSync") => bool_to_f64(crate::fs::js_fs_append_file_sync(arg(0), arg(1))),
        ("fs", "mkdirSync") => bool_to_f64(crate::fs::js_fs_mkdir_sync(arg(0))),
        ("fs", "unlinkSync") => bool_to_f64(crate::fs::js_fs_unlink_sync(arg(0))),
        ("fs", "readdirSync") => crate::fs::js_fs_readdir_sync(arg(0), arg(1)),
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
pub extern "C" fn js_create_native_module_namespace(
    module_name_ptr: *const u8,
    module_name_len: usize,
) -> f64 {
    // Create an object with one field to store the module name
    let obj = js_object_alloc(NATIVE_MODULE_CLASS_ID, 1);

    unsafe {
        // Create a string from the module name
        let module_name =
            crate::string::js_string_from_bytes(module_name_ptr, module_name_len as u32);

        // Store the module name in the first field
        js_object_set_field(obj, 0, JSValue::string_ptr(module_name));

        // Create a keys array with one key: "__module__"
        let keys_array = crate::array::js_array_alloc(1);
        let key_bytes = b"__module__";
        let key_str =
            crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        crate::array::js_array_push(keys_array, JSValue::string_ptr(key_str));
        js_object_set_keys(obj, keys_array);
    }

    // Return as NaN-boxed pointer
    crate::value::js_nanbox_pointer(obj as i64)
}

/// Issue #649: codegen entry for `PropertyGet { NativeModuleRef(name),
/// property }`. `NativeModuleRef` lowers to a literal `0.0` at the codegen
/// level, so the generic PropertyGet path can't find the namespace
/// object. This helper short-circuits to the constants dispatcher; for
/// the chained case (`fs.constants.F_OK`) the inner call returns a
/// sub-namespace ObjectHeader and the outer PropertyGet goes through
/// `js_object_get_field_by_name`'s NATIVE_MODULE_CLASS_ID arm.
#[no_mangle]
pub unsafe extern "C" fn js_native_module_property_by_name(
    module_name_ptr: *const u8,
    module_name_len: usize,
    property_name_ptr: *const u8,
    property_name_len: usize,
) -> f64 {
    let module_name =
        std::str::from_utf8(std::slice::from_raw_parts(module_name_ptr, module_name_len))
            .unwrap_or("");
    let property_name = std::str::from_utf8(std::slice::from_raw_parts(
        property_name_ptr,
        property_name_len,
    ))
    .unwrap_or("");
    if let Some(val) = get_native_module_constant(module_name, property_name, 0.0) {
        return val;
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
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
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(
            property_name_ptr,
            property_name_len,
        ))
    };

    // Extract module name from the namespace object's first field
    let module_name = unsafe { get_module_name_from_namespace(namespace_obj) };

    // Check for known constant properties first
    if let Some(val) =
        unsafe { get_native_module_constant(module_name, property_name, namespace_obj) }
    {
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

    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, namespace_obj);
    crate::closure::js_closure_set_capture_ptr(closure, 1, heap_name as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, property_name_len as i64);

    crate::value::js_nanbox_pointer(closure as i64)
}

/// Build a "bound method" closure for `obj.method` PropertyGet on a known class
/// instance. The captures (instance, method_name_ptr, method_name_len) drive
/// `dispatch_bound_method` (closure.rs), which calls `js_native_call_method`
/// — that resolves the method through `CLASS_VTABLE_REGISTRY` for any class
/// registered by `js_register_class_method` at module init.
///
/// Issue #446: previously a class method reference (`let f = obj.method`,
/// `typeof obj.method`, `arr.map(obj.method)`) silently lowered to the
/// generic property-bag lookup, which doesn't store prototype methods —
/// every such read returned `undefined`, so `typeof obj.method === "undefined"`
/// and a captured method ran no body when invoked.
///
/// Method-name pointer is expected to be stable for the closure's lifetime;
/// codegen emits it from the per-module `.str.N.bytes` rodata global.
#[no_mangle]
pub extern "C" fn js_class_method_bind(
    instance: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, instance);
    crate::closure::js_closure_set_capture_ptr(closure, 1, method_name_ptr as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 2, method_name_len as i64);
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
    _namespace_obj: f64,
) -> Option<f64> {
    let str_val = |s: &str| -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    };

    let o_nofollow: f64 = {
        #[cfg(target_os = "macos")]
        {
            0x0100 as f64
        }
        #[cfg(target_os = "linux")]
        {
            0x20000 as f64
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            0x0100 as f64
        }
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

    // Issue #649: `os.constants.signals.SIGINT`, `os.constants.errno.ENOENT`,
    // `os.constants.priority.PRIORITY_NORMAL`, `os.constants.dlopen.RTLD_LAZY`
    // are ubiquitous in Node ecosystem code. Pre-fix every read returned
    // undefined. Use `libc::*` on Unix for byte-identical parity with Node.
    let os_signal_const = |prop: &str| -> Option<f64> {
        #[cfg(unix)]
        {
            let v: Option<i32> = match prop {
                "SIGHUP" => Some(libc::SIGHUP),
                "SIGINT" => Some(libc::SIGINT),
                "SIGQUIT" => Some(libc::SIGQUIT),
                "SIGILL" => Some(libc::SIGILL),
                "SIGTRAP" => Some(libc::SIGTRAP),
                "SIGABRT" => Some(libc::SIGABRT),
                "SIGIOT" => Some(libc::SIGABRT),
                "SIGBUS" => Some(libc::SIGBUS),
                "SIGFPE" => Some(libc::SIGFPE),
                "SIGKILL" => Some(libc::SIGKILL),
                "SIGUSR1" => Some(libc::SIGUSR1),
                "SIGSEGV" => Some(libc::SIGSEGV),
                "SIGUSR2" => Some(libc::SIGUSR2),
                "SIGPIPE" => Some(libc::SIGPIPE),
                "SIGALRM" => Some(libc::SIGALRM),
                "SIGTERM" => Some(libc::SIGTERM),
                "SIGCHLD" => Some(libc::SIGCHLD),
                "SIGCONT" => Some(libc::SIGCONT),
                "SIGSTOP" => Some(libc::SIGSTOP),
                "SIGTSTP" => Some(libc::SIGTSTP),
                "SIGTTIN" => Some(libc::SIGTTIN),
                "SIGTTOU" => Some(libc::SIGTTOU),
                "SIGURG" => Some(libc::SIGURG),
                "SIGXCPU" => Some(libc::SIGXCPU),
                "SIGXFSZ" => Some(libc::SIGXFSZ),
                "SIGVTALRM" => Some(libc::SIGVTALRM),
                "SIGPROF" => Some(libc::SIGPROF),
                "SIGWINCH" => Some(libc::SIGWINCH),
                "SIGIO" => Some(libc::SIGIO),
                "SIGSYS" => Some(libc::SIGSYS),
                #[cfg(target_os = "macos")]
                "SIGINFO" => Some(29i32),
                _ => None,
            };
            v.map(|x| x as f64)
        }
        #[cfg(not(unix))]
        {
            match prop {
                "SIGHUP" => Some(1.0),
                "SIGINT" => Some(2.0),
                "SIGILL" => Some(4.0),
                "SIGABRT" => Some(22.0),
                "SIGFPE" => Some(8.0),
                "SIGKILL" => Some(9.0),
                "SIGSEGV" => Some(11.0),
                "SIGTERM" => Some(15.0),
                "SIGBREAK" => Some(21.0),
                _ => None,
            }
        }
    };

    let os_errno_const = |prop: &str| -> Option<f64> {
        #[cfg(unix)]
        {
            let v: Option<i32> = match prop {
                "E2BIG" => Some(libc::E2BIG),
                "EACCES" => Some(libc::EACCES),
                "EADDRINUSE" => Some(libc::EADDRINUSE),
                "EADDRNOTAVAIL" => Some(libc::EADDRNOTAVAIL),
                "EAFNOSUPPORT" => Some(libc::EAFNOSUPPORT),
                "EAGAIN" => Some(libc::EAGAIN),
                "EALREADY" => Some(libc::EALREADY),
                "EBADF" => Some(libc::EBADF),
                "EBADMSG" => Some(libc::EBADMSG),
                "EBUSY" => Some(libc::EBUSY),
                "ECANCELED" => Some(libc::ECANCELED),
                "ECHILD" => Some(libc::ECHILD),
                "ECONNABORTED" => Some(libc::ECONNABORTED),
                "ECONNREFUSED" => Some(libc::ECONNREFUSED),
                "ECONNRESET" => Some(libc::ECONNRESET),
                "EDEADLK" => Some(libc::EDEADLK),
                "EDESTADDRREQ" => Some(libc::EDESTADDRREQ),
                "EDOM" => Some(libc::EDOM),
                "EDQUOT" => Some(libc::EDQUOT),
                "EEXIST" => Some(libc::EEXIST),
                "EFAULT" => Some(libc::EFAULT),
                "EFBIG" => Some(libc::EFBIG),
                "EHOSTUNREACH" => Some(libc::EHOSTUNREACH),
                "EIDRM" => Some(libc::EIDRM),
                "EILSEQ" => Some(libc::EILSEQ),
                "EINPROGRESS" => Some(libc::EINPROGRESS),
                "EINTR" => Some(libc::EINTR),
                "EINVAL" => Some(libc::EINVAL),
                "EIO" => Some(libc::EIO),
                "EISCONN" => Some(libc::EISCONN),
                "EISDIR" => Some(libc::EISDIR),
                "ELOOP" => Some(libc::ELOOP),
                "EMFILE" => Some(libc::EMFILE),
                "EMLINK" => Some(libc::EMLINK),
                "EMSGSIZE" => Some(libc::EMSGSIZE),
                "EMULTIHOP" => Some(libc::EMULTIHOP),
                "ENAMETOOLONG" => Some(libc::ENAMETOOLONG),
                "ENETDOWN" => Some(libc::ENETDOWN),
                "ENETRESET" => Some(libc::ENETRESET),
                "ENETUNREACH" => Some(libc::ENETUNREACH),
                "ENFILE" => Some(libc::ENFILE),
                "ENOBUFS" => Some(libc::ENOBUFS),
                "ENODATA" => Some(libc::ENODATA),
                "ENODEV" => Some(libc::ENODEV),
                "ENOENT" => Some(libc::ENOENT),
                "ENOEXEC" => Some(libc::ENOEXEC),
                "ENOLCK" => Some(libc::ENOLCK),
                "ENOLINK" => Some(libc::ENOLINK),
                "ENOMEM" => Some(libc::ENOMEM),
                "ENOMSG" => Some(libc::ENOMSG),
                "ENOPROTOOPT" => Some(libc::ENOPROTOOPT),
                "ENOSPC" => Some(libc::ENOSPC),
                "ENOSR" => Some(libc::ENOSR),
                "ENOSTR" => Some(libc::ENOSTR),
                "ENOSYS" => Some(libc::ENOSYS),
                "ENOTCONN" => Some(libc::ENOTCONN),
                "ENOTDIR" => Some(libc::ENOTDIR),
                "ENOTEMPTY" => Some(libc::ENOTEMPTY),
                "ENOTSOCK" => Some(libc::ENOTSOCK),
                "ENOTSUP" => Some(libc::ENOTSUP),
                "ENOTTY" => Some(libc::ENOTTY),
                "ENXIO" => Some(libc::ENXIO),
                "EOPNOTSUPP" => Some(libc::EOPNOTSUPP),
                "EOVERFLOW" => Some(libc::EOVERFLOW),
                "EPERM" => Some(libc::EPERM),
                "EPIPE" => Some(libc::EPIPE),
                "EPROTO" => Some(libc::EPROTO),
                "EPROTONOSUPPORT" => Some(libc::EPROTONOSUPPORT),
                "EPROTOTYPE" => Some(libc::EPROTOTYPE),
                "ERANGE" => Some(libc::ERANGE),
                "EROFS" => Some(libc::EROFS),
                "ESPIPE" => Some(libc::ESPIPE),
                "ESRCH" => Some(libc::ESRCH),
                "ESTALE" => Some(libc::ESTALE),
                "ETIME" => Some(libc::ETIME),
                "ETIMEDOUT" => Some(libc::ETIMEDOUT),
                "ETXTBSY" => Some(libc::ETXTBSY),
                "EWOULDBLOCK" => Some(libc::EWOULDBLOCK),
                "EXDEV" => Some(libc::EXDEV),
                _ => None,
            };
            v.map(|x| x as f64)
        }
        #[cfg(not(unix))]
        {
            match prop {
                "EACCES" => Some(13.0),
                "EAGAIN" => Some(11.0),
                "EBADF" => Some(9.0),
                "EBUSY" => Some(16.0),
                "EEXIST" => Some(17.0),
                "EFAULT" => Some(14.0),
                "EINTR" => Some(4.0),
                "EINVAL" => Some(22.0),
                "EIO" => Some(5.0),
                "EISDIR" => Some(21.0),
                "EMFILE" => Some(24.0),
                "ENFILE" => Some(23.0),
                "ENODEV" => Some(19.0),
                "ENOENT" => Some(2.0),
                "ENOMEM" => Some(12.0),
                "ENOSPC" => Some(28.0),
                "ENOTDIR" => Some(20.0),
                "ENOTEMPTY" => Some(41.0),
                "EPERM" => Some(1.0),
                "EPIPE" => Some(32.0),
                "ERANGE" => Some(34.0),
                "EROFS" => Some(30.0),
                _ => None,
            }
        }
    };

    let os_priority_const = |prop: &str| -> Option<f64> {
        match prop {
            "PRIORITY_LOW" => Some(19.0),
            "PRIORITY_BELOW_NORMAL" => Some(10.0),
            "PRIORITY_NORMAL" => Some(0.0),
            "PRIORITY_ABOVE_NORMAL" => Some(-7.0),
            "PRIORITY_HIGH" => Some(-14.0),
            "PRIORITY_HIGHEST" => Some(-20.0),
            _ => None,
        }
    };

    let os_dlopen_const = |prop: &str| -> Option<f64> {
        #[cfg(unix)]
        {
            match prop {
                "RTLD_LAZY" => Some(libc::RTLD_LAZY as f64),
                "RTLD_NOW" => Some(libc::RTLD_NOW as f64),
                "RTLD_GLOBAL" => Some(libc::RTLD_GLOBAL as f64),
                "RTLD_LOCAL" => Some(libc::RTLD_LOCAL as f64),
                #[cfg(all(target_os = "linux", target_env = "gnu"))]
                "RTLD_DEEPBIND" => Some(libc::RTLD_DEEPBIND as f64),
                _ => None,
            }
        }
        #[cfg(not(unix))]
        {
            match prop {
                "RTLD_LAZY" => Some(1.0),
                "RTLD_NOW" => Some(2.0),
                "RTLD_GLOBAL" => Some(8.0),
                "RTLD_LOCAL" => Some(4.0),
                _ => None,
            }
        }
    };

    // Issue #649: `crypto.constants.RSA_PKCS1_PADDING` etc. OpenSSL-defined
    // stable values; hardcoded to match Node 24.x's published table.
    let crypto_const = |prop: &str| -> Option<f64> {
        match prop {
            "OPENSSL_VERSION_NUMBER" => Some(811597840.0),
            "SSL_OP_ALL" => Some(2147485776.0),
            "SSL_OP_ALLOW_NO_DHE_KEX" => Some(1024.0),
            "SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION" => Some(262144.0),
            "SSL_OP_CIPHER_SERVER_PREFERENCE" => Some(4194304.0),
            "SSL_OP_CISCO_ANYCONNECT" => Some(32768.0),
            "SSL_OP_COOKIE_EXCHANGE" => Some(8192.0),
            "SSL_OP_CRYPTOPRO_TLSEXT_BUG" => Some(2147483648.0),
            "SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS" => Some(2048.0),
            "SSL_OP_LEGACY_SERVER_CONNECT" => Some(4.0),
            "SSL_OP_NO_COMPRESSION" => Some(131072.0),
            "SSL_OP_NO_ENCRYPT_THEN_MAC" => Some(524288.0),
            "SSL_OP_NO_QUERY_MTU" => Some(4096.0),
            "SSL_OP_NO_RENEGOTIATION" => Some(1073741824.0),
            "SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION" => Some(65536.0),
            "SSL_OP_NO_SSLv2" => Some(0.0),
            "SSL_OP_NO_SSLv3" => Some(33554432.0),
            "SSL_OP_NO_TICKET" => Some(16384.0),
            "SSL_OP_NO_TLSv1" => Some(67108864.0),
            "SSL_OP_NO_TLSv1_1" => Some(268435456.0),
            "SSL_OP_NO_TLSv1_2" => Some(134217728.0),
            "SSL_OP_NO_TLSv1_3" => Some(536870912.0),
            "SSL_OP_PRIORITIZE_CHACHA" => Some(2097152.0),
            "SSL_OP_TLS_ROLLBACK_BUG" => Some(8388608.0),
            "ENGINE_METHOD_RSA" => Some(1.0),
            "ENGINE_METHOD_DSA" => Some(2.0),
            "ENGINE_METHOD_DH" => Some(4.0),
            "ENGINE_METHOD_RAND" => Some(8.0),
            "ENGINE_METHOD_EC" => Some(2048.0),
            "ENGINE_METHOD_CIPHERS" => Some(64.0),
            "ENGINE_METHOD_DIGESTS" => Some(128.0),
            "ENGINE_METHOD_PKEY_METHS" => Some(512.0),
            "ENGINE_METHOD_PKEY_ASN1_METHS" => Some(1024.0),
            "ENGINE_METHOD_ALL" => Some(65535.0),
            "ENGINE_METHOD_NONE" => Some(0.0),
            "DH_CHECK_P_NOT_SAFE_PRIME" => Some(2.0),
            "DH_CHECK_P_NOT_PRIME" => Some(1.0),
            "DH_UNABLE_TO_CHECK_GENERATOR" => Some(4.0),
            "DH_NOT_SUITABLE_GENERATOR" => Some(8.0),
            "RSA_PKCS1_PADDING" => Some(1.0),
            "RSA_NO_PADDING" => Some(3.0),
            "RSA_PKCS1_OAEP_PADDING" => Some(4.0),
            "RSA_X931_PADDING" => Some(5.0),
            "RSA_PKCS1_PSS_PADDING" => Some(6.0),
            "RSA_PSS_SALTLEN_DIGEST" => Some(-1.0),
            "RSA_PSS_SALTLEN_MAX_SIGN" => Some(-2.0),
            "RSA_PSS_SALTLEN_AUTO" => Some(-2.0),
            "TLS1_VERSION" => Some(769.0),
            "TLS1_1_VERSION" => Some(770.0),
            "TLS1_2_VERSION" => Some(771.0),
            "TLS1_3_VERSION" => Some(772.0),
            "POINT_CONVERSION_COMPRESSED" => Some(2.0),
            "POINT_CONVERSION_UNCOMPRESSED" => Some(4.0),
            "POINT_CONVERSION_HYBRID" => Some(6.0),
            _ => None,
        }
    };

    match module_name {
        "path" => match property {
            "sep" => {
                if cfg!(windows) {
                    Some(str_val("\\"))
                } else {
                    Some(str_val("/"))
                }
            }
            "delimiter" => {
                if cfg!(windows) {
                    Some(str_val(";"))
                } else {
                    Some(str_val(":"))
                }
            }
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
            "EOL" => {
                if cfg!(windows) {
                    Some(str_val("\r\n"))
                } else {
                    Some(str_val("\n"))
                }
            }
            "constants" => Some(create_sub_namespace("os.constants")),
            _ => None,
        },
        "os.constants" => match property {
            "signals" => Some(create_sub_namespace("os.constants.signals")),
            "errno" => Some(create_sub_namespace("os.constants.errno")),
            "priority" => Some(create_sub_namespace("os.constants.priority")),
            "dlopen" => Some(create_sub_namespace("os.constants.dlopen")),
            // Top-level libuv constant — sits directly on `os.constants`, not
            // inside one of the nested tables. Node's UDP socket impl uses it
            // for `SO_REUSEADDR`. Value is the published libuv flag (4).
            "UV_UDP_REUSEADDR" => Some(4.0),
            _ => None,
        },
        "os.constants.signals" => os_signal_const(property),
        "os.constants.errno" => os_errno_const(property),
        "os.constants.priority" => os_priority_const(property),
        "os.constants.dlopen" => os_dlopen_const(property),
        "crypto" => match property {
            "constants" => Some(create_sub_namespace("crypto.constants")),
            _ => None,
        },
        "crypto.constants" => crypto_const(property),
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
        "F_OK",
        "R_OK",
        "W_OK",
        "X_OK",
        "O_RDONLY",
        "O_WRONLY",
        "O_RDWR",
        "O_NOFOLLOW",
        "COPYFILE_EXCL",
    ];
    let o_nofollow: f64 = {
        #[cfg(target_os = "macos")]
        {
            0x0100 as f64
        }
        #[cfg(target_os = "linux")]
        {
            0x20000 as f64
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            0x0100 as f64
        }
    };
    let field_values: &[f64] = &[
        0.0, 4.0, 2.0, 1.0, // F_OK, R_OK, W_OK, X_OK
        0.0, 1.0, 2.0,        // O_RDONLY, O_WRONLY, O_RDWR
        o_nofollow, // O_NOFOLLOW
        1.0,        // COPYFILE_EXCL
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
                (arg_f64.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader,
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
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "sub" => {
            let result = crate::bigint::js_bigint_sub(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "mul" => {
            let result = crate::bigint::js_bigint_mul(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "div" => {
            let result = crate::bigint::js_bigint_div(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "mod" | "umod" => {
            let result = crate::bigint::js_bigint_mod(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "pow" => {
            let result = crate::bigint::js_bigint_pow(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "and" => {
            let result = crate::bigint::js_bigint_and(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "or" => {
            let result = crate::bigint::js_bigint_or(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "xor" => {
            let result = crate::bigint::js_bigint_xor(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "shln" => {
            let result = crate::bigint::js_bigint_shl(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "shrn" => {
            let result = crate::bigint::js_bigint_shr(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "maskn" => {
            // maskn(bits) — mask to lowest N bits
            let result = crate::bigint::js_bigint_and(a, b); // approximate
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        // Comparison → returns boolean/number
        "eq" => {
            let result = crate::bigint::js_bigint_eq(a, b);
            f64::from_bits(JSValue::bool(result != 0).bits())
        }
        "lt" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            f64::from_bits(JSValue::bool(result < 0).bits())
        }
        "lte" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            f64::from_bits(JSValue::bool(result <= 0).bits())
        }
        "gt" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            f64::from_bits(JSValue::bool(result > 0).bits())
        }
        "gte" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            f64::from_bits(JSValue::bool(result >= 0).bits())
        }
        "cmp" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            result as f64
        }
        "fromTwos" => {
            // bn.js: interpret `a` as the unsigned encoding of a signed
            // `width`-bit integer in two's complement. If bit (width-1) of
            // `a` is set the result is `a - 2^width`; otherwise return `a`.
            // `width` arrives in `b` (already a BigInt — see top of fn).
            let width = if b.is_null() { 0u64 } else { (*b).limbs[0] };
            let max_bits = (crate::bigint::BIGINT_LIMBS * 64) as u64;
            if width == 0 || width > max_bits {
                return f64::from_bits(
                    JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits(),
                );
            }
            let bit = (width - 1) as usize;
            let high_bit_set = ((*a).limbs[bit / 64] >> (bit % 64)) & 1 == 1;
            if !high_bit_set {
                return f64::from_bits(
                    JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits(),
                );
            }
            let one = crate::bigint::js_bigint_from_u64(1);
            let two_pow = crate::bigint::js_bigint_shl(one, b);
            let result = crate::bigint::js_bigint_sub(a, two_pow);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "toTwos" => {
            // bn.js: convert to `width`-bit two's complement encoding. If `a`
            // is negative the result is `a + 2^width` (mod 2^width);
            // otherwise return `a` unchanged. bn.js does not mask
            // non-negative inputs to `width` bits, so neither do we.
            let width = if b.is_null() { 0u64 } else { (*b).limbs[0] };
            let max_bits = (crate::bigint::BIGINT_LIMBS * 64) as u64;
            if width == 0 || width > max_bits {
                return f64::from_bits(
                    JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits(),
                );
            }
            if crate::bigint::js_bigint_is_negative(a) == 0 {
                return f64::from_bits(
                    JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits(),
                );
            }
            let one = crate::bigint::js_bigint_from_u64(1);
            let two_pow = crate::bigint::js_bigint_shl(one, b);
            let result = crate::bigint::js_bigint_add(a, two_pow);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
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
        let entries_data =
            (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
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
            let entry_data =
                (entry_arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let key_val = *entry_data;
            let val_val = *entry_data.add(1);
            // Convert key to string
            let key_str = crate::builtins::js_string_coerce(key_val);
            if key_str.is_null() {
                continue;
            }
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
                let data =
                    (key_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(bytes).unwrap_or("").to_string()
            };

            if !groups.contains_key(&key_string) {
                order.push(key_string.clone());
            }
            groups.entry(key_string).or_default().push(item);
        }

        // Materialize the result object. Allocate with the right field count
        // up front so the keys_array is sized correctly.
        let obj = js_object_alloc(0, order.len() as u32);
        if obj.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        for key in &order {
            // Build the JS string for the key.
            let key_str_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
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
            if a_bits == b_bits {
                return f64::from_bits(TAG_TRUE);
            }
            return f64::from_bits(TAG_FALSE);
        }
        if an == bn {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For strings, do content comparison
    if a_jsval.is_string() && b_jsval.is_string() {
        let result = crate::string::js_string_equals(
            a_jsval.as_string_ptr() as *const crate::StringHeader,
            b_jsval.as_string_ptr() as *const crate::StringHeader,
        );
        if result != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For everything else, bit-pattern equality
    if a_bits == b_bits {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
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
        // Symbol-keyed lookup: route through SYMBOL_PROPERTIES side table.
        // drizzle's `is(value, type)` checks `entityKind` which is a Symbol;
        // string-coercion would yield null and the check would always fail.
        // Refs #420.
        if crate::symbol::js_is_symbol(key_value) != 0 {
            // ClassRef receivers are NaN-boxed as INT32_TAG (top16 = 0x7FFE)
            // with the class_id in the low 32 bits. Consult the
            // class-static-symbol side table populated by
            // `js_class_register_static_symbol`. Refs #420 (drizzle's
            // `Object.prototype.hasOwnProperty.call(Table, entityKind)`).
            let bits = obj_value.to_bits();
            if (bits >> 48) == 0x7FFE {
                let class_id = (bits & 0xFFFF_FFFF) as u32;
                let present =
                    crate::symbol::class_static_symbol_lookup(class_id, key_value).is_some();
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            let present = crate::symbol::js_object_has_own_symbol(obj_value, key_value);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }
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
pub extern "C" fn js_object_define_property(
    obj_value: f64,
    key_value: f64,
    descriptor_value: f64,
) -> f64 {
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
                // Issue #450: spec says the getter/setter runs with `this === obj`
                // (the property access target). The user's descriptor literal
                // `{ get() {...}, set() {...} }` was lowered with `captures_this: true`
                // and had its reserved `this` slot patched to point to the *descriptor*
                // object at construction time — that's what every other object-literal
                // method does. Clone the closure once at defineProperty time and
                // rebind `this` to `obj`, so every subsequent get/set call sees the
                // correct receiver. Closures without CAPTURES_THIS_FLAG (e.g. arrow-form
                // `get: () => this._backing` written as a field rather than a method
                // shorthand) pass through unchanged.
                let recv_box = crate::value::js_nanbox_pointer(obj as i64);
                let get_bits = if get_field.is_undefined() {
                    0u64
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), recv_box)
                };
                let set_bits = if set_field.is_undefined() {
                    0u64
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), recv_box)
                };
                set_accessor_descriptor(
                    obj as usize,
                    k,
                    AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
            }
        } else {
            // Data descriptor: look for "value" field and store it.
            let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
            let value_field =
                js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
            // Clear any existing accessor for this key so the write doesn't fire the setter.
            if let Some(ref k) = key_rust {
                ACCESSOR_DESCRIPTORS.with(|m| {
                    m.borrow_mut().remove(&(obj as usize, k.clone()));
                });
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
            set_property_attrs(
                obj as usize,
                k,
                PropertyAttrs::new(writable, enumerable, configurable),
            );
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
            let desc =
                js_object_alloc_with_shape(0x0D_E5_C1, 4, packed.as_ptr(), packed.len() as u32);
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
        *fields = f64::from_bits(value.bits()); // value
        *fields.add(1) = bool_to_f64(attrs.writable()); // writable
        *fields.add(2) = bool_to_f64(attrs.enumerable()); // enumerable
        *fields.add(3) = bool_to_f64(attrs.configurable()); // configurable
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

/// Issue #620: returns the OWN-property value at `name` if one exists in the
/// receiver's own keys_array (a string-keyed data property), otherwise
/// returns TAG_UNDEFINED. Used by class-method dispatch to detect override
/// patterns like `this.method = X` (hono's SmartRouter.match rebinds itself
/// on first call). Distinct from `js_object_get_field_by_name` because it
/// does NOT walk the class vtable's getter chain — we only want a raw own
/// data-property read, not a side-effecting getter invocation.
#[no_mangle]
pub extern "C" fn js_object_get_own_field_or_undef(
    obj_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    const TAG_UNDEF: u64 = 0x7FFC_0000_0000_0001;
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(TAG_UNDEF);
    }
    unsafe {
        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x1000000 {
            return f64::from_bits(TAG_UNDEF);
        }
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return f64::from_bits(TAG_UNDEF);
        }
        if !is_valid_obj_ptr(obj as *const u8) {
            return f64::from_bits(TAG_UNDEF);
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return f64::from_bits(TAG_UNDEF);
        }
        // Skip closures sharing the GC_TYPE_OBJECT slot (CLOSURE_MAGIC at +12).
        let type_tag_at_12 = *((obj as *const u8).add(12) as *const u32);
        if type_tag_at_12 == crate::closure::CLOSURE_MAGIC {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_ptr = keys as usize;
        if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
            return f64::from_bits(TAG_UNDEF);
        }
        let keys_gc =
            (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
            return f64::from_bits(TAG_UNDEF);
        }
        let key_bytes = std::slice::from_raw_parts(name_ptr, name_len);
        let key_count = crate::array::js_array_length(keys) as usize;
        if key_count > 65536 {
            return f64::from_bits(TAG_UNDEF);
        }
        let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(keys, i as u32);
            if key_val.is_string() {
                let stored_key = key_val.as_string_ptr();
                if !stored_key.is_null() {
                    let stored_data =
                        (stored_key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                    let stored_len = (*stored_key).byte_len as usize;
                    let stored_bytes = std::slice::from_raw_parts(stored_data, stored_len);
                    if stored_bytes == key_bytes {
                        let val = if i < alloc_limit {
                            js_object_get_field(obj, i as u32)
                        } else {
                            match overflow_get(obj as usize, i) {
                                Some(bits) => crate::JSValue::from_bits(bits),
                                None => return f64::from_bits(TAG_UNDEF),
                            }
                        };
                        return f64::from_bits(val.bits());
                    }
                }
            }
        }
        f64::from_bits(TAG_UNDEF)
    }
}

/// Issue #611 (Effect): `globalThis[<computed>] = value` and the
/// `(globalThis as any)[id] ??= new Map()` pattern (used by hono / Effect /
/// most ESM libraries that ship a CJS-compat global side-store) wrote to
/// a 0-pointer sentinel and read back undefined — `globalStore` was always
/// undefined, callers SIGSEGV'd at the next `.has()` / `.get()` call. This
/// function lazily allocates a single shared ObjectHeader (one per process,
/// initialised on first access) and returns a NaN-boxed POINTER to it. The
/// codegen-side IndexGet / IndexSet on `Expr::GlobalGet` routes through
/// this helper instead of through the 0.0 sentinel so reads / writes
/// actually persist. Existing AST-shape patterns like
/// `PropertyGet { GlobalGet, "log" }` (console.log dispatch) match on the
/// HIR node, not the SSA value, so they continue to fire even though the
/// SSA value of GlobalGet now changes.
#[no_mangle]
pub extern "C" fn js_get_global_this() -> f64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static GLOBAL_THIS_PTR: AtomicI64 = AtomicI64::new(0);
    let cached = GLOBAL_THIS_PTR.load(Ordering::Acquire);
    let ptr = if cached != 0 {
        cached
    } else {
        // First access — allocate. Race-tolerant: if two threads race the
        // initial alloc, the loser's allocation leaks (never freed) but
        // both threads see the winner's pointer afterward via CAS.
        let new_ptr = js_object_alloc(0, 0) as i64;
        match GLOBAL_THIS_PTR.compare_exchange(0, new_ptr, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => new_ptr,
            Err(other) => other,
        }
    };
    crate::value::js_nanbox_pointer(ptr)
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
            (*gc)._reserved |= crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND;
            // Drop writable + configurable for every existing key.
            mark_all_keys(
                obj, /*drop_writable=*/ true, false, /*drop_configurable=*/ true,
            );
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
            mark_all_keys(
                obj, /*drop_writable=*/ false, false, /*drop_configurable=*/ true,
            );
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

/// Object.getPrototypeOf(obj):
/// - For an INT32-tagged class ref (top16 == 0x7FFE) — return the parent
///   class ref via CLASS_REGISTRY's parent_class_id chain, or null at
///   the root. Drizzle's `is(value, type)` chain walks this.
/// - For an object instance with a registered class_id — return the
///   class ref. Conceptually JS returns `Class.prototype`; Perry doesn't
///   maintain prototype objects, but drizzle's chain consumes
///   `Object.getPrototypeOf(value).constructor`, and class_ref's
///   `.constructor` synthesizes back to the same class ref via the
///   constructor intercept (v0.5.746). So returning the class ref here
///   makes that chain produce `value.constructor` as Node would.
/// - Other receivers — null.
/// Refs #420 / #618 followup.
#[no_mangle]
pub extern "C" fn js_object_get_prototype_of(obj_value: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bits = obj_value.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if let Some(parent_id) = get_parent_class_id(class_id) {
            if parent_id != 0 {
                let parent_bits = 0x7FFE_0000_0000_0000u64 | (parent_id as u64);
                return f64::from_bits(parent_bits);
            }
        }
        return f64::from_bits(TAG_NULL);
    }
    // Heap-pointer receiver — return the input value itself. For
    // class-id-tagged instances, `.constructor` then returns the class
    // ref (via the constructor intercept in js_object_get_field_by_name,
    // v0.5.746), making `getPrototypeOf(v).constructor === v.constructor`.
    // For object literals / arrays / other non-class-tagged heap values,
    // `.constructor` returns undefined, which collapses drizzle's
    // `if (cls)` chain to false safely (instead of throwing on
    // `null.constructor` if we returned null). Drizzle's
    // `is(value, type)` chain calls this on every chunk including
    // arrays of values, so the array case is load-bearing.
    //
    // Two NaN-shapes cover the heap-pointer case:
    //  - top16 == 0x7FFD: NaN-boxed POINTER_TAG (typical function-local).
    //  - top16 == 0x0000 with raw_addr large enough: module-level object
    //    literals get stored as raw I64 pointers (no NaN-boxing) per the
    //    "Module-level variables" note in CLAUDE.md, so we accept that
    //    form here too.
    if top16 == 0x7FFD {
        let raw_addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if raw_addr != 0 && raw_addr >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            return obj_value;
        }
    }
    if top16 == 0 {
        if bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            return obj_value;
        }
    }
    f64::from_bits(TAG_NULL)
}
