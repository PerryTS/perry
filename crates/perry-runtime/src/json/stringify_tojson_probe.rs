//! #6009: the `toJSON` fast-negative probe — prove, with nothing but direct
//! reads, that resolving `toJSON` on a plain object through the generic
//! `js_object_get_field_by_name` dispatcher would miss, so the stringify
//! probes can skip the dispatcher (whose miss path recursively re-enters
//! itself through the subclass/prototype fallbacks) and all per-probe
//! allocations. Split out of `stringify.rs` for the 2000-line file gate.

use super::*;
use crate::JSValue;

// ─── toJSON fast-negative probe (#6009) ──────────────────────────────────────

pub(crate) const PROTO_TOJSON_DIRTY: u8 = 0;
const PROTO_TOJSON_ABSENT: u8 = 1;
const PROTO_TOJSON_PRESENT: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ObjectProtoToJsonSignature {
    proto_addr: usize,
    keys_addr: usize,
    keys_len: u32,
    obj_flags: u16,
    class_id: u32,
    semantic_epoch: u64,
}

crate::perry_thread_local! {
    /// Address-bearing fields are comparison tokens only: they are never
    /// dereferenced from this cache. Moving GC therefore turns a prior entry
    /// into a signature miss without requiring another collector root.
    static OBJECT_PROTO_TOJSON_SIGNATURE: std::cell::Cell<Option<ObjectProtoToJsonSignature>> =
        const { std::cell::Cell::new(None) };
    #[cfg(test)]
    static OBJECT_PROTO_TOJSON_RECOMPUTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Invalidate the cached `Object.prototype`-has-`toJSON` verdict. Called at
/// general top-level stringify entries and after every user callback the
/// stringify machinery invokes (`toJSON` / replacer). Specialized plain-data
/// entries instead validate the complete prototype signature before reuse.
#[inline]
pub(crate) fn invalidate_object_proto_tojson_state() {
    OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(PROTO_TOJSON_DIRTY));
    OBJECT_PROTO_TOJSON_SIGNATURE.with(|c| c.set(None));
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
    let elements =
        crate::array::array_elements_ptr(keys as *const crate::ArrayHeader) as *const f64;
    for i in 0..key_count {
        let stored = JSValue::from_bits((*elements.add(i)).to_bits());
        if key_may_carry_to_json(stored) {
            return true;
        }
    }
    false
}

// All forwarding markers are longer than the inline-string representation.
// Keep the short-string and leading-byte exclusions tied to their constants.
const _: () = {
    assert!(crate::value::SHORT_STRING_MAX_LEN < b"toJSON".len());
    assert!(crate::object::FETCH_SUBCLASS_HANDLE_FIELD.len() >= b"toJSON".len());
    assert!(crate::object::FETCH_SUBCLASS_HANDLE_FIELD[0] == b'_');
    #[cfg(feature = "temporal")]
    {
        assert!(crate::object::TEMPORAL_SUBCLASS_CELL_FIELD.len() >= b"toJSON".len());
        assert!(crate::object::TEMPORAL_SUBCLASS_CELL_FIELD[0] == b'_');
    }
};

#[inline]
unsafe fn key_may_carry_to_json(stored: JSValue) -> bool {
    if !stored.is_string() {
        return false;
    }
    let header = stored.as_string_ptr();
    if header.is_null() {
        return false;
    }
    let len = (*header).byte_len as usize;
    if len < b"toJSON".len() {
        return false;
    }
    let data = crate::string::string_data(header);
    if !matches!(*data, b't' | b'_') {
        return false;
    }
    marker_bytes_may_carry_to_json(std::slice::from_raw_parts(data, len))
}

/// Reuse the marker proof when a serializer already has the key bytes.
#[inline]
pub(super) fn key_bytes_may_carry_to_json(bytes: &[u8]) -> bool {
    bytes.len() >= b"toJSON".len()
        && matches!(bytes[0], b't' | b'_')
        && marker_bytes_may_carry_to_json(bytes)
}

