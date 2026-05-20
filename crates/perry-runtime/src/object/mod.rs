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

// ---------------------------------------------------------------------------
// Submodules (issue #1103): behavior-preserving split of the former
// 11.2k-line object.rs. Each submodule does `use super::*;` so the
// shared state/helpers that remain in this trunk module stay reachable;
// everything public is re-exported here so no symbol moves in the public
// surface (all `#[no_mangle]` FFI entry points keep their exact symbol).
// ---------------------------------------------------------------------------
mod alloc;
mod delete_rest;
mod field_get_set;
mod instanceof;
mod object_ops;
pub use alloc::*;
pub use delete_rest::*;
pub use field_get_set::*;
pub use instanceof::*;
pub use object_ops::*;

// Overflow field storage for objects that exceed their pre-allocated inline slot count.
// Keyed by (obj_ptr as usize) -> Vec<JSValue bits> indexed by absolute field_index
// (inline slots 0..alloc_limit remain `TAG_UNDEFINED` placeholders in the Vec;
// they're never read since the inline slots are checked first).
//
// Was a `HashMap<usize, HashMap<usize, u64>>` through v0.5.29 — the inner HashMap
// dominated the row-decode hot path: a 20-property row object touches the overflow
// storage on each of its 12 post-8-slot writes, and HashMap ops (hash + probe +
// mut insert) cost ~40-50ns each. Flat `Vec<u64>` is ~5ns per append + index;
// removes most of the residual gap after the shape-transition cache landed.
//
// This handles cases like Object.assign() adding many fields to an object
// that was allocated with only 8 slots (e.g., @noble/curves Fp field with 21 properties).
thread_local! {
    /// Heap-pointer keyed; PtrHasher avoids the per-call SipHash on
    /// every overflow read/write. `clear_overflow_for_ptr` was 0.7%
    /// leaf samples on perf-comprehensive (called from object dispatch
    /// + arena_walk_objects in the GC path).
    static OVERFLOW_FIELDS: RefCell<crate::fast_hash::PtrHashMap<usize, Vec<u64>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    static CLASS_PROTOTYPE_METHOD_VALUES: RefCell<HashMap<(u32, String), u64>> =
        RefCell::new(HashMap::new());

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

// Last-accessed overflow Vec cache — one entry, keyed by `obj_ptr`.
// Skips the outer HashMap lookup on consecutive writes to the same
// object (exactly the row-build pattern: a single object gets its
// overflow slots filled back-to-back). Refreshed on every slow-path
// HashMap access; invalidated by `clear_overflow_for_ptr` when GC
// sweep frees the corresponding object.
//
// Safety: the cached pointer references the `Vec<u64>` struct stored
// inside a HashMap bucket. That struct only moves when the HashMap
// resizes, which only happens on `entry().or_default()` inserting a
// fresh key. The slow path below does both the potentially-resizing
// call and the cache refresh inside a single `OVERFLOW_FIELDS.with`
// closure, so no other thread-local mutation can interleave between
// obtaining `&mut Vec` and caching its address.
thread_local! {
    static OVERFLOW_LAST: std::cell::UnsafeCell<(usize, *mut Vec<u64>)> =
        const { std::cell::UnsafeCell::new((0, std::ptr::null_mut())) };
}

// Implicit `this` for closure-typed class fields invoked method-style.
//
// Issue #519: when `obj.fn(args)` calls a closure stored as a class field,
// the field-scan dispatch in `js_native_call_method` can't bind `this`
// through the closure ABI (closures take `(closure_ptr, arg0, …)` — no
// `this` slot). Hono's RegExpRouter does this with `match = match` (the
// imported function from matcher.js), and the function body's
// `this.buildAllMatchers()` reads `this = 0` and TypeErrors out.
//
// Codegen for `Expr::This` (perry-codegen/src/expr.rs) reads from this
// thread-local when the lexical `this_stack` is empty (i.e. inside a
// non-arrow function body or top-level closure body). The field-scan
// dispatch saves the previous value, sets it to the receiver, calls the
// closure, then restores. Direct function calls (`fn(args)`) don't touch
// this slot, so non-method invocations don't pollute it across calls.
//
// Defaults to `TAG_UNDEFINED`. JS spec says top-level `this` is undefined
// in strict mode, which matches.
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
        crate::gc::layout_note_slot(obj_ptr, field_index, vbits);
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
    crate::gc::layout_note_slot(obj_ptr, field_index, vbits);
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

// Recursion depth guard for js_native_call_method to prevent stack overflow
// from circular module dependencies during initialization.
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

/// Issue #692: default-import calls against unresolved modules
/// (`import jwt from "jsonwebtoken"; jwt.sign(...)` when no perry-stdlib
/// binding matched the method, or `import sanitizeHtml from
/// "sanitize-html"; sanitizeHtml(x)` when sanitize-html doesn't resolve
/// to a NativeCompiled module) used to lower to an LLVM extern named
/// literally `default`, which the system linker can't resolve —
/// surfaced as `undefined reference to 'default'`. Route those calls
/// here so the binary links; the runtime stub prints a one-shot
/// diagnostic and returns NaN-boxed undefined. The user gets a clear
/// signal at first call rather than a cryptic link error.
#[no_mangle]
pub extern "C" fn js_unresolved_default_call() -> f64 {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "perry: called a default-imported binding from an unresolved module \
             (returns undefined). The module's default export was not found in \
             perry-stdlib or perry.compilePackages — run `perry --print-api-manifest` \
             to see what's supported."
        );
    }
    f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
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
///
/// #854: kept alongside the size constant so future cache-resizing edits
/// touch both in one place. Codegen-emitted slot-index expressions match
/// against this value even when no Rust path consults it directly.
#[allow(dead_code)]
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
///
/// #855: walk the static via `&raw const` + raw pointer indexing to
/// avoid the `static_mut_refs` lint (hard error in Rust 2024). The
/// cache is thread-local-by-discipline (perry user code is single-
/// threaded), so the unsafe deref is sound.
pub fn scan_transition_cache_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_transition_cache_roots_mut(&mut visitor);
}

pub fn scan_transition_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let base: *mut TransitionEntry = (&raw mut TRANSITION_CACHE_GLOBAL).cast();
    unsafe {
        for i in 0..TRANSITION_CACHE_SIZE {
            let entry = &mut *base.add(i);
            if entry.next_keys != 0 {
                let mut invalidate = false;
                invalidate |= visitor.visit_metadata_usize_slot(&mut entry.prev_keys);
                invalidate |= visitor.visit_metadata_usize_slot(&mut entry.key_ptr);
                visitor.visit_usize_slot(&mut entry.next_keys);
                if invalidate {
                    *entry = TransitionEntry {
                        prev_keys: 0,
                        key_ptr: 0,
                        next_keys: 0,
                        slot_idx: 0,
                        _pad: 0,
                    };
                }
            }
        }
    }
}

/// GC root scanner: mark all cached shape keys arrays so they're not freed.
/// The inline cache + overflow map both hold the raw `*mut ArrayHeader`
/// pointers; without this scanner, GC would free those arrays, leaving
/// every object with that shape holding a dangling `keys_array` pointer.
pub fn scan_shape_cache_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_shape_cache_roots_mut(&mut visitor);
}

pub fn scan_shape_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SHAPE_INLINE_CACHE.with(|cache| {
        let entries = unsafe { &mut *cache.get() };
        for entry in entries.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut entry.keys_array);
        }
    });
    SHAPE_CACHE_OVERFLOW.with(|cache| {
        let mut cache = cache.borrow_mut();
        for arr_ptr in cache.values_mut() {
            visitor.visit_raw_mut_ptr_slot(arr_ptr);
        }
    });
}

/// GC root scanner: mark all JSValues stored in OVERFLOW_FIELDS.
/// OVERFLOW_FIELDS stores extra properties for objects that exceed their pre-allocated inline
/// slot count. The u64 JSValue bits may contain NaN-boxed pointers to heap objects (strings,
/// arrays, other objects) that are ONLY referenced via OVERFLOW_FIELDS. Without this scanner,
/// GC would free those referenced objects.
pub fn scan_overflow_fields_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_overflow_fields_roots_mut(&mut visitor);
}

pub fn scan_overflow_fields_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut moved = Vec::new();
    let mut moved_any = false;
    OVERFLOW_FIELDS.with(|m| {
        let mut m = m.borrow_mut();
        for (&owner, fields) in m.iter_mut() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
            if crate::gc::layout_visit_pointer_slots_for_user(new_owner, fields.len(), |i| {
                if let Some(val_bits) = fields.get_mut(i) {
                    visitor.visit_nanbox_u64_slot(val_bits);
                }
            }) {
                continue;
            }
            for val_bits in fields.iter_mut() {
                visitor.visit_nanbox_u64_slot(val_bits);
            }
        }
        for (old_owner, new_owner) in moved.drain(..) {
            if let Some(fields) = m.remove(&old_owner) {
                m.insert(new_owner, fields);
                moved_any = true;
            }
        }
    });
    if moved_any {
        OVERFLOW_LAST.with(|c| unsafe {
            *c.get() = (0, std::ptr::null_mut());
        });
    }
}

#[cfg(test)]
pub(crate) fn test_seed_shape_cache_root(shape_id: u32, keys_array: *mut ArrayHeader) {
    SHAPE_INLINE_CACHE.with(|cache| {
        let slot = (shape_id as usize) & (SHAPE_INLINE_CACHE_SIZE - 1);
        unsafe {
            (*cache.get())[slot] = ShapeCacheEntry {
                shape_id,
                keys_array,
            };
        }
    });
    SHAPE_CACHE_OVERFLOW.with(|cache| {
        cache.borrow_mut().clear();
        cache.borrow_mut().insert(shape_id, keys_array);
    });
}

#[cfg(test)]
pub(crate) fn test_shape_cache_root(shape_id: u32) -> (usize, usize) {
    let inline = SHAPE_INLINE_CACHE.with(|cache| {
        let slot = (shape_id as usize) & (SHAPE_INLINE_CACHE_SIZE - 1);
        unsafe { (*cache.get())[slot].keys_array as usize }
    });
    let overflow = SHAPE_CACHE_OVERFLOW.with(|cache| {
        cache
            .borrow()
            .get(&shape_id)
            .map(|ptr| *ptr as usize)
            .unwrap_or(0)
    });
    (inline, overflow)
}

#[cfg(test)]
pub(crate) fn test_seed_transition_cache_root(next_keys: usize) {
    unsafe {
        TRANSITION_CACHE_GLOBAL[0] = TransitionEntry {
            prev_keys: 0,
            key_ptr: 0,
            next_keys,
            slot_idx: 0,
            _pad: 0,
        };
    }
}

#[cfg(test)]
pub(crate) fn test_transition_cache_root() -> usize {
    unsafe { TRANSITION_CACHE_GLOBAL[0].next_keys }
}

#[cfg(test)]
pub(crate) fn test_clear_transition_cache_root() {
    unsafe {
        TRANSITION_CACHE_GLOBAL[0] = TransitionEntry {
            prev_keys: 0,
            key_ptr: 0,
            next_keys: 0,
            slot_idx: 0,
            _pad: 0,
        };
    }
}

#[cfg(test)]
pub(crate) fn test_seed_overflow_fields_root(owner: usize, value_bits: u64) {
    OVERFLOW_FIELDS.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        m.insert(owner, vec![value_bits]);
    });
    crate::gc::layout_note_slot(owner, 0, value_bits);
    OVERFLOW_LAST.with(|c| unsafe {
        *c.get() = (0, std::ptr::null_mut());
    });
}

#[cfg(test)]
pub(crate) fn test_clear_overflow_fields_root() {
    OVERFLOW_FIELDS.with(|m| m.borrow_mut().clear());
    OVERFLOW_LAST.with(|c| unsafe {
        *c.get() = (0, std::ptr::null_mut());
    });
}

