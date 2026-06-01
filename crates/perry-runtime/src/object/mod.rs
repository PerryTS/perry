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
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::RwLock;

// ---------------------------------------------------------------------------
// Submodules (issue #1103): behavior-preserving split of the former
// 11.2k-line object.rs. Each submodule does `use super::*;` so the
// shared state/helpers that remain in this trunk module stay reachable;
// everything public is re-exported here so no symbol moves in the public
// surface (all `#[no_mangle]` FFI entry points keep their exact symbol).
// ---------------------------------------------------------------------------
mod alloc;
mod assert;
mod bigint_dispatch;
mod buffer_dispatch;
mod class_constructors;
mod class_gc_roots;
mod class_handles;
mod class_registry;
mod collection_proto_thunks;
mod delete_rest;
mod descriptors;
mod field_get_set;
mod field_set_by_name;
mod global_fetch;
mod global_this;
mod groupby;
pub(crate) mod has_own_helpers;
mod instanceof;
mod native_call_method;
mod native_module;
mod native_module_dispatch;
mod native_module_stream;
mod object_ops;
mod object_ops_frozen;
mod polymorphic_index;
pub(crate) mod prototype_chain;
mod reflect_support;
mod util_types;
pub use alloc::*;
pub use assert::*;
pub(crate) use bigint_dispatch::*;
pub use buffer_dispatch::*;
pub use class_constructors::*;
pub use class_gc_roots::scan_class_inheritance_roots_mut;
#[cfg(test)]
pub(crate) use class_gc_roots::{
    test_class_parent_closure_root, test_class_prototype_object_root,
    test_clear_class_inheritance_roots, test_seed_class_inheritance_roots,
    test_seed_class_parent_closure_root,
};
pub use class_registry::*;
pub use delete_rest::*;
pub use descriptors::*;
pub use field_get_set::*;
pub use field_set_by_name::*;
pub use global_this::*;
pub use groupby::*;
pub use instanceof::*;
pub use native_call_method::*;
pub use native_module::*;
pub(crate) use native_module_dispatch::*;
pub(crate) use native_module_stream::*;
pub use object_ops::*;
pub use object_ops_frozen::*;
pub use polymorphic_index::*;
pub(crate) use reflect_support::*;
pub use util_types::*;

static HTTP_METHODS_CACHE: AtomicU64 = AtomicU64::new(0);
static FS_CONSTANTS_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_SIGNALS_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_ERRNO_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_PRIORITY_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_DLOPEN_CACHE: AtomicU64 = AtomicU64::new(0);
static GLOBAL_THIS_PTR: AtomicI64 = AtomicI64::new(0);
static GLOBAL_THIS_READY: AtomicBool = AtomicBool::new(false);
// #2145: the `%TypedArray%` intrinsic constructor (a closure) and its
// `.prototype` (an object). Lazily allocated by
// `populate_global_this_builtins` so the per-kind typed-array constructors
// (`Int8Array`, ...) can chain `__proto__` to `%TypedArray%`, and each per-kind
// `.prototype` carries `OBJ_FLAG_TYPED_ARRAY_PROTO` whose
// `js_object_get_prototype_of` returns the shared `%TypedArray%.prototype` here.
// Both are mutable roots scanned by `scan_object_cache_roots_mut`.
pub(crate) static TYPED_ARRAY_INTRINSIC_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static TYPED_ARRAY_INTRINSIC_PROTO_PTR: AtomicI64 = AtomicI64::new(0);

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

/// Read implicit `this` using ordinary (non-strict) function binding rules.
#[no_mangle]
pub extern "C" fn js_implicit_this_get_sloppy() -> f64 {
    let value = js_implicit_this_get();
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return js_get_global_this();
    }
    if jv.is_bool() {
        return crate::builtins::js_boxed_boolean_new(value);
    }
    if jv.is_any_string() {
        return crate::builtins::js_boxed_string_new(value);
    }
    let bits = value.to_bits();
    if jv.is_int32()
        || (jv.is_number() && ((bits >> 48) != 0 || bits <= crate::gc::GC_HEADER_SIZE as u64))
    {
        return crate::builtins::js_boxed_number_new(value);
    }
    value
}

