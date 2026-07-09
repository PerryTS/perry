//! Dynamic per-closure property side-table, `this`-rebind/unbind helpers,
//! and the closure-magic-tag pointer predicate.

use super::*;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

static CLOSURE_PROPS: OnceLock<Mutex<HashMap<usize, HashMap<String, f64>>>> = OnceLock::new();

fn get_closure_props() -> &'static Mutex<HashMap<usize, HashMap<String, f64>>> {
    CLOSURE_PROPS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// #3655: keys deleted off a closure via `delete fn.name` etc.
///
/// Functions carry built-in own data properties (`name`, `length`, and —
/// for constructors — `prototype`) that aren't stored in `CLOSURE_PROPS`:
/// they're synthesized from the arity/name registries on read. Those
/// properties are spec'd `configurable: true`, so `delete fn.name` must make
/// them disappear from every subsequent `hasOwnProperty` / `getOwnProperty*`
/// / value read. We can't remove a synthesized slot, so we record the
/// deletion here and have every property-protocol site consult it. test262's
/// `verifyProperty` exercises exactly this (delete-then-`hasOwnProperty`)
/// when checking `configurable`.
static CLOSURE_DELETED_KEYS: OnceLock<Mutex<HashMap<usize, HashSet<String>>>> = OnceLock::new();

fn get_closure_deleted_keys() -> &'static Mutex<HashMap<usize, HashSet<String>>> {
    CLOSURE_DELETED_KEYS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record that `key` was `delete`d off the closure at `ptr`.
pub fn closure_mark_key_deleted(ptr: usize, key: &str) {
    if ptr == 0 {
        return;
    }
    if let Ok(mut map) = get_closure_deleted_keys().lock() {
        map.entry(ptr).or_default().insert(key.to_string());
    }
}

/// True if `key` was previously `delete`d off the closure at `ptr`.
pub fn closure_is_key_deleted(ptr: usize, key: &str) -> bool {
    if ptr == 0 {
        return false;
    }
    get_closure_deleted_keys()
        .lock()
        .ok()
        .map(|map| map.get(&ptr).map(|s| s.contains(key)).unwrap_or(false))
        .unwrap_or(false)
}

/// True if `prop` is an OWN dynamic property of the closure at `ptr` (does NOT
/// walk the static-prototype chain, unlike `closure_get_dynamic_prop`). Used
/// by `hasOwnProperty`/`getOwnPropertyNames` to report own user props and the
/// constructor `prototype` slot without inheriting from a set prototype.
pub fn closure_has_own_dynamic_prop(ptr: usize, prop: &str) -> bool {
    get_closure_props()
        .lock()
        .ok()
        .map(|m| m.get(&ptr).map(|p| p.contains_key(prop)).unwrap_or(false))
        .unwrap_or(false)
}

/// #36 / #321: `Object.setPrototypeOf(closure, protoObj)` side-table.
///
/// Maps a closure pointer to the NaN-box bits of the object that was set as
/// its static prototype. effect's `Context.Tag(id)` returns a plain function
/// `TagClass` whose `_op: "Tag"`, `[TagTypeId]`, and `[EffectTypeId]` live on
/// `TagProto` (a regular object), wired by `Object.setPrototypeOf(TagClass,
/// TagProto)`. Perry bakes class IDs at allocation time so it can't mutate a
/// real prototype chain, but recording the (closure → proto) link here lets
/// string- and symbol-keyed property reads on the closure walk to the proto's
/// own properties — so `TagClass._op === "Tag"` and `isTag(TagClass)` hold.
static CLOSURE_STATIC_PROTOTYPES: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();

fn get_closure_prototypes() -> &'static Mutex<HashMap<usize, u64>> {
    CLOSURE_STATIC_PROTOTYPES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record `Object.setPrototypeOf(closure_ptr, proto)`. `proto_bits` is the
/// NaN-box bits of the prototype object (POINTER-tagged). Idempotent overwrite.
pub fn closure_set_static_prototype(closure_ptr: usize, proto_bits: u64) {
    if closure_ptr == 0 {
        return;
    }
    let mut slot_addr = 0usize;
    if let Ok(mut map) = get_closure_prototypes().lock() {
        let slot = map.entry(closure_ptr).or_insert(0);
        *slot = proto_bits;
        slot_addr = slot as *mut u64 as usize;
    }
    if slot_addr != 0 {
        crate::gc::runtime_write_barrier_external_slot(closure_ptr, slot_addr, proto_bits);
    }
}

/// Look up the static prototype object bits recorded for a closure, if any.
pub fn closure_static_prototype(closure_ptr: usize) -> Option<u64> {
    get_closure_prototypes()
        .lock()
        .ok()
        .and_then(|map| map.get(&closure_ptr).copied())
}

fn barrier_closure_dynamic_props(owner: usize, props: &mut HashMap<String, f64>) {
    for value in props.values_mut() {
        crate::gc::runtime_write_barrier_external_slot(
            owner,
            value as *mut f64 as usize,
            value.to_bits(),
        );
    }
}

fn merge_closure_prop_map(
    props: &mut HashMap<usize, HashMap<String, f64>>,
    owner: usize,
    owner_props: HashMap<String, f64>,
) {
    match props.entry(owner) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            entry.get_mut().extend(owner_props);
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(owner_props);
        }
    }
}