#[cfg(test)]
pub(crate) fn test_overflow_fields_root() -> (usize, u64) {
    OVERFLOW_FIELDS.with(|m| {
        let m = m.borrow();
        let Some((&owner, fields)) = m.iter().next() else {
            return (0, 0);
        };
        (owner, fields.first().copied().unwrap_or(0))
    })
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
    use crate::value::JSValue;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    let bits = value.to_bits();
    let jsv = JSValue::from_bits(bits);
    // Spec-defined primitive tags (ramda's `_isString.js` / `_isObject.js`
    // / `_isRegExp.js` / `_isArguments.js` IIFEs distinguish on these
    // exact strings; returning `[object Object]` everywhere folded all
    // five branches into the catch-all).
    if jsv.is_undefined() {
        let bytes = b"[object Undefined]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_null() {
        let bytes = b"[object Null]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_bool() {
        let bytes = b"[object Boolean]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_any_string() {
        let bytes = b"[object String]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if jsv.is_int32() || jsv.is_number() {
        let bytes = b"[object Number]";
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    // Heap-allocated pointers: discriminate Array / Error from generic
    // Object via the GC header type byte.
    let raw_ptr = if jsv.is_pointer() {
        (bits & POINTER_MASK) as *const u8
    } else {
        bits as *const u8
    };
    if !raw_ptr.is_null() && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        let gc_header = raw_ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            let bytes = b"[object Array]";
            let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
        }
        if gc_type == crate::gc::GC_TYPE_ERROR {
            let bytes = b"[object Error]";
            let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
        }
    }
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

/// Issue #711 part 2: `function Base() {}; Base.prototype = obj` pattern.
/// Effect's `internal/effectable.ts` declares classes via prototype
/// assignment on a plain function, not via `class` syntax. To make
/// `class Derived extends Base {}` walk into `obj`'s methods at dispatch
/// time, we model this as a synthetic class:
///   - `js_set_function_prototype(func, obj)` allocates a synthetic
///     class_id (high-bit-set to avoid collision with codegen-assigned
///     ids), stores `func_bits → synthetic_cid` in `FUNCTION_CLASS_IDS`,
///     and `synthetic_cid → obj_ptr` in `CLASS_PROTOTYPE_OBJECTS`.
///   - `js_register_class_parent_dynamic` extends to detect closure
///     parent values, looks up the synthetic class_id, and registers
///     the (child, synthetic) edge in CLASS_REGISTRY.
///   - The method-dispatch chain walk in `js_native_call_method`
///     consults `CLASS_PROTOTYPE_OBJECTS` when it reaches a synthetic
///     class_id: it resolves the method as a regular field lookup on
///     the prototype object and calls it with `this` bound to the
///     receiver.
pub static FUNCTION_CLASS_IDS: RwLock<Option<HashMap<u64, u32>>> = RwLock::new(None);
// Stored as `usize` (raw address) so the map is Send + Sync. The
// pointer is always converted back to `*mut ObjectHeader` at call sites
// (`class_prototype_object` / the dispatch walk) where single-threaded
// usage is guaranteed.
pub static CLASS_PROTOTYPE_OBJECTS: RwLock<Option<HashMap<u32, usize>>> = RwLock::new(None);

/// Synthetic class id allocator for prototype-object classes. High bit
/// set (0x8000_0000+) to keep them separate from codegen-assigned ids
/// (which start from 1 and grow by module). u32 wraparound is not a
/// concern in practice — would require ~2 billion `Function.prototype = X`
/// statements at module init.
pub static NEXT_SYNTHETIC_CLASS_ID: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x8000_0000);

/// Register a function's prototype object. Called by codegen-emitted
/// init code whenever the HIR detects `<expr>.prototype = <expr>` at
/// the assignment-statement level (lower_expr_assignment Member arm).
///
/// Returns the synthetic class_id allocated for this function (0 if
/// validation fails). The synthetic id is folded into CLASS_REGISTRY
/// when a class extends `func` via the #711 dynamic-parent path.
#[no_mangle]
pub extern "C" fn js_set_function_prototype(func: f64, proto: f64) -> u32 {
    let func_bits = func.to_bits();
    let func_tag = func_bits & 0xFFFF_0000_0000_0000;
    let proto_bits = proto.to_bits();
    let proto_tag = proto_bits & 0xFFFF_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    // Both must be heap-allocated pointers. Anything else (primitives,
    // ClassRef, etc.) is a no-op — preserves the pre-fix baseline
    // where `<not-a-function>.prototype = X` was just a property write
    // on a non-function value (effectively no-op in practice).
    if func_tag != POINTER_TAG || proto_tag != POINTER_TAG {
        return 0;
    }
    // Validate the proto pointer points at a real Object. If it's a
    // builtin header (Set/Map/Regex) or null, bail — Perry can't
    // currently model those as prototype sources.
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *mut ObjectHeader;
    if proto_ptr.is_null() {
        return 0;
    }
    let proto_addr = proto_ptr as usize;
    if crate::set::is_registered_set(proto_addr)
        || crate::map::is_registered_map(proto_addr)
        || crate::regex::is_regex_pointer(proto_ptr as *const u8)
    {
        return 0;
    }
    unsafe {
        if !is_valid_obj_ptr(proto_ptr as *const u8) {
            return 0;
        }
        let gc_header =
            (proto_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return 0;
        }
    }

    // Allocate or reuse a synthetic class id for this function value.
    // The same `function Base() {}` ident can be assigned a prototype
    // multiple times in pathological code; we keep the FIRST mapping
    // and quietly ignore subsequent calls so existing parent edges
    // don't dangle.
    {
        let read = FUNCTION_CLASS_IDS.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                // Update the prototype object (allow re-pointing)
                // without changing the class_id.
                let mut proto_write = CLASS_PROTOTYPE_OBJECTS.write().unwrap();
                if proto_write.is_none() {
                    *proto_write = Some(HashMap::new());
                }
                proto_write
                    .as_mut()
                    .unwrap()
                    .insert(existing, proto_ptr as usize);
                return existing;
            }
        }
    }
    let new_cid = NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut write = FUNCTION_CLASS_IDS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    }
    {
        let mut write = CLASS_PROTOTYPE_OBJECTS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(new_cid, proto_ptr as usize);
    }
    // Register the synthetic id so REGISTERED_CLASS_IDS-gated paths
    // (e.g., the #687 ClassRef-as-receiver short-circuit) recognize it.
    unsafe { js_register_class_id(new_cid) };
    new_cid
}

/// Lookup helper for the dispatch chain walk: returns the prototype
/// object pointer for a synthetic class id, or null if none.
#[inline]
pub(crate) fn class_prototype_object(class_id: u32) -> *mut ObjectHeader {
    if let Ok(read) = CLASS_PROTOTYPE_OBJECTS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&class_id).copied().unwrap_or(0) as *mut ObjectHeader;
        }
    }
    std::ptr::null_mut()
}

/// #711 / #809: resolve `key` by walking the synthetic-class-id prototype
/// chain (`CLASS_PROTOTYPE_OBJECTS`), recursing into each prototype object
/// as a normal field lookup. Used both when a receiver's own keys miss AND
/// when it has no `keys_array` at all (an `Object.create(proto)` result, or
/// a `Function.prototype = obj` instance with no own props). Returns the
/// first defined, non-null field found on the chain.
unsafe fn resolve_proto_chain_field(
    class_id: u32,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        let proto_obj = class_prototype_object(cid);
        if !proto_obj.is_null() {
            let field_val = js_object_get_field_by_name(proto_obj as *const _, key);
            if !field_val.is_undefined() && !field_val.is_null() {
                return Some(field_val);
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
    None
}

/// Lookup the synthetic class id for a function value, if one was
/// registered via `js_set_function_prototype`.
#[inline]
pub(crate) fn function_class_id(value: f64) -> u32 {
    let bits = value.to_bits();
    if let Ok(read) = FUNCTION_CLASS_IDS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&bits).copied().unwrap_or(0);
        }
    }
    0
}

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

/// Maps `class_id → user-visible class name`. Populated by codegen via
/// `js_register_class_name`. Read back by V8-bridge code when surfacing a
/// Perry class to JS — NestJS's `ModuleTokenFactory.create()` reads
/// `metatype.name` to build the module token, so the empty default name
/// from `v8::Function::builder(...)` would collide every module under the
/// same token. (#1021.)
pub static CLASS_NAMES: RwLock<Option<HashMap<u32, String>>> = RwLock::new(None);

/// Register the user-visible name of a class so the V8 bridge can label
/// the V8-side wrapper for nice `metatype.name` reads. Idempotent.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_name(class_id: u32, name_ptr: *const u8, name_len: u32) {
    if class_id == 0 || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let slice = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let name = match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let mut guard = CLASS_NAMES.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard.as_mut().unwrap().insert(class_id, name);
}

/// Look up the user-visible name of a registered class. Returns `None`
/// when the class id was never registered with `js_register_class_name`.
pub fn class_name_for_id(class_id: u32) -> Option<String> {
    let guard = CLASS_NAMES.read().ok()?;
    guard.as_ref()?.get(&class_id).cloned()
}

/// Resolve a closure-typed JSValue back to a built-in constructor name
/// (`"Date"`/`"Array"`/`"Object"`/...) when it matches one of the
/// singleton-installed thunks. Returns `None` for closures that aren't
/// the globalThis built-in constructors. Used by
/// `js_new_function_construct` to dispatch `new <inst.constructor>(...)`
/// shapes (date-fns `constructFrom`, lodash-style `Array` cloning, ...)
/// to the right runtime factory.
fn identify_global_builtin_constructor(func_value: f64) -> Option<&'static str> {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() {
        return None;
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return None;
    }
    // Identify by the closure's read-only `func_ptr` rather than the
    // GC-movable ClosureHeader address. Both the date-fns ctor closure
    // and the (later-evacuated) ctor closure carry the same
    // `global_this_builtin_noop_thunk` function pointer, so this match
    // survives GC moves. The per-name lookup must then walk the
    // globalThis singleton's keys to recover the constructor name —
    // accept the extra hop only when the func_ptr matches.
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return None;
        }
        let func_ptr = (*ptr).func_ptr as usize;
        let noop_thunk = global_this_builtin_noop_thunk as *const u8 as usize;
        if func_ptr != noop_thunk {
            return None;
        }
    }
    // Find which builtin name maps to this exact closure header on the
    // singleton. Walk via the existing
    // `js_get_global_this_builtin_value` helper — short loop (≤ ~50
    // entries), only fires on the constructFrom hot path.
    let global_this_f64 = js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return None;
    }
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let v = unsafe { js_object_get_field_by_name(global_obj, key) };
        if v.bits() == jv.bits() {
            return Some(name);
        }
    }
    None
}

/// Synthetic-anonymous-shape class IDs: classes the HIR generates for
/// bare object literals (`{ x: 1 }` → `__AnonShape_<hash>`). Instances
/// of these shapes should report `Object` from `.constructor`, not the
/// synthetic class itself, so date-fns's `new value.constructor(...)`,
/// drizzle's `value.constructor === Object` duck checks, and the standard
/// `({}).constructor === Object` semantics all match Node. The HIR
/// lowering registers each anon shape's id here at module init.
pub static ANON_SHAPE_CLASS_IDS: RwLock<Option<std::collections::HashSet<u32>>> = RwLock::new(None);

/// Mark `class_id` as a synthetic anon-shape class so `.constructor`
/// reads on instances of that class return the global `Object`
/// constructor rather than the synthetic class ref.
#[no_mangle]
pub unsafe extern "C" fn js_register_anon_shape_class_id(class_id: u32) {
    if class_id == 0 {
        return;
    }
    let mut guard = ANON_SHAPE_CLASS_IDS.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashSet::new());
    }
    guard.as_mut().unwrap().insert(class_id);
}

