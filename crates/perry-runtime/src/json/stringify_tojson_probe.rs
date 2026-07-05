//! #6009: the `toJSON` fast-negative probe — prove, with nothing but direct
//! reads, that resolving `toJSON` on a plain object through the generic
//! `js_object_get_field_by_name` dispatcher would miss, so the stringify
//! probes can skip the dispatcher (whose miss path recursively re-enters
//! itself through the subclass/prototype fallbacks) and all per-probe
//! allocations. Split out of `stringify.rs` for the 2000-line file gate.

use super::*;
use crate::{JSValue, StringHeader};

// ─── toJSON fast-negative probe (#6009) ──────────────────────────────────────

pub(crate) const PROTO_TOJSON_DIRTY: u8 = 0;
const PROTO_TOJSON_ABSENT: u8 = 1;
const PROTO_TOJSON_PRESENT: u8 = 2;

/// Invalidate the cached `Object.prototype`-has-`toJSON` verdict. Called at
/// every top-level stringify entry and after every user callback the
/// stringify machinery invokes (`toJSON` / replacer) — the only points where
/// user code could have (un)installed an `Object.prototype.toJSON` since the
/// verdict was last computed.
#[inline]
pub(crate) fn invalidate_object_proto_tojson_state() {
    OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(PROTO_TOJSON_DIRTY));
}

/// Scan `keys` (an object's `keys_array`) for any key that could make the
/// generic property walk resolve a `toJSON`: the key itself, or one of the
/// hidden marker fields through which `js_object_get_field_by_name` forwards
/// reads to a native backing that carries its own `toJSON` surface —
/// `class X extends Temporal.*` stash cells, `class X extends
/// Request/Response` handles, and native-module namespace objects. Returns
/// `true` (= caller must take the full slow-path resolution) on any match or
/// whenever the array doesn't look like a well-formed keys array.
unsafe fn keys_array_may_carry_to_json(keys: *mut crate::ArrayHeader) -> bool {
    let keys_addr = keys as usize;
    if keys_addr & 0x7 != 0 {
        return true;
    }
    let Some(keys_gc) = crate::value::addr_class::try_read_gc_header(keys_addr) else {
        return true;
    };
    if keys_gc.obj_type != crate::gc::GC_TYPE_ARRAY {
        return true;
    }
    let key_count = (*keys).length as usize;
    if key_count > (*keys).capacity as usize {
        return true;
    }
    // Wide objects (barrel namespaces etc.) keep the slow path, which probes
    // them through the O(1) wide-key index instead of a linear scan.
    if key_count > 4096 {
        return true;
    }
    // Keys arrays are dense inline-element arrays (built exclusively by
    // `ensure_key_in_keys_array` / the shape allocators), so read the element
    // slots raw — same layout walk `stringify_object_inner` does — instead of
    // paying the exported `js_array_get` validation per element.
    let elements = (keys as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    for i in 0..key_count {
        let stored = JSValue::from_bits((*elements.add(i)).to_bits());
        if crate::string::js_string_key_matches_bytes(stored, b"toJSON")
            || crate::string::js_string_key_matches_bytes(
                stored,
                crate::object::FETCH_SUBCLASS_HANDLE_FIELD,
            )
            || crate::string::js_string_key_matches_bytes(stored, b"__module__")
        {
            return true;
        }
        #[cfg(feature = "temporal")]
        if crate::string::js_string_key_matches_bytes(
            stored,
            crate::object::TEMPORAL_SUBCLASS_CELL_FIELD,
        ) {
            return true;
        }
    }
    false
}

/// Compute whether the DEFAULT `Object.prototype` — the object
/// `default_object_prototype_property_value` consults when a plain object's
/// own/recorded-prototype lookup misses — carries a `toJSON` property.
/// Conservative: any state we can't cheaply inspect reports PRESENT, which
/// only means the probe falls back to the (correct) slow path.
#[cold]
unsafe fn compute_object_proto_tojson_state() -> u8 {
    // Resolve the default `Object.prototype` once per thread and cache the
    // bits — the `Object.prototype` property is non-writable/non-configurable
    // per spec, so re-resolving per call (a globalThis generic-getter walk
    // plus a key-string allocation) buys nothing. The cached slot is a GC
    // mutable root (see `scan_parse_roots_mut`), so evacuation rewrites it.
    let mut proto_bits = CACHED_OBJECT_PROTO_BITS.with(|c| c.get());
    if proto_bits == 0 {
        let Some(resolved) = crate::object::prototype_chain::default_object_prototype_bits() else {
            // No resolvable Object.prototype: the slow path's default-proto
            // fallback would find nothing either. Not cached, so a later call
            // retries in case the builtin singleton initializes afterwards.
            return PROTO_TOJSON_ABSENT;
        };
        CACHED_OBJECT_PROTO_BITS.with(|c| c.set(resolved));
        proto_bits = resolved;
    }
    let proto_ptr = (proto_bits & POINTER_MASK) as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(proto_ptr)
        || proto_ptr & 0x7 != 0
        || gc_obj_type(proto_ptr as *const u8) != crate::gc::GC_TYPE_OBJECT
    {
        return PROTO_TOJSON_PRESENT;
    }
    let proto = proto_ptr as *const crate::ObjectHeader;
    // A class-linked or re-prototyped Object.prototype is out of the ordinary
    // enough to always defer to the slow path.
    if (*proto).class_id != 0
        || crate::object::prototype_chain::object_static_prototype(proto_ptr).is_some()
    {
        return PROTO_TOJSON_PRESENT;
    }
    let keys = (*proto).keys_array;
    if keys.is_null() {
        return PROTO_TOJSON_ABSENT;
    }
    if keys_array_may_carry_to_json(keys) {
        return PROTO_TOJSON_PRESENT;
    }
    PROTO_TOJSON_ABSENT
}

#[inline]
unsafe fn object_proto_may_have_to_json() -> bool {
    let state = OBJECT_PROTO_TOJSON_STATE.with(|c| c.get());
    if state != PROTO_TOJSON_DIRTY {
        return state == PROTO_TOJSON_PRESENT;
    }
    let computed = compute_object_proto_tojson_state();
    OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(computed));
    computed == PROTO_TOJSON_PRESENT
}