/// Set the implicit `this` and return the previous value.
/// Callers must restore the previous value to scope the binding to the
/// duration of a single method-style call.
#[no_mangle]
pub extern "C" fn js_implicit_this_set(value: f64) -> f64 {
    IMPLICIT_THIS.with(|c| f64::from_bits(c.replace(value.to_bits())))
}

/// GC mutable-root scanner for the implicit-`this` cell (issue #1813).
///
/// `IMPLICIT_THIS` holds the NaN-boxed receiver for the duration of a
/// dynamically-dispatched non-arrow method body — set then restored by
/// `js_native_call_method` and by the codegen `js_implicit_this_set`
/// save/restore around `js_native_call_value`. That receiver is a live
/// heap object for the whole call, but the cell is plain thread-local
/// storage, so before this scanner it was invisible to GC: not a root.
///
/// When a moving GC runs *during* the method body — e.g. a nested stdlib
/// pump draining network IO for `@perryts/mysql`'s `Pool.acquire` →
/// handshake → `nativeScramble` under concurrent load — the receiver is
/// evacuated/copied. Without a root slot to rewrite, the cell kept the
/// stale pre-move pointer and the body's next `this`-derived dispatch
/// dereferenced freed/relocated memory: the concurrent-load SIGSEGV in
/// `js_native_call_method` reported in #1813. (It only surfaced under
/// memory pressure because nursery copying / old-gen evacuation only move
/// objects then — hence the load-dependent heisenbug.)
///
/// Marking also keeps `this` reachable when the cell is its only root.
/// Non-pointer tags (the `TAG_UNDEFINED` default, plus null/int/bool)
/// flow through `visit_nanbox_bits` as no-ops, so scanning the idle cell
/// is safe.
pub fn scan_implicit_this_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    IMPLICIT_THIS.with(|c| {
        let mut bits = c.get();
        if visitor.visit_nanbox_u64_slot(&mut bits) {
            c.set(bits);
        }
    });
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
    let cached_slot = OVERFLOW_LAST.with(|c| unsafe {
        let (cached_obj, cached_vec) = *c.get();
        if cached_obj == obj_ptr && !cached_vec.is_null() {
            let v = &mut *cached_vec;
            if v.len() <= field_index {
                v.resize(field_index + 1, crate::value::TAG_UNDEFINED);
            }
            let slot = v.get_unchecked_mut(field_index);
            *slot = vbits;
            Some(slot as *mut u64 as usize)
        } else {
            None
        }
    });
    if let Some(slot_addr) = cached_slot {
        crate::gc::layout_note_slot(obj_ptr, field_index, vbits);
        crate::gc::runtime_write_barrier_external_slot(obj_ptr, slot_addr, vbits);
        return;
    }
    let mut slot_addr = 0;
    OVERFLOW_FIELDS.with(|m| {
        let mut map = m.borrow_mut();
        let v = map.entry(obj_ptr).or_default();
        if v.len() <= field_index {
            v.resize(field_index + 1, crate::value::TAG_UNDEFINED);
        }
        v[field_index] = vbits;
        slot_addr = (&mut v[field_index]) as *mut u64 as usize;
        let vec_ptr = v as *mut Vec<u64>;
        OVERFLOW_LAST.with(|c| unsafe {
            *c.get() = (obj_ptr, vec_ptr);
        });
    });
    crate::gc::layout_note_slot(obj_ptr, field_index, vbits);
    crate::gc::runtime_write_barrier_external_slot(obj_ptr, slot_addr, vbits);
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

/// Has any property descriptor or accessor ever been installed in this
/// process? Used by inspect/format code paths to skip per-key
/// descriptor lookups on objects whose enumerability hasn't been
/// touched (the common case). Relaxed load is fine — false positives
/// are harmless (just an extra HashMap lookup) and false negatives
/// can't happen because the store happens before the property is
/// observable.
pub(crate) fn descriptors_in_use() -> bool {
    GLOBAL_DESCRIPTORS_IN_USE.load(Ordering::Relaxed)
}

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