fn forwarded_heap_owner(owner: usize) -> Option<usize> {
    if owner == 0 {
        return None;
    }
    if matches!(
        crate::arena::classify_heap_generation(owner),
        crate::arena::HeapGeneration::Unknown
    ) {
        return None;
    }
    unsafe {
        let header = crate::value::addr_class::try_read_gc_header(owner)?;
        if header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
            return None;
        }
        Some(crate::gc::forwarding_address(header as *const _) as usize)
    }
}

pub(crate) fn closure_dynamic_props_owner_moved(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner {
        return;
    }
    if let Ok(mut props) = get_closure_props().lock() {
        if let Some(old_props) = props.remove(&old_owner) {
            merge_closure_prop_map(&mut props, new_owner, old_props);
        }
    }
    if let Ok(mut prototypes) = get_closure_prototypes().lock() {
        if let Some(proto_bits) = prototypes.remove(&old_owner) {
            prototypes.insert(new_owner, proto_bits);
        }
    }
    if let Ok(mut deleted) = get_closure_deleted_keys().lock() {
        if let Some(keys) = deleted.remove(&old_owner) {
            deleted.entry(new_owner).or_default().extend(keys);
        }
    }
}

pub(crate) fn visit_closure_dynamic_prop_values_mut(owner: usize, mut visit: impl FnMut(&mut f64)) {
    if owner == 0 {
        return;
    }
    let Some(mut owner_props) = get_closure_props()
        .lock()
        .ok()
        .and_then(|mut props| props.remove(&owner))
    else {
        return;
    };

    for value in owner_props.values_mut() {
        visit(value);
    }

    if let Ok(mut props) = get_closure_props().lock() {
        merge_closure_prop_map(&mut props, owner, owner_props);
    }
}

pub(crate) fn visit_closure_dynamic_prop_value_slots_mut(
    owner: usize,
    mut visit: impl FnMut(*mut u64),
) {
    visit_closure_dynamic_prop_values_mut(owner, |value| {
        visit(value as *mut f64 as *mut u64);
    });
}

pub(crate) fn visit_closure_static_prototype_slot_mut(
    owner: usize,
    mut visit: impl FnMut(*mut u64),
) {
    if owner == 0 {
        return;
    }
    // Take the entry OUT and run the visit with the lock RELEASED: a
    // copying-minor rewrite visitor can move the prototype closure, and
    // move fixup re-enters `closure_dynamic_props_owner_moved`, which
    // takes this same lock — visiting under it self-deadlocks the
    // collector (the geisterhand+reviver GC test wedged CI's cargo-test
    // at the 3h job timeout). Same remove → visit → merge-back pattern
    // as `visit_closure_dynamic_prop_values_mut` above and the roots
    // scanner below.
    let Some(mut proto_bits) = get_closure_prototypes()
        .lock()
        .ok()
        .and_then(|mut prototypes| prototypes.remove(&owner))
    else {
        return;
    };
    visit(&mut proto_bits as *mut u64);
    // The visit can forward the owner itself (self-referential
    // prototype); re-key like the roots scanner does.
    let new_owner = forwarded_heap_owner(owner).unwrap_or(owner);
    if let Ok(mut prototypes) = get_closure_prototypes().lock() {
        prototypes.insert(new_owner, proto_bits);
    }
}

/// Mutable GC scanner for closure dynamic-property side-table metadata.
///
/// The side table is keyed by closure address. The key itself is metadata
/// (visited only so a moved closure has its entry re-keyed; the metadata
/// visitor is a no-op in mark phases), but the **values** are real JS
/// references that must be marked alive in every phase, just like the
/// parallel `scan_overflow_fields_roots_mut` (`object/mod.rs`) does for
/// object overflow fields. #1802: pre-fix this scanner early-returned
/// unless `is_metadata_rewrite_phase()`, so during `Mark` /
/// `CopyingMark` the values were never traced, and a closure prop whose
/// transitive contents were reachable only via the side table (e.g.
/// ajv's `validate.errors = [{ msg }]`) had its element objects freed
/// behind the still-live array.
pub fn scan_closure_dynamic_props_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let prop_owners = get_closure_props()
        .lock()
        .ok()
        .map(|props| props.keys().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    for owner in prop_owners {
        let Some(mut closure_props) = get_closure_props()
            .lock()
            .ok()
            .and_then(|mut props| props.remove(&owner))
        else {
            continue;
        };

        // Metadata key rewrite. Only fires in rewrite-phase modes; mark phases
        // return `false` here without recording the key as a root (so the
        // side-table entry doesn't itself keep the closure alive).
        let mut new_owner = owner;
        visitor.visit_metadata_usize_slot(&mut new_owner);
        // #1802: trace every stored value in every phase. In `Mark` /
        // `CopyingMark` this keeps `fn.errors = [...]` and its transitive
        // contents reachable; in rewrite phases it updates slot bits when a
        // value was forwarded.
        for value in closure_props.values_mut() {
            visitor.visit_nanbox_f64_slot(value);
        }
        if new_owner == owner {
            new_owner = forwarded_heap_owner(owner).unwrap_or(owner);
        }

        if let Ok(mut props) = get_closure_props().lock() {
            merge_closure_prop_map(&mut props, new_owner, closure_props);
        }
    }

    let prototype_owners = get_closure_prototypes()
        .lock()
        .ok()
        .map(|prototypes| prototypes.keys().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    for owner in prototype_owners {
        let Some(mut proto_bits) = get_closure_prototypes()
            .lock()
            .ok()
            .and_then(|mut prototypes| prototypes.remove(&owner))
        else {
            continue;
        };

        let mut new_owner = owner;
        visitor.visit_metadata_usize_slot(&mut new_owner);
        visitor.visit_nanbox_u64_slot(&mut proto_bits);
        if new_owner == owner {
            new_owner = forwarded_heap_owner(owner).unwrap_or(owner);
        }

        if let Ok(mut prototypes) = get_closure_prototypes().lock() {
            prototypes.insert(new_owner, proto_bits);
        }
    }
    // #3655: re-key the deleted-keys side table when a closure moves. The
    // entries are pure metadata (string keys, no JS references), so the
    // metadata-key visitor only records a re-key; nothing to trace.
    let mut moved_deleted = Vec::new();
    if let Ok(mut deleted) = get_closure_deleted_keys().lock() {
        for owner in deleted.keys().copied().collect::<Vec<_>>() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved_deleted.push((owner, new_owner));
            }
        }
        for (old_owner, new_owner) in moved_deleted {
            if let Some(keys) = deleted.remove(&old_owner) {
                deleted.entry(new_owner).or_default().extend(keys);
            }
        }
    }
}