/// Ordinary keys do not need the long marker constants in their hot loop.
/// No verdict is retained: changed keys are read again on the next probe.
#[cold]
#[inline(never)]
fn marker_bytes_may_carry_to_json(bytes: &[u8]) -> bool {
    if bytes == b"toJSON"
        || bytes == crate::object::FETCH_SUBCLASS_HANDLE_FIELD
        || bytes == b"__module__"
    {
        return true;
    }
    #[cfg(feature = "temporal")]
    if bytes == crate::object::TEMPORAL_SUBCLASS_CELL_FIELD {
        return true;
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
    #[cfg(test)]
    OBJECT_PROTO_TOJSON_RECOMPUTES.with(|count| count.set(count.get() + 1));
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
    let keys = crate::object::object_keys_array(proto);
    if keys.is_null() {
        return PROTO_TOJSON_ABSENT;
    }
    if keys_array_may_carry_to_json(keys) {
        return PROTO_TOJSON_PRESENT;
    }
    PROTO_TOJSON_ABSENT
}

#[cfg(test)]
pub(super) fn test_reset_object_proto_tojson_recomputes() {
    OBJECT_PROTO_TOJSON_RECOMPUTES.with(|count| count.set(0));
}

#[cfg(test)]
pub(super) fn test_object_proto_tojson_recomputes() -> u64 {
    OBJECT_PROTO_TOJSON_RECOMPUTES.with(std::cell::Cell::get)
}

/// Snapshot every part of the default-prototype lookup whose change can alter
/// the negative verdict. Key-array contents can only change without changing
/// identity/length through delete or descriptor/prototype operations, all of
/// which advance the shared semantic property epoch.
unsafe fn object_proto_tojson_signature() -> Option<ObjectProtoToJsonSignature> {
    let proto_bits = CACHED_OBJECT_PROTO_BITS.with(|c| c.get());
    if proto_bits == 0 {
        return None;
    }
    let proto_addr = (proto_bits & POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_tracked_gc_header(proto_addr)?.as_ref();
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let proto = proto_addr as *const crate::ObjectHeader;
    let keys = crate::object::object_keys_array(proto);
    let (keys_addr, keys_len) = if keys.is_null() {
        (0, 0)
    } else {
        let keys_addr = keys as usize;
        let keys_header = crate::value::addr_class::try_read_tracked_gc_header(keys_addr)?.as_ref();
        if keys_header.obj_type != crate::gc::GC_TYPE_ARRAY
            || keys_header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || (*keys).length > (*keys).capacity
        {
            return None;
        }
        (keys_addr, (*keys).length)
    };
    Some(ObjectProtoToJsonSignature {
        proto_addr,
        keys_addr,
        keys_len,
        obj_flags: header._reserved,
        class_id: (*proto).class_id,
        semantic_epoch: crate::object::prop_plan::prop_plan_semantic_epoch(),
    })
}

/// Re-derive the same six fields as [`object_proto_tojson_signature`] and
/// compare them to `cached`, WITHOUT re-running `try_read_tracked_gc_header`'s
/// allocator-ownership proof. It answers the identical question; only the
/// *proof that the addresses are ours* is skipped, and only because a prior
/// full validation already established it for these exact addresses.
///
/// The ordering is the safety argument, and it is compare-then-dereference at
/// every step:
///
/// * `proto_addr` is re-read from `CACHED_OBJECT_PROTO_BITS` — a GC MUTABLE
///   root (`scan_parse_roots_mut`), so an evacuation rewrites it — and is
///   compared BEFORE anything is dereferenced. A match means the root still
///   names the very allocation `object_proto_tojson_signature` proved to be a
///   live, non-forwarded `GC_TYPE_OBJECT` when the verdict was recorded. A
///   relocation rewrites the root and lands here as a MISMATCH, which falls
///   through to the full recompute. That is the same moving-GC defence the
///   validated builder relies on, stated as a comparison instead of a probe.
/// * the keys array address is likewise re-read out of the live prototype's
///   keys slot and compared before `length` / `capacity` are touched.
/// * a header address is `addr - GC_HEADER_SIZE` for arena and malloc
///   allocations alike (`value::addr_class::classify_tracked_gc_header_with`),
///   so no address is taken FROM the cache and dereferenced: every dereference
///   is of an address just re-derived from a live root, after that address
///   compared equal to a validated one.
///
/// Worth 584 instructions per object visited, 260 of them the two arena-range
/// classifications this skips (#10696).
#[inline]
unsafe fn object_proto_tojson_signature_matches(cached: &ObjectProtoToJsonSignature) -> bool {
    let proto_bits = CACHED_OBJECT_PROTO_BITS.with(|c| c.get());
    if proto_bits == 0 {
        return false;
    }
    let proto_addr = (proto_bits & POINTER_MASK) as usize;
    if proto_addr != cached.proto_addr
        || crate::object::prop_plan::prop_plan_semantic_epoch() != cached.semantic_epoch
    {
        return false;
    }
    let header = &*((proto_addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved != cached.obj_flags
    {
        return false;
    }
    let proto = proto_addr as *const crate::ObjectHeader;
    if (*proto).class_id != cached.class_id {
        return false;
    }
    let keys = crate::object::object_keys_array(proto);
    let keys_addr = keys as usize;
    if keys_addr != cached.keys_addr {
        return false;
    }
    if keys_addr == 0 {
        return cached.keys_len == 0;
    }
    let keys_header = &*((keys_addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
    keys_header.obj_type == crate::gc::GC_TYPE_ARRAY
        && keys_header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
        && (*keys).length == cached.keys_len
        && (*keys).length <= (*keys).capacity
}

#[inline]
pub(super) unsafe fn object_proto_may_have_to_json() -> bool {
    let state = OBJECT_PROTO_TOJSON_STATE.with(|c| c.get());
    if state != PROTO_TOJSON_DIRTY {
        if let Some(cached) = OBJECT_PROTO_TOJSON_SIGNATURE.with(std::cell::Cell::get) {
            if object_proto_tojson_signature_matches(&cached) {
                return state == PROTO_TOJSON_PRESENT;
            }
        }
    }
    object_proto_may_have_to_json_recompute()
}

#[cold]
#[inline(never)]
unsafe fn object_proto_may_have_to_json_recompute() -> bool {
    let computed = compute_object_proto_tojson_state();
    // Record under the FULLY VALIDATED signature: the cheap comparison above
    // is only sound against an entry whose addresses were once proven.
    let signature = object_proto_tojson_signature();
    if signature.is_some() {
        OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(computed));
        OBJECT_PROTO_TOJSON_SIGNATURE.with(|c| c.set(signature));
    } else {
        invalidate_object_proto_tojson_state();
    }
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
fn class_chain_may_have_to_json_uncached(class_id: u32) -> bool {
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

// ─── per-`class_id` chain verdict memo (#10696) ──────────────────────────────

/// A memoized [`class_chain_may_have_to_json_uncached`] answer.
///
/// The walk it replaces costs **448 instructions per object visited** — five
/// by-name/by-id lookups across five separate registries — and its only input
/// is the class id, which is a property of the SHAPE, not of the instance.
/// (Witness, #10696: allocating a fresh `{a:{b:1}}` every iteration costs a
/// byte-identical probe to a hoisted one.)
///
/// The dangerous staleness direction is a cached `false` — "nothing on this
/// chain can produce a `toJSON`" — that should have become `true`; a stale
/// `true` only costs the slow path, which is the correct answer path. Every
/// route that can flip the answer that way is covered by one of the three
/// generations keyed on here:
///
/// | route to a newly reachable `toJSON` | caught by |
/// |---|---|
/// | `class C { toJSON() {} }`, a getter or a setter registered for this class or any ancestor (`CLASS_VTABLE_REGISTRY`) | `VTABLE_GEN` — `js_register_class_method` / `_getter` / `_setter`, `js_register_class_computed_method` / `_accessor`, and the bound-method vtable copy in `object_ops/define_property.rs` all bump it |
/// | `C.prototype.toJSON = fn` (`CLASS_PROTOTYPE_METHODS`, possibly with no prototype object at all) | `VTABLE_GEN` — `class_prototype_method_root_store` bumps it through `invalidate_class_prototype_fast_guards_for_method` |
/// | a NEW parent edge splicing in an ancestor that carries any of the above | the SEMANTIC property epoch — `class_registry::parent_static::register_class` is the only writer of the parent map and calls `prop_plan_epoch_bump` before publishing |
/// | `Object.setPrototypeOf`, a descriptor install, or a `delete` anywhere | the SEMANTIC property epoch |
/// | a prototype OBJECT materializing for this class or an ancestor — the very thing the walk looks for, since such an object can carry arbitrary later-added properties | `CLASS_LOOKUP_SURFACE_GEN`, bumped inside `class_prototype_object_root_store` and `class_decl_prototype_object_root_store` |
/// | `js_register_class_generic_origin`, which redirects both prototype-object readers and `lookup_prototype_method`'s chain hop | `CLASS_LOOKUP_SURFACE_GEN` |
/// | re-exposing a `delete`d prototype key through the in-place `CLASS_DELETED_KEYS` un-mark in `class_dynamic_prop_root_store` | `CLASS_LOOKUP_SURFACE_GEN` |
///
/// Garbage collection is deliberately NOT an input. The class side-table
/// scanners only rewrite EXISTING slots, so no collection can add a registry
/// key; the dead-owner prune only removes entries, which can make a cached
/// `true` conservative but never a cached `false` wrong. Keying a per-object
/// cache on a GC-bumped counter is a measured performance CLIFF rather than
/// mere waste — `object::prop_plan`'s module docs record +35 % from exactly
/// that mistake (#7910).
///
/// Thread-local, like `promise::then_probe`'s `ADMISSIBLE_MEMO`: two of the
/// four tables summarized here are themselves `perry_thread_local!`, so a
/// process-global table would be unsound against another thread's stores.
#[derive(Clone, Copy)]
struct ClassChainToJsonEntry {
    class_id: u32,
    vtable_gen: u64,
    semantic_epoch: u64,
    surface_gen: u64,
    may_have: bool,
}

/// Sized for the distinct object-literal SHAPES a serialization walk touches,
/// not for classes: `{a:{b:{c:{d:{e:1}}}}}` alone is five anon shape ids, and
/// an evicting pair costs a full 448-instruction walk on EVERY operation. That
/// is not hypothetical — a 16-slot table indexed by `>> 12` did exactly that
/// on `z_nest4`, and the collision was visible as a per-object registry cost
/// that refused to go to zero while the other shapes' went (#10696).
const CLASS_CHAIN_TOJSON_SLOTS: usize = 64;

const EMPTY_CLASS_CHAIN_TOJSON: ClassChainToJsonEntry = ClassChainToJsonEntry {
    // Class id 0 is answered by the caller without consulting the memo, so it
    // is a safe "empty" tag.
    class_id: 0,
    vtable_gen: 0,
    semantic_epoch: 0,
    surface_gen: 0,
    may_have: false,
};

crate::perry_thread_local! {
    /// Per-slot `Cell`s rather than a `Cell<[…; N]>`: the latter copies the
    /// whole table in and out on every probe, and this path runs once per
    /// object visited by `JSON.stringify`.
    static CLASS_CHAIN_TOJSON_MEMO: [std::cell::Cell<ClassChainToJsonEntry>;
        CLASS_CHAIN_TOJSON_SLOTS] =
        const { [const { std::cell::Cell::new(EMPTY_CLASS_CHAIN_TOJSON) }; CLASS_CHAIN_TOJSON_SLOTS] };
    #[cfg(test)]
    static CLASS_CHAIN_TOJSON_RECOMPUTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Index a class id into the memo. The multiply mixes the whole 32-bit id into
/// the HIGH bits and the shift takes them from there, so ids that differ only
/// in their low bits — which is exactly how consecutive anon shape ids are
/// minted — land in different slots.
#[inline]
fn class_chain_tojson_slot(class_id: u32) -> usize {
    const SLOT_BITS: u32 = CLASS_CHAIN_TOJSON_SLOTS.trailing_zeros();
    ((class_id as u64).wrapping_mul(0x9E37_79B1_85EB_CA87) >> (64 - SLOT_BITS)) as usize
}

#[inline]
fn class_chain_may_have_to_json(class_id: u32) -> bool {
    debug_assert_ne!(class_id, 0, "class id 0 is answered by the caller");
    let vtable_gen = crate::object::vtable_generation();
    let semantic_epoch = crate::object::prop_plan::prop_plan_semantic_epoch();
    let surface_gen = crate::object::class_lookup_surface_generation();
    let slot = class_chain_tojson_slot(class_id);
    let entry = CLASS_CHAIN_TOJSON_MEMO.with(|table| table[slot].get());
    if entry.class_id == class_id
        && entry.vtable_gen == vtable_gen
        && entry.semantic_epoch == semantic_epoch
        && entry.surface_gen == surface_gen
    {
        return entry.may_have;
    }
    class_chain_to_json_memo_fill(class_id, vtable_gen, semantic_epoch, surface_gen, slot)
}

#[cold]
#[inline(never)]
fn class_chain_to_json_memo_fill(
    class_id: u32,
    vtable_gen: u64,
    semantic_epoch: u64,
    surface_gen: u64,
    slot: usize,
) -> bool {
    #[cfg(test)]
    CLASS_CHAIN_TOJSON_RECOMPUTES.with(|count| count.set(count.get() + 1));
    let may_have = class_chain_may_have_to_json_uncached(class_id);
    CLASS_CHAIN_TOJSON_MEMO.with(|table| {
        table[slot].set(ClassChainToJsonEntry {
            class_id,
            vtable_gen,
            semantic_epoch,
            surface_gen,
            may_have,
        })
    });
    may_have
}

#[cfg(test)]
pub(super) fn test_reset_class_chain_tojson_recomputes() {
    CLASS_CHAIN_TOJSON_RECOMPUTES.with(|count| count.set(0));
}

#[cfg(test)]
pub(super) fn test_class_chain_tojson_recomputes() -> u64 {
    CLASS_CHAIN_TOJSON_RECOMPUTES.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(super) fn test_class_chain_may_have_to_json(class_id: u32) -> bool {
    class_chain_may_have_to_json(class_id)
}

#[cfg(test)]
pub(super) fn test_class_chain_may_have_to_json_uncached(class_id: u32) -> bool {
    class_chain_may_have_to_json_uncached(class_id)
}

/// #6009: prove that resolving `toJSON` on
/// `ptr` (a validated `GC_TYPE_OBJECT`) through `js_object_get_field_by_name`
/// would miss, so the probe can skip the generic dispatcher entirely. The
/// generic walk can only produce a `toJSON` from four places, each covered
/// here:
///
/// Own/class/prototype checks use direct reads, but the first default-prototype
/// lookup can initialize globalThis and allocate its lookup key. Callers that
/// need the receiver afterward must root it across this probe. The data-only
/// emitter uses `to_json_definitely_absent_without_gc` to decline that lookup.
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
///    `object_proto_may_have_to_json` (verdict cached under a live mutation
///    signature across callback-free specialized stringify calls).
///
/// Before this probe, every object literal paid ~3 recursive
/// `js_object_get_field_by_name` miss cascades per stringify call — ~90% of
/// `JSON.stringify` time on small objects and a ~250x gap vs V8 (#6009).
pub(crate) unsafe fn to_json_definitely_absent(ptr: *const u8) -> bool {
    let obj = ptr as *const crate::ObjectHeader;
    let keys = crate::object::object_keys_array(obj);
    if !keys.is_null() && keys_array_may_carry_to_json(keys) {
        return false;
    }
    to_json_definitely_absent_after_own_keys(ptr)
}

/// Finish the negative probe after the caller validated and scanned every
/// own key with `key_bytes_may_carry_to_json`. That proof must still apply:
/// no callback or key mutation may intervene. The receiver must be rooted
/// because the default-prototype lookup can allocate on its first use.
pub(super) unsafe fn to_json_definitely_absent_after_own_keys(ptr: *const u8) -> bool {
    let obj = ptr as *const crate::ObjectHeader;
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

/// The plain-data emitter cannot enter the allocating first-time lookup of
/// Object.prototype. Let the rooted general path populate that cache first.
/// Once its constructor property is cached, even a dirty verdict is recomputed
/// with direct reads only; class/prototype overrides still decline normally.
pub(super) unsafe fn to_json_definitely_absent_without_gc(ptr: *const u8) -> bool {
    if (*(ptr as *const crate::ObjectHeader)).class_id != 0
        || (OBJECT_PROTO_TOJSON_STATE.with(|c| c.get()) == PROTO_TOJSON_DIRTY
            && CACHED_OBJECT_PROTO_BITS.with(|c| c.get()) == 0)
    {
        return false;
    }
    to_json_definitely_absent(ptr)
}

/// Establish the stringify-wide part of the plain-data record proof.
///
/// A successful data-record emission cannot invoke user code or managed
/// allocation, so an array walker may reuse this verdict across consecutive
/// records. It must discard the verdict before entering any fallback path
/// that can call `toJSON` or a getter. Per-object recorded prototypes remain
/// checked by the record emitter itself.
#[inline]
pub(super) unsafe fn data_record_global_to_json_absent_without_gc() -> bool {
    if SUPPRESS_NEXT_TO_JSON.with(|c| c.get())
        || (OBJECT_PROTO_TOJSON_STATE.with(|c| c.get()) == PROTO_TOJSON_DIRTY
            && CACHED_OBJECT_PROTO_BITS.with(|c| c.get()) == 0)
    {
        return false;
    }
    !object_proto_may_have_to_json()
}

// ─── SerializeJSONProperty key (#5909) ───────────────────────────────────────
// ECMA-262 §25.5.2.2 SerializeJSONProperty step 2.b.i calls `toJSON` with the
// property key. `TO_JSON_KEY` carries that key from each serialization loop
// (which knows the child's key) down to the three `toJSON` probes (which don't).
// See the thread-local's doc comment in `mod.rs`.

/// Reset the pending `toJSON` key to the empty String — the root key, and the
/// safe default at every top-level `JSON.stringify` entry so a key set during
/// a previous call can't leak into the next.
#[inline]
pub(crate) fn reset_to_json_key() {
    TO_JSON_KEY.with(|c| c.borrow_mut().clear());
}

/// Record an object's own property name as the key to hand `toJSON` for the
/// member about to be serialized.
#[inline]
pub(crate) fn set_to_json_key_str(key: &str) {
    TO_JSON_KEY.with(|c| {
        let mut s = c.borrow_mut();
        s.clear();
        s.push_str(key);
    });
}

/// Record an array index (its stringified form) as the key to hand `toJSON`
/// for the element about to be serialized.
#[inline]
pub(crate) fn set_to_json_key_index(index: usize) {
    use std::fmt::Write;
    TO_JSON_KEY.with(|c| {
        let mut s = c.borrow_mut();
        s.clear();
        let _ = write!(s, "{index}");
    });
}

/// Record a NaN-boxed JS string value as the pending `toJSON` key. The
/// replacer walk (`apply_to_json_keyed`) already carries the property key as a
/// value rather than a `&str`, so decode it here. Copies the bytes into the
/// owned `String`, so nothing borrows the source header afterward.
#[inline]
pub(crate) unsafe fn set_to_json_key_value(key: f64) {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    match crate::string::str_bytes_from_jsvalue(key, &mut scratch) {
        Some((ptr, len)) => {
            let bytes = std::slice::from_raw_parts(ptr, len as usize);
            TO_JSON_KEY.with(|c| {
                let mut s = c.borrow_mut();
                s.clear();
                s.push_str(&String::from_utf8_lossy(bytes));
            });
        }
        None => reset_to_json_key(),
    }
}

/// Allocate the current pending `toJSON` key as a JS string value to pass as
/// the `toJSON(key)` argument. The bytes are copied out of the thread-local
/// first so the string allocation can't observe the cell still borrowed.
#[inline]
pub(crate) unsafe fn current_to_json_key_arg() -> f64 {
    let bytes = TO_JSON_KEY.with(|c| c.borrow().as_bytes().to_vec());
    let key = crate::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(STRING_TAG | (key as u64 & POINTER_MASK))
}

#[cfg(test)]
#[path = "stringify_tojson_probe_tests.rs"]
mod tests;
