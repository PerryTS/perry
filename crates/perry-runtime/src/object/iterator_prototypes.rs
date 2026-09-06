//! Shared `%IteratorPrototype%`-style prototype singletons for the built-in
//! iterator objects (Array / Map / Set / String / RegExp-string / Iterator
//! Helper iterators).
//!
//! test262's `verifyProperty` suite (built-ins/{Array,Map,Set,String}
//! IteratorPrototype) requires that:
//!   1. `Object.getPrototypeOf([][Symbol.iterator]())` returns a SHARED
//!      singleton `%ArrayIteratorPrototype%` (the same object every call), not
//!      the iterator instance itself.
//!   2. `.next` is an OWN property of that prototype (the instance inherits it)
//!      with descriptor `{ writable: true, enumerable: false, configurable: true }`.
//!   3. `proto.next.name === "next"` (non-writable, non-enum, configurable) and
//!      `proto.next.length === 0`.
//!   4. Each family prototype chains up to a shared `%IteratorPrototype%` that
//!      carries `[Symbol.iterator]` returning `this`.
//!
//! Design: each iterator instance (allocated in `array/iter_object.rs` and
//! `collection_iter_object.rs` / `string_iter_object.rs`) has its `[[Prototype]]`
//! set to the matching singleton via `prototype_chain::object_set_static_prototype`
//! at allocation time. From there ALL existing machinery just works:
//!   - `Object.getPrototypeOf(it)` resolves through the early
//!     `object_static_prototype` check in `js_object_get_prototype_of`.
//!   - `it.next` (a value READ) resolves through `resolve_inherited_field`, which
//!     binds `this` to the instance before reading the inherited `next` closure.
//!   - `getOwnPropertyDescriptor(proto, "next")` reads the recorded builtin
//!     attrs off the prototype object (it's a regular `GC_TYPE_OBJECT` field).
//!
//! The `next` closures are thin thunks: they read `js_implicit_this_get()` and
//! route by the receiver's class id to the existing
//! `dispatch_{array,map,set,string}_iterator_method`. This is the ONLY behaviour
//! addition for `proto.next.call(it)` / value-read `it.next()`; the class-id CALL
//! fast path in `native_call_method.rs` is untouched, so `for-of`, spread, and
//! `Array.from` keep driving `.next()` directly.

use super::{
    install_proto_method, js_object_alloc, set_builtin_property_attrs, ObjectHeader, PropertyAttrs,
};
use crate::value::JSValue;
use std::sync::atomic::{AtomicI64, Ordering};

// GC-rooted singleton slots. Each realm builds its own tower in its own arena;
// the process-global handles resolve to per-agent atomics and are scanned in
// `object/mod.rs::scan_object_cache_roots_mut` (#8002).
crate::perry_thread_local! {
    static ITERATOR_PROTOTYPE_PTR_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static ARRAY_ITERATOR_PROTOTYPE_PTR_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static MAP_ITERATOR_PROTOTYPE_PTR_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static SET_ITERATOR_PROTOTYPE_PTR_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static STRING_ITERATOR_PROTOTYPE_PTR_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static REGEXP_STRING_ITERATOR_PROTOTYPE_PTR_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static ITERATOR_HELPER_PROTOTYPE_PTR_SLOT: AtomicI64 = const { AtomicI64::new(0) };
}

pub(crate) static ITERATOR_PROTOTYPE_PTR: super::RealmAtomicI64 =
    super::RealmAtomicI64::new(&ITERATOR_PROTOTYPE_PTR_SLOT);
pub(crate) static ARRAY_ITERATOR_PROTOTYPE_PTR: super::RealmAtomicI64 =
    super::RealmAtomicI64::new(&ARRAY_ITERATOR_PROTOTYPE_PTR_SLOT);
pub(crate) static MAP_ITERATOR_PROTOTYPE_PTR: super::RealmAtomicI64 =
    super::RealmAtomicI64::new(&MAP_ITERATOR_PROTOTYPE_PTR_SLOT);
pub(crate) static SET_ITERATOR_PROTOTYPE_PTR: super::RealmAtomicI64 =
    super::RealmAtomicI64::new(&SET_ITERATOR_PROTOTYPE_PTR_SLOT);
pub(crate) static STRING_ITERATOR_PROTOTYPE_PTR: super::RealmAtomicI64 =
    super::RealmAtomicI64::new(&STRING_ITERATOR_PROTOTYPE_PTR_SLOT);