/// Check if a raw pointer points to a ClosureHeader by checking CLOSURE_MAGIC at offset 12.
/// Safe to call with any non-null, sufficiently aligned pointer >= 0x10000.
pub fn is_closure_ptr(ptr: usize) -> bool {
    // Reject the native / Web-Fetch small-handle band (see
    // `value::addr_class` for the band map). Fetch handles, node:http
    // handles, and revocable-proxy ids are NaN-boxed POINTER_TAG values
    // holding a small registry id, not heap pointers — a real closure is
    // always a heap allocation above the band. The old 0x10000 floor let a
    // 0x40000 Headers handle through, so the `*(ptr + 12)` CLOSURE_MAGIC
    // probe below dereferenced unmapped low memory and SIGSEGVd on Linux
    // (macOS masked it via the much higher is_valid_obj_ptr heap floor).
    if crate::value::addr_class::is_handle_band(ptr) {
        return false;
    }
    // #wall2: reject any address outside the platform heap range BEFORE the
    // `*(ptr + 12)` magic probe. The handle-band check only covers the low
    // small-id bands; a MIS-BOXED value like `0x4_0000_0000` (i32 4 << 32 — a
    // Next.js route-module options object whose codegen boxing went wrong) is
    // aligned and above the handle band, so it passed both guards and the magic
    // read dereferenced unmapped memory → SIGSEGV (the Next.js startup crash
    // after app-page-turbo loads). `is_valid_obj_ptr` is the real heap floor
    // (macOS: 0x2000_0000_0000); a non-heap address is definitively not a
    // closure, so return false instead of faulting.
    if !crate::value::addr_class::is_valid_obj_ptr(ptr as *const u8) {
        return false;
    }
    if !ptr.is_multiple_of(std::mem::align_of::<ClosureHeader>()) {
        return false;
    }
    unsafe {
        let type_tag = *((ptr as *const u8).add(CLOSURE_TYPE_TAG_OFFSET) as *const u32);
        type_tag == CLOSURE_MAGIC
    }
}

/// C-ABI predicate: returns 1 when `value_bits` (a NaN-boxed JSValue passed as
/// raw bits) is a closure/function — a `POINTER_TAG` value whose pointee
/// carries `CLOSURE_MAGIC` — and 0 for objects, arrays, strings, numbers, and
/// everything else. Exposed for external wrapper crates that link the runtime
/// only by C ABI (e.g. perry-ext-http-server's `parse_listen_args`, #2041),
/// which need to tell a callback argument apart from an options-object
/// argument without a Cargo dependency on perry-runtime.
#[no_mangle]
pub extern "C" fn js_value_is_closure(value_bits: i64) -> i32 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value_bits as u64;
    if (bits & !POINTER_MASK) != POINTER_TAG {
        return 0;
    }
    if is_closure_ptr((bits & POINTER_MASK) as usize) {
        1
    } else {
        0
    }
}

