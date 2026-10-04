//! A function object's own properties, `this`-rebind/unbind helpers, and the
//! closure kind predicate.
//!
//! Own properties live IN the function object (D1, the every-receiver-shape
//! lane): `ClosureHeader::props` points at a runtime-internal null-prototype
//! `ObjectHeader` (the "bag", `closure::props`) whose keys and slots are the
//! function's own string-keyed data properties, in ordinary creation order.
//! The bag is a traced child edge of the closure, so it moves and dies with
//! it; the three address-keyed side tables it replaces (own values, deleted
//! synthesized keys, recorded [[Prototype]]) and their young log, re-key hook,
//! dead-owner prune and root scanner are gone. What remains keyed by address
//! is the wasm-host funcref table below.

use super::*;
#[cfg(feature = "wasm-host")]
use crate::fast_hash::{new_ptr_hash_map, PtrHashMap};
#[cfg(feature = "wasm-host")]
use std::sync::{Mutex, OnceLock};

#[cfg(feature = "wasm-host")]
per_test_global! {
    /// Host-owned funcref handles whose JavaScript wrappers are closures.
    /// The closure address is a weak owner key; move/death hooks rekey or
    /// release the handle without keeping the wrapper alive.
    static WASM_FUNCREF_EXTERNALS: OnceLock<Mutex<PtrHashMap<usize, usize>>> = OnceLock::new();
}

#[cfg(feature = "wasm-host")]
fn get_wasm_funcref_externals() -> &'static Mutex<PtrHashMap<usize, usize>> {
    crate::once_init::get_or_init(&WASM_FUNCREF_EXTERNALS, || Mutex::new(new_ptr_hash_map()))
}

#[cfg(feature = "wasm-host")]
fn drop_wasm_funcref_external(handle: usize) {
    crate::webassembly::drop_host_extern_handle(handle);
}

#[cfg(feature = "wasm-host")]
pub(crate) fn register_wasm_funcref_external(owner: usize, handle: usize) {
    if owner == 0 || handle == 0 {
        drop_wasm_funcref_external(handle);
        return;
    }
    let replaced = match get_wasm_funcref_externals().lock() {
        Ok(mut externals) => externals.insert(owner, handle),
        Err(_) => Some(handle),
    };
    if let Some(replaced) = replaced {
        drop_wasm_funcref_external(replaced);
    }
}

/// Record that `key` was `delete`d off the closure at `ptr` — the #3655
/// marker for a SYNTHESIZED own property (`name`, `length`, `prototype`, a
/// builtin static) that has no stored value to drop. Kept in the bag's
/// internal state record (`closure::props`).
pub fn closure_mark_key_deleted(ptr: usize, key: &str) {
    if ptr == 0 || !is_closure_ptr(ptr) {
        return;
    }
    unsafe { super::props::state_mark_deleted(ptr, key) };
    super::shape::note_function_own_state_changed(ptr);
}

/// True if `key` was previously `delete`d off the closure at `ptr`.
pub fn closure_is_key_deleted(ptr: usize, key: &str) -> bool {
    if ptr == 0 || !is_closure_ptr(ptr) {
        return false;
    }
    unsafe { super::props::state_is_deleted(ptr, key) }
}

/// True if `prop` is an OWN dynamic property of the closure at `ptr` (does NOT
/// walk the static-prototype chain, unlike `closure_get_dynamic_prop`). Used
/// by `hasOwnProperty`/`getOwnPropertyNames` to report own user props and the
/// constructor `prototype` slot without inheriting from a set prototype.
pub fn closure_has_own_dynamic_prop(ptr: usize, prop: &str) -> bool {
    closure_get_own_dynamic_prop(ptr, prop).is_some()
}