pub(crate) static REGEXP_STRING_ITERATOR_PROTOTYPE_PTR: super::RealmAtomicI64 =
    super::RealmAtomicI64::new(&REGEXP_STRING_ITERATOR_PROTOTYPE_PTR_SLOT);
pub(crate) static ITERATOR_HELPER_PROTOTYPE_PTR: super::RealmAtomicI64 =
    super::RealmAtomicI64::new(&ITERATOR_HELPER_PROTOTYPE_PTR_SLOT);

/// Resolve and validate the implicit-`this` object shared by the family
/// prototype thunks. Keeping the raw-address probe here gives both the generic
/// family dispatcher and the helper-specific brand check one audited path.
unsafe fn implicit_this_iterator_object() -> Option<*mut ObjectHeader> {
    let this = super::js_implicit_this_get();
    let jv = JSValue::from_bits(this.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let obj = jv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if obj.is_null() || !super::is_valid_obj_ptr(obj as *const u8) {
        return None;
    }
    Some(obj)
}

/// Dispatch `method` on the implicit-`this` iterator instance, routing by class
/// id to the matching existing iterator dispatcher. Shared by the per-family
/// `next` thunks (read as a value or invoked via `.call`) and the parent
/// `[Symbol.iterator]` thunk. Returns a `{ value:undefined, done:true }`-ish
/// throw when `this` is not a recognized iterator (test262 `this-not-object` /
/// `does-not-have-...-internal-slots` brand checks).
unsafe fn dispatch_on_implicit_this(method: &str) -> f64 {
    let Some(obj) = implicit_this_iterator_object() else {
        return brand_type_error(method);
    };
    let class_id = (*obj).class_id;
    // The `_builtin` variants skip the own-`next` override probe (#9019):
    // these thunks ARE the canonical prototype `next` functions, so invoking
    // one directly (`proto.next.call(it)`, or a `.bind(it)` taken before a
    // patch landed) must run the builtin algorithm — probing here would send
    // a patch that delegates to its bound original into infinite recursion.
    match class_id {
        crate::array::ARRAY_ITERATOR_CLASS_ID => {
            crate::array::dispatch_array_iterator_method_builtin(obj, method)
        }
        crate::collection_iter_object::MAP_ITERATOR_CLASS_ID => {
            crate::collection_iter_object::dispatch_map_iterator_method_builtin(obj, method)
        }
        crate::collection_iter_object::SET_ITERATOR_CLASS_ID => {
            crate::collection_iter_object::dispatch_set_iterator_method_builtin(obj, method)
        }
        crate::string::STRING_ITERATOR_CLASS_ID => {
            crate::string::dispatch_string_iterator_method_builtin(obj, method)
        }
        #[cfg(feature = "regex-engine")]
        crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID => {
            crate::regex::dispatch_regexp_string_iterator_method_builtin(obj, method)
        }
        _ => brand_type_error(method),
    }
}

/// TypeError thrown by an iterator-prototype method invoked on an incompatible
/// receiver (test262's brand-check cases).
fn brand_type_error(method: &str) -> f64 {
    let mut msg = b"Method %IteratorPrototype%.".to_vec();
    msg.extend_from_slice(method.as_bytes());
    msg.extend_from_slice(b" called on incompatible receiver");
    let h = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(h);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

// --- `next` thunks, one per family (all read implicit-this, dispatch by id) ---

extern "C" fn array_iterator_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    unsafe { dispatch_on_implicit_this("next") }
}
extern "C" fn map_iterator_next_thunk(_c: *const crate::closure::ClosureHeader, _arg: f64) -> f64 {
    unsafe { dispatch_on_implicit_this("next") }
}
extern "C" fn set_iterator_next_thunk(_c: *const crate::closure::ClosureHeader, _arg: f64) -> f64 {
    unsafe { dispatch_on_implicit_this("next") }
}
extern "C" fn string_iterator_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    unsafe { dispatch_on_implicit_this("next") }
}
extern "C" fn regexp_string_iterator_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    unsafe { dispatch_on_implicit_this("next") }
}

/// `%Iterator Helper Prototype%.next` has a helper-specific brand check. Use
/// the intrinsic algorithm directly rather than the general method dispatcher:
/// a saved/bound canonical method must not re-enter a later `next` override.
extern "C" fn iterator_helper_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    unsafe {
        let Some(obj) = implicit_this_iterator_object() else {
            return brand_type_error("next");
        };
        if (*obj).class_id != crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID {
            return brand_type_error("next");
        }
        crate::iterator_helpers::iterator_helper_next_builtin(obj)
    }
}

