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
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::RwLock;

/// Minimum number of inline field slots every object is allocated with, even
/// when it has fewer fields. This is a corruption-critical invariant: allocation,
/// every field get/set bounds check, and every direct-slot read MUST use the
/// SAME floor, or a write/read past the allocated slots corrupts the heap. It is
/// centralized here so all sites move in lockstep. (Also mirrored in
/// perry-codegen `lower_call/new.rs` MIN_FIELD_SLOTS for the PERRY_INLINE_NEW path.)
pub(crate) const INLINE_SLOT_FLOOR: usize = 4;

// Submodules (issue #1103): behavior-preserving split of the former
// 11.2k-line object.rs. Public re-exports keep FFI symbols stable.
mod alloc;
mod arguments;
mod array_object_ops;
mod assert;
mod async_generator_queue;
mod bigint_dispatch;
mod buffer_dispatch;
mod class_constructors;
mod class_gc_roots;
mod class_handles;
mod class_registry;
mod collection_proto_thunks;
mod data_view_registry;
mod dataview_proto_thunks;
mod date_proto_thunks;
mod delete_rest;
mod descriptors;
mod disposable_proto_thunks;
pub(crate) mod exotic_expando;
mod field_get_set;
mod field_set_by_name;
mod global_fetch;
mod global_this;
pub mod handle_expando;
pub(crate) mod prop_plan;
pub(crate) use global_this::{default_prepare_stack_trace_func_ptr, ERROR_CONSTRUCTOR_PTR};
mod global_this_tables;
mod groupby;
pub(crate) mod has_own_helpers;
mod instanceof;
pub(crate) mod iterator_prototypes;
pub(crate) mod map_set_subclass;
mod namespace_create;
mod native_call_method;
mod native_module;
pub(crate) use native_module::class_instance_has_member;
pub(crate) use native_module::class_ref_id;
pub(crate) use native_module::install_native_module_vtable;
pub(crate) use native_module::{class_prototype_ref_id, SYMBOL_BOUND_METHOD_NAME};
mod native_module_crypto_key_object;
mod native_module_crypto_random;
mod native_module_dispatch;
mod native_module_dispatch_crypto;
mod native_module_registry;
pub(crate) use native_module_registry::js_nm_enable_install_all;
pub(crate) use native_module_registry::nm_ctor_lookup;
// Re-exported for submodule installers that delegate to a native module
// (`fs/promises` → `fs.constants`, `sys` → `util`).
pub(crate) use native_module_registry::{js_nm_install_fs, js_nm_install_util};
mod native_module_stream;
mod native_this_alias;
mod object_literal_ops;
mod object_ops;
pub(crate) use object_ops::{ensure_key_in_keys_array, install_builtin_getter};
mod object_ops_frozen;
mod polymorphic_index;
mod primitive_proto_thunks;
mod property_key;
pub(crate) mod prototype_chain;
mod prototype_helpers;
mod reflect_support;
mod regex_proto_thunks;
mod string_proto_thunks;
#[cfg(feature = "temporal")]
mod temporal_proto;
mod typed_array_define;
mod typed_array_proto_thunks;
mod util_types;
mod websocket_global;
mod with_env;
// Issue #1103 follow-up: behavior-preserving split of the residual top-level
// helpers that lived directly in `object/mod.rs`.
mod class_meta_registry;
pub(crate) mod descriptor_state;
mod this_binding;
mod to_string_tag;
pub use alloc::*;
pub use arguments::*;
pub(crate) use array_object_ops::*;
pub use assert::*;
pub(crate) use async_generator_queue::is_async_generator_instance_value;
pub(crate) use bigint_dispatch::*;
pub use buffer_dispatch::*;
pub use class_constructors::*;
pub use class_gc_roots::scan_class_inheritance_roots_mut;
#[cfg(test)]
pub(crate) use class_gc_roots::{
    test_class_parent_closure_root, test_class_prototype_object_root,
    test_clear_class_inheritance_roots, test_decl_class_prototype_root,
    test_seed_class_inheritance_roots, test_seed_class_parent_closure_root,
    test_seed_decl_class_prototype_root,
};
pub use class_registry::*;
pub(crate) use collection_proto_thunks::{is_builtin_map_set_value, is_builtin_set_add_value};
pub(crate) use data_view_registry::extends_builtin_data_view;
pub use delete_rest::*;
pub use descriptors::*;
pub use exotic_expando::scan_exotic_expando_roots_mut;
pub use field_get_set::*;
pub use field_set_by_name::*;
pub use global_this::*;
pub(crate) use global_this_tables::*;
pub use groupby::*;
pub use instanceof::*;
pub(crate) use iterator_prototypes::{attach_iterator_prototype, iterator_prototype_for_class_id};
pub use namespace_create::*;
pub use native_call_method::*;
pub use native_module::*;
pub(crate) use native_module_dispatch::*;
pub(crate) use native_module_stream::*;
pub use object_literal_ops::*;
pub use object_ops::*;
pub use object_ops_frozen::*;
pub use polymorphic_index::*;
pub(crate) use primitive_proto_thunks::primitive_proto_method_value;
pub use property_key::*;
pub(crate) use prototype_helpers::*;
pub(crate) use reflect_support::*;
pub(crate) use typed_array_define::{
    typed_array_define_own_property, typed_array_own_index, TypedArrayDefineOutcome,
    TypedArrayOwnIndex,
};
pub use util_types::*;
pub use with_env::*;
// Re-exports for the residual-helper split (issue #1103 follow-up). Explicit
// named re-exports keep existing `crate::object::X` / bare-name call sites in
// the object submodules resolving unchanged.
pub(crate) use class_meta_registry::{
    extends_builtin_error, fetch_parent_kind, lookup_has_instance_hook, lookup_to_string_tag_hook,
    register_fetch_parent_kind, CLASS_REGISTRY,
};
pub use class_meta_registry::{
    js_register_class_extends_error, js_register_class_has_instance,
    js_register_class_to_string_tag,
};
pub use descriptor_state::PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED;
pub(crate) use descriptor_state::{
    accessor_descriptor_keys_for_obj, class_instance_set_may_intercept, clear_accessor_descriptor,
    clear_property_attrs, constructor_accessor_ever_installed, descriptors_in_use,
    disable_class_field_inline_guard, get_accessor_descriptor, get_property_attrs,
    json_object_getter_value, mark_all_keys, object_has_descriptors,
    object_proto_may_intercept_key, plain_data_write_may_intercept,
    prune_dead_descriptor_owner_entries, reflect_getter_closure_bits, set_accessor_descriptor,
    set_builtin_accessor_descriptor, set_builtin_property_attrs, set_property_attrs,
    AccessorDescriptor, PropertyAttrs, ACCESSORS_IN_USE, ACCESSOR_DESCRIPTORS,
    PROPERTY_ATTRS_IN_USE, PROPERTY_DESCRIPTORS,
};
pub use this_binding::{
    js_implicit_this_get, js_implicit_this_get_sloppy, js_implicit_this_set, js_new_target_get,
    js_new_target_set, js_static_this_arm_classref, js_static_this_arm_value,
    js_static_this_resolve,
};
pub(crate) use this_binding::{
    scan_implicit_this_roots_mut, static_this_arm, static_this_arm_if_unarmed, static_this_disarm,
    IMPLICIT_THIS,
};
pub use to_string_tag::js_object_to_string;
pub(crate) use to_string_tag::typed_array_to_string_tag_name;