/// Record `Object.setPrototypeOf(closure_ptr, proto)`. `proto_bits` is the
/// NaN-box bits of the prototype object (POINTER-tagged). Idempotent overwrite.
///
/// #36 / #321: effect's `Context.Tag(id)` wires `Object.setPrototypeOf(TagClass,
/// TagProto)`; recording the link lets string- and symbol-keyed reads on the
/// closure walk to the proto's own properties. The link lives in the bag's
/// internal state record, a traced edge of the closure.
pub fn closure_set_static_prototype(closure_ptr: usize, proto_bits: u64) {
    if closure_ptr == 0 || !is_closure_ptr(closure_ptr) {
        return;
    }
    unsafe { super::props::state_set_prototype(closure_ptr, proto_bits) };
    super::shape::note_function_own_state_changed(closure_ptr);
}

/// Look up the static prototype object bits recorded for a closure, if any.
pub fn closure_static_prototype(closure_ptr: usize) -> Option<u64> {
    if closure_ptr == 0 || !is_closure_ptr(closure_ptr) {
        return None;
    }
    unsafe { super::props::state_prototype(closure_ptr) }
}

/// Dead-payload sweep arm: release the wasm-host funcref handle owned by the
/// DEAD closure at `ptr`. Own properties need nothing — they die with it.
pub(crate) fn clear_closure_side_tables_for_dead_ptr(ptr: usize) {
    if ptr == 0 {
        return;
    }
    #[cfg(feature = "wasm-host")]
    let external = get_wasm_funcref_externals()
        .lock()
        .ok()
        .and_then(|mut externals| externals.remove(&ptr));
    #[cfg(feature = "wasm-host")]
    if let Some(external) = external {
        drop_wasm_funcref_external(external);
    }
}

/// Cheap sweep gate: true when any closure-keyed side table has entries.
pub(crate) fn closure_dynamic_side_tables_nonempty() -> bool {
    #[cfg(feature = "wasm-host")]
    return get_wasm_funcref_externals()
        .lock()
        .is_ok_and(|m| !m.is_empty());
    #[cfg(not(feature = "wasm-host"))]
    false
}

/// Death pruning for the closure-keyed wasm-host funcref table, with one of
/// the GC's deadness predicates.
pub(crate) fn prune_dead_closure_side_table_owners(is_dead_closure: &dyn Fn(usize) -> bool) {
    #[cfg(not(feature = "wasm-host"))]
    let _ = is_dead_closure;
    #[cfg(feature = "wasm-host")]
    let removed = if let Ok(mut externals) = get_wasm_funcref_externals().lock() {
        let mut removed = Vec::new();
        externals.retain(|owner, handle| {
            let keep = !is_dead_closure(*owner);
            if !keep {
                removed.push(*handle);
            }
            keep
        });
        removed
    } else {
        Vec::new()
    };
    #[cfg(feature = "wasm-host")]
    for external in removed {
        drop_wasm_funcref_external(external);
    }
}

/// Thread-heap teardown (#11319): drop every entry of the PROCESS-GLOBAL
/// closure side table (the wasm-host funcref table) whose owner lies in one
/// of `ranges` — the blocks an exiting thread's arena is about to free.
///
/// The table outlives the thread that inserted an entry, and the owner's
/// memory goes back to the allocator. [`prune_dead_closure_side_table_owners`]
/// cannot reach these entries (a foreign address does not attribute), so
/// without this they leak for the life of the process, and once another
/// thread's arena reuses the address range a stale entry would name a new
/// closure allocated there. A function's own properties are not here: they
/// live in the function object (`closure/props.rs`) and die with it.
///
/// Only the mutexed process-global table is touched: this runs from a TLS
/// destructor, where the thread-local tables (box captures) may already be
/// gone, and they die with the thread anyway.
pub(crate) fn release_closure_side_table_owners_in_ranges(ranges: &[(usize, usize)]) {
    #[cfg(not(feature = "wasm-host"))]
    let _ = ranges;
    #[cfg(feature = "wasm-host")]
    {
        if ranges.is_empty() {
            return;
        }
        // Arena blocks never overlap, so a start-sorted list answers
        // membership with one binary search per owner.
        let mut ranges = ranges.to_vec();
        ranges.sort_unstable_by_key(|&(start, _)| start);
        let in_ranges = |owner: usize| {
            let after = ranges.partition_point(|&(start, _)| start <= owner);
            after > 0 && owner < ranges[after - 1].1
        };
        let removed = if let Ok(mut externals) = get_wasm_funcref_externals().lock() {
            let mut removed = Vec::new();
            externals.retain(|owner, handle| {
                let keep = !in_ranges(*owner);
                if !keep {
                    removed.push(*handle);
                }
                keep
            });
            removed
        } else {
            Vec::new()
        };
        for external in removed {
            drop_wasm_funcref_external(external);
        }
    }
}