/// `%IteratorPrototype%[Symbol.iterator]()` returns `this` (the iterator).
extern "C" fn iterator_proto_symbol_iterator_thunk(
    _c: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    super::js_implicit_this_get()
}

/// Set `obj[Symbol.toStringTag] = tag` with the spec descriptor
/// `{ writable:false, enumerable:false, configurable:true }`. Mirrors the
/// generator-tower helper in `global_this.rs`.
fn set_to_string_tag(obj: *mut ObjectHeader, tag: &str) {
    let sym = crate::symbol::well_known_symbol("toStringTag");
    if sym.is_null() {
        return;
    }
    let tag_str = crate::string::js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(obj as i64),
            f64::from_bits(JSValue::pointer(sym as *const u8).bits()),
            f64::from_bits(crate::js_nanbox_string(tag_str as i64).to_bits()),
        );
    }
}

/// Link `child`'s `[[Prototype]]` to `parent`.
///
/// Uses the class-DEFAULT variant, not `object_set_static_prototype`. Attaching
/// `%ArrayIteratorPrototype%` to a fresh array iterator is exactly what that
/// function documents — a chain identical for every instance of the class — and
/// not a user `Object.setPrototypeOf`. Before #9251, the loud variant's
/// divergence bit was also the user-override signal, so every built-in iterator
/// appeared user-reparented. A caller treating that as "the per-instance chain
/// is authoritative, resolve methods by ordinary inheriting lookup" then
/// reached the `%…IteratorPrototype%` `next` THUNK,
/// which resolves its receiver from `js_implicit_this_get()` rather than the
/// bound `this` (#7576) — producing `Method %IteratorPrototype%.next called on
/// incompatible receiver`. The prototype itself is still recorded either way;
/// the class-default variant also avoids the divergence bit and cache flushes.
fn chain_to(child: *mut ObjectHeader, parent: *mut ObjectHeader) {
    let parent_bits = crate::value::js_nanbox_pointer(parent as i64).to_bits();
    super::prototype_chain::object_link_class_default_prototype(child as usize, parent_bits);
}

/// Build the shared `%IteratorPrototype%` and the four family prototypes,
/// storing them in the GC-rooted slots. Idempotent.
fn build_iterator_prototypes() {
    // The tower is reachable lazily from the first iterator allocation, not
    // only from globalThis bootstrap. Keep its raw locals stable across the
    // allocating method/tag installs, just like the generator and TypedArray
    // intrinsic builders (#7251).
    let _no_move = crate::gc::GcSuppressScope::new();
    // Shared %IteratorPrototype% — carries [Symbol.iterator] returning `this`.
    let shared = js_object_alloc(0, 0);
    if shared.is_null() {
        return;
    }
    install_symbol_iterator(shared);

    let array_proto = build_family_proto(array_iterator_next_thunk, "Array Iterator", shared);
    let map_proto = build_family_proto(map_iterator_next_thunk, "Map Iterator", shared);
    let set_proto = build_family_proto(set_iterator_next_thunk, "Set Iterator", shared);
    let string_proto = build_family_proto(string_iterator_next_thunk, "String Iterator", shared);
    let regexp_string_proto = build_family_proto(
        regexp_string_iterator_next_thunk,
        "RegExp String Iterator",
        shared,
    );
    let iterator_helper_proto =
        build_family_proto(iterator_helper_next_thunk, "Iterator Helper", shared);

    ITERATOR_PROTOTYPE_PTR.store(shared as i64, Ordering::Release);
    ARRAY_ITERATOR_PROTOTYPE_PTR.store(array_proto as i64, Ordering::Release);
    MAP_ITERATOR_PROTOTYPE_PTR.store(map_proto as i64, Ordering::Release);
    SET_ITERATOR_PROTOTYPE_PTR.store(set_proto as i64, Ordering::Release);
    STRING_ITERATOR_PROTOTYPE_PTR.store(string_proto as i64, Ordering::Release);
    REGEXP_STRING_ITERATOR_PROTOTYPE_PTR.store(regexp_string_proto as i64, Ordering::Release);
    ITERATOR_HELPER_PROTOTYPE_PTR.store(iterator_helper_proto as i64, Ordering::Release);
}