static HTTP_METHODS_CACHE: AtomicU64 = AtomicU64::new(0);
static FS_CONSTANTS_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_SIGNALS_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_ERRNO_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_PRIORITY_CACHE: AtomicU64 = AtomicU64::new(0);
static OS_CONSTANTS_DLOPEN_CACHE: AtomicU64 = AtomicU64::new(0);
static GLOBAL_THIS_PTR: AtomicI64 = AtomicI64::new(0);
static GLOBAL_THIS_READY: AtomicBool = AtomicBool::new(false);
// `%TypedArray%` intrinsic constructor/prototype roots used by per-kind typed
// array constructors and scanned by `scan_object_cache_roots_mut`.
pub(crate) static TYPED_ARRAY_INTRINSIC_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static TYPED_ARRAY_INTRINSIC_PROTO_PTR: AtomicI64 = AtomicI64::new(0);
// #3664: the generator / async-generator intrinsic prototype towers.
// `*_FUNCTION_INTRINSIC_PTR` = `%GeneratorFunction%` / `%AsyncGeneratorFunction%`
// (the constructor closures); `*_INTRINSIC_PROTO_PTR` = `%Generator%` /
// `%AsyncGenerator%` (a.k.a. `<Ctor>.prototype`), the object
// `Object.getPrototypeOf(function*(){})` resolves to; `*_PROTOTYPE_PTR` =
// `%Generator.prototype%` / `%AsyncGenerator.prototype%` (a.k.a.
// `<Ctor>.prototype.prototype`), carrying `next`/`return`/`throw`. All six are
// GC roots scanned by `scan_object_cache_roots_mut`.
pub(crate) static GENERATOR_FUNCTION_INTRINSIC_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static GENERATOR_INTRINSIC_PROTO_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static GENERATOR_PROTOTYPE_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static ASYNC_GENERATOR_INTRINSIC_PROTO_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static ASYNC_GENERATOR_PROTOTYPE_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static LOCAL_STORAGE_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static SESSION_STORAGE_PTR: AtomicI64 = AtomicI64::new(0);

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
/// Learned per-class inline sizing: the dynamic-construct path allocates 8
/// inline slots (it cannot see the constructor body), so a 23-field
/// ES5-pattern object keeps 15 fields in [`OVERFLOW_FIELDS`] — a `Vec<u64>`
/// plus map entry per object (~250B, more than the object payload), visited,
/// rekeyed and finalized by every GC cycle. The FIRST instance that
/// overflows records its class's high-water field index here; every LATER
/// `new` of the same (synthetic or registered) class right-sizes its
/// allocation so all fields land inline. Capped so a pathological dynamic
/// writer can't inflate every future instance.
const LEARNED_INLINE_MAX_FIELDS: u32 = 64;
const LEARNED_INLINE_TABLE_SIZE: usize = 1024;