/// [`prune_dead_closure_side_table_owners`] for a MINOR. The wasm-host table
/// is tiny and only exists with that feature, so a minor walks it whole.
pub(crate) fn prune_dead_closure_side_table_owners_young(is_dead_closure: &dyn Fn(usize) -> bool) {
    prune_dead_closure_side_table_owners(is_dead_closure);
}

/// Owner-move hook (`GcMoveHookKind::ClosureDynamicProps`): re-key the
/// wasm-host funcref table. Own properties move with the closure.
pub(crate) fn closure_dynamic_props_owner_moved(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner {
        return;
    }
    #[cfg(feature = "wasm-host")]
    let replaced = get_wasm_funcref_externals()
        .lock()
        .ok()
        .and_then(|mut externals| {
            let handle = externals.remove(&old_owner)?;
            externals.insert(new_owner, handle)
        });
    #[cfg(feature = "wasm-host")]
    if let Some(replaced) = replaced {
        drop_wasm_funcref_external(replaced);
    }
}

/// Mutable GC scanner for the wasm-host funcref table: its keys are
/// metadata (re-keyed when a closure moves, never a root).
pub fn scan_closure_dynamic_props_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    #[cfg(feature = "wasm-host")]
    {
        let owners: Vec<usize> = get_wasm_funcref_externals()
            .lock()
            .map(|m| m.keys().copied().collect())
            .unwrap_or_default();
        for owner in owners {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) && new_owner != owner {
                closure_dynamic_props_owner_moved(owner, new_owner);
            }
        }
    }
    #[cfg(not(feature = "wasm-host"))]
    let _ = visitor;
}

/// Is `ptr` a live function object (`GC_TYPE_CLOSURE` cell)? Safe for an
/// ARBITRARY word: it proves ownership before trusting any header byte.
///
/// Two terms, cheapest and most selective first:
///
/// 1. the word at +4 is an exotic-band ShapeId. Every closure is born with one
///    (`closure::shape`), and shape rule 3 keeps every non-object kind's +4
///    below the ShapeId range, while an `ObjectHeader`'s +4 is an ordinary- or
///    dictionary-band id — so arrays, strings, Maps, plain objects and class
///    instances all fail here with one load, the job the old "CLOS" magic did;
/// 2. ownership, then the authoritative kind byte: arena page membership
///    (`classify_heap_generation`, the page-class table — the same proof the
///    pre-shape predicate used) or, for a cell in no arena, an exact
///    malloc-registry hit (`try_read_tracked_gc_header`); then the GcHeader
///    type byte says CLOSURE and the cell is not an evacuated stub. The
///    general tracked resolver's range search measured 17% of a `fn.length`
///    read, which is why the arena case does not take it.
pub fn is_closure_ptr(ptr: usize) -> bool {
    // Reject the native / Web-Fetch small-handle band (see `value::addr_class`)
    // and anything outside the platform heap range BEFORE the +4 load: fetch
    // handles, proxy ids and mis-boxed words are not heap addresses.
    if crate::value::addr_class::is_handle_band(ptr) {
        return false;
    }
    if !crate::value::addr_class::is_valid_obj_ptr(ptr as *const u8) {
        return false;
    }
    if !ptr.is_multiple_of(std::mem::align_of::<ClosureHeader>()) {
        return false;
    }
    let shape = unsafe { *((ptr as *const u8).add(super::CLOSURE_SHAPE_OFFSET) as *const u32) };
    if !crate::object::shapes::is_exotic_shape_id(shape) {
        return false;
    }
    let (obj_type, gc_flags) = if matches!(
        crate::arena::classify_heap_generation(ptr),
        crate::arena::HeapGeneration::Unknown
    ) {
        let Some(header) = (unsafe { crate::value::addr_class::try_read_tracked_gc_header(ptr) })
        else {
            return false;
        };
        let header = unsafe { header.as_ref() };
        (header.obj_type, header.gc_flags)
    } else {
        let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(ptr) }) else {
            return false;
        };
        (header.obj_type, header.gc_flags)
    };
    obj_type == crate::gc::GC_TYPE_CLOSURE && gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
}