/// True if `class_id` was registered via `js_register_anon_shape_class_id`.
pub fn is_anon_shape_class_id(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if let Ok(guard) = ANON_SHAPE_CLASS_IDS.read() {
        if let Some(set) = guard.as_ref() {
            return set.contains(&class_id);
        }
    }
    false
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

/// Issue #838: JS-classic prototype method assignment.
///
/// `Class.prototype.method = function() {…}` (and the aliased form
/// `var p = Class.prototype; p.method = function() {…}`) is a pre-ES6
/// idiom dayjs, chalk, and a long tail of libraries still ship.
/// Pre-fix the assignment was lowered to a generic `PropertySet` whose
/// receiver evaluated to a class-prototype-shaped object that nothing
/// downstream consulted, so `(new Class()).method` came back as
/// `undefined`.
///
/// The HIR-level fix routes recognised shapes to
/// `js_register_prototype_method(class_id, name, value)`, which stores
/// the closure value into a per-class side-table here. The dispatch
/// hot paths (`js_object_get_field_by_name` for `inst.method` reads
/// and `js_native_call_method` for `inst.method(...)` calls) consult
/// this table after the regular vtable / proto-object lookups miss,
/// invoking the closure with `this` bound to the receiver.
///
/// Stored values use their full NaN-boxed bits (f64) — typically a
/// POINTER_TAG'd closure, but the dispatch path treats whatever is
/// stored as a callable value and routes it through
/// `js_native_call_value`, which itself accepts both closures and raw
/// `*ClosureHeader` shapes.
pub static CLASS_PROTOTYPE_METHODS: RwLock<Option<HashMap<u32, HashMap<String, u64>>>> =
    RwLock::new(None);

/// Register a JS-classic prototype-method assignment on a class.
/// Called by codegen-emitted init code for each `Class.prototype.<name>
/// = <fn>` (or aliased form) that the HIR recognises. `value` is the
/// NaN-boxed callable to be invoked with `this` bound to the receiver
/// at dispatch time.
#[no_mangle]
pub unsafe extern "C" fn js_register_prototype_method(
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
    let mut guard = CLASS_PROTOTYPE_METHODS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .entry(class_id)
        .or_insert_with(HashMap::new)
        .insert(name, value.to_bits());
    // Ensure the receiver class can be `typeof`-detected. Method-less
    // classes that only get extended via `Class.prototype.m = fn`
    // wouldn't otherwise reach js_register_class_id.
    js_register_class_id(class_id);
}

/// Issue #838 followup (b): function-classic prototype-method dispatch.
/// dayjs's minified bundle declares its instance class via a function
/// declaration inside an IIFE (`function M(cfg) {…}; var m = M.prototype;
/// m.format = function(){…}; return M`). At HIR time `M` is a function
/// (no `class M` block), so the #838 recogniser bailed because
/// `lookup_class("M")` returned None. This helper closes the gap on the
/// runtime side: a single call takes the closure value of `M`, allocates
/// (or reuses) a synthetic class id keyed by the closure's NaN-boxed
/// bits, registers the method on that synthetic class, and returns the
/// id so a paired `new <FuncRef>(args)` allocator can stamp the same id
/// on the instance header. After both arms run, the existing dispatch
/// hot paths (`js_object_get_field_by_name`, `js_native_call_method`)
/// find the method without further changes.
///
/// `func_value` must be a POINTER_TAG'd ClosureHeader (the shape
/// `Expr::FuncRef` lowers to via `js_closure_alloc_singleton`). Anything
/// else is a no-op — preserves the pre-fix baseline where non-callable
/// `.prototype.m = fn` writes were silent property sets.
/// Issue #838 followup (b) — read side: look up a method previously
/// registered via `js_register_function_prototype_method` against the
/// synthetic class id derived from `func_value`. Pre-fix the AST shape
/// `<funcDecl>.prototype.<name>` lowered to a generic PropertyGet on a
/// `Function.prototype` object that never materialised, so the read
/// was always `undefined` — `typeof Foo.prototype.method` came back
/// `'undefined'` even when the method was correctly dispatched through
/// `(new Foo()).method` via the side-table walk. Pairs with the new
/// `Expr::GetFunctionPrototypeMethod` HIR variant.
///
/// Returns the NaN-boxed `undefined` tag if the function value isn't a
/// registered closure, or no method by that name was registered.
#[no_mangle]
pub unsafe extern "C" fn js_get_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    if name_ptr.is_null() || name_len == 0 {
        return undef;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s,
        Err(_) => return undef,
    };
    // Look up the (already-allocated) synthetic class id for this
    // function value. Don't allocate one here — reads on a function
    // that never had any `.prototype.x = fn` assignment should
    // return `undefined`, matching the spec'd behavior of reading a
    // missing property on the `Function.prototype` object.
    let cid = function_class_id(func_value);
    if cid == 0 {
        return undef;
    }
    match lookup_prototype_method(cid, name) {
        Some(v) => v,
        None => undef,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_register_function_prototype_method(
    func_value: f64,
    name_ptr: *const u8,
    name_len: usize,
    value: f64,
) -> u32 {
    let cid = synthetic_class_id_for_function(func_value);
    if cid == 0 || name_ptr.is_null() || name_len == 0 {
        return cid;
    }
    let name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        Ok(s) => s.to_string(),
        Err(_) => return cid,
    };
    let mut guard = CLASS_PROTOTYPE_METHODS.write().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .entry(cid)
        .or_insert_with(HashMap::new)
        .insert(name, value.to_bits());
    js_register_class_id(cid);
    cid
}

/// Get-or-allocate a synthetic class id keyed by a function value's
/// NaN-boxed bits. Used by `js_register_function_prototype_method` (HIR
/// "Func.prototype.x = fn" recogniser) and `js_new_function_construct`
/// (HIR "new Func(args)" allocator) so both sides agree on the same id
/// — the instance's `(*obj).class_id` lands in the same bucket the
/// method registration stored against. Returns 0 if `func_value` isn't a
/// POINTER_TAG'd value (callable shape requirement).
pub(crate) fn synthetic_class_id_for_function(func_value: f64) -> u32 {
    let func_bits = func_value.to_bits();
    // Require a verified closure shape so we don't store arbitrary
    // POINTER_TAG'd pointers (arrays, objects, etc. all share the tag)
    // in `FUNCTION_CLASS_IDS`. The bits-as-key invariant only makes
    // sense for callable values that produced a stable singleton
    // closure pointer.
    if !is_callable_function_value(func_value) {
        return 0;
    }
    {
        let read = FUNCTION_CLASS_IDS.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                return existing;
            }
        }
    }
    let new_cid = NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut write = FUNCTION_CLASS_IDS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    }
    unsafe { js_register_class_id(new_cid) };
    new_cid
}

/// Issue #838 followup (b): construct an instance from a function value.
/// Pairs with `js_register_function_prototype_method` — both arms route
/// through `synthetic_class_id_for_function` so the instance's
/// `class_id` matches the bucket prototype methods were registered
/// against. Allocates a fresh object stamped with the synthetic id,
/// then invokes the function as the constructor with `IMPLICIT_THIS`
/// bound to the new object so any `this.foo = …` writes in the
/// function body land on the instance. Returns the NaN-boxed new
/// instance pointer.
///
/// `func_value` must be a POINTER_TAG'd closure. `args_ptr` is a flat
/// f64 array of length `args_len`. Falls back to a class_id=0
/// empty-object allocation when the function value isn't a closure
/// (preserves the pre-fix baseline for misuse).
#[no_mangle]
pub unsafe extern "C" fn js_new_function_construct(
    func_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // date-fns `constructFrom` clones a Date via
    // `new date.constructor(value)`. `date.constructor` resolves to
    // the global `Date` closure pointer (the noop thunk installed by
    // `populate_global_this_builtins`). Without this intercept the
    // call falls through to the generic empty-object path and
    // `cloned.getTime()` reads garbage. Detect the global Date /
    // Array / Object constructor pointers and dispatch into the
    // matching real factory. Refs date-fns blocker.
    if let Some(name) = identify_global_builtin_constructor(func_value) {
        let args = if args_ptr.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(args_ptr, args_len)
        };
        match name {
            "Date" => {
                if args.is_empty() {
                    return crate::date::js_date_new();
                }
                if args.len() == 1 {
                    return crate::date::js_date_new_from_value(args[0]);
                }
                let mut vals = [f64::from_bits(crate::value::TAG_UNDEFINED); 7];
                for (i, slot) in vals.iter_mut().enumerate() {
                    if i < args.len() {
                        *slot = args[i];
                    }
                }
                return crate::date::js_date_new_local_components(
                    vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6],
                );
            }
            "Array" => {
                // `new Array(n)`: empty array of length n.
                // `new Array(a, b, c)`: array filled with the args.
                let single_len = args.len() == 1 && args[0].is_finite() && args[0] >= 0.0;
                let len = if single_len {
                    args[0] as u32
                } else {
                    args.len() as u32
                };
                let arr = crate::array::js_array_alloc(len);
                if !single_len {
                    for (i, &v) in args.iter().enumerate() {
                        crate::array::js_array_set_f64(arr, i as u32, v);
                    }
                }
                return crate::value::js_nanbox_pointer(arr as i64);
            }
            "Object" => {
                let obj = js_object_alloc(0, 0);
                return crate::value::js_nanbox_pointer(obj as i64);
            }
            _ => {}
        }
    }
    let cid = synthetic_class_id_for_function(func_value);
    // Allocate the instance with the synthetic class id (or 0 if the
    // value isn't callable). The object starts with no own props; the
    // constructor body fills `this.<field>` writes through
    // PropertySet, and prototype-method dispatch consults the
    // synthetic class id's entry in CLASS_PROTOTYPE_METHODS.
    let obj_ptr = js_object_alloc(cid, 0);
    let nan_boxed = crate::value::js_nanbox_pointer(obj_ptr as i64);
    // Only run the constructor body when the callee is recognised as
    // a closure shape. The codegen LocalGet path widens the route to
    // any local-resolved callee, so we have to gate the
    // `js_native_call_value` dispatch on a verified closure pointer
    // here — otherwise `new <non-callable>()` would dereference an
    // arbitrary pointer as a `ClosureHeader` and crash.
    if is_callable_function_value(func_value) {
        // Bind `this` to the new instance, dispatch the constructor,
        // then restore the previous IMPLICIT_THIS. The dispatch
        // result is discarded — JS `new` semantics use the receiver,
        // not the returned value (object returns would override, but
        // dayjs and siblings rely on the receiver mutation pattern).
        let prev_this = crate::object::js_implicit_this_get();
        crate::object::js_implicit_this_set(nan_boxed);
        let _ = crate::closure::js_native_call_value(func_value, args_ptr, args_len);
        crate::object::js_implicit_this_set(prev_this);
    }
    nan_boxed
}

/// Verify that a JSValue is a NaN-boxed pointer to a registered
/// closure header. `js_native_call_value` itself doesn't validate the
/// pointer shape — it dereferences whatever lower-48 bits it gets — so
/// the `new <LocalGet>(args)` widened path here in
/// `js_new_function_construct` needs to gate the constructor dispatch
/// on a real closure to avoid SIGSEGV'ing on non-callable callees
/// (`new someObject()`, `new someStringVar()`, etc.). Uses the
/// `_reserved` magic word `crate::closure::CLOSURE_MAGIC` that every
/// `js_closure_alloc*` site stamps on allocation.
fn is_callable_function_value(value: f64) -> bool {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() {
        return false;
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return false;
    }
    unsafe { (*ptr).type_tag == crate::closure::CLOSURE_MAGIC }
}