/// Install `[Symbol.iterator]` on the shared parent as a real method whose
/// `name`/`length` own props match the spec (`"[Symbol.iterator]"`, length 0).
fn install_symbol_iterator(shared: *mut ObjectHeader) {
    let func_ptr = iterator_proto_symbol_iterator_thunk as *const u8;
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 0);
    super::native_module::set_bound_native_closure_name(closure, "[Symbol.iterator]");
    super::native_module::set_builtin_closure_length(closure as usize, 0);
    set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    let sym = crate::symbol::well_known_symbol("iterator");
    if sym.is_null() {
        return;
    }
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(shared as i64),
            f64::from_bits(JSValue::pointer(sym as *const u8).bits()),
            crate::value::js_nanbox_pointer(closure as i64),
        );
    }
    crate::symbol::set_symbol_property_attrs(
        shared as usize,
        sym as usize,
        PropertyAttrs::new(true, false, true),
    );
}

/// Allocate one family prototype with an own `next` method (spec descriptor),
/// a `[Symbol.toStringTag]`, and `[[Prototype]] === shared %IteratorPrototype%`.
fn build_family_proto(
    next_thunk: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64,
    tag: &str,
    shared: *mut ObjectHeader,
) -> *mut ObjectHeader {
    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return std::ptr::null_mut();
    }
    // `install_proto_method` records `next` as `{ writable:true, enumerable:false,
    // configurable:true }` and the closure's `name`/`length` as
    // `{ writable:false, enumerable:false, configurable:true }` — exactly the
    // spec descriptor shape test262 verifies. `.length` 0 (next takes no args).
    install_proto_method(proto, "next", next_thunk as *const u8, 0);
    set_to_string_tag(proto, tag);
    chain_to(proto, shared);
    proto
}

/// Whether any iterator-prototype tower has been materialized on this thread.
#[cfg(test)]
pub(crate) fn iterator_prototypes_materialized() -> bool {
    ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) != 0
}

#[cfg(test)]
mod override_probe_premise_tests {
    use super::*;

    /// The fast path in `call_overridden_iterator_next` returns `None` on a
    /// null tower, treating that as PROOF that no override exists. That is only
    /// sound if every route to the prototype object materializes the tower —
    /// this pins the route user code takes, `Object.getPrototypeOf(iter)`,
    /// which lands in `iterator_prototype_for_class_id`.
    ///
    /// Deliberately one-directional: `perry-runtime`'s suite shares process
    /// globals, so asserting the tower starts null would make this depend on
    /// test order. The implication is what the fast path actually relies on.
    #[test]
    fn reaching_an_iterator_prototype_materializes_the_tower() {
        assert!(
            iterator_prototype_for_class_id(crate::array::ARRAY_ITERATOR_CLASS_ID).is_some(),
            "array iterator must have a prototype to reach",
        );
        assert!(
            iterator_prototypes_materialized(),
            "reaching a prototype must materialize the tower, or a null tower \
             would no longer prove the absence of an override",
        );
    }
}

/// Lazily build the prototypes (idempotent). Cheap after the first call.

pub(crate) fn ensure_iterator_prototypes() {
    if ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) == 0 {
        build_iterator_prototypes();
    }
}

/// The singleton prototype for an iterator class id, NaN-boxed, or `None` if the
/// class id is not a built-in iterator. Used by `js_object_get_prototype_of`.
pub(crate) fn iterator_prototype_for_class_id(class_id: u32) -> Option<f64> {
    ensure_iterator_prototypes();
    let slot = match class_id {
        crate::array::ARRAY_ITERATOR_CLASS_ID => &ARRAY_ITERATOR_PROTOTYPE_PTR,
        crate::collection_iter_object::MAP_ITERATOR_CLASS_ID => &MAP_ITERATOR_PROTOTYPE_PTR,
        crate::collection_iter_object::SET_ITERATOR_CLASS_ID => &SET_ITERATOR_PROTOTYPE_PTR,
        crate::string::STRING_ITERATOR_CLASS_ID => &STRING_ITERATOR_PROTOTYPE_PTR,
        crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID => &REGEXP_STRING_ITERATOR_PROTOTYPE_PTR,
        crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID => &ITERATOR_HELPER_PROTOTYPE_PTR,
        _ => return None,
    };
    let ptr = slot.load(Ordering::Acquire);
    if ptr == 0 {
        None
    } else {
        Some(crate::value::js_nanbox_pointer(ptr))
    }
}