/// #2766: resolve an accessor *getter* closure for `(value, key)` if one is
/// installed (e.g. an object-literal `get x() {…}` or
/// `Object.defineProperty(obj, k, { get })`). Returns the NaN-boxed getter
/// closure bits, or `0` when no getter exists. Used by `Reflect.get(target,
/// key, receiver)` so it can rebind the getter's `this` to the receiver before
/// invoking it. Returns `None` (rather than reading the field) when there is no
/// accessor at all, so the caller falls back to an ordinary field read.
pub(crate) fn reflect_getter_closure_bits(value: f64, key: f64) -> Option<u64> {
    if !ACCESSORS_IN_USE.with(|c| c.get()) {
        return None;
    }
    let obj = unsafe { extract_obj_ptr(value) };
    if obj.is_null() {
        return None;
    }
    let key_str = crate::builtins::js_string_coerce(key);
    if key_str.is_null() {
        return None;
    }
    let name = unsafe {
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s.to_string(),
            Err(_) => return None,
        }
    };
    let acc = get_accessor_descriptor(obj as usize, &name)?;
    if acc.get != 0 {
        Some(acc.get)
    } else {
        // Accessor exists but has no getter → reading yields undefined; signal
        // that via 0 so the caller returns undefined rather than a field read.
        Some(0)
    }
}

/// Store an accessor descriptor for (obj, key).
pub(crate) fn set_accessor_descriptor(obj: usize, key: String, acc: AccessorDescriptor) {
    ACCESSORS_IN_USE.with(|c| c.set(true));
    GLOBAL_DESCRIPTORS_IN_USE.store(true, Ordering::Relaxed);
    ACCESSOR_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), acc);
    });
}

/// Install a built-in *reflection-only* accessor descriptor for (obj, key)
/// WITHOUT flipping the process-wide `GLOBAL_DESCRIPTORS_IN_USE` /
/// `ACCESSORS_IN_USE` / `PROPERTY_ATTRS_IN_USE` hot-path gates.
///
/// `Object.getOwnPropertyDescriptor` reads `ACCESSOR_DESCRIPTORS` and
/// `PROPERTY_DESCRIPTORS` *unconditionally*, so the descriptor is fully
/// reflectable — but the hot object get/set paths (which only consult the
/// side tables once a gate has flipped) keep skipping the HashMap lookup.
/// This matters because built-in prototype accessors such as
/// `%TypedArray%.prototype.length` are installed lazily at globalThis
/// init for *every* program that merely touches a builtin global; flipping
/// the gate there would slow the property-write fast path process-wide for
/// no behavioral gain (these accessors have no setter and are never written
/// in real workloads — they exist purely so reflection sees them). See #2060.
pub(crate) fn set_builtin_accessor_descriptor(
    obj: usize,
    key: String,
    acc: AccessorDescriptor,
    attrs: PropertyAttrs,
) {
    ACCESSOR_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key.clone()), acc);
    });
    PROPERTY_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), attrs);
    });
}