thread_local! {
    static LEARNED_INLINE_FIELDS: std::cell::UnsafeCell<[(u32, u32); LEARNED_INLINE_TABLE_SIZE]> =
        const { std::cell::UnsafeCell::new([(0u32, 0u32); LEARNED_INLINE_TABLE_SIZE]) };
}

#[inline]
fn note_learned_inline_fields(class_id: u32, needed_fields: u32) {
    if class_id == 0 || needed_fields > LEARNED_INLINE_MAX_FIELDS {
        return;
    }
    let slot = (class_id as usize).wrapping_mul(0x9E37_79B1) % LEARNED_INLINE_TABLE_SIZE;
    LEARNED_INLINE_FIELDS.with(|t| unsafe {
        let e = &mut (*t.get())[slot];
        if e.0 != class_id {
            *e = (class_id, needed_fields);
        } else if e.1 < needed_fields {
            e.1 = needed_fields;
        }
    });
}

/// Inline field count to pre-size a dynamic construct of `class_id` with —
/// the learned high-water mark, or 0 when nothing was learned (caller keeps
/// its default).
#[inline]
pub(crate) fn learned_inline_field_count(class_id: u32) -> u32 {
    // Bisection kill-switch: PERRY_LEARNED_INLINE=0 disables consumption.
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_LEARNED_INLINE").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    }) {
        return 0;
    }
    if class_id == 0 {
        return 0;
    }
    let slot = (class_id as usize).wrapping_mul(0x9E37_79B1) % LEARNED_INLINE_TABLE_SIZE;
    LEARNED_INLINE_FIELDS.with(|t| unsafe {
        let e = (*t.get())[slot];
        if e.0 == class_id {
            e.1
        } else {
            0
        }
    })
}