/// Set a freshly-allocated iterator instance's `[[Prototype]]` to the matching
/// family singleton. Called from each iterator allocator so `it.next` reads and
/// `getPrototypeOf(it)` resolve through the shared prototype. No-op for unknown
/// class ids.
pub(crate) fn attach_iterator_prototype(obj_ptr: *mut ObjectHeader, class_id: u32) {
    if obj_ptr.is_null() {
        return;
    }
    ensure_iterator_prototypes();
    let slot = match class_id {
        crate::array::ARRAY_ITERATOR_CLASS_ID => &ARRAY_ITERATOR_PROTOTYPE_PTR,
        crate::collection_iter_object::MAP_ITERATOR_CLASS_ID => &MAP_ITERATOR_PROTOTYPE_PTR,
        crate::collection_iter_object::SET_ITERATOR_CLASS_ID => &SET_ITERATOR_PROTOTYPE_PTR,
        crate::string::STRING_ITERATOR_CLASS_ID => &STRING_ITERATOR_PROTOTYPE_PTR,
        crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID => &REGEXP_STRING_ITERATOR_PROTOTYPE_PTR,
        crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID => &ITERATOR_HELPER_PROTOTYPE_PTR,
        _ => return,
    };
    let proto_ptr = slot.load(Ordering::Acquire);
    if proto_ptr == 0 {
        return;
    }
    chain_to(obj_ptr, proto_ptr as *mut ObjectHeader);
}