/// Install a built-in *reflection-only* data-property descriptor for (obj, key)
/// WITHOUT flipping the process-wide `GLOBAL_DESCRIPTORS_IN_USE` /
/// `PROPERTY_ATTRS_IN_USE` hot-path gates — the data-property analogue of
/// [`set_builtin_accessor_descriptor`].
///
/// Built-in prototype methods are spec'd as `{ writable: true,
/// enumerable: false, configurable: true }`, but `install_proto_method`
/// stores them via the ordinary field-set path (default all-true), so
/// `Object.getOwnPropertyDescriptor(Array.prototype, "map").enumerable` and a
/// `for (k in Array.prototype)` scan both reported them as enumerable —
/// failing Test262's pervasive `verifyProperty` checks. Recording a
/// non-enumerable descriptor here fixes all three observation paths
/// (`getOwnPropertyDescriptor`, `Object.keys`, `for-in`), each of which reads
/// `PROPERTY_DESCRIPTORS` per-object and unconditionally. The gate stays
/// down, so the object get/set hot path is unaffected for every program.
pub(crate) fn set_builtin_property_attrs(obj: usize, key: String, attrs: PropertyAttrs) {
    PROPERTY_DESCRIPTORS.with(|m| {
        m.borrow_mut().insert((obj, key), attrs);
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
            // GC_STORE_AUDIT(ROOT): SHAPE_INLINE_CACHE entries are scanned by scan_shape_cache_roots_mut.
            let entry = &mut (*cache.get())[slot];
            entry.shape_id = shape_id;
            crate::gc::runtime_store_root_raw_mut_ptr_slot(&mut entry.keys_array, keys_array);
        }
    });
    SHAPE_CACHE_OVERFLOW.with(|m| {
        m.borrow_mut().insert(shape_id, keys_array);
        crate::gc::runtime_write_barrier_root_raw_ptr(keys_array);
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
    target_len: u32,  // offset 28, nonzero when target was validated at insert
}

const TRANSITION_CACHE_SIZE: usize = 16384;
/// Mask for slot computation: TRANSITION_CACHE_SIZE - 1
///
/// #854: kept alongside the size constant so future cache-resizing edits
/// touch both in one place. Codegen-emitted slot-index expressions match
/// against this value even when no Rust path consults it directly.
#[allow(dead_code)]
const TRANSITION_CACHE_MASK: usize = TRANSITION_CACHE_SIZE - 1;

/// Per-thread transition cache. Was a process-wide `static mut`, but with
/// `perry/thread` user code allocating objects on worker threads each
/// thread has its own arena — cached `next_keys` / `key_ptr` pointers
/// from another thread are use-after-free in our address space. The
/// previous `#[no_mangle]` exposed the symbol for inline LLVM lookups
/// but a grep across crates/perry-codegen confirms no codegen path ever
/// resolved against it, so the export was dead.
thread_local! {
    static TRANSITION_CACHE_GLOBAL: std::cell::UnsafeCell<[TransitionEntry; TRANSITION_CACHE_SIZE]> =
        const { std::cell::UnsafeCell::new([TransitionEntry {
            prev_keys: 0,
            key_ptr: 0,
            next_keys: 0,
            slot_idx: 0,
            target_len: 0,
        }; TRANSITION_CACHE_SIZE]) };
}

#[inline]
fn with_transition_cache<R>(
    f: impl FnOnce(*mut [TransitionEntry; TRANSITION_CACHE_SIZE]) -> R,
) -> R {
    TRANSITION_CACHE_GLOBAL.with(|c| f(c.get()))
}

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
/// On HIT we ensure the returned keys_array has
/// `GC_FLAG_SHAPE_SHARED` because the caller is about to reuse it for
/// a SECOND object — any future extension on either object must now
/// clone-before-mutate. We eagerly stabilize small dynamic shapes on
/// insert so repeated row-object builders get valid cache targets;
/// larger shapes stay lazy to avoid O(N²) prefix cloning for one-off
/// dictionaries and are validated on lookup.
#[inline(always)]
fn transition_cache_lookup(
    prev_keys: usize,
    interned_key: *const crate::StringHeader,
) -> Option<(usize, u32)> {
    let kp = interned_key as usize;
    let slot = transition_cache_slot(prev_keys, kp);
    let entry = with_transition_cache(|t| unsafe { (*t)[slot] });
    if entry.next_keys != 0 && entry.prev_keys == prev_keys && entry.key_ptr == kp {
        let expected_len = entry.slot_idx.checked_add(1)?;
        if entry.target_len == expected_len {
            return Some((entry.next_keys, entry.slot_idx));
        }
        // Stamp SHAPE_SHARED on the returned keys_array — this is the
        // moment we observe that a SECOND object is reusing the
        // pre-existing shape. Both this caller and the original
        // owner (whose keys_array points at the same memory) must
        // now treat the array as shared.
        unsafe {
            if !transition_cache_stamp_shape_shared(entry.next_keys) {
                return None;
            }
            let keys = entry.next_keys as *const ArrayHeader;
            if (*keys).length != expected_len || (*keys).length > (*keys).capacity {
                return None;
            }
        }
        Some((entry.next_keys, entry.slot_idx))
    } else {
        None
    }
}

const TRANSITION_CACHE_EAGER_SHARE_MAX_SLOT: u32 = 64;

#[inline(always)]
unsafe fn transition_cache_stamp_shape_shared(next_keys: usize) -> bool {
    if next_keys < crate::gc::GC_HEADER_SIZE {
        return false;
    }
    let gc_header = (next_keys as *const u8).wrapping_sub(crate::gc::GC_HEADER_SIZE)
        as *mut crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_ARRAY {
        return false;
    }
    (*gc_header).gc_flags |= crate::gc::GC_FLAG_SHAPE_SHARED;
    true
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
    let mut target_len = 0;
    unsafe {
        if slot_idx < TRANSITION_CACHE_EAGER_SHARE_MAX_SLOT
            && transition_cache_stamp_shape_shared(next_keys)
        {
            let expected_len = slot_idx.saturating_add(1);
            let keys = next_keys as *const ArrayHeader;
            if (*keys).length == expected_len && (*keys).length <= (*keys).capacity {
                target_len = expected_len;
            }
        }
    }
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): TRANSITION_CACHE_GLOBAL entries are scanned by scan_transition_cache_roots_mut.
        let entry = &mut (*t)[slot];
        entry.prev_keys = prev_keys;
        entry.key_ptr = kp;
        crate::gc::runtime_store_root_usize_slot(&mut entry.next_keys, next_keys);
        entry.slot_idx = slot_idx;
        entry.target_len = target_len;
    });
    // Small dynamic shapes are stabilized eagerly because otherwise
    // the original builder can grow the cached target in place and
    // force future lookups to reject it. Large one-off dictionaries
    // stay lazy to avoid cloning every growing prefix.
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
    with_transition_cache(|table| unsafe {
        for i in 0..TRANSITION_CACHE_SIZE {
            let entry = &mut (*table)[i];
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
                        target_len: 0,
                    };
                }
            }
        }
    });
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