/// accessed Vec — the common row-build pattern where an object's
/// overflow slots fill in sequence.
#[inline]
fn overflow_set(obj_ptr: usize, field_index: usize, vbits: u64) {
    // Learn the class's true width so FUTURE instances allocate it inline.
    unsafe {
        let hdr = obj_ptr as *const ObjectHeader;
        note_learned_inline_fields((*hdr).class_id, (field_index as u32).saturating_add(1));
    }
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

/// Snapshot the current `js_native_call_method` recursion depth. Exception
/// handling (`js_try_push`) records this at each `try` so the unwind path can
/// restore it: a `js_throw` `longjmp`s past the in-flight method frames and
/// skips their `CallMethodDepthGuard` `Drop`s, so without an explicit restore
/// the counter leaks one per caught throw and — after `MAX_CALL_METHOD_DEPTH`
/// throw/catch cycles — wedges every subsequent method call into the
/// stack-overflow fallback (returning the empty null-object instead of
/// dispatching). See `crate::exception::{js_try_push, js_throw}`.
pub(crate) fn call_method_depth_savepoint() -> u32 {
    CALL_METHOD_DEPTH.with(|d| d.get())
}

/// Restore the `js_native_call_method` recursion depth captured by
/// [`call_method_depth_savepoint`]. Called on the `longjmp` unwind path so the
/// frames the throw skips don't leak their depth increments (see above).
pub(crate) fn call_method_depth_restore(depth: u32) {
    CALL_METHOD_DEPTH.with(|d| d.set(depth));
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
    /// Configurable synthetic class-ref keys that were deleted (currently
    /// `name`). Mirrors the closure deleted-key side table for ClassRef values,
    /// which are tagged integers rather than ObjectHeader/ClosureHeader values.
    pub(crate) static CLASS_DELETED_KEYS: std::cell::RefCell<std::collections::HashMap<u32, std::collections::HashSet<String>>> =
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
    // arm64_32 fix: HEAP-allocate the 320KB cache (Box) instead of storing it
    // inline in TLS. Oversized `#[thread_local]` storage overflows the ILP32
    // TLS layout and its writes corrupt adjacent thread-locals (confirmed on a
    // real Series 7: shrinking OR boxing removes the corruption). `vec!` builds
    // directly on the heap (no 320KB stack temporary).
    static TRANSITION_CACHE_GLOBAL: std::cell::UnsafeCell<Box<[TransitionEntry]>> =
        std::cell::UnsafeCell::new(
            vec![
                TransitionEntry {
                    prev_keys: 0,
                    key_ptr: 0,
                    next_keys: 0,
                    slot_idx: 0,
                    target_len: 0,
                };
                TRANSITION_CACHE_SIZE
            ]
            .into_boxed_slice(),
        );
}

#[inline]
fn with_transition_cache<R>(
    f: impl FnOnce(*mut [TransitionEntry; TRANSITION_CACHE_SIZE]) -> R,
) -> R {
    TRANSITION_CACHE_GLOBAL.with(|c| unsafe {
        let boxed = &mut *c.get();
        f(boxed.as_mut_ptr() as *mut [TransitionEntry; TRANSITION_CACHE_SIZE])
    })
}

/// FNV-1a content hash for a property-name string.
/// Exported as `perry_key_content_hash` for the codegen write-PIC to
/// call without going through the full `js_object_set_field_by_name`.
#[no_mangle]
pub extern "C" fn perry_key_content_hash(key: *const crate::StringHeader) -> u64 {
    key_content_hash_impl(key)
}

#[inline(always)]
pub(crate) fn key_content_hash(key: *const crate::StringHeader) -> u64 {
    key_content_hash_impl(key)
}

/// Resolve `key` to its canonical interned `StringHeader` pointer (as a
/// `usize`), the identity the `prop_plan` store/read caches key on. Returns 0
/// for a null / handle-band key. Mirrors the inline interning both field
/// stores do, so a plan recorded on one store path is found by another.
#[inline]
pub(crate) unsafe fn interned_key_ptr(key: *const crate::StringHeader) -> usize {
    if key.is_null() || !crate::value::addr_class::is_above_handle_band(key as usize) {
        return 0;
    }
    let gc_hdr = (key as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_hdr).gc_flags & crate::gc::GC_FLAG_INTERNED != 0 {
        key as usize
    } else {
        crate::string::js_string_intern(key, key_content_hash(key)) as usize
    }
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

/// #6006: verify the cached transition edge really adds `key` at `slot_idx`,
/// i.e. `next_keys[slot_idx]` string-matches `key`. Guards against a stale
/// pointer-keyed cache entry (freed keys_array address recycled by GC) that
/// pointer-matches but describes a different shape. Returns false on any
/// structural mismatch so the caller falls back to the (correct) slow path.
#[inline]
fn transition_edge_places_key(
    next_keys: usize,
    slot_idx: u32,
    key: *const crate::StringHeader,
) -> bool {
    if next_keys < crate::gc::GC_HEADER_SIZE || key.is_null() {
        return false;
    }
    unsafe {
        let gc_header = (next_keys as *const u8).wrapping_sub(crate::gc::GC_HEADER_SIZE)
            as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_ARRAY {
            return false;
        }
        let keys = next_keys as *const ArrayHeader;
        // A single-key transition edge (prev + key) always produces a target
        // shape of exactly `slot_idx + 1` keys, with `key` at the last slot.
        // Requiring the EXACT length (not just `slot_idx < length`) also
        // rejects the "shared target grew in place after caching" case — where
        // the cached `target_len` still matches but the actual array is now
        // longer, so adopting it would give the object a keys_array with more
        // keys than field_count tracks (keys present, values undefined).
        if (*keys).length != slot_idx.wrapping_add(1) {
            return false;
        }
        let stored = crate::array::js_array_get(keys, slot_idx);
        crate::string::js_string_key_matches(stored, key)
    }
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
        // #6006: `prev_keys` / `key_ptr` are raw addresses, NOT GC roots (only
        // `next_keys` is scanned/relocated). After GC frees a keys_array and
        // recycles its address into an unrelated array, a stale entry here can
        // pointer-match falsely — the object would then adopt `next_keys` and
        // store the value at `slot_idx`, which belongs to a *different* shape.
        // At bundle scale (frequent GC address reuse) this silently mis-places
        // property values (keys_array looks right, but reads return undefined
        // for the mis-slotted keys). Content-validate that the cached
        // transition actually places THIS key at `slot_idx` before trusting it;
        // a genuine edge always does, a recycled-address false match never will.
        if !transition_edge_places_key(entry.next_keys, entry.slot_idx, interned_key) {
            return None;
        }
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
            // #6495: same contract as `visit_overflow_field_slots_mut` — the
            // layout mask under-reports overflow pointer slots on paths that
            // skip `layout_note_slot`, so scan every slot.
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
        // #6495: visit EVERY overflow slot — never the layout-mask subset.
        // The per-object slot mask is maintained by `layout_note_slot` at
        // store time, but not every overflow write path notes (GC owner
        // moves merge entries via `merge_overflow_fields` with no notes), so
        // a usable-looking SIDE_MASK can under-report pointer-bearing
        // overflow slots; the trace would then skip live children and the
        // sweep frees them while referenced. The Vec's length is the live
        // overflow region, and objects with large overflow populations are
        // in UNKNOWN layout state in practice (dynamic-shape stores degrade
        // the layout), so the mask bought little here.
        let mut slots = Vec::with_capacity(fields.len());
        let base = fields.as_ptr() as *mut u64;
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
    // #3664: generator / async-generator intrinsic tower roots.
    visitor.visit_atomic_i64_slot(
        &GENERATOR_FUNCTION_INTRINSIC_PTR,
        Ordering::Acquire,
        Ordering::Release,
    );
    visitor.visit_atomic_i64_slot(
        &GENERATOR_INTRINSIC_PROTO_PTR,
        Ordering::Acquire,
        Ordering::Release,
    );
    visitor.visit_atomic_i64_slot(
        &GENERATOR_PROTOTYPE_PTR,
        Ordering::Acquire,
        Ordering::Release,
    );
    visitor.visit_atomic_i64_slot(
        &ASYNC_GENERATOR_FUNCTION_INTRINSIC_PTR,
        Ordering::Acquire,
        Ordering::Release,
    );
    visitor.visit_atomic_i64_slot(
        &ASYNC_GENERATOR_INTRINSIC_PROTO_PTR,
        Ordering::Acquire,
        Ordering::Release,
    );
    visitor.visit_atomic_i64_slot(
        &ASYNC_GENERATOR_PROTOTYPE_PTR,
        Ordering::Acquire,
        Ordering::Release,
    );
    async_generator_queue::scan_async_generator_queue_roots_mut(visitor);
    visitor.visit_atomic_i64_slot(&LOCAL_STORAGE_PTR, Ordering::Acquire, Ordering::Release);
    visitor.visit_atomic_i64_slot(&SESSION_STORAGE_PTR, Ordering::Acquire, Ordering::Release);
    // Shared `%IteratorPrototype%`-style singletons for Array/Map/Set/String
    // iterator objects. Each iterator instance's `[[Prototype]]` points here, so
    // these must stay live for the lifetime of any iterator.
    for slot in [
        &iterator_prototypes::ITERATOR_PROTOTYPE_PTR,
        &iterator_prototypes::ARRAY_ITERATOR_PROTOTYPE_PTR,
        &iterator_prototypes::MAP_ITERATOR_PROTOTYPE_PTR,
        &iterator_prototypes::SET_ITERATOR_PROTOTYPE_PTR,
        &iterator_prototypes::STRING_ITERATOR_PROTOTYPE_PTR,
        &iterator_prototypes::REGEXP_STRING_ITERATOR_PROTOTYPE_PTR,
    ] {
        visitor.visit_atomic_i64_slot(slot, Ordering::Acquire, Ordering::Release);
    }
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
pub(crate) fn debug_overflow_entry_len(owner: usize) -> Option<usize> {
    OVERFLOW_FIELDS.with(|m| m.borrow().get(&owner).map(|v| v.len()))
}

#[cfg(test)]
pub(crate) fn test_seed_overflow_fields_vec(owner: usize, values: Vec<u64>) {
    OVERFLOW_FIELDS.with(|m| {
        m.borrow_mut().insert(owner, values);
    });
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
pub(crate) fn test_seed_keys_index_entry(owner: usize) {
    KEYS_INDEX.with(|m| {
        m.borrow_mut()
            .insert(owner, (0, std::collections::HashMap::new()));
    });
}

#[cfg(test)]
pub(crate) fn test_keys_index_entry_exists(owner: usize) -> bool {
    KEYS_INDEX.with(|m| m.borrow().get(&owner).is_some())
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

/// Remove the `KEYS_INDEX` sidecar entry for a freed object pointer.
/// Sibling of `clear_overflow_for_ptr`: called from the same GC
/// dead-owner dispatch when a `GC_TYPE_OBJECT` header is reclaimed.
/// `KEYS_INDEX` is keyed on the object address, so without this prune
/// the entry (a whole `HashMap<u64, Vec<u32>>`) would persist forever
/// keyed by a recycled address — a monotonic leak over a long-running
/// process, and a stale index a fresh object at the same address could
/// read. Unlike `clear_overflow_for_ptr` there is no last-accessed
/// cache to invalidate: `keys_index_lookup` always goes through the map.
pub fn clear_keys_index_for_ptr(obj_ptr: usize) {
    KEYS_INDEX.with(|m| {
        m.borrow_mut().remove(&obj_ptr);
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

// `is_valid_obj_ptr` moved to `value/addr_class.rs` (the centralized
// handle-vs-heap-pointer classification module); re-exported here so the
// existing `crate::object::is_valid_obj_ptr` call sites keep compiling
// unchanged.
pub(crate) use crate::value::addr_class::is_valid_obj_ptr;

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