/// Lookup helper: returns the registered prototype-method value for
/// `(class_id, name)`, or None if no assignment matched. Walks the
/// parent-class chain so methods registered on a base class are found
/// via subclass instances.
pub(crate) fn lookup_prototype_method(class_id: u32, name: &str) -> Option<f64> {
    let guard = CLASS_PROTOTYPE_METHODS.read().ok()?;
    let map = guard.as_ref()?;
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        if let Some(per_class) = map.get(&cid) {
            if let Some(&bits) = per_class.get(name) {
                return Some(f64::from_bits(bits));
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
    None
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

/// Issue #711: dynamic parent-class registration for
/// `class X extends fn(...)` shapes where the parent class_id is only
/// known at runtime. Called from codegen-emitted module-init code at
/// the source-order position of the class declaration (so the
/// extends expression's free variables — imports, top-level `let`s,
/// factory functions — are already initialized by the time we
/// evaluate the parent).
///
/// `parent_value` is the evaluated extends expression as a Perry
/// NaN-boxed value. We resolve a parent class_id from it via:
///   1. INT32-tagged ClassRef (the value `String$` produces) — the
///      payload IS the class_id, verified against REGISTERED_CLASS_IDS.
///   2. POINTER-tagged Object instance (the value a `make<T>(...)`
///      factory might return when it constructs and returns an
///      object) — read `class_id` from the ObjectHeader.
/// Anything else (closures, primitives, null/undefined) is a no-op:
/// the class stays parentless, identical to the pre-#711 behavior.
/// Self-registration (`parent_cid == class_id`) is rejected so a
/// recursive helper that returns its receiver can't create a cycle.
#[no_mangle]
pub extern "C" fn js_register_class_parent_dynamic(class_id: u32, parent_value: f64) {
    let bits = parent_value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;

    let parent_cid: u32 = if tag == INT32_TAG {
        // ClassRef: lower 32 bits are the class id. Verify it's
        // actually a registered class id before trusting it.
        let payload = bits as u32;
        if payload == 0 {
            0
        } else {
            let guard = REGISTERED_CLASS_IDS.read().unwrap();
            match guard.as_ref() {
                Some(set) if set.contains(&payload) => payload,
                _ => 0,
            }
        }
    } else if tag == POINTER_TAG {
        // Object instance: read class_id from the ObjectHeader.
        let ptr = crate::value::js_nanbox_get_pointer(parent_value) as *const ObjectHeader;
        let from_obj = js_object_get_class_id(ptr);
        if from_obj != 0 {
            from_obj
        } else {
            // Issue #711 part 2: the value might be a closure whose
            // `.prototype` was assigned to an object via the
            // `function Base() {}; Base.prototype = X` pattern. Look
            // up the synthetic class id assigned at
            // `js_set_function_prototype` time. Returns 0 if the
            // closure has no registered prototype object — falls
            // through to the parentless baseline.
            function_class_id(parent_value)
        }
    } else {
        0
    };

    if parent_cid != 0 && parent_cid != class_id {
        register_class(class_id, parent_cid);
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

#[inline]
unsafe fn set_object_keys_array(obj: *mut ObjectHeader, keys_array: *mut ArrayHeader) {
    (*obj).keys_array = keys_array;
    crate::gc::runtime_write_barrier_slot(
        obj as usize,
        &(*obj).keys_array as *const _ as usize,
        keys_array as u64,
    );
}

#[inline]
pub(super) unsafe fn note_object_field_slot(
    obj: *mut ObjectHeader,
    field_index: usize,
    value_bits: u64,
) {
    crate::gc::layout_note_slot(obj as usize, field_index, value_bits);
}

#[inline]
pub(super) unsafe fn rebuild_object_field_layout(obj: *mut ObjectHeader, slot_count: usize) {
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    crate::gc::layout_rebuild_from_slots(obj as *mut u8, fields, slot_count);
}

#[inline]
pub(super) unsafe fn rebuild_array_layout_from_slots(arr: *mut ArrayHeader) {
    if arr.is_null() {
        return;
    }
    let len = (*arr).length as usize;
    let slots = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    crate::gc::layout_rebuild_from_slots(arr as *mut u8, slots, len);
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

    // Issue #489 followup: Promise's `then` / `catch` / `finally` are
    // intrinsic — when the dynamic dispatch path lands a `.then(cb)` on
    // a Promise (drizzle's `mysql-proxy/session.js`:
    // `this.client(...).then(({rows}) => rows)` where the static
    // analyzer couldn't prove the receiver is a Promise), route directly
    // to `js_promise_then` / `js_promise_catch` / `js_promise_finally`.
    // Without this, the field-scan + class-id walks below find nothing
    // and return undefined — drizzle's `MySqlRemoteSession.all` then
    // resolves to undefined and downstream `data[0].insertId` accesses
    // silently fail.
    if matches!(method_name, "then" | "catch" | "finally")
        && crate::promise::js_value_is_promise(object) != 0
    {
        let promise_handle = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut crate::Promise;
        let arg0_box = if args_len >= 1 && !args_ptr.is_null() {
            *args_ptr
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let arg1_box = if args_len >= 2 && !args_ptr.is_null() {
            *args_ptr.add(1)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        // Closures arrive here in two shapes:
        //  - NaN-boxed `POINTER_TAG | (closure_ptr & 0x0000_FFFF_FFFF_FFFF)`
        //    (the codegen `js_closure_alloc_singleton` + OR-with-tag form)
        //  - Raw `*ClosureHeader` bit-cast to f64 — the convention used
        //    by `js_assimilate_thenable` when it propagates
        //    `then(resolve, reject)` callbacks through a user-defined
        //    `then` method's param slots (see `promise.rs:2438-2442`).
        // Accept both. TAG_UNDEFINED / null / non-pointer values stay
        // null so `js_promise_then` treats the handler as missing.
        let extract_closure = |v: f64| -> crate::promise::ClosurePtr {
            let b = v.to_bits();
            let candidate = if (b & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
                b & 0x0000_FFFF_FFFF_FFFF
            } else if (b & 0xFFFF_0000_0000_0000) == 0 {
                b
            } else {
                0
            };
            if candidate < 0x10000 {
                std::ptr::null()
            } else {
                candidate as crate::promise::ClosurePtr
            }
        };
        let result = match method_name {
            "then" => crate::promise::js_promise_then(
                promise_handle,
                extract_closure(arg0_box),
                extract_closure(arg1_box),
            ),
            "catch" => crate::promise::js_promise_catch(promise_handle, extract_closure(arg0_box)),
            "finally" => {
                crate::promise::js_promise_finally(promise_handle, extract_closure(arg0_box))
            }
            _ => unreachable!(),
        };
        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
    }

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
                // #853: the `is_valid_obj_ptr` guard that used to live after
                // this return was dead — the early return claims the path
                // unconditionally. Removed.
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
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
                            // Issue #711 part 2: if this class id has a
                            // registered prototype object (from
                            // `Function.prototype = X`), look up the
                            // method as a regular property of that
                            // object. Effect's `EffectPrototype.pipe()`
                            // and friends are own-properties of the
                            // proto object; the value is a closure that
                            // expects `this = receiver`.
                            let proto_obj = class_prototype_object(cur_cid);
                            if !proto_obj.is_null() {
                                let method_key = crate::string::js_string_from_bytes(
                                    method_name.as_ptr(),
                                    method_name.len() as u32,
                                );
                                let field_val = js_object_get_field_by_name(
                                    proto_obj as *const _,
                                    method_key as *const crate::StringHeader,
                                );
                                if !field_val.is_undefined() && !field_val.is_null() {
                                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                                    let result = crate::closure::js_native_call_value(
                                        f64::from_bits(field_val.bits()),
                                        args_ptr,
                                        args_len,
                                    );
                                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                                    return result;
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
                // #809: independent prototype-object resolution. The walk
                // above only runs when `CLASS_VTABLE_REGISTRY` is `Some` —
                // a program with no user classes that only does
                // `Object.create(objLiteral).method()` has an empty/None
                // registry, so `inst.method()` never reached
                // `class_prototype_object` and threw `<m> is not a
                // function`. Resolve the method off the synthetic-class-id
                // prototype chain directly (reuses the same helper as
                // `js_object_get_field_by_name`), then invoke it with
                // `this` bound to the receiver.
                let method_key = crate::string::js_string_from_bytes(
                    method_name.as_ptr(),
                    method_name.len() as u32,
                );
                if let Some(field_val) =
                    resolve_proto_chain_field(class_id, method_key as *const crate::StringHeader)
                {
                    if !field_val.is_undefined() && !field_val.is_null() {
                        let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                        let result = crate::closure::js_native_call_value(
                            f64::from_bits(field_val.bits()),
                            args_ptr,
                            args_len,
                        );
                        IMPLICIT_THIS.with(|c| c.set(prev_this));
                        return result;
                    }
                }

                // Issue #838: JS-classic `Class.prototype.method = fn`
                // method dispatch. The vtable / proto-object walks above
                // cover ES-class methods and synthetic-prototype-object
                // shapes; this arm catches the case where the method
                // only exists in `CLASS_PROTOTYPE_METHODS`. Bind `this`
                // to the receiver and call the stored closure.
                if let Some(method_value) = lookup_prototype_method(class_id, method_name) {
                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                    let result =
                        crate::closure::js_native_call_value(method_value, args_ptr, args_len);
                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                    return result;
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
                // #853: same dead-after-return as the first arm above.
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
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

        // `obj.hasOwnProperty(key)` — duck-types as truthy for any
        // non-null/undefined receiver where the field-scan and class
        // dispatch above couldn't find a user-defined override. Walking
        // the actual key set on every shape (ObjectHeader fields,
        // closure dynamic props, array keys, …) is more work than this
        // entry point is meant to do; ramda's `_clone` / `_has` only
        // need a non-throwing return so the surrounding pattern doesn't
        // fall into the spec gap. Pre-fix, the chained
        // `Object.prototype.hasOwnProperty.call(obj, key)` reads
        // `Object.prototype.hasOwnProperty` as `undefined` from the
        // empty proto and threw `value is not a function` at module
        // init in `_clone.js` / `_isArguments.js`.
        "hasOwnProperty" => {
            if jsval.is_undefined() || jsval.is_null() {
                return f64::from_bits(JSValue::bool(false).bits());
            }
            return f64::from_bits(JSValue::bool(true).bits());
        }

        // `obj.propertyIsEnumerable(key)` — same shape as
        // `hasOwnProperty`. Spec says true for own enumerable
        // properties (the typical case for object literals). Without
        // walking the receiver's keys, we approximate as
        // `truthy receiver → true` — matches Node for ramda's
        // `keys.js` IIFE (`!{toString:null}.propertyIsEnumerable('toString')`
        // expects `true`, so `hasEnumBug` resolves to `false`).
        // Arguments-like receivers also return true here, which
        // matches the legacy non-Safari behavior ramda's IIFE checks
        // against.
        "propertyIsEnumerable" => {
            if jsval.is_undefined() || jsval.is_null() {
                return f64::from_bits(JSValue::bool(false).bits());
            }
            return f64::from_bits(JSValue::bool(true).bits());
        }

        // Function.prototype.call(thisArg, ...args) — invoke the receiver
        // closure with `thisArg` bound as `this` and the remaining args
        // passed positionally. Ramda's curry helpers (`_curry1`, `_curry2`,
        // `_curry3`) build their dispatch chain around
        // `fn.apply(this, arguments)` / `fn.call(this, x)`, so without these
        // arms ramda fails immediately on the first curried export.
        "call" if jsval.is_pointer() => {
            let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::closure::is_closure_ptr(raw_ptr) {
                let this_arg = if args_len >= 1 && !args_ptr.is_null() {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let rest_ptr = if args_len > 1 && !args_ptr.is_null() {
                    args_ptr.add(1)
                } else {
                    std::ptr::null()
                };
                let rest_len = if args_len > 1 { args_len - 1 } else { 0 };
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
                let result = crate::closure::js_native_call_value(object, rest_ptr, rest_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return result;
            }
        }

        // Function.prototype.apply(thisArg, argsArray) — invoke the receiver
        // closure with `thisArg` bound as `this` and the elements of
        // `argsArray` spread as positional arguments. `argsArray` may be
        // null / undefined (treat as no args). Mirrors `js_native_call_method_apply`
        // but for the `Function.prototype.apply` path rather than the
        // dynamic-spread method-call codegen path.
        "apply" if jsval.is_pointer() => {
            let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::closure::is_closure_ptr(raw_ptr) {
                let this_arg = if args_len >= 1 && !args_ptr.is_null() {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let args_arr_val = if args_len >= 2 && !args_ptr.is_null() {
                    *args_ptr.add(1)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let args_arr_jsval = JSValue::from_bits(args_arr_val.to_bits());
                let buf: Vec<f64> = if args_arr_jsval.is_pointer() {
                    let arr_ptr = (args_arr_val.to_bits() & 0x0000_FFFF_FFFF_FFFF)
                        as *const crate::array::ArrayHeader;
                    if arr_ptr.is_null() {
                        Vec::new()
                    } else {
                        let n = crate::array::js_array_length(arr_ptr) as usize;
                        (0..n)
                            .map(|i| crate::array::js_array_get_f64(arr_ptr, i as u32))
                            .collect()
                    }
                } else {
                    Vec::new()
                };
                let (call_args_ptr, call_args_len) = if buf.is_empty() {
                    (std::ptr::null::<f64>(), 0_usize)
                } else {
                    (buf.as_ptr(), buf.len())
                };
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
                let result =
                    crate::closure::js_native_call_value(object, call_args_ptr, call_args_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return result;
            }
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
            // Object.prototype methods exposed on Buffer instances so
            // safer-buffer's `if (buffer.hasOwnProperty(...))` probe (and
            // similar duck-type tests in express / body-parser dependents)
            // resolve to a callable, not undefined. Without these,
            // `typeof buf.hasOwnProperty` is `"undefined"` and the
            // subsequent invocation throws "buffer.hasOwnProperty is not
            // a function" at express startup.
            | "hasOwnProperty"
            | "propertyIsEnumerable"
            | "valueOf"
            | "isPrototypeOf"
            | "toLocaleString"
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
        // ── Object.prototype fallbacks on Buffer instances ──
        // safer-buffer (loaded by express) probes Buffer instances with
        // `if (buffer.hasOwnProperty(...))`. Pre-fix every non-buffer-specific
        // method read returned undefined, so the call threw
        // "buffer.hasOwnProperty is not a function". Mirror the generic
        // ObjectHeader behaviour wired up in PR #978: hasOwnProperty checks
        // numeric indices against the buffer length (Node spec — indexed
        // bytes are own properties, `length` is on the prototype), and
        // the remaining Object.prototype methods get spec-shaped stubs.
        "hasOwnProperty" => {
            let key_is_own = if args.is_empty() {
                false
            } else {
                let key_bits = args[0].to_bits();
                if (key_bits >> 48) == 0x7FFF {
                    // string key
                    let sptr =
                        (key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader;
                    if sptr.is_null() {
                        false
                    } else {
                        let slen = (*sptr).byte_len as usize;
                        let sdata =
                            (sptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let bytes = std::slice::from_raw_parts(sdata, slen);
                        if let Ok(s) = std::str::from_utf8(bytes) {
                            // Only numeric-string indices that are in bounds
                            // count as own properties for Buffer/Uint8Array.
                            if let Ok(idx) = s.parse::<u32>() {
                                let buf_len = (*buf_ptr).length as u32;
                                idx < buf_len
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                } else if (key_bits >> 48) == 0x7FFE {
                    // int32 key
                    let idx = (key_bits & 0xFFFF_FFFF) as i32;
                    let buf_len = (*buf_ptr).length as i32;
                    idx >= 0 && idx < buf_len
                } else if !(0x7FF8..=0x7FFF).contains(&(key_bits >> 48)) {
                    // raw f64 numeric key (NaN-boxing tags occupy 0x7FF8..=0x7FFF)
                    let n = args[0];
                    if n.is_finite() && n.fract() == 0.0 && n >= 0.0 {
                        let idx = n as u32;
                        let buf_len = (*buf_ptr).length as u32;
                        idx < buf_len
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            i32_bool(key_is_own as i32)
        }
        "propertyIsEnumerable" => {
            // Same key→own check as hasOwnProperty; indexed bytes on a
            // Buffer are enumerable own data properties.
            let key_is_own = if args.is_empty() {
                false
            } else {
                let key_bits = args[0].to_bits();
                if (key_bits >> 48) == 0x7FFF {
                    let sptr =
                        (key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader;
                    if sptr.is_null() {
                        false
                    } else {
                        let slen = (*sptr).byte_len as usize;
                        let sdata =
                            (sptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        let bytes = std::slice::from_raw_parts(sdata, slen);
                        if let Ok(s) = std::str::from_utf8(bytes) {
                            if let Ok(idx) = s.parse::<u32>() {
                                let buf_len = (*buf_ptr).length as u32;
                                idx < buf_len
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                } else if (key_bits >> 48) == 0x7FFE {
                    let idx = (key_bits & 0xFFFF_FFFF) as i32;
                    let buf_len = (*buf_ptr).length as i32;
                    idx >= 0 && idx < buf_len
                } else if !args[0].is_nan() {
                    let n = args[0];
                    if n.is_finite() && n.fract() == 0.0 && n >= 0.0 {
                        let idx = n as u32;
                        let buf_len = (*buf_ptr).length as u32;
                        idx < buf_len
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            i32_bool(key_is_own as i32)
        }
        // `buf.valueOf()` returns the Buffer itself in Node (Uint8Array
        // inherits the no-op valueOf from Object.prototype, but for the
        // duck-test usage in safer-buffer/express-graph the receiver
        // round-trip is what matters).
        "valueOf" => f64::from_bits(JSValue::pointer(addr as *mut u8).bits()),
        // `buf.toLocaleString()` — Node delegates to toString() with no
        // args, which yields the utf8 decode. Match that.
        "toLocaleString" => {
            let str_ptr = crate::buffer::js_buffer_to_string(buf_ptr, 0);
            f64::from_bits(JSValue::string_ptr(str_ptr).bits())
        }
        // `buf.isPrototypeOf(other)` — buffers aren't prototype objects in
        // user code, so this is always false (matches Node when `buf` is
        // a Buffer instance rather than `Buffer.prototype`).
        "isPrototypeOf" => i32_bool(0),
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
    let pack_args = || -> *mut crate::array::ArrayHeader {
        let mut arr = crate::array::js_array_alloc(args_len as u32);
        for i in 0..args_len {
            arr = crate::array::js_array_push_f64(arr, arg(i));
        }
        arr
    };
    let bool_tag = |v: bool| -> f64 {
        if v {
            f64::from_bits(0x7FFC_0000_0000_0004)
        } else {
            f64::from_bits(0x7FFC_0000_0000_0003)
        }
    };
    let ptr_addr = |v: f64| -> usize {
        let bits = v.to_bits();
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        }
    };
    let typed_kind = |v: f64| -> Option<u8> {
        let addr = ptr_addr(v);
        if crate::buffer::is_uint8array_buffer(addr) {
            Some(crate::typedarray::KIND_UINT8)
        } else {
            crate::typedarray::lookup_typed_array_kind(addr)
        }
    };

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
        ("os", "devNull") => str_to_f64(crate::os::js_os_dev_null()),
        ("os", "totalmem") => crate::os::js_os_totalmem(),
        ("os", "freemem") => crate::os::js_os_freemem(),
        ("os", "uptime") => crate::os::js_os_uptime(),
        ("os", "availableParallelism") => crate::os::js_os_available_parallelism(),
        ("os", "endianness") => str_to_f64(crate::os::js_os_endianness()),
        ("os", "machine") => str_to_f64(crate::os::js_os_machine()),
        ("os", "loadavg") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_loadavg() as *const u8).bits())
        }
        ("os", "version") => str_to_f64(crate::os::js_os_version()),
        ("os", "cpus") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_cpus() as *const u8).bits())
        }
        ("os", "networkInterfaces") => f64::from_bits(
            JSValue::pointer(crate::os::js_os_network_interfaces() as *const u8).bits(),
        ),
        ("os", "userInfo") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_user_info() as *const u8).bits())
        }

        // ── path module (args are NaN-boxed strings → extract raw StringHeader ptr) ──
        ("path", "dirname") => str_to_f64(crate::path::js_path_dirname(arg_str_ptr(0))),
        ("path", "basename") => str_to_f64(crate::path::js_path_basename(arg_str_ptr(0))),
        ("path", "extname") => str_to_f64(crate::path::js_path_extname(arg_str_ptr(0))),
        ("path", "resolve") => str_to_f64(crate::path::js_path_resolve(arg_str_ptr(0))),
        ("path", "join") => str_to_f64(crate::path::js_path_join(arg_str_ptr(0), arg_str_ptr(1))),
        ("path", "isAbsolute") => bool_to_f64(crate::path::js_path_is_absolute(arg_str_ptr(0))),

        // ── util module ──
        ("util", "format") => crate::builtins::js_util_format(pack_args()),
        ("util", "formatWithOptions") => {
            let effective = args_len.saturating_sub(1);
            let mut arr = crate::array::js_array_alloc(effective as u32);
            for i in 1..args_len {
                arr = crate::array::js_array_push_f64(arr, arg(i));
            }
            crate::builtins::js_util_format(arr)
        }
        ("util", "inspect") => crate::builtins::js_util_inspect(arg(0), arg(1)),
        ("util", "isDeepStrictEqual") => {
            crate::builtins::js_util_is_deep_strict_equal(arg(0), arg(1))
        }
        ("util", "stripVTControlCharacters") => {
            crate::builtins::js_util_strip_vt_control_characters(arg(0))
        }

        ("util", "isPromise") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(
                v.is_pointer()
                    && crate::promise::js_is_promise(
                        v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                    ) != 0,
            )
        }
        ("util", "isArrayBuffer") | ("util", "isAnyArrayBuffer") => {
            bool_tag(crate::buffer::is_array_buffer(ptr_addr(arg(0))))
        }
        ("util", "isArrayBufferView") => {
            let addr = ptr_addr(arg(0));
            bool_tag(crate::buffer::is_uint8array_buffer(addr) || typed_kind(arg(0)).is_some())
        }
        ("util", "isTypedArray") => bool_tag(typed_kind(arg(0)).is_some()),
        ("util", "isUint8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8))
        }
        ("util", "isUint16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT16))
        }
        ("util", "isInt32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT32))
        }
        ("util", "isFloat64Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT64))
        }
        ("util", "isMap") => bool_tag(crate::map::is_registered_map(ptr_addr(arg(0)))),
        ("util", "isSet") => bool_tag(crate::set::is_registered_set(ptr_addr(arg(0)))),

        // ── util.types namespace ──
        ("util.types", "isPromise") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(
                v.is_pointer()
                    && crate::promise::js_is_promise(
                        v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                    ) != 0,
            )
        }
        ("util.types", "isArrayBuffer") | ("util.types", "isAnyArrayBuffer") => {
            bool_tag(crate::buffer::is_array_buffer(ptr_addr(arg(0))))
        }
        ("util.types", "isArrayBufferView") => {
            let addr = ptr_addr(arg(0));
            bool_tag(crate::buffer::is_uint8array_buffer(addr) || typed_kind(arg(0)).is_some())
        }
        ("util.types", "isTypedArray") => bool_tag(typed_kind(arg(0)).is_some()),
        ("util.types", "isUint8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8))
        }
        ("util.types", "isUint16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT16))
        }
        ("util.types", "isInt32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT32))
        }
        ("util.types", "isFloat64Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT64))
        }
        ("util.types", "isMap") => bool_tag(crate::map::is_registered_map(ptr_addr(arg(0)))),
        ("util.types", "isSet") => bool_tag(crate::set::is_registered_set(ptr_addr(arg(0)))),
        ("util.types", "isDate") => {
            bool_tag(crate::date::is_registered_date_bits(arg(0).to_bits()))
        }
        ("util.types", "isRegExp") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(v.is_pointer() && crate::regex::is_regex_pointer(v.as_pointer::<u8>()))
        }

        // ── node:util/types direct module ──
        ("util/types", "isPromise") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(
                v.is_pointer()
                    && crate::promise::js_is_promise(
                        v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                    ) != 0,
            )
        }
        ("util/types", "isArrayBuffer") | ("util/types", "isAnyArrayBuffer") => {
            bool_tag(crate::buffer::is_array_buffer(ptr_addr(arg(0))))
        }
        ("util/types", "isArrayBufferView") => {
            let addr = ptr_addr(arg(0));
            bool_tag(crate::buffer::is_uint8array_buffer(addr) || typed_kind(arg(0)).is_some())
        }
        ("util/types", "isTypedArray") => bool_tag(typed_kind(arg(0)).is_some()),
        ("util/types", "isUint8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8))
        }
        ("util/types", "isUint16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT16))
        }
        ("util/types", "isInt32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT32))
        }
        ("util/types", "isFloat64Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT64))
        }
        ("util/types", "isMap") => bool_tag(crate::map::is_registered_map(ptr_addr(arg(0)))),
        ("util/types", "isSet") => bool_tag(crate::set::is_registered_set(ptr_addr(arg(0)))),
        ("util/types", "isDate") => {
            bool_tag(crate::date::is_registered_date_bits(arg(0).to_bits()))
        }
        ("util/types", "isRegExp") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(v.is_pointer() && crate::regex::is_regex_pointer(v.as_pointer::<u8>()))
        }

        _ => {
            // Method not found on native module — return undefined
            f64::from_bits(JSValue::undefined().bits())
        }
    }
}

#[inline]
fn nanbox_bool(v: bool) -> f64 {
    f64::from_bits(
        if v {
            JSValue::bool(true)
        } else {
            JSValue::bool(false)
        }
        .bits(),
    )
}

#[inline]
fn jsvalue_addr(v: f64) -> usize {
    let bits = v.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

#[inline]
fn jsvalue_typed_array_kind(v: f64) -> Option<u8> {
    let addr = jsvalue_addr(v);
    if crate::buffer::is_uint8array_buffer(addr) {
        Some(crate::typedarray::KIND_UINT8)
    } else {
        crate::typedarray::lookup_typed_array_kind(addr)
    }
}

#[no_mangle]
pub extern "C" fn js_util_types_is_promise(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    nanbox_bool(
        v.is_pointer()
            && unsafe {
                crate::promise::js_is_promise(
                    v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                ) != 0
            },
    )
}

#[no_mangle]
pub extern "C" fn js_util_types_is_array_buffer(value: f64) -> f64 {
    nanbox_bool(crate::buffer::is_array_buffer(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_array_buffer_view(value: f64) -> f64 {
    let addr = jsvalue_addr(value);
    nanbox_bool(
        crate::buffer::is_uint8array_buffer(addr) || jsvalue_typed_array_kind(value).is_some(),
    )
}

#[no_mangle]
pub extern "C" fn js_util_types_is_typed_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value).is_some())
}

#[no_mangle]
pub extern "C" fn js_util_types_is_uint8_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_UINT8))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_uint16_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_UINT16))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_int32_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_INT32))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_float64_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_FLOAT64))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_map(value: f64) -> f64 {
    nanbox_bool(crate::map::is_registered_map(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_set(value: f64) -> f64 {
    nanbox_bool(crate::set::is_registered_set(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_date(value: f64) -> f64 {
    nanbox_bool(crate::date::is_registered_date_bits(value.to_bits()))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_reg_exp(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    nanbox_bool(v.is_pointer() && crate::regex::is_regex_pointer(v.as_pointer::<u8>()))
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
    // For native modules whose surface includes known callable methods or
    // class exports, return a bound-method closure so `typeof` and property
    // capture (`const f = tty.isatty`) match Node's "function" shape. The
    // closure routes back through js_native_call_method when invoked. Kept
    // narrow to specific (module, property) pairs so a typo'd access still
    // returns undefined.
    if is_native_module_callable_export(module_name, property_name) {
        let heap_name = {
            let layout = std::alloc::Layout::from_size_align(property_name_len.max(1), 1).unwrap();
            let ptr = std::alloc::alloc(layout);
            std::ptr::copy_nonoverlapping(property_name_ptr, ptr, property_name_len);
            ptr
        };
        let closure = crate::closure::js_closure_alloc(crate::closure::BOUND_METHOD_FUNC_PTR, 3);
        let ns = js_create_native_module_namespace(module_name_ptr, module_name_len);
        crate::closure::js_closure_set_capture_f64(closure, 0, ns);
        crate::closure::js_closure_set_capture_ptr(closure, 1, heap_name as i64);
        crate::closure::js_closure_set_capture_ptr(closure, 2, property_name_len as i64);
        return crate::value::js_nanbox_pointer(closure as i64);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Whitelist of (module, property) pairs for which property-read should
/// produce a callable handle (a bound-method closure) rather than undefined.
/// Needed so `typeof tty.ReadStream === "function"` matches Node — the
/// method-call form (`tty.isatty(0)`) is already handled by a dedicated
/// codegen path, this just keeps the property-read form coherent.
///
/// Issue #894: also list `("events", "EventEmitter")` here so pino's
/// `const { EventEmitter } = require('node:events'); /* ... */
/// Object.setPrototypeOf(prototype, EventEmitter.prototype)` survives —
/// pre-fix `EventEmitter` was `undefined`, and the subsequent
/// `EventEmitter.prototype` read threw a spec TypeError at module init.
/// Returning a callable closure makes `EventEmitter` truthy and gives
/// `typeof EventEmitter === "function"` (matching Node); the chained
/// `.prototype` read on a closure pointer returns `undefined` (no method
/// dispatch table tracks `.prototype` on closures), which
/// `Object.setPrototypeOf` then ignores (Perry's runtime helper is a
/// no-op anyway). `new EventEmitter()` still routes through the dedicated
/// builtin path at lower_call/builtin.rs that allocates a real
/// `EventEmitterHandle`, so dispatch coherence is preserved.
fn is_native_module_callable_export(module: &str, prop: &str) -> bool {
    matches!(
        (module, prop),
        ("tty", "isatty")
            | ("tty", "ReadStream")
            | ("tty", "WriteStream")
            | ("events", "EventEmitter")
            | ("os", "platform")
            | ("os", "arch")
            | ("os", "hostname")
            | ("os", "homedir")
            | ("os", "tmpdir")
            | ("os", "totalmem")
            | ("os", "freemem")
            | ("os", "uptime")
            | ("os", "type")
            | ("os", "release")
            | ("os", "cpus")
            | ("os", "networkInterfaces")
            | ("os", "userInfo")
            | ("os", "availableParallelism")
            | ("os", "endianness")
            | ("os", "loadavg")
            | ("os", "machine")
            | ("os", "version")
            // node:cluster — namespace property reads of these callables
            // need to satisfy `typeof cluster.fork === "function"` etc.
            // The fixtures only probe types, but compiled npm code that
            // calls `cluster.fork()` would also land on the bound-method
            // dispatch (currently a stub — see runtime entries below).
            | ("cluster", "fork")
            | ("cluster", "disconnect")
            | ("cluster", "setupPrimary")
            | ("cluster", "setupMaster")
            | ("cluster", "Worker")
            | ("util", "format")
            | ("util", "formatWithOptions")
            | ("util", "inspect")
            | ("util", "promisify")
            | ("util", "callbackify")
            | ("util", "deprecate")
            | ("util", "inherits")
            | ("util", "isDeepStrictEqual")
            | ("util", "stripVTControlCharacters")
            | ("util.types", "isPromise")
            | ("util.types", "isArrayBuffer")
            | ("util.types", "isAnyArrayBuffer")
            | ("util.types", "isArrayBufferView")
            | ("util.types", "isTypedArray")
            | ("util.types", "isUint8Array")
            | ("util.types", "isUint16Array")
            | ("util.types", "isInt32Array")
            | ("util.types", "isFloat64Array")
            | ("util.types", "isMap")
            | ("util.types", "isSet")
            | ("util.types", "isDate")
            | ("util.types", "isRegExp")
            | ("util/types", "isPromise")
            | ("util/types", "isArrayBuffer")
            | ("util/types", "isAnyArrayBuffer")
            | ("util/types", "isArrayBufferView")
            | ("util/types", "isTypedArray")
            | ("util/types", "isUint8Array")
            | ("util/types", "isUint16Array")
            | ("util/types", "isInt32Array")
            | ("util/types", "isFloat64Array")
            | ("util/types", "isMap")
            | ("util/types", "isSet")
            | ("util/types", "isDate")
            | ("util/types", "isRegExp")
    )
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

fn class_ref_id(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if class_id != 0 && is_class_id_registered(class_id) {
            return Some(class_id);
        }
    }
    None
}

unsafe fn metadata_key_to_string(value: f64) -> Option<String> {
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
        .ok()
        .map(|s| s.to_string())
}

fn class_has_own_method(class_id: u32, method_name: &str) -> bool {
    let registry = match CLASS_VTABLE_REGISTRY.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    registry
        .as_ref()
        .and_then(|reg| reg.get(&class_id))
        .map(|vtable| vtable.methods.contains_key(method_name))
        .unwrap_or(false)
}

pub fn class_prototype_method_value_for_name(class_id: u32, method_name: &str) -> f64 {
    CLASS_PROTOTYPE_METHOD_VALUES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(bits) = cache.get(&(class_id, method_name.to_string())).copied() {
            return f64::from_bits(bits);
        }

        // Bounded leak: `js_class_method_bind` keeps the byte pointer for the
        // lifetime of the bound closure (it's stashed inside the closure's
        // capture frame). We leak one allocation per unique
        // `(class_id, method_name)` pair the program ever asks for, so the
        // total leak is bounded by the static set of decorated method
        // descriptors. The cache below short-circuits repeat queries.
        let leaked: &'static [u8] = method_name.as_bytes().to_vec().leak();
        let class_bits = 0x7FFE_0000_0000_0000u64 | (class_id as u64 & 0xFFFF_FFFF);
        let class_ref = f64::from_bits(class_bits);
        let value = js_class_method_bind(class_ref, leaked.as_ptr(), leaked.len());
        cache.insert((class_id, method_name.to_string()), value.to_bits());
        value
    })
}

#[no_mangle]
pub extern "C" fn js_class_prototype_method_value(class_ref: f64, method_key: f64) -> f64 {
    let Some(class_id) = class_ref_id(class_ref) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let method_name = unsafe { metadata_key_to_string(method_key) };
    let Some(method_name) = method_name else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    class_prototype_method_value_for_name(class_id, &method_name)
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

    // `zlib.constants` — the Z_*/DEFLATE/INFLATE/GZIP/BROTLI_*/ZSTD_*
    // table Node exposes on `require('node:zlib').constants`. Values
    // are taken straight from `node-internal/zlib/constants.h` (the
    // upstream lib snapshots) so reads are byte-identical to Node.
    // Required by axios for its stream wiring.
    let zlib_const = |prop: &str| -> Option<f64> {
        let v: i64 = match prop {
            // Compression levels
            "Z_NO_COMPRESSION" => 0,
            "Z_BEST_SPEED" => 1,
            "Z_BEST_COMPRESSION" => 9,
            "Z_DEFAULT_COMPRESSION" => -1,
            // Compression strategies
            "Z_FILTERED" => 1,
            "Z_HUFFMAN_ONLY" => 2,
            "Z_RLE" => 3,
            "Z_FIXED" => 4,
            "Z_DEFAULT_STRATEGY" => 0,
            // Flush values
            "Z_NO_FLUSH" => 0,
            "Z_PARTIAL_FLUSH" => 1,
            "Z_SYNC_FLUSH" => 2,
            "Z_FULL_FLUSH" => 3,
            "Z_FINISH" => 4,
            "Z_BLOCK" => 5,
            "Z_TREES" => 6,
            // Return codes
            "Z_OK" => 0,
            "Z_STREAM_END" => 1,
            "Z_NEED_DICT" => 2,
            "Z_ERRNO" => -1,
            "Z_STREAM_ERROR" => -2,
            "Z_DATA_ERROR" => -3,
            "Z_MEM_ERROR" => -4,
            "Z_BUF_ERROR" => -5,
            "Z_VERSION_ERROR" => -6,
            // Min/Max window bits and memlevel
            "Z_MIN_WINDOWBITS" => 8,
            "Z_MAX_WINDOWBITS" => 15,
            "Z_DEFAULT_WINDOWBITS" => 15,
            "Z_MIN_CHUNK" => 64,
            "Z_MAX_CHUNK" => 0x7fff_ffff,
            "Z_DEFAULT_CHUNK" => 16384,
            "Z_MIN_MEMLEVEL" => 1,
            "Z_MAX_MEMLEVEL" => 9,
            "Z_DEFAULT_MEMLEVEL" => 8,
            "Z_MIN_LEVEL" => -1,
            "Z_MAX_LEVEL" => 9,
            "Z_DEFAULT_LEVEL" => -1,
            // Mode (zlib stream modes — used by zlib.createDeflate etc.)
            "DEFLATE" => 1,
            "INFLATE" => 2,
            "GZIP" => 3,
            "GUNZIP" => 4,
            "DEFLATERAW" => 5,
            "INFLATERAW" => 6,
            "UNZIP" => 7,
            "BROTLI_DECODE" => 8,
            "BROTLI_ENCODE" => 9,
            "ZSTD_COMPRESS" => 10,
            "ZSTD_DECOMPRESS" => 11,
            // Brotli operation/parameter constants — match Node's
            // `zlib.constants` exactly (these are the BrotliEncoder/
            // BrotliDecoder parameter ids the underlying brotli library
            // exposes).
            "BROTLI_OPERATION_PROCESS" => 0,
            "BROTLI_OPERATION_FLUSH" => 1,
            "BROTLI_OPERATION_FINISH" => 2,
            "BROTLI_OPERATION_EMIT_METADATA" => 3,
            "BROTLI_PARAM_MODE" => 0,
            "BROTLI_MODE_GENERIC" => 0,
            "BROTLI_MODE_TEXT" => 1,
            "BROTLI_MODE_FONT" => 2,
            "BROTLI_DEFAULT_MODE" => 0,
            "BROTLI_PARAM_QUALITY" => 1,
            "BROTLI_MIN_QUALITY" => 0,
            "BROTLI_MAX_QUALITY" => 11,
            "BROTLI_DEFAULT_QUALITY" => 11,
            "BROTLI_PARAM_LGWIN" => 2,
            "BROTLI_MIN_WINDOW_BITS" => 10,
            "BROTLI_MAX_WINDOW_BITS" => 24,
            "BROTLI_LARGE_MAX_WINDOW_BITS" => 30,
            "BROTLI_DEFAULT_WINDOW" => 22,
            "BROTLI_PARAM_LGBLOCK" => 3,
            "BROTLI_MIN_INPUT_BLOCK_BITS" => 16,
            "BROTLI_MAX_INPUT_BLOCK_BITS" => 24,
            "BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING" => 4,
            "BROTLI_PARAM_SIZE_HINT" => 5,
            "BROTLI_PARAM_LARGE_WINDOW" => 6,
            "BROTLI_PARAM_NPOSTFIX" => 7,
            "BROTLI_PARAM_NDIRECT" => 8,
            "BROTLI_DECODER_RESULT_ERROR" => 0,
            "BROTLI_DECODER_RESULT_SUCCESS" => 1,
            "BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT" => 2,
            "BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT" => 3,
            "BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION" => 0,
            "BROTLI_DECODER_PARAM_LARGE_WINDOW" => 1,
            // Zstd parameter ids — match Node's `zlib.constants`.
            "ZSTD_e_continue" => 0,
            "ZSTD_e_flush" => 1,
            "ZSTD_e_end" => 2,
            "ZSTD_fast" => 1,
            "ZSTD_dfast" => 2,
            "ZSTD_greedy" => 3,
            "ZSTD_lazy" => 4,
            "ZSTD_lazy2" => 5,
            "ZSTD_btlazy2" => 6,
            "ZSTD_btopt" => 7,
            "ZSTD_btultra" => 8,
            "ZSTD_btultra2" => 9,
            "ZSTD_c_compressionLevel" => 100,
            "ZSTD_c_windowLog" => 101,
            "ZSTD_c_hashLog" => 102,
            "ZSTD_c_chainLog" => 103,
            "ZSTD_c_searchLog" => 104,
            "ZSTD_c_minMatch" => 105,
            "ZSTD_c_targetLength" => 106,
            "ZSTD_c_strategy" => 107,
            "ZSTD_c_enableLongDistanceMatching" => 160,
            "ZSTD_c_ldmHashLog" => 161,
            "ZSTD_c_ldmMinMatch" => 162,
            "ZSTD_c_ldmBucketSizeLog" => 163,
            "ZSTD_c_ldmHashRateLog" => 164,
            "ZSTD_c_contentSizeFlag" => 200,
            "ZSTD_c_checksumFlag" => 201,
            "ZSTD_c_dictIDFlag" => 202,
            "ZSTD_c_nbWorkers" => 400,
            "ZSTD_c_jobSize" => 401,
            "ZSTD_c_overlapLog" => 402,
            "ZSTD_d_windowLogMax" => 100,
            "ZSTD_CLEVEL_DEFAULT" => 3,
            "ZSTD_MINCLEVEL" => -131072,
            "ZSTD_MAXCLEVEL" => 22,
            _ => return None,
        };
        Some(v as f64)
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
            "devNull" => {
                if cfg!(windows) {
                    Some(str_val("\\\\.\\nul"))
                } else {
                    Some(str_val("/dev/null"))
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
        "util" => match property {
            "types" => Some(create_sub_namespace("util.types")),
            _ => None,
        },
        "crypto" => match property {
            "constants" => Some(create_sub_namespace("crypto.constants")),
            _ => None,
        },
        "crypto.constants" => crypto_const(property),
        "events" => match property {
            "defaultMaxListeners" => Some(10.0),
            "captureRejections" => Some(f64::from_bits(JSValue::bool(false).bits())),
            "errorMonitor" => Some(crate::symbol::js_symbol_for(str_val("events.errorMonitor"))),
            "captureRejectionSymbol" => {
                Some(crate::symbol::js_symbol_for(str_val("nodejs.rejection")))
            }
            _ => None,
        },
        // `zlib.constants` and the top-level Z_*/DEFLATE/INFLATE shortcuts
        // Node also exposes directly on `require('node:zlib')`.
        "zlib" => match property {
            "constants" => Some(create_sub_namespace("zlib.constants")),
            _ => zlib_const(property),
        },
        "zlib.constants" => zlib_const(property),
        // Issue #912 (#909 follow-up): express reads
        // `const { METHODS } = require('node:http')` at module init and
        // immediately calls `METHODS.map(...)` — pre-fix METHODS resolved
        // to undefined and threw `TypeError: Cannot read properties of
        // undefined (reading 'map')`. Node's `http.METHODS` is a sorted
        // array of HTTP verb strings sourced from llhttp (only exposed
        // on `node:http`, not on `https`/`http2`). We materialize the
        // array once (`http_methods_array` caches the long-lived
        // pointer) and hand it back for every read.
        "http" => match property {
            "METHODS" => Some(unsafe { http_methods_array() }),
            _ => None,
        },
        // node:cluster — all property reads are static constants on the
        // primary process. The test fixture only exercises shape, never
        // forks a worker; the `fork` / `disconnect` / `setupPrimary` /
        // `setupMaster` / `Worker` callables are produced separately by
        // `is_native_module_callable_export` (bound-method closure path).
        "cluster" => match property {
            // Identity flags: we always identify as the primary
            // process. A future `cluster.fork` impl would need to flip
            // these in the spawned child.
            "isPrimary" | "isMaster" => Some(f64::from_bits(JSValue::bool(true).bits())),
            "isWorker" => Some(f64::from_bits(JSValue::bool(false).bits())),
            // No active worker on the primary side.
            "worker" => Some(f64::from_bits(JSValue::undefined().bits())),
            // Empty registries — each read allocates a fresh empty
            // object (the test only reads them once, so the allocation
            // churn is irrelevant).
            "workers" | "settings" => {
                let obj = unsafe { js_object_alloc(0, 0) };
                Some(f64::from_bits(JSValue::pointer(obj as *const u8).bits()))
            }
            // SCHED_RR is the cross-platform default (port-based on
            // Linux/macOS, manual scheduling on Windows). `SCHED_NONE`
            // is 1, `SCHED_RR` is 2; `schedulingPolicy` defaults to RR.
            "schedulingPolicy" | "SCHED_RR" => Some(2.0),
            "SCHED_NONE" => Some(1.0),
            // EventEmitter methods on the cluster module aren't named
            // exports — Node's namespace import reads them as
            // `undefined`. We register them in the api-manifest so the
            // #463 gate doesn't reject the typeof read at compile time;
            // here we resolve them to undefined at runtime.
            "on" | "addListener" => Some(f64::from_bits(JSValue::undefined().bits())),
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

/// Issue #912 (#909 follow-up): cached `http.METHODS` array. Matches
/// Node 22's exposed list (alphabetically sorted, derived from llhttp's
/// HTTP method table). The array is allocated in the longlived arena so
/// it survives every GC sweep — the cached pointer is shared across
/// every `http.METHODS` / `https.METHODS` / `http2.METHODS` read.
unsafe fn http_methods_array() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CACHED: AtomicU64 = AtomicU64::new(0);
    let cached = CACHED.load(Ordering::Relaxed);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    // Node 22 `require('node:http').METHODS` snapshot.
    const METHODS: &[&str] = &[
        "ACL",
        "BIND",
        "CHECKOUT",
        "CONNECT",
        "COPY",
        "DELETE",
        "GET",
        "HEAD",
        "LINK",
        "LOCK",
        "M-SEARCH",
        "MERGE",
        "MKACTIVITY",
        "MKCALENDAR",
        "MKCOL",
        "MOVE",
        "NOTIFY",
        "OPTIONS",
        "PATCH",
        "POST",
        "PROPFIND",
        "PROPPATCH",
        "PURGE",
        "PUT",
        "QUERY",
        "REBIND",
        "REPORT",
        "SEARCH",
        "SOURCE",
        "SUBSCRIBE",
        "TRACE",
        "UNBIND",
        "UNLINK",
        "UNLOCK",
        "UNSUBSCRIBE",
    ];
    let arr = crate::array::js_array_alloc_with_length_longlived(METHODS.len() as u32);
    let elements_ptr = (arr as *mut u8).add(8) as *mut f64;
    for (i, m) in METHODS.iter().enumerate() {
        let bytes = m.as_bytes();
        let str_ptr =
            crate::string::js_string_from_bytes_longlived(bytes.as_ptr(), bytes.len() as u32);
        let nanboxed = f64::from_bits(
            crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK),
        );
        *elements_ptr.add(i) = nanboxed;
        crate::gc::layout_note_slot(arr as usize, i, nanboxed.to_bits());
    }
    let value = crate::value::js_nanbox_pointer(arr as i64);
    CACHED.store(value.to_bits(), Ordering::Relaxed);
    value
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
            Ok(_) => {
                // Winner: populate built-in constructor properties on the
                // singleton so `globalThis.Array` / `context.Array` (lodash's
                // `runInContext` pattern) return non-undefined values. Each
                // value is a tiny ObjectHeader carrying a `prototype` field
                // pointing at another empty object — enough that
                // `var arrayProto = Array.prototype` doesn't throw and the
                // chained `.toString` reads return undefined rather than
                // tripping the "Cannot read properties of undefined" gate at
                // module-init time. Full constructor dispatch on these
                // sentinels still falls through to existing code paths (bare
                // `new Array(n)` continues to work through `lower_new`); the
                // goal here is just to unblock libraries that read the
                // constructors off `globalThis` as values. Refs lodash
                // `runInContext` blocker after PR #963.
                populate_global_this_builtins(new_ptr as *mut ObjectHeader);
                new_ptr
            }
            Err(other) => other,
        }
    };
    crate::value::js_nanbox_pointer(ptr)
}

/// JS built-in constructor names exposed on `globalThis`. Pre-populated by
/// the singleton init in `js_get_global_this` so libraries that read these
/// off the global (lodash's `var Array = context.Array; var arrayProto =
/// Array.prototype`, the same `(globalThis as any).X` read shape) see a
/// non-undefined backing object. Codegen mirrors this list in
/// `perry-codegen/src/expr.rs::is_global_this_builtin_name` to decide when
/// `globalThis.<Name>` should route through the singleton instead of the
/// legacy `0.0` no-value placeholder.
pub(crate) const GLOBAL_THIS_BUILTIN_CONSTRUCTORS: &[&str] = &[
    "Array",
    "Object",
    "String",
    "Number",
    "Boolean",
    "Function",
    "RegExp",
    "Date",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "Symbol",
    "Promise",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "Proxy",
    "BigInt",
    "Uint8Array",
    "Int8Array",
    "Uint16Array",
    "Int16Array",
    "Uint32Array",
    "Int32Array",
    "Float32Array",
    "Float64Array",
    "Uint8ClampedArray",
    "BigInt64Array",
    "BigUint64Array",
    "ArrayBuffer",
    "SharedArrayBuffer",
    "DataView",
    "TextEncoder",
    "TextDecoder",
    "URL",
    "URLSearchParams",
    "AbortController",
    "AbortSignal",
    "FormData",
    "Headers",
    "Request",
    "Response",
    "FinalizationRegistry",
];

/// JS built-in namespaces (typeof === "object", not "function"). Same
/// shape on the singleton — a backing object with `prototype` so chained
/// reads degrade gracefully — but typeof reports "object".
pub(crate) const GLOBAL_THIS_BUILTIN_NAMESPACES: &[&str] = &["Math", "JSON", "Reflect"];

/// No-op thunk used as the function body for the singleton globalThis
/// built-in constructor values. Lets `globalThis.Array` carry a real
/// ClosureHeader (so `typeof globalThis.Array === "function"`) without
/// implementing actual constructor dispatch through this path — bare
/// `new Array(n)` continues to flow through codegen's `lower_new` arm and
/// the runtime `js_array_alloc` machinery, so callers that follow the
/// usual `new <Ident>(...)` pattern are unaffected. Calling these
/// sentinels directly (e.g. `globalThis.Array(3)`) returns undefined —
/// best-effort no-op rather than throwing — and is a known gap for
/// libraries that rely on call-form constructors after re-binding the
/// global to a local.
extern "C" fn global_this_builtin_noop_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Thunk for `Object.prototype.toString` exposed as a callable closure
/// value. Mirrors `Object.prototype.toString.call(x)` — returns the
/// `"[object Tag]"` string for the receiver in IMPLICIT_THIS.
///
/// Tag detection uses the same coarse NaN-box / GC-type discrimination
/// the rest of the runtime relies on: arrays → `"[object Array]"`,
/// strings → `"[object String]"`, null/undefined → matching tags,
/// numbers/bools → primitive tags, generic objects/closures →
/// `"[object Object]"`.
///
/// Unblocks ramda's `_isArguments.js` IIFE which evaluates
/// `Object.prototype.toString.call(arguments)` at module-init time
/// — pre-fix the chained `Object.prototype.toString` read returned
/// `undefined`, so the `.call` access threw before the IIFE body ran.
extern "C" fn object_prototype_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    use crate::value::JSValue;
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let tag: &[u8] = if this_jsv.is_undefined() {
        b"[object Undefined]"
    } else if this_jsv.is_null() {
        b"[object Null]"
    } else if this_jsv.is_bool() {
        b"[object Boolean]"
    } else if this_jsv.is_any_string() {
        b"[object String]"
    } else if this_jsv.is_int32() || this_jsv.is_number() {
        b"[object Number]"
    } else {
        // Discriminate by GC header type for heap-allocated values.
        // Accept both NaN-boxed pointers and raw-i64 pointers (the
        // codegen's two representations for non-numeric values — see
        // CLAUDE.md "Module-level variables"). Module-level arrays
        // arrive here as raw i64 because the codegen stores them
        // unboxed; function-arg-passed arrays arrive NaN-boxed.
        let raw = if this_jsv.is_pointer() {
            (this_bits & 0x0000_FFFF_FFFF_FFFF) as *const u8
        } else {
            this_bits as *const u8
        };
        if !raw.is_null() && (raw as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            unsafe {
                let gc_header = raw.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                let gc_type = (*gc_header).obj_type;
                if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                    b"[object Array]"
                } else if gc_type == crate::gc::GC_TYPE_ERROR {
                    b"[object Error]"
                } else {
                    b"[object Object]"
                }
            }
        } else {
            b"[object Object]"
        }
    };
    let s = crate::string::js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
    f64::from_bits(crate::js_nanbox_string(s as i64).to_bits())
}

/// Thunk for `Array.prototype.slice` exposed as a real callable closure
/// value. Reads the array receiver from `IMPLICIT_THIS` (set by
/// `Function.prototype.call`/`.apply`'s runtime arm in
/// `js_native_call_method`) and forwards to `js_array_slice`.
///
/// Coerces start/end through `JSValue::to_number`, with `undefined`
/// mapping to `0` for start and `i32::MAX` for end — matching
/// `Array.prototype.slice`'s ECMA-262 defaults.
///
/// Unblocks the `Array.prototype.slice.call(list, …)` pattern that
/// ramda's curry/variadic helpers use heavily (refs `_curry1`,
/// `_curry2`, and every variadic op like `addIndex`/`addIndexRight`/
/// `useWith`/`unapply`/`flip`/`call`). Without this, `Array.prototype.slice`
/// read off the singleton's empty proto object as `undefined` and the
/// chained `.call` access threw
/// `Cannot read properties of undefined (reading 'call')` at module init.
extern "C" fn array_prototype_slice_thunk(
    _closure: *const crate::closure::ClosureHeader,
    start_val: f64,
    end_val: f64,
) -> f64 {
    use crate::value::JSValue;
    let this_bits = IMPLICIT_THIS.with(|c| c.get());
    let this_jsv = JSValue::from_bits(this_bits);
    let arr_ptr = if this_jsv.is_pointer() {
        this_jsv.as_pointer::<crate::array::ArrayHeader>()
    } else {
        // Tolerate raw-i64-encoded array receivers (some module-init
        // call sites stash array pointers in IMPLICIT_THIS without
        // NaN-boxing). The clean_arr_ptr check inside js_array_slice
        // re-validates.
        let raw = this_bits as *const crate::array::ArrayHeader;
        if (raw as usize) > 0x10000 {
            raw
        } else {
            std::ptr::null()
        }
    };
    if arr_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let start_jsv = JSValue::from_bits(start_val.to_bits());
    let end_jsv = JSValue::from_bits(end_val.to_bits());
    let start_i32 = if start_jsv.is_undefined() {
        0
    } else {
        let n = start_jsv.to_number();
        if n.is_nan() {
            0
        } else {
            n as i32
        }
    };
    let end_i32 = if end_jsv.is_undefined() {
        i32::MAX
    } else {
        let n = end_jsv.to_number();
        if n.is_nan() {
            0
        } else {
            n as i32
        }
    };
    let result = crate::array::js_array_slice(arr_ptr, start_i32, end_i32);
    f64::from_bits(crate::value::js_nanbox_pointer(result as i64).to_bits())
}

/// Populate the freshly-allocated globalThis singleton with built-in
/// constructor / namespace properties. Called exactly once from the CAS
/// winner in `js_get_global_this`. Constructors get a ClosureHeader-
/// backed value so `typeof globalThis.Array === "function"`; namespaces
/// (`Math`, `JSON`, `Reflect`) get a plain ObjectHeader (`typeof ===
/// "object"`). Both shapes carry a `prototype` dynamic property pointing
/// at an empty object so `<Builtin>.prototype` reads return a real
/// pointer instead of undefined, which is what unblocks lodash's
/// `var arrayProto = Array.prototype` chained read inside
/// `runInContext`.
fn populate_global_this_builtins(singleton: *mut ObjectHeader) {
    if singleton.is_null() {
        return;
    }
    let proto_key_bytes = b"prototype";
    let proto_key =
        crate::string::js_string_from_bytes(proto_key_bytes.as_ptr(), proto_key_bytes.len() as u32);
    // Constructors: ClosureHeader-backed so typeof is "function".
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        let closure_ptr =
            crate::closure::js_closure_alloc(global_this_builtin_noop_thunk as *const u8, 0);
        if closure_ptr.is_null() {
            continue;
        }
        // Stash `prototype` on the closure's dynamic-prop side table.
        // `js_object_set_field_by_name` detects the CLOSURE_MAGIC tag
        // at offset 12 and dispatches into `closure_set_dynamic_prop`
        // for us; both reads and writes share that side table.
        let proto_obj = js_object_alloc(0, 0);
        if !proto_obj.is_null() {
            let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
            js_object_set_field_by_name(closure_ptr as *mut ObjectHeader, proto_key, proto_value);
            // Populate well-known method properties on the prototype
            // (currently just `Array.prototype.slice`). Methods are
            // ClosureHeader-backed thunks that read their receiver from
            // `IMPLICIT_THIS` and dispatch to the corresponding native
            // entry point — works in tandem with `.call`/`.apply` since
            // those arms (#970) rebind IMPLICIT_THIS before forwarding.
            populate_builtin_prototype_methods(name, proto_obj);
        }
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        js_object_set_field_by_name(singleton, name_key, ctor_value);
    }
    // Namespaces: plain ObjectHeader so typeof is "object" per spec.
    for name in GLOBAL_THIS_BUILTIN_NAMESPACES.iter().copied() {
        let ns_obj = js_object_alloc(0, 0);
        if ns_obj.is_null() {
            continue;
        }
        let name_bytes = name.as_bytes();
        let name_key =
            crate::string::js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
        let ns_value = crate::value::js_nanbox_pointer(ns_obj as i64);
        js_object_set_field_by_name(singleton, name_key, ns_value);
    }
}

/// Populate well-known method properties on a builtin constructor's
/// prototype object. Each registered method is a closure that, when
/// invoked through `.call(thisArg, …args)` / `.apply(thisArg, args)`,
/// reads its receiver from `IMPLICIT_THIS` and dispatches to the
/// corresponding native runtime entry point.
///
/// Currently only `Array.prototype.slice` is wired up — that's the one
/// pattern ramda's curry/variadic helpers depend on. Other builtins
/// (`Function.prototype.bind`, `String.prototype.split`, …) and other
/// Array methods (`concat`, `forEach`, `indexOf`, `map`, `reduce`,
/// `reduceRight`) can be added here as additional packages need them
/// (ramda only uses those on real array receivers, where the codegen
/// method-dispatch path already handles them — the prototype route is
/// only required when the call site reaches through `.call(arr, …)`).
fn populate_builtin_prototype_methods(builtin_name: &str, proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    match builtin_name {
        "Array" => {
            let slice_closure =
                crate::closure::js_closure_alloc(array_prototype_slice_thunk as *const u8, 0);
            if !slice_closure.is_null() {
                // Register arity so `.call(this, start)` (1 user arg
                // after the receiver) pads the missing `end` with
                // `undefined` instead of dispatching to a 1-arg
                // signature that reads `end_val` out of an
                // uninitialised register.
                crate::closure::js_register_closure_arity(
                    array_prototype_slice_thunk as *const u8,
                    2,
                );
                let key_bytes = b"slice";
                let key =
                    crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
                let value = crate::value::js_nanbox_pointer(slice_closure as i64);
                js_object_set_field_by_name(proto_obj, key, value);
            }
        }
        "Object" => {
            let to_string_closure =
                crate::closure::js_closure_alloc(object_prototype_to_string_thunk as *const u8, 0);
            if !to_string_closure.is_null() {
                // 0-arg thunk — `.call(this)` forwards 0 user args to
                // `js_native_call_value`, which dispatches via
                // `js_closure_call0`.
                crate::closure::js_register_closure_arity(
                    object_prototype_to_string_thunk as *const u8,
                    0,
                );
                let key_bytes = b"toString";
                let key =
                    crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
                let value = crate::value::js_nanbox_pointer(to_string_closure as i64);
                js_object_set_field_by_name(proto_obj, key, value);
            }
        }
        _ => {}
    }
}