pub(crate) fn visit_overflow_field_slots_mut(owner: usize, mut visit: impl FnMut(*mut u64)) {
    if owner == 0 {
        return;
    }
    let slots = OVERFLOW_FIELDS.with(|m| {
        let map = m.borrow();
        let Some(fields) = map.get(&owner) else {
            return Vec::new();
        };
        if fields.is_empty() {
            return Vec::new();
        }
        let mut slots = Vec::new();
        let base = fields.as_ptr() as *mut u64;
        if crate::gc::layout_visit_pointer_slots_for_user(owner, fields.len(), |i| {
            if i < fields.len() {
                unsafe {
                    slots.push(base.add(i));
                }
            }
        }) {
            return slots;
        }
        for i in 0..fields.len() {
            unsafe {
                slots.push(base.add(i));
            }
        }
        slots
    });
    for slot in slots {
        visit(slot);
    }
}

fn merge_overflow_fields(owner_fields: &mut Vec<u64>, moved_fields: Vec<u64>) {
    if owner_fields.len() < moved_fields.len() {
        owner_fields.resize(moved_fields.len(), crate::value::TAG_UNDEFINED);
    }
    for (i, bits) in moved_fields.into_iter().enumerate() {
        if bits != crate::value::TAG_UNDEFINED {
            owner_fields[i] = bits;
        }
    }
}

pub(crate) fn overflow_fields_owner_moved(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner {
        return;
    }
    OVERFLOW_FIELDS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(old_fields) = map.remove(&old_owner) else {
            return;
        };
        match map.entry(new_owner) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                merge_overflow_fields(entry.get_mut(), old_fields);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(old_fields);
            }
        }
    });
    OVERFLOW_LAST.with(|c| unsafe {
        *c.get() = (0, std::ptr::null_mut());
    });
}

pub fn scan_object_cache_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_object_cache_roots_mut(&mut visitor);
}