/// C-ABI predicate: returns 1 when `value_bits` (a NaN-boxed JSValue passed as
/// raw bits) is a closure/function — a `POINTER_TAG` value whose pointee
/// is a `GC_TYPE_CLOSURE` cell — and 0 for objects, arrays, strings, numbers, and
/// everything else. Exposed for external wrapper crates that link the runtime
/// only by C ABI (e.g. perry-ext-http's `parse_listen_args`, #2041),
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
    closure_get_dynamic_prop_keyed(ptr, prop, std::ptr::null())
}

/// [`closure_get_dynamic_prop`] with the caller's key header, when it has one
/// (`key` may be null): a class constructor's read then builds no key string.
pub(crate) fn closure_get_dynamic_prop_keyed(
    ptr: usize,
    prop: &str,
    key: *const crate::StringHeader,
) -> f64 {
    if !is_closure_ptr(ptr) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    // `PerformanceObserver.supportedEntryTypes` is a built-in static accessor:
    // reflection reads its descriptor below, while ordinary property reads
    // must invoke it and receive a fresh frozen array. Native-module class
    // exports otherwise store only ordinary dynamic data properties, so keep
    // this constructor-specific accessor at the common closure read seam.
    if prop == "supportedEntryTypes" {
        let value = crate::value::js_nanbox_pointer(ptr as i64);
        if unsafe { crate::object::bound_native_callable_module_and_method(value) }.is_some_and(
            |(module, method)| module == "perf_hooks" && method == "PerformanceObserver",
        ) {
            return crate::perf_hooks::perf_supported_entry_types_value();
        }
    }

    // A closure on its base Function shape has no accessor and no deleted
    // key: every funnel that installs either moves it to FunctionDictionary
    // (`closure::shape`). So the shape answers those two side-table probes.
    let on_base = unsafe { super::shape::closure_on_base_shape(ptr as *const ClosureHeader) };
    if on_base {
        // fall through to the data lookups below
    } else if super::shape::is_class_info(unsafe { (*(ptr as *const ClosureHeader)).info }) {
        // A class constructor: its class lookup (statics, the parent chain,
        // `name`/`length`/`prototype`, Function.prototype) — never the plain
        // function fallbacks below.
        return crate::object::class_value::class_static_read(ptr, prop, key);
    } else if let Some(acc) = crate::object::get_accessor_descriptor(ptr, prop) {
        if acc.get == 0 {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let closure =
            (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
        if closure.is_null() {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let receiver = crate::value::js_nanbox_pointer(ptr as i64);
        return crate::closure::js_closure_call0(
            closure,
            crate::closure::JsThis::from_f64(receiver),
        );
    }

    if let Some(val) = closure_get_own_dynamic_prop(ptr, prop) {
        return val;
    }
    // #11175: resolve these inherited values from the actual prototype,
    // including explicit null/custom chains. Do this before the legacy walk
    // so an inherited getter returning undefined is not invoked twice.
    if matches!(prop, "call" | "apply" | "bind") {
        let method = crate::object::reified_function_method_name(prop).unwrap();
        let receiver = crate::value::js_nanbox_pointer(ptr as i64);
        return unsafe { crate::closure::reify_function_method_value(receiver, method) };
    }
    // Function length is an own intrinsic property.
    if prop == "length" && (on_base || !closure_is_key_deleted(ptr, "length")) {
        let value = crate::value::js_nanbox_pointer(ptr as i64);
        if let Some(arity) = unsafe { crate::object::bound_native_callable_value_arity(value) } {
            return arity as f64;
        }
        if let Some(length) = crate::object::builtin_closure_length(ptr) {
            return length as f64;
        }
        return crate::closure::closure_length(ptr as *const ClosureHeader).unwrap_or(0) as f64;
    }
    // #10084: a `Function.prototype.bind` result's `.name` is built lazily —
    // `js_function_bind` skips the "bound " + target-name string allocation
    // on every call and only snapshots the raw target-name value (capture
    // slot 3). Synthesize and cache the real string here, on first read, so
    // every other reader of a closure's `.name` (ordinary property-get below,
    // `Object.getOwnPropertyDescriptor`, a chained `.bind()`'s own read of an
    // already-bound target) gets it for free through this one seam. Once
    // cached, the `closure_props` lookup above intercepts before this runs
    // again.
    if prop == "name" && (on_base || !closure_is_key_deleted(ptr, "name")) {
        let func_ptr = unsafe { (*(ptr as *const ClosureHeader)).code() };
        if func_ptr == crate::closure::BOUND_FUNCTION_FUNC_PTR {
            return unsafe { crate::closure::bound_function_lazy_name(ptr) };
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
                let this_scope = crate::gc::RuntimeHandleScope::new(); // #9445
                let receiver =
                    this_scope.root_nanbox_f64(crate::value::js_nanbox_pointer(ptr as i64));
                let getter_bits = clone_closure_rebind_this(acc.get, receiver.get_nanbox_f64());
                let getter = (getter_bits & crate::value::POINTER_MASK)
                    as *const crate::closure::ClosureHeader;
                if getter.is_null() {
                    return f64::from_bits(crate::value::TAG_UNDEFINED);
                }
                return crate::closure::js_closure_call0(
                    getter,
                    crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
                );
            }
            if let Some(p) = closure_get_own_dynamic_prop(proto_ptr, prop) {
                return p;
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
            let this_scope = crate::gc::RuntimeHandleScope::new(); // #9445
            let receiver = this_scope.root_nanbox_f64(crate::value::js_nanbox_pointer(ptr as i64));
            let getter_bits = clone_closure_rebind_this(acc.get, receiver.get_nanbox_f64());
            let getter =
                (getter_bits & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
            if getter.is_null() {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            return crate::closure::js_closure_call0(
                getter,
                crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
            );
        }
        {
            // The thread's canonical interned header: no allocation per read,
            // and one stable key identity, so the read below can be served by
            // (and prime) the inherited-read cache instead of minting a fresh
            // key string that no cache entry can ever match again.
            let key_hdr = crate::string::canonical_key(prop.as_bytes());
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
    // #3655 + spec: a DELETED own `name`/`length` is not an own property any
    // more, so the read continues on the prototype — `Function.prototype`
    // itself has own `name` ("") and `length` (0).
    if matches!(prop, "name" | "length") && !on_base && closure_is_key_deleted(ptr, prop) {
        let proto = super::shape::FUNCTION_PROTOTYPE_PTR.load(std::sync::atomic::Ordering::Acquire);
        if proto != 0 && proto as usize != ptr {
            let key_hdr = crate::string::js_string_from_bytes(prop.as_ptr(), prop.len() as u32);
            let v = crate::object::js_object_get_field_by_name(
                proto as *const crate::object::ObjectHeader,
                key_hdr as *const crate::StringHeader,
            );
            return f64::from_bits(v.bits());
        }
    }
    // Every function's [[Prototype]] is %Function.prototype% — an expando
    // installed there (`Function.prototype.property = 12`), or a property
    // installed via `Object.defineProperty(Function.prototype, k, {...})`,
    // must be readable through any closure (`fn.property`, `boundFn.property`,
    // `Function.indicator`).
    let receiver = f64::from_bits(crate::value::js_nanbox_pointer(ptr as i64).to_bits());
    function_prototype_inherited_get(ptr, prop, receiver)
        .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `[[Get]]` of `prop` on the real `%Function.prototype%` object, for a
/// function-object receiver whose own/static lookup already missed — a
/// user method or expando (`Function.prototype.myHelper = fn`) or a
/// `defineProperty` data/accessor property. An accessor's getter runs with
/// `receiver` as `this`. `self_ptr` is the receiver's heap pointer, or 0 for
/// a receiver with none (a ClassRef constructor, #11492), and only guards
/// against Function.prototype reading itself. `None` means nothing is
/// installed there, so the caller keeps its own miss result.
pub(crate) fn function_prototype_inherited_get(
    self_ptr: usize,
    prop: &str,
    receiver: f64,
) -> Option<f64> {
    let proto_ptr = function_prototype_fallback_target(self_ptr, prop)?;
    // A defineProperty accessor on Function.prototype
    // (`{ get: () => 12 }`) is invoked with the reading
    // function as receiver.
    if let Some(acc) = crate::object::get_accessor_descriptor(proto_ptr, prop) {
        if acc.get != 0 {
            let getter =
                (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
            if !getter.is_null() {
                return Some(crate::closure::js_closure_call0(
                    getter,
                    crate::closure::JsThis::from_f64(receiver),
                ));
            }
        }
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    // The thread's canonical interned header: no allocation per read,
    // and one stable key identity, so the read below can be served by
    // (and prime) the inherited-read cache instead of minting a fresh
    // key string that no cache entry can ever match again.
    let key_hdr = crate::string::canonical_key(prop.as_bytes());
    let v = crate::object::js_object_get_field_by_name(
        proto_ptr as *const crate::object::ObjectHeader,
        key_hdr as *const crate::StringHeader,
    );
    if v.is_undefined() {
        None
    } else {
        Some(f64::from_bits(v.bits()))
    }
}

/// Resolve the real, mutable `%Function.prototype%` object pointer for a
/// closure-receiver fallback (GET or SET), or `None` if `prop` doesn't
/// qualify — a synthesized own slot, a method handled by dedicated dispatch
/// (the Function.prototype methods are resolved separately above), an
/// array-index-shaped key, or Function.prototype itself. Shared by
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
    ) || crate::object::reified_function_method_name(prop).is_some()
    {
        return None;
    }
    crate::perry_thread_local! {
        static IN_FN_PROTO_FALLBACK: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }
    let reentrant = IN_FN_PROTO_FALLBACK.with(|c| c.replace(true));
    if reentrant {
        return None;
    }
    // THIS realm's %Function.prototype% (the memoized intrinsic), not whatever
    // `globalThis.Function` names now. It is 0 while the realm global has not
    // been built: %Function.prototype% does not exist yet, so no descriptor
    // can sit on it — and asking must not build the realm global (defining a
    // static on %Object% or a class function object would otherwise do so).
    let proto_ptr = crate::array::function_prototype_addr();
    IN_FN_PROTO_FALLBACK.with(|c| c.set(false));
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

/// Set a dynamic property on a closure (an own data property in its bag).
/// A non-closure `ptr` is ignored.
pub fn closure_set_dynamic_prop(ptr: usize, prop: &str, value: f64) {
    if ptr == 0 || !is_closure_ptr(ptr) {
        return;
    }
    unsafe { super::props::bag_set(ptr, prop, value) };
    // #3655: re-defining a previously deleted slot makes it present again.
    unsafe { super::props::state_clear_deleted(ptr, prop) };
    super::shape::refresh_closure_shape(ptr);
}

/// Read an OWN dynamic property without any prototype/builtin fallback.
/// Used by `bind` to honor an `Object.defineProperty(fn, "length", …)`
/// override before falling back to the registered declared length.
pub fn closure_get_own_dynamic_prop(ptr: usize, prop: &str) -> Option<f64> {
    if ptr == 0 || !is_closure_ptr(ptr) {
        return None;
    }
    unsafe { super::props::bag_get(ptr, prop.as_bytes()) }
}

/// #3655: remove an OWN user dynamic property from a closure (used by
/// `delete fn.userProp`). Returns true if a property was actually removed.
/// Built-in synthesized slots (`name`/`length`/`prototype`) are handled by
/// `closure_mark_key_deleted` instead, since they have no stored value.
pub fn closure_delete_own_dynamic_prop(ptr: usize, prop: &str) -> bool {
    if ptr == 0 || !is_closure_ptr(ptr) {
        return false;
    }
    let removed = unsafe { super::props::bag_remove(ptr, prop) };
    super::shape::refresh_closure_shape(ptr);
    removed
}

#[cfg(test)]
pub(crate) fn test_clear_closure_side_tables() {
    #[cfg(feature = "wasm-host")]
    let externals = get_wasm_funcref_externals()
        .lock()
        .map(|mut externals| {
            externals
                .drain()
                .map(|(_, handle)| handle)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    #[cfg(feature = "wasm-host")]
    for external in externals {
        drop_wasm_funcref_external(external);
    }
}

/// Snapshot every dynamic property on a closure as `(name, value)` pairs in
/// ECMA-262 own-key order: integer indices ascending, then other strings in
/// property-creation order. Used by all function-object enumeration paths and
/// by `format_jsvalue` to emit `[Function: f] { ownProp: value }`. See #1203
/// and #9148.
pub fn closure_dynamic_props_snapshot(ptr: usize) -> Vec<(String, f64)> {
    if ptr == 0 || !is_closure_ptr(ptr) {
        return Vec::new();
    }
    unsafe { super::props::bag_snapshot(ptr) }
}

#[cfg(test)]
mod tests_1802 {
    use super::*;

    static SIDE_TABLE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Poison-tolerant acquisition: one test's assert failure must read as
    /// ONE failure, not cascade `PoisonError` panics into every sibling that
    /// serializes on this lock (#6965 — the observed second failure). The
    /// guarded data is `()`, so poison carries no corruption to tolerate.
    fn side_table_test_lock() -> std::sync::MutexGuard<'static, ()> {
        SIDE_TABLE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn dyn_prop_get_ignores_non_closure_receivers() {
        // CLOSURE_PROPS is PROCESS-global; the gc test guards' state reset
        // (`test_clear_closure_side_tables`) clears it from parallel test
        // threads, wiping this test's parked entry mid-assertion. Serialize
        // against those guards, THEN against this module's own tests.
        let _global = crate::gc::global_side_table_test_lock();
        let _guard = side_table_test_lock();
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

    /// A managed cell's GC kind must outrank a +4 word that merely looks like
    /// a Function ShapeId (forged into an ErrorHeader's `error_kind` here).
    #[test]
    fn managed_error_with_a_function_shape_word_is_not_a_closure() {
        unsafe {
            let message = crate::string::js_string_from_bytes(b"survives".as_ptr(), 8);
            let error = crate::error::js_error_new_with_message(message);
            // GC_STORE_AUDIT(POINTER_FREE): writes the u32 magic constant into
            // an ErrorHeader's padding on purpose, so the assertion below proves
            // the GC kind outranks look-alike bytes. No heap pointer is stored,
            // so there is nothing for a barrier to track.
            let word = (error as *mut u8).add(super::super::CLOSURE_SHAPE_OFFSET) as *mut u32;
            let saved = word.read();
            word.write(super::super::shape::function_base_shape(
                super::super::shape::FunctionProtoKind::Function,
            ));

            assert!(!is_closure_ptr(error as usize));
            // GC_STORE_AUDIT(POINTER_FREE): restores the scalar error_kind word.
            word.write(saved);
            assert_eq!((*error).message, message);

            let key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
            let value = crate::object::js_object_get_field_by_name(error.cast(), key);
            assert_eq!(
                value.bits() & crate::value::POINTER_MASK,
                message as usize as u64,
                "property lookup must reach Error handling, not the closure path",
            );
        }
    }
}

/// Does `header`'s body read `this` from its reserved last capture slot, so
/// that calling it with a receiver other than the one captured needs
/// [`clone_closure_rebind_this`]? When this is false the body binds `this`
/// from the call's receiver parameter (or lexically, or never reads it), and
/// a call through the closure ABI with the receiver as `this` is the whole
/// [[Call]].
///
/// # Safety
/// `header` passed `is_closure_ptr`.
#[inline]
pub(crate) unsafe fn closure_reads_this_from_capture(header: *const ClosureHeader) -> bool {
    // Arrow functions bind `this` lexically: their `this` capture slot holds
    // the enclosing instance and must NEVER be overwritten with a call-time
    // receiver (proxy handler, getter receiver, method-call object, …).
    // They still carry CAPTURES_THIS_FLAG (the body reads `this`), so the
    // flag check below does not exclude them — guard explicitly. Without this,
    // an arrow used as a proxy trap / accessor would observe the rebind
    // receiver and lose its captured instance's data fields (#wall11).
    if crate::closure::closure_is_arrow(header) {
        return false;
    }
    let raw_count = (*header).capture_count;
    // No CAPTURES_THIS_FLAG → the closure body doesn't read `this`, no rebind needed.
    if raw_count & CAPTURES_THIS_FLAG == 0 {
        return false;
    }
    // Generator state-machine step closures (`next`/`return`/`throw`) capture
    // the generator BODY's `this` lexically — it is fixed at generator
    // creation and must NOT be re-bound by `.call`/method dispatch. The
    // `yield* gen` desugar calls `next.call(iter, v)`; rebinding here would
    // clobber the captured body-`this` with the iterator object. The flag is
    // stamped on the closure header (per-closure, no global table) by
    // `js_generator_attach_prototype` when it wires the generator instance.
    if raw_count & NO_THIS_REBIND_FLAG != 0 {
        return false;
    }
    real_capture_count(raw_count) != 0
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
        if !closure_reads_this_from_capture(header) {
            return closure_bits;
        }
        let raw_count = (*header).capture_count;
        let count = real_capture_count(raw_count) as usize;
        // Allocate a fresh closure of the same body + capture_count (preserving the flag).
        let scope = crate::gc::RuntimeHandleScope::new();
        let closure_handle = scope.root_nanbox_u64(closure_bits);
        let recv_handle = scope.root_nanbox_f64(recv_box);
        let new_closure = js_closure_alloc((*header).info, raw_count);
        let source_bits = closure_handle.get_nanbox_u64();
        let source_ptr = (source_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if !closure_kind_probe(source_ptr) {
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

/// `PERRY_GC_CENSUS`: closure-keyed side tables (own properties are object
/// storage now and are counted with the heap).
pub(super) fn dynamic_props_census() -> Vec<crate::gc::census::SideTableRow> {
    #[allow(unused_mut)]
    let mut rows = Vec::new();
    #[cfg(feature = "wasm-host")]
    if let Ok(m) = get_wasm_funcref_externals().lock() {
        rows.push((
            "closure.wasm_funcref_externals",
            m.len(),
            crate::gc::census::map_bytes(&m),
        ));
    }
    rows
}