/// Invoke a user replacement of a built-in iterator prototype's `next`
/// method. Returns `None` while the canonical native thunk is installed.
pub(crate) unsafe fn call_overridden_iterator_next(
    iter_obj: *mut ObjectHeader,
    class_id: u32,
) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(iter_obj as i64));
    let previous = scope.root_nanbox_f64(super::js_implicit_this_get());
    // #9019: an OWN `next` (`it.next = fn`, stored past the reserved floor
    // by `object/reserved_floor.rs`) shadows the prototype thunk and exists
    // independently of the tower, so probe it BEFORE the tower-null
    // early-out — the assignment alone materializes nothing. For every
    // unpatched iterator the probe is one descriptor lookup ending at a
    // null keys edge. A PRESENT own value that is not a closure throws,
    // matching IteratorNext's GetV+Call — it must never fall through to the
    // builtin advance, which would ignore the patch the user installed.
    let own = super::js_object_get_own_field_or_undef(iter.get_nanbox_f64(), b"next".as_ptr(), 4);
    // `it.next = undefined` is PRESENT but non-callable (GetV yields the
    // stored undefined, Call throws), which the value read alone cannot
    // distinguish from absence. The bytes-based keys scan allocates nothing,
    // and an unpatched iterator's keys edge is null, so the hot path pays
    // one null check.
    let own_present = own.to_bits() != crate::value::TAG_UNDEFINED || {
        let obj = crate::value::js_nanbox_get_pointer(iter.get_nanbox_f64()) as *const ObjectHeader;
        let keys = super::object_keys_array(obj);
        !keys.is_null()
            && super::keys_find_slot_by_bytes(
                keys,
                crate::array::js_array_length(keys) as u32,
                b"next",
            )
            .is_some()
    };
    if own_present {
        if !JSValue::from_bits(own.to_bits()).is_pointer() {
            crate::closure::throw_not_callable();
        }
        let own_raw = crate::value::js_nanbox_get_pointer(own);
        // `is_closure_ptr` self-validates the address (handle band + heap
        // floor + magic probe), so a null or mis-boxed value throws rather
        // than faulting.
        if !crate::closure::is_closure_ptr(own_raw as usize) {
            crate::closure::throw_not_callable();
        }
        let method = scope.root_nanbox_f64(own);
        super::js_implicit_this_set(iter.get_nanbox_f64());
        let result = crate::exception::js_call_catching(|| {
            crate::closure::js_native_call_value(method.get_nanbox_f64(), std::ptr::null(), 0)
        });
        super::js_implicit_this_set(previous.get_nanbox_f64());
        return match result {
            Ok(value) => Some(value),
            Err(error) => crate::exception::js_throw(error),
        };
    }
    // An override can only be installed through the prototype OBJECT, and the
    // only way user code obtains that object is `Object.getPrototypeOf(iter)`
    // (or a direct prototype write), both of which materialize the tower. A
    // null tower therefore PROVES no override exists — and building the tower
    // here, as this probe used to, made every builtin `.next()` allocate a
    // "next" key string and run a by-name prototype lookup just to learn
    // nothing was patched.
    if ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) == 0 {
        return None;
    }
    let (slot, canonical): (&crate::object::RealmAtomicI64, *const u8) = match class_id {
        crate::array::ARRAY_ITERATOR_CLASS_ID => (
            &ARRAY_ITERATOR_PROTOTYPE_PTR,
            array_iterator_next_thunk as *const u8,
        ),
        crate::collection_iter_object::MAP_ITERATOR_CLASS_ID => (
            &MAP_ITERATOR_PROTOTYPE_PTR,
            map_iterator_next_thunk as *const u8,
        ),
        crate::collection_iter_object::SET_ITERATOR_CLASS_ID => (
            &SET_ITERATOR_PROTOTYPE_PTR,
            set_iterator_next_thunk as *const u8,
        ),
        crate::string::STRING_ITERATOR_CLASS_ID => (
            &STRING_ITERATOR_PROTOTYPE_PTR,
            string_iterator_next_thunk as *const u8,
        ),
        _ => return None,
    };
    // Building the tower above may collect. Reload its realm-owned root only
    // after the build rather than retaining a pre-build raw address.
    let proto = scope.root_raw_const_ptr(slot.load(Ordering::Acquire) as *const ObjectHeader);
    if proto.with_const_ptr::<ObjectHeader, _>(|proto| proto.is_null()) {
        return None;
    }
    // The null-tower proof above is dead on any program that has allocated
    // one iterator: `attach_iterator_prototype` materializes the tower at the
    // FIRST iterator allocation, so every builtin advance after that reached
    // the by-name lookup below and minted a fresh "next" key string just to
    // learn nothing was patched — one 24-byte string per `for…of` step, on
    // every array / Map / Set / string iterator in the program (~137,000 per
    // 400-character claude-code reply; the second 32-byte site of the
    // 2026-09-06 allocation census).
    //
    // Allocation-free proof of "not overridden": the prototype's OWN `next`
    // slot still holds a closure whose native entry is the canonical thunk,
    // AND no accessor descriptor is recorded for "next" on it. The own read
    // is the certified non-allocating leaf (#9480); the accessor check is
    // the per-key Bloom bit `set_accessor_descriptor` sets BEFORE inserting
    // (#6759 C2), needed because `defineProperty(proto, "next", {get})` on
    // an existing data property leaves the old closure in the slot and puts
    // the accessor in the side table. Anything else — replaced, deleted,
    // accessor, a bound copy — takes the by-name path, unchanged.
    // The closure body is NOT covered by the enclosing `unsafe fn`'s implicit
    // unsafe block, so the call is spelled out.
    if proto.with_const_ptr::<ObjectHeader, _>(|proto| unsafe {
        prototype_next_is_canonical(proto, canonical)
    }) {
        return None;
    }
    let key = scope.root_raw_const_ptr(crate::string::js_string_from_bytes(b"next".as_ptr(), 4));
    let method = proto.with_const_ptr::<ObjectHeader, _>(|proto| {
        key.with_const_ptr::<crate::string::StringHeader, _>(|key| {
            f64::from_bits(super::js_object_get_field_by_name(proto, key).bits())
        })
    });
    let method_ptr =
        crate::value::js_nanbox_get_pointer(method) as *const crate::closure::ClosureHeader;
    if !method_ptr.is_null() && crate::closure::get_valid_func_ptr(method_ptr) == canonical {
        return None;
    }

    let method = scope.root_nanbox_f64(method);
    super::js_implicit_this_set(iter.get_nanbox_f64());
    let result = crate::exception::js_call_catching(|| {
        crate::closure::js_native_call_value(method.get_nanbox_f64(), std::ptr::null(), 0)
    });
    super::js_implicit_this_set(previous.get_nanbox_f64());
    match result {
        Ok(value) => Some(value),
        Err(error) => crate::exception::js_throw(error),
    }
}

/// Does `proto`'s OWN `next` data slot hold a closure whose native entry is
/// `canonical`, with no accessor descriptor recorded for `"next"`? A `true`
/// proves the prototype's `next` is the builtin (a user restoring the
/// original closure object after a patch matches too, by entry rather than
/// by object identity); a `false` proves nothing and the caller must run the
/// full by-name lookup. Reads only: no allocation, no collection point.
#[inline]
unsafe fn prototype_next_is_canonical(proto: *const ObjectHeader, canonical: *const u8) -> bool {
    let own = super::js_object_get_own_field_or_undef(
        crate::value::js_nanbox_pointer(proto as i64),
        b"next".as_ptr(),
        4,
    );
    if !JSValue::from_bits(own.to_bits()).is_pointer() {
        return false;
    }
    let own_ptr = crate::value::js_nanbox_get_pointer(own) as *const crate::closure::ClosureHeader;
    if own_ptr.is_null() || crate::closure::get_valid_func_ptr(own_ptr) != canonical {
        return false;
    }
    !super::descriptor_state::may_have_descriptor_entry(proto as usize, "next", true)
}