/// Get a dynamic property stored on a closure.
/// Returns TAG_UNDEFINED if not found.
pub fn closure_get_dynamic_prop(ptr: usize, prop: &str) -> f64 {
    if !is_closure_ptr(ptr) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    if let Some(acc) = crate::object::get_accessor_descriptor(ptr, prop) {
        if acc.get == 0 {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let closure =
            (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let receiver = crate::value::js_nanbox_pointer(ptr as i64);
        let prev = crate::object::js_implicit_this_set(receiver);
        let result = crate::closure::js_closure_call0(closure);
        crate::object::js_implicit_this_set(prev);
        return result;
    }

    if let Ok(props) = get_closure_props().lock() {
        if let Some(closure_props) = props.get(&ptr) {
            if let Some(&val) = closure_props.get(prop) {
                return val;
            }
        }
    }
    // #36 / #321: own prop miss — walk the closure's static prototype chain
    // (`Object.setPrototypeOf(closure, protoObj)`). Reads a string-keyed field
    // off the proto object. Lets effect's `TagClass._op` resolve to "Tag" on
    // the proto. Bounded depth guards against an accidental cycle.
    let mut cur = ptr;
    let mut depth = 0usize;
    while depth < 8 {
        let Some(proto_bits) = closure_static_prototype(cur) else {
            break;
        };
        let proto_f64 = f64::from_bits(proto_bits);
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto_f64) as usize;
        if proto_ptr == 0 || proto_ptr == cur {
            break;
        }
        // The proto may itself be a closure (rare) or a regular object. For a
        // regular object, read the named field via the field getter; for a
        // closure, recurse via its own props. Distinguish by CLOSURE_MAGIC.
        if is_closure_ptr(proto_ptr) {
            // #5039: the proto may carry accessor properties — chalk's style
            // proto is `Object.defineProperties(() => {}, styles)` where every
            // style is `{ get() {...} }`. Invoke the getter with the ORIGINAL
            // receiver (`ptr`, not the proto) so chalk's
            // `Object.defineProperty(this, styleName, {value: builder})`
            // caches the builder on the chalk instance, and nested builders
            // chain their stylers off the right `this`.
            if let Some(acc) = crate::object::get_accessor_descriptor(proto_ptr, prop) {
                if acc.get == 0 {
                    return f64::from_bits(crate::value::TAG_UNDEFINED);
                }
                let receiver = crate::value::js_nanbox_pointer(ptr as i64);
                let getter_bits = clone_closure_rebind_this(acc.get, receiver);
                let getter = (getter_bits & crate::value::POINTER_MASK)
                    as *const crate::closure::ClosureHeader;
                if getter.is_null() {
                    return f64::from_bits(crate::value::TAG_UNDEFINED);
                }
                let prev = crate::object::js_implicit_this_set(receiver);
                let result = crate::closure::js_closure_call0(getter);
                crate::object::js_implicit_this_set(prev);
                return result;
            }
            if let Ok(props) = get_closure_props().lock() {
                if let Some(p) = props.get(&proto_ptr).and_then(|m| m.get(prop)) {
                    return *p;
                }
            }
            cur = proto_ptr;
            depth += 1;
            continue;
        }
        // #5039: an accessor on the proto object must run with the ORIGINAL
        // closure as receiver, not the proto. chalk's style getters live on
        // `createChalk.prototype` and cache the built style via
        // `Object.defineProperty(this, styleName, {value: builder})` — with
        // `this` = proto that's a TypeError (redefining the non-configurable
        // accessor) instead of an own-property cache on the chalk instance.
        if let Some(acc) = crate::object::get_accessor_descriptor(proto_ptr, prop) {
            if acc.get == 0 {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            let receiver = crate::value::js_nanbox_pointer(ptr as i64);
            let getter_bits = clone_closure_rebind_this(acc.get, receiver);
            let getter =
                (getter_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
            if getter.is_null() {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            let prev = crate::object::js_implicit_this_set(receiver);
            let result = crate::closure::js_closure_call0(getter);
            crate::object::js_implicit_this_set(prev);
            return result;
        }
        unsafe {
            let key_hdr = crate::string::js_string_from_bytes(prop.as_ptr(), prop.len() as u32);
            let v = crate::object::js_object_get_field_by_name(
                proto_ptr as *const crate::object::ObjectHeader,
                key_hdr as *const crate::StringHeader,
            );
            if !v.is_undefined() && !v.is_null() {
                return f64::from_bits(v.bits());
            }
        }
        break;
    }
    // Every function's [[Prototype]] is %Function.prototype% — an expando
    // installed there (`Function.prototype.property = 12`), or a property
    // installed via `Object.defineProperty(Function.prototype, k, {...})`,
    // must be readable through any closure (`fn.property`, `boundFn.property`,
    // `Function.indicator`).
    if let Some(proto_ptr) = function_prototype_fallback_target(ptr, prop) {
        // A defineProperty accessor on Function.prototype
        // (`{ get: () => 12 }`) is invoked with the reading
        // closure as receiver.
        if let Some(acc) = crate::object::get_accessor_descriptor(proto_ptr, prop) {
            if acc.get != 0 {
                let getter =
                    (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
                if !getter.is_null() {
                    let receiver = crate::value::js_nanbox_pointer(ptr as i64);
                    let prev = crate::object::js_implicit_this_set(receiver);
                    let result = crate::closure::js_closure_call0(getter);
                    crate::object::js_implicit_this_set(prev);
                    return result;
                }
            }
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        unsafe {
            let key_hdr = crate::string::js_string_from_bytes(prop.as_ptr(), prop.len() as u32);
            let v = crate::object::js_object_get_field_by_name(
                proto_ptr as *const crate::object::ObjectHeader,
                key_hdr as *const crate::StringHeader,
            );
            if !v.is_undefined() {
                return f64::from_bits(v.bits());
            }
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Resolve the real, mutable `%Function.prototype%` object pointer for a
/// closure-receiver fallback (GET or SET), or `None` if `prop` doesn't
/// qualify — a synthesized own slot, a reified method name (`apply`, `call`,
/// `bind`, …: serving those generic thunks to closure reads/writes hijacks
/// the dedicated dispatch arms, e.g. `p.call(...)`'s undefined-read fallback
/// to method-dispatch-by-name routes the proxy APPLY trap — `fn.apply`-style
/// VALUE reads through a proxy are reified receiver-correctly by
/// `js_proxy_get` instead), an array-index-shaped key, or resolving would
/// recurse back into `Function.prototype` itself. Shared by
/// [`closure_get_dynamic_prop`]'s expando/defineProperty walk and the
/// closure SET path in `object::field_set_by_name`, so
/// `Object.defineProperty(Function.prototype, k, {get,set})` round-trips
/// through `boundFn.k = v` the same way it does through `boundFn.k`. A
/// re-entrancy guard covers the recursion through `builtin_prototype_value`
/// (which reads `Function.prototype` via `closure_get_dynamic_prop` itself).
pub(crate) fn function_prototype_fallback_target(ptr: usize, prop: &str) -> Option<usize> {
    if matches!(
        prop,
        "prototype" | "name" | "length" | "caller" | "arguments" | "constructor"
        // Universal Object.prototype method names: every receiver (closures
        // included) resolves these through a dedicated native dispatch arm,
        // not a literal field on the walked prototype object. Serving a
        // generic-lookup result for one of these hijacks that dispatch —
        // e.g. `m.propertyIsEnumerable` resolved a same-named-but-wrong
        // value via this fallback, so `m.propertyIsEnumerable("length")`
        // called the wrong thing (test262 S15.2.4.3_A8 / S15.2.4.4_A8 /
        // S15.2.4.7_A8 regressions caught after the initial fix).
        | "toString" | "valueOf" | "hasOwnProperty" | "isPrototypeOf"
        | "propertyIsEnumerable" | "toLocaleString"
    ) || crate::object::canonical_array_index(prop).is_some()
        || crate::object::reified_function_method_name(prop).is_some()
    {
        return None;
    }
    thread_local! {
        static IN_FN_PROTO_FALLBACK: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }
    let reentrant = IN_FN_PROTO_FALLBACK.with(|c| c.replace(true));
    if reentrant {
        return None;
    }
    let proto_val = crate::object::builtin_prototype_value("Function");
    IN_FN_PROTO_FALLBACK.with(|c| c.set(false));
    let proto_jv = crate::value::JSValue::from_bits(proto_val.to_bits());
    if !proto_jv.is_pointer() {
        return None;
    }
    let proto_ptr = (proto_jv.bits() & crate::value::POINTER_MASK) as usize;
    if proto_ptr == 0 || proto_ptr == ptr || is_closure_ptr(proto_ptr) {
        return None;
    }
    Some(proto_ptr)
}

/// SET-side analog of `closure_get_dynamic_prop`'s inherited-accessor read:
/// if `prop` resolves to a descriptor installed on the real
/// `%Function.prototype%` object (`Object.defineProperty(Function.prototype,
/// k, {...})`), apply spec `[[Set]]` semantics for it and report the write
/// handled — an ACCESSOR invokes its setter (if any) with `receiver` as
/// `this`; a non-writable DATA property blocks the write (matches the
/// silent-no-op convention this file already uses for a non-writable OWN
/// attrs record, just above this function's callers). Returns `false` when
/// there's no inherited descriptor at all, or it's a writable DATA property —
/// an ordinary `[[Set]]` on those creates a new OWN property on the receiver,
/// which the caller's existing own-dynamic-prop fallback already does
/// correctly. `ptr` is the closure being checked against (used only to
/// reject the Function.prototype self-reference); `receiver` is the spec
/// `[[Set]]` receiver — ordinarily the same object, but callers reached via
/// `Reflect.set(target, k, v, R)` pass a distinct `R`.
pub(crate) fn closure_set_via_function_prototype_descriptor(
    ptr: usize,
    prop: &str,
    value: f64,
    receiver: f64,
) -> bool {
    let Some(proto_ptr) = function_prototype_fallback_target(ptr, prop) else {
        return false;
    };
    if let Some(acc) = crate::object::get_accessor_descriptor(proto_ptr, prop) {
        if acc.set == 0 {
            // Getter-only: matches `al_set_length`'s getter-only `length` throw
            // (array/generic.rs) — a strict-mode write to an accessor with no
            // setter is a TypeError, not a silent no-op.
            crate::collection_iter::throw_type_error(&format!(
                "Cannot set property {prop} of #<Function> which has only a getter"
            ));
        }
        unsafe { crate::object::invoke_accessor_setter(acc.set, receiver, value) };
        return true;
    }
    if let Some(attrs) = crate::object::get_property_attrs(proto_ptr, prop) {
        if !attrs.writable() {
            return true;
        }
    }
    false
}

/// Set a dynamic property on a closure.
pub fn closure_set_dynamic_prop(ptr: usize, prop: &str, value: f64) {
    if let Ok(mut props) = get_closure_props().lock() {
        let closure_props = props.entry(ptr).or_insert_with(HashMap::new);
        closure_props.insert(prop.to_string(), value);
        barrier_closure_dynamic_props(ptr, closure_props);
    }
    // #3655: re-defining a previously deleted slot makes it present again.
    if let Ok(mut deleted) = get_closure_deleted_keys().lock() {
        if let Some(keys) = deleted.get_mut(&ptr) {
            keys.remove(prop);
        }
    }
}

/// Read an OWN dynamic property without any prototype/builtin fallback.
/// Used by `bind` to honor an `Object.defineProperty(fn, "length", …)`
/// override before falling back to the registered declared length.
pub fn closure_get_own_dynamic_prop(ptr: usize, prop: &str) -> Option<f64> {
    if let Ok(props) = get_closure_props().lock() {
        return props.get(&ptr).and_then(|m| m.get(prop).copied());
    }
    None
}

/// #3655: remove an OWN user dynamic property from a closure (used by
/// `delete fn.userProp`). Returns true if a property was actually removed.
/// Built-in synthesized slots (`name`/`length`/`prototype`) are handled by
/// `closure_mark_key_deleted` instead, since they have no map entry to drop.
pub fn closure_delete_own_dynamic_prop(ptr: usize, prop: &str) -> bool {
    if let Ok(mut props) = get_closure_props().lock() {
        if let Some(closure_props) = props.get_mut(&ptr) {
            return closure_props.remove(prop).is_some();
        }
    }
    false
}

#[cfg(test)]
pub(crate) fn test_clear_closure_side_tables() {
    if let Ok(mut props) = get_closure_props().lock() {
        props.clear();
    }
    if let Ok(mut prototypes) = get_closure_prototypes().lock() {
        prototypes.clear();
    }
    if let Ok(mut deleted) = get_closure_deleted_keys().lock() {
        deleted.clear();
    }
}

/// Snapshot every dynamic property on a closure as `(name, value)` pairs.
/// Sorted alphabetically for stable output (`HashMap` iteration order is
/// non-deterministic). Used by `format_jsvalue` to emit `[Function: f]
/// { ownProp: value }` for functions with user-attached properties. See
/// #1203.
pub fn closure_dynamic_props_snapshot(ptr: usize) -> Vec<(String, f64)> {
    if let Ok(props) = get_closure_props().lock() {
        if let Some(map) = props.get(&ptr) {
            let mut out: Vec<(String, f64)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
            out.sort_by(|a, b| a.0.cmp(&b.0));
            return out;
        }
    }
    Vec::new()
}

/// Unbind `this` from a detached method closure.
///
/// When a method is read from an object via PropertyGet (e.g., `const fn = holder.getX`),
/// this function is called on the result. If the value is a closure whose capture_count
/// has CAPTURES_THIS_FLAG set (indicating slot 0 is `this`), it allocates a new closure
/// with the same func_ptr and captures but slot 0 set to undefined.
///
/// For non-closure values (numbers, strings, objects, arrays), this is a no-op.
#[no_mangle]
pub extern "C" fn js_closure_unbind_this(val: f64) -> f64 {
    let bits = val.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    // Only process POINTER_TAG values (closures are NaN-boxed with POINTER_TAG)
    if tag != 0x7FFD_0000_0000_0000 {
        return val;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if ptr < 0x10000 {
        return val;
    }
    // Check CLOSURE_MAGIC
    unsafe {
        let type_tag = *((ptr as *const u8).add(CLOSURE_TYPE_TAG_OFFSET) as *const u32);
        if type_tag != CLOSURE_MAGIC {
            return val;
        }
        let header = ptr as *const ClosureHeader;
        let raw_count = (*header).capture_count;
        // Only unbind if the closure has the CAPTURES_THIS_FLAG
        if raw_count & CAPTURES_THIS_FLAG == 0 {
            return val;
        }
        let count = real_capture_count(raw_count) as usize;
        if count == 0 {
            return val;
        }
        // Clone the closure with slot 0 set to undefined
        let scope = crate::gc::RuntimeHandleScope::new();
        let val_handle = scope.root_nanbox_f64(val);
        let func_ptr = (*header).func_ptr;
        let new_closure = js_closure_alloc(func_ptr, raw_count);
        let source_bits = val_handle.get_nanbox_f64().to_bits();
        let source_ptr = (source_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        let source_type_tag = std::ptr::read_volatile(
            (source_ptr as *const u8).add(CLOSURE_TYPE_TAG_OFFSET) as *const u32,
        );
        if source_type_tag != CLOSURE_MAGIC {
            return val_handle.get_nanbox_f64();
        }
        let src_captures = closure_capture_slots_mut(source_ptr as *mut ClosureHeader);
        let dst_captures = closure_capture_slots_mut(new_closure);
        // Set slot 0 to undefined
        // GC_STORE_AUDIT(BARRIERED): cloned closure capture stores are followed by layout/barrier rebuild.
        *dst_captures = crate::value::TAG_UNDEFINED;
        // Copy remaining captures (slots 1..count)
        for i in 1..count {
            *dst_captures.add(i) = *src_captures.add(i);
        }
        rebuild_closure_layout_and_barriers(new_closure, count);
        // NaN-box the new closure pointer
        let new_ptr = new_closure as u64;
        f64::from_bits(0x7FFD_0000_0000_0000 | (new_ptr & 0x0000_FFFF_FFFF_FFFF))
    }
}

#[cfg(test)]
mod tests_1802 {
    use super::*;

    static SIDE_TABLE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// #1802: the side-table values must be visited in mark phases, not
    /// only during the metadata-rewrite tail. Pre-fix
    /// `scan_closure_dynamic_props_roots_mut` early-returned unless
    /// `is_metadata_rewrite_phase()`, so the `for_copy` adapter (which
    /// wraps a non-rewrite callback) saw nothing — proving the values
    /// were never traced in `Mark` / `CopyingMark`. With the early-return
    /// removed, the adapter sees every stored value's bits.
    #[test]
    fn dyn_prop_values_are_visited_in_mark_phase() {
        let _guard = SIDE_TABLE_TEST_LOCK.lock().unwrap();
        // A unique synthetic closure address (just an integer key — the
        // scanner doesn't deref it during value visitation; the
        // metadata-key visitor is a no-op for non-heap addresses).
        let owner: usize = 0xC10C_AB1E_0000_1802;
        let value_bits: u64 = 0x7FFD_AAAA_BBBB_CCCC;
        closure_set_dynamic_prop(owner, "errors", f64::from_bits(value_bits));

        // Copy-mode visitor calls our closure for every nanbox-bits
        // slot the scanner visits. Pre-fix this produced an empty
        // `seen` vec because the scanner early-returned.
        let mut seen: Vec<u64> = Vec::new();
        {
            let mut mark = |v: f64| seen.push(v.to_bits());
            let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(&mut mark);
            scan_closure_dynamic_props_roots_mut(&mut visitor);
        }

        assert!(
            seen.contains(&value_bits),
            "expected stored prop value bits {:x} in seen={:x?} — \
             scanner did not trace the value during the mark phase",
            value_bits,
            seen,
        );

        // Cleanup so other tests don't see the synthetic entry.
        if let Ok(mut props) = get_closure_props().lock() {
            props.remove(&owner);
        }
    }

    #[test]
    fn dyn_prop_scanner_visits_values_without_holding_props_lock() {
        let _guard = SIDE_TABLE_TEST_LOCK.lock().unwrap();
        let owner: usize = 0xC10C_AB1E_0000_1803;
        let value_bits: u64 = 0x7FFD_AAAA_BBBB_CCCD;
        closure_set_dynamic_prop(owner, "errors", f64::from_bits(value_bits));

        let mut saw_value = false;
        let mut lock_was_free = false;
        {
            let mut mark = |v: f64| {
                if v.to_bits() == value_bits {
                    saw_value = true;
                    lock_was_free = get_closure_props().try_lock().is_ok();
                }
            };
            let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(&mut mark);
            scan_closure_dynamic_props_roots_mut(&mut visitor);
        }

        assert!(
            saw_value,
            "scanner did not visit the stored closure prop value"
        );
        assert!(
            lock_was_free,
            "scanner must not hold CLOSURE_PROPS while visitor callbacks can move closures"
        );

        if let Ok(mut props) = get_closure_props().lock() {
            props.remove(&owner);
        }
    }

    #[test]
    fn dyn_prop_get_ignores_non_closure_receivers() {
        let _guard = SIDE_TABLE_TEST_LOCK.lock().unwrap();
        let obj = crate::object::js_object_alloc(0, 0) as usize;

        assert_eq!(
            closure_get_dynamic_prop(obj, "payload").to_bits(),
            crate::value::TAG_UNDEFINED,
            "ordinary objects must not enter closure-only Function.prototype fallback"
        );
    }

    /// #4740: `is_closure_ptr` must NOT dereference an address in the
    /// `[0x10000, 0x100000)` native-handle band. Web Fetch response handles
    /// (`0x40000+`), node:http / axios / fastify ids live there and are not
    /// real pointers — probing `*(ptr + 12)` for `CLOSURE_MAGIC` on one reads
    /// a tiny unmapped address (the reported `0x4000c`) and SIGSEGVs on the
    /// IC-miss property-lookup path. With the floor at `0x100000` the probe is
    /// skipped and these return `false` without touching memory. Complements
    /// the #4739 own-field-probe integration repro with a direct unit assertion
    /// on the predicate's floor.
    #[test]
    fn small_handle_band_is_not_a_closure_ptr() {
        // These would have dereferenced 0x4000c / 0x40014 / 0xF000c under the
        // old 0x10000 floor; under the fix they short-circuit to false.
        for handle in [0x10000usize, 0x40000, 0x40008, 0x4_0000, 0xF_0000, 0xF_FFF8] {
            assert!(
                !is_closure_ptr(handle),
                "is_closure_ptr({handle:#x}) must be false (small-handle band) \
                 without dereferencing the handle as a pointer",
            );
        }
    }
}

/// Issue #450: clone an accessor closure (from `Object.defineProperty(obj, k, { get, set })`)
/// and patch its reserved `this` slot with `recv_box` (the NaN-boxed target object pointer).
///
/// The user's descriptor object literal's `{ get() {...}, set() {...} }` methods are codegen'd
/// with `captures_this: true` — at object-literal construction the codegen patches their
/// reserved `this` slot to point to the *descriptor* object. But spec says the getter/setter
/// runs with `this === obj` (the property access target, NOT the descriptor). So we clone
/// the closure once at defineProperty time and rebind `this` to `obj`. The original
/// descriptor closure is untouched (in case the user reuses it).
///
/// `closure_bits` is the NaN-boxed closure value (POINTER_TAG | ptr); `recv_box` is the
/// NaN-boxed target receiver (POINTER_TAG | obj). Returns the new closure as NaN-boxed bits,
/// or returns `closure_bits` unchanged if the input isn't a CAPTURES_THIS closure.
///
/// Reserved `this` slot index is `auto_captures.len()` per the codegen convention
/// (`crates/perry-codegen/src/expr.rs::lower_object_literal` and
/// `crates/perry-runtime/src/symbol.rs::js_object_set_symbol_method` — both use the LAST
/// capture slot, i.e. `real_count - 1`, as the `this` slot for `captures_this` closures).
pub(crate) fn clone_closure_rebind_this(closure_bits: u64, recv_box: f64) -> u64 {
    let tag = closure_bits & 0xFFFF_0000_0000_0000;
    if tag != 0x7FFD_0000_0000_0000 {
        return closure_bits;
    }
    let ptr = (closure_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    // Validate the payload is a real heap closure BEFORE any header read.
    // `is_closure_ptr` rejects the native/fetch/proxy small-handle band, any
    // address outside the platform heap range, misaligned pointers, AND
    // confirms CLOSURE_MAGIC — so a mis-boxed POINTER_TAG value (a fetch handle,
    // or an `i32 << 32` style value above the band) can't SIGSEGV the probe
    // (#4740, #wall2). This subsumes the old hand-rolled band + magic checks.
    if !is_closure_ptr(ptr) {
        return closure_bits;
    }
    unsafe {
        let header = ptr as *const ClosureHeader;
        // Arrow functions bind `this` lexically: their `this` capture slot holds
        // the enclosing instance and must NEVER be overwritten with a call-time
        // receiver (proxy handler, getter receiver, method-call object, …).
        // They still carry CAPTURES_THIS_FLAG (the body reads `this`), so the
        // flag check below does not exclude them — guard explicitly. Without this,
        // an arrow used as a proxy trap / accessor would observe the rebind
        // receiver and lose its captured instance's data fields (#wall11).
        if crate::closure::closure_is_arrow(header) {
            return closure_bits;
        }
        let raw_count = (*header).capture_count;
        // No CAPTURES_THIS_FLAG → the closure body doesn't read `this`, no rebind needed.
        if raw_count & CAPTURES_THIS_FLAG == 0 {
            return closure_bits;
        }
        // Generator state-machine step closures (`next`/`return`/`throw`) capture
        // the generator BODY's `this` lexically — it is fixed at generator
        // creation and must NOT be re-bound by `.call`/method dispatch. The
        // `yield* gen` desugar calls `next.call(iter, v)`; rebinding here would
        // clobber the captured body-`this` with the iterator object. The flag is
        // stamped on the closure header (per-closure, no global table) by
        // `js_generator_attach_prototype` when it wires the generator instance.
        if raw_count & NO_THIS_REBIND_FLAG != 0 {
            return closure_bits;
        }
        let count = real_capture_count(raw_count) as usize;
        if count == 0 {
            return closure_bits;
        }
        // Allocate a fresh closure with the same func_ptr + capture_count (preserving the flag).
        let scope = crate::gc::RuntimeHandleScope::new();
        let closure_handle = scope.root_nanbox_u64(closure_bits);
        let recv_handle = scope.root_nanbox_f64(recv_box);
        let func_ptr = (*header).func_ptr;
        let new_closure = js_closure_alloc(func_ptr, raw_count);
        let source_bits = closure_handle.get_nanbox_u64();
        let source_ptr = (source_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        let source_type_tag = std::ptr::read_volatile(
            (source_ptr as *const u8).add(CLOSURE_TYPE_TAG_OFFSET) as *const u32,
        );
        if source_type_tag != CLOSURE_MAGIC {
            return source_bits;
        }
        let src_captures = closure_capture_slots_mut(source_ptr as *mut ClosureHeader);
        let dst_captures = closure_capture_slots_mut(new_closure);
        // Copy every capture verbatim, then overwrite the `this` slot (last) with recv_box.
        // GC_STORE_AUDIT(BARRIERED): rebound closure captures are followed by layout/barrier rebuild.
        for i in 0..count {
            *dst_captures.add(i) = *src_captures.add(i);
        }
        let this_slot = count - 1;
        // GC_STORE_AUDIT(BARRIERED): rebound this capture is included in the layout/barrier rebuild.
        *dst_captures.add(this_slot) = recv_handle.get_nanbox_f64().to_bits();
        rebuild_closure_layout_and_barriers(new_closure, count);
        let new_ptr = new_closure as u64;
        0x7FFD_0000_0000_0000 | (new_ptr & 0x0000_FFFF_FFFF_FFFF)
    }
}