pub fn scan_object_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    visitor.visit_atomic_nanbox_u64_slot(&HTTP_METHODS_CACHE, Ordering::Relaxed, Ordering::Relaxed);
    visitor.visit_atomic_nanbox_u64_slot(&FS_CONSTANTS_CACHE, Ordering::Relaxed, Ordering::Relaxed);
    visitor.visit_atomic_nanbox_u64_slot(&OS_CONSTANTS_CACHE, Ordering::Relaxed, Ordering::Relaxed);
    visitor.visit_atomic_nanbox_u64_slot(
        &OS_CONSTANTS_SIGNALS_CACHE,
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
    visitor.visit_atomic_nanbox_u64_slot(
        &OS_CONSTANTS_ERRNO_CACHE,
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
    visitor.visit_atomic_nanbox_u64_slot(
        &OS_CONSTANTS_PRIORITY_CACHE,
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
    visitor.visit_atomic_nanbox_u64_slot(
        &OS_CONSTANTS_DLOPEN_CACHE,
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
    visitor.visit_atomic_i64_slot(&GLOBAL_THIS_PTR, Ordering::Acquire, Ordering::Release);
    visitor.visit_atomic_i64_slot(
        &TYPED_ARRAY_INTRINSIC_PTR,
        Ordering::Acquire,
        Ordering::Release,
    );
    visitor.visit_atomic_i64_slot(
        &TYPED_ARRAY_INTRINSIC_PROTO_PTR,
        Ordering::Acquire,
        Ordering::Release,
    );
}

#[cfg(test)]
pub(crate) fn test_seed_shape_cache_root(shape_id: u32, keys_array: *mut ArrayHeader) {
    SHAPE_INLINE_CACHE.with(|cache| {
        let slot = (shape_id as usize) & (SHAPE_INLINE_CACHE_SIZE - 1);
        unsafe {
            // GC_STORE_AUDIT(ROOT): test seed mirrors SHAPE_INLINE_CACHE roots scanned by scan_shape_cache_roots_mut.
            let entry = &mut (*cache.get())[slot];
            entry.shape_id = shape_id;
            crate::gc::runtime_store_root_raw_mut_ptr_slot(&mut entry.keys_array, keys_array);
        }
    });
    SHAPE_CACHE_OVERFLOW.with(|cache| {
        cache.borrow_mut().clear();
        cache.borrow_mut().insert(shape_id, keys_array);
        crate::gc::runtime_write_barrier_root_raw_ptr(keys_array);
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
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test seed mirrors TRANSITION_CACHE_GLOBAL roots scanned by scan_transition_cache_roots_mut.
        let entry = &mut (*t)[0];
        entry.prev_keys = 0;
        entry.key_ptr = 0;
        crate::gc::runtime_store_root_usize_slot(&mut entry.next_keys, next_keys);
        entry.slot_idx = 0;
        entry.target_len = 0;
    });
}

#[cfg(test)]
pub(crate) fn test_transition_cache_root() -> usize {
    with_transition_cache(|t| unsafe { (*t)[0].next_keys })
}

#[cfg(test)]
pub(crate) fn test_clear_transition_cache_root() {
    with_transition_cache(|t| unsafe {
        for i in 0..TRANSITION_CACHE_SIZE {
            // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
            (*t)[i] = TransitionEntry {
                prev_keys: 0,
                key_ptr: 0,
                next_keys: 0,
                slot_idx: 0,
                target_len: 0,
            };
        }
    });
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

#[cfg(test)]
pub(crate) fn test_overflow_field_bits(owner: usize, index: usize) -> u64 {
    OVERFLOW_FIELDS.with(|m| {
        m.borrow()
            .get(&owner)
            .and_then(|fields| fields.get(index).copied())
            .unwrap_or(0)
    })
}

#[cfg(test)]
pub(crate) fn test_seed_object_cache_roots(object_cache_bits: [u64; 7], global_this_ptr: i64) {
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &HTTP_METHODS_CACHE,
        object_cache_bits[0],
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &FS_CONSTANTS_CACHE,
        object_cache_bits[1],
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_CACHE,
        object_cache_bits[2],
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_SIGNALS_CACHE,
        object_cache_bits[3],
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_ERRNO_CACHE,
        object_cache_bits[4],
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_PRIORITY_CACHE,
        object_cache_bits[5],
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test seed mirrors object cache roots scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_DLOPEN_CACHE,
        object_cache_bits[6],
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test seed mirrors GLOBAL_THIS_PTR scanned by scan_object_cache_roots_mut.
    crate::gc::runtime_store_root_atomic_raw_i64(
        &GLOBAL_THIS_PTR,
        global_this_ptr,
        Ordering::Release,
    );
    GLOBAL_THIS_READY.store(true, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_object_cache_roots() -> ([u64; 7], i64) {
    (
        [
            HTTP_METHODS_CACHE.load(Ordering::Relaxed),
            FS_CONSTANTS_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_SIGNALS_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_ERRNO_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_PRIORITY_CACHE.load(Ordering::Relaxed),
            OS_CONSTANTS_DLOPEN_CACHE.load(Ordering::Relaxed),
        ],
        GLOBAL_THIS_PTR.load(Ordering::Acquire),
    )
}

#[cfg(test)]
pub(crate) fn test_clear_object_cache_roots() {
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    crate::gc::runtime_store_root_atomic_nanbox_u64(&HTTP_METHODS_CACHE, 0, Ordering::Relaxed);
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    crate::gc::runtime_store_root_atomic_nanbox_u64(&FS_CONSTANTS_CACHE, 0, Ordering::Relaxed);
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    crate::gc::runtime_store_root_atomic_nanbox_u64(&OS_CONSTANTS_CACHE, 0, Ordering::Relaxed);
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_SIGNALS_CACHE,
        0,
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_ERRNO_CACHE,
        0,
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_PRIORITY_CACHE,
        0,
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned object cache roots.
    crate::gc::runtime_store_root_atomic_nanbox_u64(
        &OS_CONSTANTS_DLOPEN_CACHE,
        0,
        Ordering::Relaxed,
    );
    // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinel into scanned GLOBAL_THIS_PTR.
    crate::gc::runtime_store_root_atomic_raw_i64(&GLOBAL_THIS_PTR, 0, Ordering::Release);
    GLOBAL_THIS_READY.store(false, Ordering::Release);
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

pub(crate) fn web_stream_to_string_tag(value: f64) -> Option<&'static str> {
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 {
        return None;
    }
    let kind_probe = stream_handle_kind_probe()?;
    match unsafe { kind_probe(value as usize) } {
        1 => Some("ReadableStream"),
        2 => Some("WritableStream"),
        5 => Some("TransformStream"),
        _ => None,
    }
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
    let raw_addr = if jsv.is_pointer() {
        (bits & POINTER_MASK) as usize
    } else if bits > 0x1000 && (bits >> 48) == 0 {
        bits as usize
    } else {
        0
    };
    if raw_addr >= 0x1000 && crate::buffer::is_registered_buffer(raw_addr) {
        let tag = if crate::buffer::is_array_buffer(raw_addr) {
            "ArrayBuffer"
        } else if crate::buffer::is_shared_array_buffer(raw_addr) {
            "SharedArrayBuffer"
        } else if crate::buffer::is_data_view(raw_addr) {
            "DataView"
        } else {
            "Uint8Array"
        };
        let formatted = format!("[object {}]", tag);
        let bytes = formatted.as_bytes();
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        return f64::from_bits(STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    }
    if let Some(tag) = web_stream_to_string_tag(value) {
        let formatted = format!("[object {}]", tag);
        let bytes = formatted.as_bytes();
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
    let raw_ptr = raw_addr as *const u8;
    if !raw_ptr.is_null() && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        let gc_header = raw_ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;
        if gc_type == crate::gc::GC_TYPE_ARRAY || gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            // #3553: a function's `arguments` object is represented as an array
            // carrying the GC_ARRAY_ARGUMENTS_OBJECT flag. Node tags it
            // `[object Arguments]`, not `[object Array]`.
            let bytes: &[u8] = if crate::array::array_has_arguments_object_flag(
                raw_addr as *const crate::array::ArrayHeader,
            ) {
                b"[object Arguments]"
            } else {
                b"[object Array]"
            };
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
            // #1479: native-module namespaces don't go through the
            // class toStringTag hook (they share one synthetic
            // class_id), so look them up by module name. Node tags
            // `performance` as "Performance" — wire that up here so
            // `Object.prototype.toString.call(performance)` matches.
            if tag_str.is_none() && class_id == crate::object::native_module::NATIVE_MODULE_CLASS_ID
            {
                if let Some(module_name) =
                    crate::object::native_module::read_native_module_name(obj_ptr)
                {
                    if let Some(tag) = native_module_to_string_tag(&module_name) {
                        tag_str = Some(tag.to_string());
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

/// #1479: Map a native-module name (as stored in the namespace
/// ObjectHeader's field 0) to its `Symbol.toStringTag` value. Only
/// modules whose namespace is exposed as a singleton with a defined
/// Node tag belong here — others fall back to "Object" via the
/// caller's `None` arm.
fn native_module_to_string_tag(module: &str) -> Option<&'static str> {
    match module {
        // `Object.prototype.toString.call(performance)` is
        // "[object Performance]" in Node.
        "perf_hooks" => Some("Performance"),
        "crypto.webcrypto" => Some("Crypto"),
        "crypto.subtle" => Some("SubtleCrypto"),
        _ => None,
    }
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

/// Check if a pointer is a valid heap object (safe to dereference GcHeader).
/// Values below 0x100000 (1MB) are likely INT32_TAG extracts, small handles,
/// or null. The upper bound filters out NaN-box tag bits that leaked through.
///
/// Issue #73 follow-up: raised the lower bound from 1 MB to 2 TB to reject
/// corrupted NaN-boxes whose 48-bit handle lands in the 1-2 TB window
/// (e.g. `0x00FF_0000_0000` from an `ArrayHeader { length: 0, capacity:
/// 255 }` read as u64). Real macOS mimalloc + arena allocations all
/// land in the 3-5 TB range; anything below 2 TB is certainly bogus on
/// that platform. Linux glibc and Windows mimalloc allocate well below
/// 2 TB though (often in the GB-to-tens-of-GB range), so the macOS floor
/// silently rejects every legitimate object pointer there — issues
/// #385/#386/#387 traced back to this exact filter on Windows.
///
/// #1136 / #1129: iOS-family *device* targets (aarch64-apple-ios,
/// -tvos, -watchos, -visionos) ship without mimalloc and use
/// libsystem_malloc, whose user allocations land in the same low range
/// as Android/Linux/Windows. Treat them like those platforms — the
/// downstream `GcHeader.obj_type` check is the real liveness guard.
/// The simulator (e.g. ios + target_abi = "sim") runs on the macOS
/// host's mimalloc so its allocations still land above 2 TB; lowering
/// the floor here is safe because the obj_type validation does the
/// work.
#[inline(always)]
pub(crate) fn is_valid_obj_ptr(ptr: *const u8) -> bool {
    let addr = ptr as u64;
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    ))]
    const HEAP_MIN: u64 = 0x1000;
    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    )))]
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
    // GC_STORE_AUDIT(BARRIERED): keys_array pointer field is followed by an object-slot barrier.
    (*obj).keys_array = keys_array;
    crate::gc::runtime_write_barrier_slot(
        obj as usize,
        &(*obj).keys_array as *const _ as usize,
        keys_array as u64,
    );
}

#[inline]
// #854: object field-slot bookkeeping helper retained for shape tracking
#[allow(dead_code)]
pub(super) unsafe fn note_object_field_slot(
    obj: *mut ObjectHeader,
    field_index: usize,
    value_bits: u64,
) {
    crate::gc::layout_note_slot(obj as usize, field_index, value_bits);
}

#[inline]
pub(crate) unsafe fn store_object_field_slot(
    obj: *mut ObjectHeader,
    field_index: usize,
    value_bits: u64,
) {
    let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    let slot = fields_ptr.add(field_index);
    crate::gc::runtime_store_jsvalue_slot(obj as usize, slot as usize, field_index, value_bits);
}

#[inline]
pub(super) unsafe fn mark_object_dynamic_shape_unknown(obj: *mut ObjectHeader) {
    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return;
    }
    let header = (obj as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    let state = (*header)._reserved & crate::gc::GC_LAYOUT_STATE_MASK;
    if state != crate::gc::GC_LAYOUT_SIDE_MASK
        && !crate::gc::layout_has_typed_descriptor(obj as usize)
    {
        return;
    }
    crate::gc::layout_mark_unknown(obj as *mut u8);
}

pub(crate) unsafe fn gc_keys_array_slot(obj: *mut ObjectHeader) -> Option<*mut u64> {
    if obj.is_null() || (*obj).keys_array.is_null() {
        return None;
    }
    Some(&mut (*obj).keys_array as *mut _ as *mut u64)
}

pub(crate) unsafe fn gc_field_slot_range(
    obj: *mut ObjectHeader,
) -> Option<crate::gc::HeapSlotRange> {
    if obj.is_null() {
        return None;
    }
    let field_count = (*obj).field_count as usize;
    if field_count > 1_000_000 {
        return None;
    }
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    Some(crate::gc::HeapSlotRange::new(fields, field_count))
}

#[inline]
pub(super) unsafe fn rebuild_object_field_layout(obj: *mut ObjectHeader, slot_count: usize) {
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    crate::gc::layout_rebuild_from_slots(obj as *mut u8, fields, slot_count);
    if crate::arena::pointer_in_old_gen(obj as usize) {
        for i in 0..slot_count {
            let slot = fields.add(i);
            crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, *slot);
        }
    }
}

#[inline]
pub(super) unsafe fn rebuild_array_layout_from_slots(arr: *mut ArrayHeader) {
    if arr.is_null() {
        return;
    }
    let len = (*arr).length as usize;
    let slots = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
    crate::gc::layout_rebuild_from_slots(arr as *mut u8, slots, len);
    if crate::arena::pointer_in_old_gen(arr as usize) {
        for i in 0..len {
            let slot = slots.add(i);
            crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        }
    }
}
#[cfg(test)]
mod tests;