/// The prototype-override probe must be free on the path every real program
/// takes: tower materialized (any iterator allocation does that), nothing
/// patched. Before this module's `prototype_next_is_canonical`, that path
/// allocated a "next" key string per call — the second-largest 32-byte
/// allocation site of a claude-code reply (2026-09-06 census, ~137,000 per
/// 400 characters), mislabelled there as a substring copy.
#[cfg(test)]
mod override_probe_allocation_tests {
    use super::*;
    use crate::closure::ClosureHeader;
    use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, TAG_UNDEFINED};

    const PATCHED_SENTINEL: f64 = 4242.0;

    extern "C" fn patched_next_thunk(_closure: *const ClosureHeader) -> f64 {
        PATCHED_SENTINEL
    }

    extern "C" fn accessor_getter_thunk(_closure: *const ClosureHeader) -> f64 {
        f64::from_bits(TAG_UNDEFINED)
    }

    /// One array iterator, rooted; materializes the tower as a side effect.
    unsafe fn rooted_array_iterator(
        scope: &crate::gc::RuntimeHandleScope,
    ) -> crate::gc::RuntimeHandle<'_> {
        let arr = crate::array::js_array_alloc(1);
        crate::array::js_array_push_f64(arr, 1.0);
        let iter = crate::array::array_values_iter(js_nanbox_pointer(arr as i64));
        assert!(
            iterator_prototypes_materialized(),
            "premise: allocating an iterator materializes the tower"
        );
        scope.root_nanbox_f64(iter)
    }

    unsafe fn array_proto() -> *mut ObjectHeader {
        ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader
    }

    unsafe fn set_proto_next(value: f64) {
        let key = crate::string::js_string_from_bytes(b"next".as_ptr(), 4);
        super::super::js_object_set_field_by_name(array_proto(), key, value);
    }

    unsafe fn own_next(proto: *const ObjectHeader) -> f64 {
        super::super::js_object_get_own_field_or_undef(
            js_nanbox_pointer(proto as i64),
            b"next".as_ptr(),
            4,
        )
    }

    /// The counter, and the falsifier for the fix: N probes on an unpatched
    /// iterator with the tower up must bump the arena by ZERO bytes. Before
    /// the fix every probe minted a 24-byte "next" string (32 B rounded), so
    /// this read N × 32 — the number the census reported per grapheme.
    #[test]
    fn probe_on_an_unpatched_iterator_allocates_nothing() {
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let iter_h = rooted_array_iterator(&scope);
            let iter_obj = || js_nanbox_get_pointer(iter_h.get_nanbox_f64()) as *mut ObjectHeader;

            // Warm once: a first call may lazily build anything it builds.
            assert!(call_overridden_iterator_next(
                iter_obj(),
                crate::array::ARRAY_ITERATOR_CLASS_ID
            )
            .is_none());
            const N: usize = 1000;
            let minors_before = crate::gc::instruments::copying_minor_cycles();
            let bytes_before = crate::arena::arena_in_use_bytes();
            for _ in 0..N {
                assert!(
                    call_overridden_iterator_next(
                        iter_obj(),
                        crate::array::ARRAY_ITERATOR_CLASS_ID
                    )
                    .is_none(),
                    "nothing is patched, so the probe must decline"
                );
            }
            let bytes_after = crate::arena::arena_in_use_bytes();
            assert_eq!(
                crate::gc::instruments::copying_minor_cycles(),
                minors_before,
                "a collection inside the window would make a zero delta prove nothing"
            );
            assert_eq!(
                bytes_after.saturating_sub(bytes_before),
                0,
                "the override probe allocated {} bytes over {N} calls on an unpatched \
                 iterator with the tower materialized (it minted a \"next\" key string per call)",
                bytes_after.saturating_sub(bytes_before)
            );
        }
    }

    /// The fast path must not be too eager: a replaced prototype `next` is
    /// still honoured, and restoring the ORIGINAL closure object (what
    /// `test_gap_array_iterator_manual_next.ts` (7) does) returns the probe
    /// to its allocation-free decline — by native entry, not by identity.
    #[test]
    fn probe_honours_a_replaced_prototype_next_and_a_restored_one() {
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let iter_h = rooted_array_iterator(&scope);
            let iter_obj = || js_nanbox_get_pointer(iter_h.get_nanbox_f64()) as *mut ObjectHeader;

            let original = scope.root_nanbox_f64(own_next(array_proto()));
            assert!(
                JSValue::from_bits(original.get_nanbox_f64().to_bits()).is_pointer(),
                "premise: the prototype carries an own `next` closure"
            );

            let patched = crate::closure::js_closure_alloc(patched_next_thunk as *const u8, 0);
            crate::closure::js_register_closure_arity(patched_next_thunk as *const u8, 0);
            let patched_h = scope.root_nanbox_f64(js_nanbox_pointer(patched as i64));
            set_proto_next(patched_h.get_nanbox_f64());
            assert!(
                !prototype_next_is_canonical(array_proto(), array_iterator_next_thunk as *const u8),
                "a replaced prototype `next` must defeat the allocation-free proof"
            );
            assert_eq!(
                call_overridden_iterator_next(iter_obj(), crate::array::ARRAY_ITERATOR_CLASS_ID),
                Some(PATCHED_SENTINEL),
                "the replacement installed on the prototype must be the one called"
            );

            set_proto_next(original.get_nanbox_f64());
            assert!(
                prototype_next_is_canonical(array_proto(), array_iterator_next_thunk as *const u8),
                "restoring the original closure must re-enable the allocation-free proof"
            );
            assert!(
                call_overridden_iterator_next(iter_obj(), crate::array::ARRAY_ITERATOR_CLASS_ID)
                    .is_none(),
                "after the restore the builtin advance is back"
            );
        }
    }

    /// `Object.defineProperty(proto, "next", { get })` records the accessor in
    /// the descriptor side table and leaves the old data slot behind, so the
    /// own-slot read alone would still see the canonical closure. The per-key
    /// accessor bit is what makes the proof decline; without it the getter
    /// would be silently bypassed.
    #[test]
    fn probe_declines_when_an_accessor_next_is_defined_on_the_prototype() {
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let _iter_h = rooted_array_iterator(&scope);
            let original = scope.root_nanbox_f64(own_next(array_proto()));
            assert!(
                prototype_next_is_canonical(array_proto(), array_iterator_next_thunk as *const u8),
                "premise: unpatched prototype passes the proof"
            );

            let getter = crate::closure::js_closure_alloc(accessor_getter_thunk as *const u8, 0);
            crate::closure::js_register_closure_arity(accessor_getter_thunk as *const u8, 0);
            let getter_h = scope.root_nanbox_f64(js_nanbox_pointer(getter as i64));
            let key =
                scope.root_string_ptr(crate::string::js_string_from_bytes(b"next".as_ptr(), 4));
            super::super::js_object_define_accessor(
                js_nanbox_pointer(array_proto() as i64),
                key.with_const_ptr::<crate::StringHeader, _>(|k| {
                    f64::from_bits(JSValue::string_ptr(k as *mut crate::StringHeader).bits())
                }),
                getter_h.get_nanbox_f64(),
                f64::from_bits(TAG_UNDEFINED),
            );
            assert!(
                !prototype_next_is_canonical(array_proto(), array_iterator_next_thunk as *const u8),
                "an accessor `next` on the prototype must defeat the allocation-free proof \
                 even though the data slot may still hold the canonical closure"
            );

            // Delete the accessor and put the data property back. The Bloom
            // bit is sticky (zeroed only at meta creation), so the PROOF stays
            // declined on this prototype for good — conservative: the by-name
            // path runs, exactly as before the fix. Only the semantics are
            // pinned here: the builtin advance is back.
            key.with_const_ptr::<crate::StringHeader, _>(|k| {
                super::super::js_object_delete_field(array_proto(), k);
            });
            set_proto_next(original.get_nanbox_f64());
            let iter_obj = js_nanbox_get_pointer(_iter_h.get_nanbox_f64()) as *mut ObjectHeader;
            assert!(
                call_overridden_iterator_next(iter_obj, crate::array::ARRAY_ITERATOR_CLASS_ID)
                    .is_none(),
                "after delete + restore the builtin advance must be back"
            );
        }
    }
}