/// Could `class_id`'s prototype chain resolve a `toJSON`? Consults every
/// store the generic chain walk reads:
///
/// - the class vtable registry (methods/getters/setters; deletion-aware,
///   parent-chain walk) — `class_instance_has_member`;
/// - the assignment side table (`Class.prototype.toJSON = fn` registers in
///   `CLASS_PROTOTYPE_METHODS`, possibly with no prototype OBJECT
///   materialized at all) — `lookup_prototype_method`;
/// - the two prototype-object tables: synthetic `Object.create(proto)` /
///   `Function.prototype = obj` prototypes (`CLASS_PROTOTYPE_OBJECTS`) and
///   reflective `ClassName.prototype` decl objects
///   (`CLASS_DECL_PROTOTYPE_OBJECTS`). A materialized prototype object can
///   carry arbitrary runtime-added properties, so ANY entry anywhere on the
///   parent chain defers to the slow path. Both tables are lazily populated
///   (only a reflective `C.prototype` read or an `Object.create` materializes
///   an entry), so plain literals' anonymous shape classes never hit this.
fn class_chain_may_have_to_json(class_id: u32) -> bool {
    if crate::object::class_instance_has_member(class_id, "toJSON") {
        return true;
    }
    if crate::object::lookup_prototype_method(class_id, "toJSON").is_some() {
        return true;
    }
    let mut cid = class_id;
    let mut depth = 0u32;
    while cid != 0 && depth < 32 {
        if !crate::object::class_prototype_object(cid).is_null()
            || !crate::object::class_decl_prototype_object(cid).is_null()
        {
            return true;
        }
        match crate::object::get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

/// #6009: prove — with nothing but direct reads — that resolving `toJSON` on
/// `ptr` (a validated `GC_TYPE_OBJECT`) through `js_object_get_field_by_name`
/// would miss, so the probe can skip the generic dispatcher entirely. The
/// generic walk can only produce a `toJSON` from four places, each covered
/// here:
///
/// 1. an OWN key (object-literal method, expando, or an
///    `Object.defineProperty` accessor — all of which register the key in
///    `keys_array`), or a hidden native-backing marker key that forwards
///    reads to a `toJSON`-bearing native surface —
///    `keys_array_may_carry_to_json`;
/// 2. a class prototype/vtable/prototype-object method anywhere on the class
///    parent chain — `class_chain_may_have_to_json` (HIR lowers even plain
///    object literals to anonymous shape classes, so `class_id != 0` alone
///    proves nothing: the registries must actually be consulted);
/// 3. an explicitly recorded `[[Prototype]]` (`Object.setPrototypeOf`, or
///    `Object.create` with a Proxy prototype) — any recorded entry defers to
///    the slow path;
/// 4. a `toJSON` monkey-patched onto the default `Object.prototype` —
///    `object_proto_may_have_to_json` (verdict cached per stringify call).
///
/// Before this probe, every object literal paid ~3 recursive
/// `js_object_get_field_by_name` miss cascades per stringify call — ~90% of
/// `JSON.stringify` time on small objects and a ~250x gap vs V8 (#6009).
pub(crate) unsafe fn to_json_definitely_absent(ptr: *const u8) -> bool {
    let obj = ptr as *const crate::ObjectHeader;
    let keys = (*obj).keys_array;
    if !keys.is_null() && keys_array_may_carry_to_json(keys) {
        return false;
    }
    let class_id = (*obj).class_id;
    if class_id != 0 && class_chain_may_have_to_json(class_id) {
        return false;
    }
    if crate::object::prototype_chain::object_static_prototype(ptr as usize).is_some() {
        return false;
    }
    if object_proto_may_have_to_json() {
        return false;
    }
    true
}
