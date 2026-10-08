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
//! Each `next` thunk checks its receiver's iterator brand, then invokes the
//! existing built-in advance algorithm. The family-specific checks also make
//! the function bodies distinct under release identical-code folding, so the
//! canonical code-pointer proofs below retain their meaning.

use super::{js_object_alloc, set_builtin_property_attrs, ObjectHeader, PropertyAttrs};
use crate::value::JSValue;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

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

/// Sticky per-family siblings of `PERRY_ARRAY_ITERATION_NOT_PRISTINE`: the
/// Map / Set / String iterator PROTOTYPE object escaped to user code, so
/// `%MapIteratorPrototype%.next` (etc.) may be patched from here on.
///
/// The array byte is exported because GENERATED code reads it (the `for…of`
/// index loop, #10086's destructuring arm). These three are read only from
/// Rust — `array_from_spread_value`'s Map / Set / string element-copy arms —
/// so a plain `AtomicBool` is enough. `AtomicBool` holds no heap pointer, so
/// none of them is a GC root (`scripts/gc_runtime_root_holders.py`).
static MAP_ITERATION_NOT_PRISTINE: AtomicBool = AtomicBool::new(false);
static SET_ITERATION_NOT_PRISTINE: AtomicBool = AtomicBool::new(false);
static STRING_ITERATION_NOT_PRISTINE: AtomicBool = AtomicBool::new(false);

/// Has `%MapIteratorPrototype%` escaped to user code? See
/// [`note_iterator_prototype_exposed`].
#[inline]
pub(crate) fn map_iteration_not_pristine() -> bool {
    MAP_ITERATION_NOT_PRISTINE.load(Ordering::Acquire)
}

/// Has `%SetIteratorPrototype%` escaped to user code? See
/// [`note_iterator_prototype_exposed`].
#[inline]
pub(crate) fn set_iteration_not_pristine() -> bool {
    SET_ITERATION_NOT_PRISTINE.load(Ordering::Acquire)
}

/// Has `%StringIteratorPrototype%` escaped to user code? See
/// [`note_iterator_prototype_exposed`].
#[inline]
pub(crate) fn string_iteration_not_pristine() -> bool {
    STRING_ITERATION_NOT_PRISTINE.load(Ordering::Acquire)
}

/// #10086: a built-in iterator prototype object is about to be handed to user
/// code, so `%ArrayIteratorPrototype%.next` (or its Map / Set / String
/// sibling) may be replaced at any point after this. Publish that.
///
/// #9846 widened this from the array family alone. The array byte covers the
/// two GENERATED fast arms; the three booleans cover the RUNTIME element-copy
/// arms in `array_from_spread_value`, which are the same kind of hole —
/// `[...new Set([1, 2])]` memcpy'd the Set's backing and `[..."ab"]` cut the
/// string into chars, so a patched `%SetIteratorPrototype%.next` /
/// `%StringIteratorPrototype%.next` never ran.
///
/// A replaced `next` is detected per `.next()` call by
/// [`prototype_next_is_canonical`] — which a non-iterator fast arm (the
/// `for…of` index loop, #10086's array-destructuring arm) never reaches,
/// because it never calls `.next()`. There is no cheap sticky signal for the
/// write itself: the object is an ordinary `ObjectHeader`, so a precise hook
/// would have to cover every mutation funnel (assignment, computed assignment,
/// `defineProperty`, `delete`, `Object.assign`) and MISSING one fails silently,
/// in the direction of a wrong answer.
///
/// Escape is the choke point instead. User code cannot patch an object it
/// cannot name, and in Perry the only way to name this one is
/// `Object.getPrototypeOf` / `Reflect.getPrototypeOf` (both
/// `js_object_get_prototype_of`; `iter.__proto__` answers `undefined` here).
/// So the flag is set when the object escapes, patched or not. The cost lands
/// only on programs that introspect an array iterator — and those are exactly
/// the programs about to patch one.
pub(crate) fn note_iterator_prototype_exposed(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return;
    }
    let addr = jv.as_pointer::<ObjectHeader>() as i64;
    if addr == 0 {
        return;
    }
    if ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) == addr {
        crate::array::note_array_iteration_not_pristine();
        return;
    }
    if MAP_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) == addr {
        MAP_ITERATION_NOT_PRISTINE.store(true, Ordering::Release);
        return;
    }
    if SET_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) == addr {
        SET_ITERATION_NOT_PRISTINE.store(true, Ordering::Release);
        return;
    }
    if STRING_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) == addr {
        STRING_ITERATION_NOT_PRISTINE.store(true, Ordering::Release);
        return;
    }
    // `%IteratorPrototype%` itself is the parent of all four families, and a
    // patch there is inherited by every one of them. Reaching it needs a
    // second `getPrototypeOf` hop off a family prototype, so this arm is
    // strictly rarer than the four above — mark them all.
    if ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) == addr {
        crate::array::note_array_iteration_not_pristine();
        MAP_ITERATION_NOT_PRISTINE.store(true, Ordering::Release);
        SET_ITERATION_NOT_PRISTINE.store(true, Ordering::Release);
        STRING_ITERATION_NOT_PRISTINE.store(true, Ordering::Release);
    }
}

/// Resolve and validate the `this` object shared by the family
/// prototype thunks. Keeping the raw-address probe here gives the
/// family-specific brand checks one audited path.
unsafe fn this_iterator_object(this: crate::closure::JsThis) -> Option<*mut ObjectHeader> {
    let this = this.as_f64();
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

// --- `next` thunks: the receiver's class is the iterator brand. ---
//
// Call the built-in dispatchers after validation: a saved canonical next must
// skip own-next overrides, including overrides that delegate to the saved next.
// Do not replace these checks with a receiver-wide dispatcher: that both accepts
// foreign brands and lets release code folding collapse canonical identities.

extern "C" fn array_iterator_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    unsafe {
        let Some(obj) = this_iterator_object(this) else {
            return brand_type_error("next");
        };
        // Buffer/typed-array values use the ECMAScript Array Iterator brand.
        match (*obj).class_id {
            crate::array::ARRAY_ITERATOR_CLASS_ID => {
                crate::array::dispatch_array_iterator_method_builtin(obj, "next")
            }
            crate::buffer::BUFFER_ITERATOR_CLASS_ID => {
                crate::buffer::dispatch_buffer_iterator_method_builtin(obj, "next")
            }
            _ => brand_type_error("next"),
        }
    }
}
extern "C" fn map_iterator_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    unsafe {
        let Some(obj) = this_iterator_object(this) else {
            return brand_type_error("next");
        };
        if (*obj).class_id != crate::collection_iter_object::MAP_ITERATOR_CLASS_ID {
            return brand_type_error("next");
        }
        crate::collection_iter_object::dispatch_map_iterator_method_builtin(obj, "next")
    }
}
extern "C" fn set_iterator_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    unsafe {
        let Some(obj) = this_iterator_object(this) else {
            return brand_type_error("next");
        };
        if (*obj).class_id != crate::collection_iter_object::SET_ITERATOR_CLASS_ID {
            return brand_type_error("next");
        }
        crate::collection_iter_object::dispatch_set_iterator_method_builtin(obj, "next")
    }
}
extern "C" fn string_iterator_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    unsafe {
        let Some(obj) = this_iterator_object(this) else {
            return brand_type_error("next");
        };
        if (*obj).class_id != crate::string::STRING_ITERATOR_CLASS_ID {
            return brand_type_error("next");
        }
        crate::string::dispatch_string_iterator_method_builtin(obj, "next")
    }
}
extern "C" fn regexp_string_iterator_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    unsafe {
        let Some(obj) = this_iterator_object(this) else {
            return brand_type_error("next");
        };
        if (*obj).class_id != crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID {
            return brand_type_error("next");
        }
        crate::regex::hooked_iterator_method_builtin(obj, "next")
            .unwrap_or_else(|| brand_type_error("next"))
    }
}

/// `%Iterator Helper Prototype%.next` has a helper-specific brand check. Use
/// the intrinsic algorithm directly rather than the general method dispatcher:
/// a saved/bound canonical method must not re-enter a later `next` override.
extern "C" fn iterator_helper_next_thunk(
    _c: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    unsafe {
        let Some(obj) = this_iterator_object(this) else {
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
    this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    this.as_f64()
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
/// which resolves its receiver from the call-site `this` rather than the
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
    // `Iterator.prototype[Symbol.toStringTag]` is "Iterator": an iterator
    // whose own prototype carries no tag (node:sqlite's
    // StatementSyncIterator) prints as `[object Iterator]`.
    set_to_string_tag(shared, "Iterator");

    let array_proto = build_family_proto(
        crate::fn_info!(array_iterator_next_thunk, 1; with_declared(0), with_flags(crate::closure::FN_BUILTIN | crate::codegen_abi::FN_PERMANENT_IMAGE)),
        "Array Iterator",
        shared,
    );
    let map_proto = build_family_proto(
        crate::fn_info!(map_iterator_next_thunk, 1; with_declared(0), with_flags(crate::closure::FN_BUILTIN | crate::codegen_abi::FN_PERMANENT_IMAGE)),
        "Map Iterator",
        shared,
    );
    let set_proto = build_family_proto(
        crate::fn_info!(set_iterator_next_thunk, 1; with_declared(0), with_flags(crate::closure::FN_BUILTIN | crate::codegen_abi::FN_PERMANENT_IMAGE)),
        "Set Iterator",
        shared,
    );
    let string_proto = build_family_proto(
        crate::fn_info!(string_iterator_next_thunk, 1; with_declared(0), with_flags(crate::closure::FN_BUILTIN | crate::codegen_abi::FN_PERMANENT_IMAGE)),
        "String Iterator",
        shared,
    );
    let regexp_string_proto = build_family_proto(
        crate::fn_info!(regexp_string_iterator_next_thunk, 1; with_declared(0), with_flags(crate::closure::FN_BUILTIN | crate::codegen_abi::FN_PERMANENT_IMAGE)),
        "RegExp String Iterator",
        shared,
    );
    let iterator_helper_proto = build_family_proto(
        crate::fn_info!(iterator_helper_next_thunk, 1; with_declared(0), with_flags(crate::closure::FN_BUILTIN)),
        "Iterator Helper",
        shared,
    );

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
    let closure = crate::closure::js_closure_alloc(
        crate::fn_info!(iterator_proto_symbol_iterator_thunk, 1; with_declared(0)),
        0,
    );
    if closure.is_null() {
        return;
    }
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
    next_info: *const crate::closure::JsFunctionInfo,
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
    super::global_this::install_proto_method(proto, "next", next_info, 0);
    set_to_string_tag(proto, tag);
    chain_to(proto, shared);
    // Populate the existing ConstFn representation after descriptor/symbol
    // installation; no iterator-specific flag or table is introduced.
    unsafe {
        super::shapes::learn_object_constfn_lanes(proto, |slot, _| slot == 0);
    }
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
        crate::array::ARRAY_ITERATOR_CLASS_ID | crate::buffer::BUFFER_ITERATOR_CLASS_ID => {
            &ARRAY_ITERATOR_PROTOTYPE_PTR
        }
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
        crate::array::ARRAY_ITERATOR_CLASS_ID | crate::buffer::BUFFER_ITERATOR_CLASS_ID => {
            &ARRAY_ITERATOR_PROTOTYPE_PTR
        }
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
        let keys_view = super::object_keys(obj);
        let keys = keys_view.arr();
        !keys.is_null()
            && super::keys_find_slot_by_bytes(keys, keys_view.count() as u32, b"next").is_some()
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
        let result = crate::exception::js_call_catching(|| {
            crate::closure::native_call_value_this(
                method.get_nanbox_f64(),
                crate::closure::JsThis::from_f64(iter.get_nanbox_f64()),
                std::ptr::null(),
                0,
            )
        });
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
    let result = crate::exception::js_call_catching(|| {
        crate::closure::native_call_value_this(
            method.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(iter.get_nanbox_f64()),
            std::ptr::null(),
            0,
        )
    });
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

    extern "C" fn patched_next_thunk(
        _closure: *const ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        PATCHED_SENTINEL
    }

    extern "C" fn accessor_getter_thunk(
        _closure: *const ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
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

            let patched = crate::closure::js_closure_alloc(
                crate::fn_info!(patched_next_thunk, 0; with_declared(0)),
                0,
            );
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

            let getter = crate::closure::js_closure_alloc(
                crate::fn_info!(accessor_getter_thunk, 0; with_declared(0)),
                0,
            );
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

/// Prove a built-in advance using the receiver and prototype shapes.
/// Any own property conservatively leaves the protocol intact. ConstFn is
/// revoked by ordinary stores; accessors and reparenting also change the shape.
pub(crate) unsafe fn iterator_step_is_builtin(obj: *const ObjectHeader) -> bool {
    // The existing realm intrinsic roots identify the family; all mutation
    // facts come from the current shapes, never an exposure/pristine latch.
    let (canonical, family_root) = match (*obj).class_id {
        crate::array::ARRAY_ITERATOR_CLASS_ID | crate::buffer::BUFFER_ITERATOR_CLASS_ID => (
            array_iterator_next_thunk as *const u8,
            &ARRAY_ITERATOR_PROTOTYPE_PTR,
        ),
        crate::collection_iter_object::MAP_ITERATOR_CLASS_ID => (
            map_iterator_next_thunk as *const u8,
            &MAP_ITERATOR_PROTOTYPE_PTR,
        ),
        crate::collection_iter_object::SET_ITERATOR_CLASS_ID => (
            set_iterator_next_thunk as *const u8,
            &SET_ITERATOR_PROTOTYPE_PTR,
        ),
        crate::string::STRING_ITERATOR_CLASS_ID => (
            string_iterator_next_thunk as *const u8,
            &STRING_ITERATOR_PROTOTYPE_PTR,
        ),
        _ => return false,
    };
    let Some(own) = super::shapes::object_shape_record(obj) else {
        return false;
    };
    if own.logical_key_count() != 0 || own.summary() != 0 {
        return false;
    }
    let proto_bits = super::shapes::object_prototype_word(obj);
    if !JSValue::from_bits(proto_bits).is_pointer() {
        return false;
    }
    let proto =
        crate::value::js_nanbox_get_pointer(f64::from_bits(proto_bits)) as *const ObjectHeader;
    if proto as i64 != family_root.load(Ordering::Acquire) {
        return false;
    }
    if !crate::value::addr_class::try_read_gc_header(proto as usize)
        .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
    {
        return false;
    }
    let Some(family) = super::shapes::object_shape_record(proto) else {
        return false;
    };
    if family.logical_key_count() != 2
        || family.summary() & super::key_attrs::SUMMARY_ACCESSOR != 0
        || super::field_rep::slot_rep(super::field_rep::identity_with_special(family.rep()), 0)
            != super::field_rep::REP_SPECIAL
    {
        return false;
    }
    // The intrinsic is born with next in ConstFn slot 0 and a symbol in
    // slot 1. Ordinary writes/delete revoke that lane; they cannot learn a
    // different named method as ConstFn. No name lookup or pristine latch.
    let keys = family.keys() as *const crate::array::ArrayHeader;
    let slots = crate::array::array_elements_ptr(keys);
    if !JSValue::from_bits(*slots.add(1)).is_pointer() {
        return false;
    }
    let Some(info) = family.constfn_info(0) else {
        return false;
    };
    if (*(info as *const crate::closure::JsFunctionInfo)).code != canonical {
        return false;
    }
    let parent_bits = super::shapes::object_prototype_word(proto);
    if !JSValue::from_bits(parent_bits).is_pointer() {
        return false;
    }
    let parent =
        crate::value::js_nanbox_get_pointer(f64::from_bits(parent_bits)) as *const ObjectHeader;
    if !crate::value::addr_class::try_read_gc_header(parent as usize)
        .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_OBJECT)
    {
        return false;
    }
    let Some(shared) = super::shapes::object_shape_record(parent) else {
        return false;
    };
    // The shared parent may carry symbols, but no named protocol methods.
    let symbol_keys_only = if shared.logical_key_count() == 0 {
        true
    } else {
        let keys = shared.keys() as *const crate::array::ArrayHeader;
        let slots = crate::array::array_elements_ptr(keys);
        (0..shared.logical_key_count())
            .all(|i| JSValue::from_bits(*slots.add(i as usize)).is_pointer())
    };
    shared.summary() & super::key_attrs::SUMMARY_ACCESSOR == 0
        && symbol_keys_only
        && super::shapes::object_prototype_word(parent) == 0
}

#[cfg(test)]
mod native_step_tests {
    use super::*;
    use crate::value::js_nanbox_pointer;

    unsafe fn fixture(class_id: u32) -> f64 {
        // A fresh tower isolates semantic tests from other tests that revoke
        // ConstFn on the realm singleton and later restore only its JS value.
        let _stable = crate::gc::GcSuppressScope::new();
        build_iterator_prototypes();
        let iter = match class_id {
            crate::array::ARRAY_ITERATOR_CLASS_ID => {
                let a = crate::array::js_array_alloc(256);
                for i in 0..256 {
                    crate::array::js_array_push_f64(a, i as f64);
                }
                crate::array::array_values_iter(js_nanbox_pointer(a as i64))
            }
            crate::buffer::BUFFER_ITERATOR_CLASS_ID => {
                let buf = crate::buffer::js_buffer_alloc(256, 0);
                for i in 0..256 {
                    crate::buffer::js_buffer_set(buf, i, i);
                }
                crate::buffer::js_buffer_values(js_nanbox_pointer(buf as i64))
            }
            crate::collection_iter_object::MAP_ITERATOR_CLASS_ID => {
                let m = crate::map::js_map_alloc(256);
                for i in 0..256 {
                    crate::map::js_map_set(m, i as f64, i as f64);
                }
                js_nanbox_pointer(crate::collection_iter_object::js_map_values_iter_obj(m))
            }
            crate::collection_iter_object::SET_ITERATOR_CLASS_ID => {
                let s = crate::set::js_set_alloc(256);
                for i in 0..256 {
                    crate::set::js_set_add(s, i as f64);
                }
                js_nanbox_pointer(crate::collection_iter_object::js_set_values_iter_obj(s))
            }
            _ => {
                let bytes = vec![b'a'; 256];
                let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                crate::string::string_values_iter(s)
            }
        };
        iter
    }

    fn kinds() -> [u32; 5] {
        [
            crate::array::ARRAY_ITERATOR_CLASS_ID,
            crate::buffer::BUFFER_ITERATOR_CLASS_ID,
            crate::collection_iter_object::MAP_ITERATOR_CLASS_ID,
            crate::collection_iter_object::SET_ITERATOR_CLASS_ID,
            crate::string::STRING_ITERATOR_CLASS_ID,
        ]
    }

    #[test]
    fn iterator_next_brands_and_release_code_identities() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            let families = [
                (
                    crate::array::ARRAY_ITERATOR_CLASS_ID,
                    crate::fn_info!(array_iterator_next_thunk, 1),
                ),
                (
                    crate::collection_iter_object::MAP_ITERATOR_CLASS_ID,
                    crate::fn_info!(map_iterator_next_thunk, 1),
                ),
                (
                    crate::collection_iter_object::SET_ITERATOR_CLASS_ID,
                    crate::fn_info!(set_iterator_next_thunk, 1),
                ),
                (
                    crate::string::STRING_ITERATOR_CLASS_ID,
                    crate::fn_info!(string_iterator_next_thunk, 1),
                ),
                (
                    crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID,
                    crate::fn_info!(regexp_string_iterator_next_thunk, 1),
                ),
                (
                    crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID,
                    crate::fn_info!(iterator_helper_next_thunk, 1),
                ),
            ];
            let closures: Vec<_> = families
                .iter()
                .map(|(_, info)| crate::closure::js_closure_alloc(*info, 0))
                .collect();
            // Read the actual allocated closures' code words. Run this test in
            // release: source-level function names cannot prove linker identity.
            for (i, closure) in closures.iter().enumerate() {
                for other in &closures[..i] {
                    assert_ne!(
                        (**closure).code(),
                        (**other).code(),
                        "different iterator brands must retain different code pointers"
                    );
                }
            }
            for ((brand, _), closure) in families.iter().zip(&closures) {
                for receiver_brand in kinds() {
                    let receiver = fixture(receiver_brand);
                    let result = crate::exception::js_call_catching(|| {
                        crate::closure::js_closure_call1(
                            *closure,
                            crate::closure::JsThis::from_f64(receiver),
                            f64::from_bits(crate::value::TAG_UNDEFINED),
                        )
                    });
                    let matches = brand == &receiver_brand
                        || (*brand == crate::array::ARRAY_ITERATOR_CLASS_ID
                            && receiver_brand == crate::buffer::BUFFER_ITERATOR_CLASS_ID);
                    assert_eq!(
                        result.is_ok(),
                        matches,
                        "next brand {brand:x}, receiver {receiver_brand:x}"
                    );
                    if let Err(error) = result {
                        let name = super::super::js_object_get_field_by_name_f64(
                            crate::value::js_nanbox_get_pointer(error) as *const ObjectHeader,
                            crate::string::intern_ascii_literal(b"name"),
                        );
                        assert_eq!(
                            crate::string::string_as_str(crate::value::js_jsvalue_to_string(name)),
                            "TypeError"
                        );
                    }
                }
                for receiver in [
                    js_nanbox_pointer(js_object_alloc(0, 0) as i64),
                    f64::from_bits(crate::value::TAG_NULL),
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                    1.0,
                ] {
                    assert!(
                        crate::exception::js_call_catching(|| {
                            crate::closure::js_closure_call1(
                                *closure,
                                crate::closure::JsThis::from_f64(receiver),
                                0.0,
                            )
                        })
                        .is_err(),
                        "brand {brand:x} must reject non-iterators"
                    );
                }
            }
            build_iterator_prototypes();
        }
    }

    #[test]
    fn native_step_foreign_next_never_proves_canonical_or_advances() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            let brands = [
                crate::array::ARRAY_ITERATOR_CLASS_ID,
                crate::collection_iter_object::MAP_ITERATOR_CLASS_ID,
                crate::collection_iter_object::SET_ITERATOR_CLASS_ID,
                crate::string::STRING_ITERATOR_CLASS_ID,
            ];
            for target in brands {
                for source in brands {
                    if target == source {
                        continue;
                    }
                    let foreign_iter = fixture(source);
                    let foreign = crate::array::js_iterator_next_method(foreign_iter);
                    let foreign_info =
                        crate::closure::closure_info(crate::value::js_nanbox_get_pointer(foreign)
                            as *const crate::closure::ClosureHeader)
                        .unwrap();
                    let iter = fixture(target);
                    let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
                    let proto = crate::value::js_nanbox_get_pointer(f64::from_bits(
                        super::super::shapes::object_prototype_word(obj),
                    )) as *mut ObjectHeader;
                    let original = crate::array::js_iterator_next_method(iter);
                    let canonical = (*(crate::value::js_nanbox_get_pointer(original)
                        as *const crate::closure::ClosureHeader))
                        .code();
                    assert!(iterator_step_is_builtin(obj), "pristine premise");
                    super::super::global_this::install_proto_method(proto, "next", foreign_info, 0);
                    // Preserve the canonical-shaped ConstFn premise: the body
                    // proof itself must distinguish the foreign function.
                    super::super::shapes::learn_object_constfn_lanes(proto, |slot, _| slot == 0);
                    assert!(
                        !prototype_next_is_canonical(proto, canonical),
                        "foreign next must defeat the ordinary advance proof"
                    );
                    assert!(
                        !iterator_step_is_builtin(obj),
                        "foreign next must defeat native-step admission"
                    );
                    let next = crate::array::js_iterator_next_method(iter);
                    let mut value = 0.0;
                    assert!(
                        crate::exception::js_call_catching(|| {
                            crate::array::js_iterator_step(iter, next, &mut value) as f64
                        })
                        .is_err(),
                        "foreign next must throw before advancing"
                    );
                    assert_eq!(
                        crate::array::js_iterator_step(iter, original, &mut value),
                        0
                    );
                    if target != crate::string::STRING_ITERATOR_CLASS_ID {
                        assert_eq!(value, 0.0, "failed foreign next must not consume a value");
                    }
                }
            }
            build_iterator_prototypes();
        }
    }

    #[test]
    fn native_step_consumes_all_families_without_result_allocation() {
        unsafe {
            for kind in kinds() {
                let _stable = crate::gc::GcSuppressScope::new();
                let iter = fixture(kind);
                let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
                assert!(
                    iterator_step_is_builtin(obj),
                    "pristine shape must be admitted: {kind:x}, own={:?}, family={:?}",
                    super::super::shapes::object_shape_descriptor(obj),
                    super::super::shapes::object_shape_descriptor(
                        crate::value::js_nanbox_get_pointer(f64::from_bits(
                            super::super::shapes::object_prototype_word(obj)
                        )) as *const ObjectHeader
                    )
                );
                let next = crate::array::js_iterator_next_method(iter);
                crate::string::test_evict_interned(b"next");
                assert!(
                    iterator_step_is_builtin(obj),
                    "shape facts survive intern-cache eviction"
                );
                let bytes = crate::arena::arena_in_use_bytes();
                let minors = crate::gc::instruments::copying_minor_cycles();
                let mut value = 0.0;
                for i in 0..256 {
                    assert_eq!(crate::array::js_iterator_step(iter, next, &mut value), 0);
                    if kind != crate::string::STRING_ITERATOR_CLASS_ID {
                        assert_eq!(value, i as f64);
                    } else {
                        assert!(JSValue::from_bits(value.to_bits()).is_string());
                    }
                }
                assert_eq!(crate::array::js_iterator_step(iter, next, &mut value), 1);
                assert_eq!(crate::array::js_iterator_step(iter, next, &mut value), 1);
                assert_eq!(value.to_bits(), crate::value::TAG_UNDEFINED);
                assert_eq!(crate::gc::instruments::copying_minor_cycles(), minors);
                assert_eq!(
                    crate::arena::arena_in_use_bytes(),
                    bytes,
                    "no result or key allocation on built-in steps"
                );
            }
        }
    }

    #[test]
    fn native_step_manual_results_stay_fresh_and_collection_delete_rebases() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            for kind in kinds() {
                let iter = fixture(kind);
                let first = crate::collection_iter_object::js_for_of_next(iter);
                let second = crate::collection_iter_object::js_for_of_next(iter);
                assert_ne!(
                    first.to_bits(),
                    second.to_bits(),
                    "manual result identity must stay fresh"
                );
                if kind != crate::string::STRING_ITERATOR_CLASS_ID {
                    let obj = crate::value::js_nanbox_get_pointer(first) as *const ObjectHeader;
                    assert_eq!(
                        f64::from_bits(super::super::js_object_get_field(obj, 0).bits()),
                        0.0
                    );
                }
            }
            let iter = fixture(crate::collection_iter_object::MAP_ITERATOR_CLASS_ID);
            let obj = crate::value::js_nanbox_get_pointer(iter) as *const ObjectHeader;
            let map = crate::value::js_nanbox_get_pointer(f64::from_bits(
                super::super::js_object_get_field(obj, 0).bits(),
            )) as *mut crate::map::MapHeader;
            let next = crate::array::js_iterator_next_method(iter);
            let mut value = 0.0;
            assert_eq!(crate::array::js_iterator_step(iter, next, &mut value), 0);
            crate::map::js_map_delete(map, 0.0);
            assert_eq!(crate::array::js_iterator_step(iter, next, &mut value), 0);
            assert_eq!(value, 1.0, "delete below cursor must not skip");
        }
    }

    #[test]
    fn native_step_each_shape_level_revokes_on_next_and_return() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            for kind in kinds() {
                for level in 0..3 {
                    for name in ["next", "return"] {
                        let iter = fixture(kind);
                        let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
                        assert!(iterator_step_is_builtin(obj), "guard premise");
                        let mut target = obj;
                        for _ in 0..level {
                            target = crate::value::js_nanbox_get_pointer(f64::from_bits(
                                super::super::shapes::object_prototype_word(target),
                            )) as *mut ObjectHeader;
                        }
                        let key =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        super::super::js_object_set_field_by_name(
                            target,
                            key,
                            f64::from_bits(crate::value::TAG_UNDEFINED),
                        );
                        assert!(
                            !iterator_step_is_builtin(obj),
                            "{kind:x} level {level} {name}"
                        );
                    }
                }
                let iter = fixture(kind);
                let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
                assert!(iterator_step_is_builtin(obj));
                let other = js_object_alloc(0, 0);
                chain_to(obj, other);
                assert!(!iterator_step_is_builtin(obj), "reparenting revokes");
            }
            let ordinary = js_object_alloc(0, 0);
            assert!(
                !iterator_step_is_builtin(ordinary),
                "non-built-in must never be admitted"
            );
        }
    }
    #[test]
    fn native_step_intrinsic_identity_and_body_are_independent_proofs() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            for (name, wrong_body, publish_fake, symbol_slot, extra_return) in [
                ("next", false, false, true, false),
                ("next", true, true, true, false),
                ("next", false, true, false, false),
                ("next", false, true, true, true),
            ] {
                let iter = fixture(crate::array::ARRAY_ITERATOR_CLASS_ID);
                let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
                let old = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
                let shared = crate::value::js_nanbox_get_pointer(f64::from_bits(
                    super::super::shapes::object_prototype_word(old),
                )) as *mut ObjectHeader;
                let info = if wrong_body {
                    crate::fn_info!(string_iterator_next_thunk, 1; with_flags(crate::closure::FN_BUILTIN | crate::codegen_abi::FN_PERMANENT_IMAGE))
                } else {
                    super::super::shapes::object_shape_record(old)
                        .unwrap()
                        .constfn_info(0)
                        .unwrap() as *const crate::closure::JsFunctionInfo
                };
                let fake = js_object_alloc(0, 0);
                super::super::global_this::install_proto_method(fake, name, info, 0);
                if symbol_slot {
                    set_to_string_tag(fake, "test");
                } else {
                    super::super::global_this::install_proto_method(
                        fake,
                        "return",
                        crate::fn_info!(user_next, 0),
                        0,
                    );
                }
                if extra_return {
                    super::super::global_this::install_proto_method(
                        fake,
                        "return",
                        crate::fn_info!(user_next, 0),
                        0,
                    );
                }
                chain_to(fake, shared);
                super::super::shapes::learn_object_constfn_lanes(fake, |slot, _| slot == 0);
                if publish_fake {
                    ARRAY_ITERATOR_PROTOTYPE_PTR.store(fake as i64, Ordering::Release);
                }
                chain_to(obj, fake);
                assert!(!iterator_step_is_builtin(obj),
                    "identity and body each require their own proof: {name} {wrong_body} {publish_fake}");
            }
            let iter = fixture(crate::array::ARRAY_ITERATOR_CLASS_ID);
            let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
            let family = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
            let original = crate::array::js_iterator_next_method(iter);
            let next_key = crate::string::intern_ascii_literal(b"next");
            super::super::js_object_delete_field(family, next_key);
            let return_key = crate::string::intern_ascii_literal(b"return");
            super::super::js_object_set_field_by_name(family, return_key, original);
            assert!(
                !iterator_step_is_builtin(obj),
                "delete next then install it as return must revoke the native lane"
            );
            let iter = fixture(crate::array::ARRAY_ITERATOR_CLASS_ID);
            let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
            let family = ARRAY_ITERATOR_PROTOTYPE_PTR.load(Ordering::Acquire) as *mut ObjectHeader;
            let saved = crate::array::js_iterator_next_method(iter);
            let tag = crate::symbol::well_known_symbol("toStringTag");
            crate::symbol::js_object_delete_symbol_property(
                js_nanbox_pointer(family as i64),
                f64::from_bits(JSValue::pointer(tag as *const u8).bits()),
            );
            let key = crate::string::intern_ascii_literal(b"return");
            super::super::js_object_set_field_by_name(family, key, saved);
            assert!(
                !iterator_step_is_builtin(obj),
                "replacing the symbol slot with a named return requires protocol"
            );
            let iter = fixture(crate::array::ARRAY_ITERATOR_CLASS_ID);
            let ordinary = js_object_alloc(0, 0);
            let family = crate::value::js_nanbox_get_pointer(f64::from_bits(
                super::super::shapes::object_prototype_word(crate::value::js_nanbox_get_pointer(
                    iter,
                )
                    as *const ObjectHeader),
            )) as *mut ObjectHeader;
            chain_to(ordinary, family);
            assert!(
                !iterator_step_is_builtin(ordinary),
                "a shaped ordinary object is not an iterator"
            );
            build_iterator_prototypes();
        }
    }

    extern "C" fn hole_value_getter(
        _c: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        crate::exception::js_throw(1234.0)
    }
    extern "C" fn hole_next(
        _c: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        let scope = crate::gc::RuntimeHandleScope::new();
        let result = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
        let done = crate::string::intern_ascii_literal(b"done");
        result.with_mut_ptr(|r| {
            super::super::js_object_set_field_by_name(
                r,
                done,
                f64::from_bits(JSValue::bool(false).bits()),
            )
        });
        let getter = scope.root_nanbox_f64(js_nanbox_pointer(crate::closure::js_closure_alloc(
            crate::fn_info!(hole_value_getter, 0),
            0,
        ) as i64));
        let value = crate::string::intern_ascii_literal(b"value");
        super::super::js_object_define_accessor(
            result.with_mut_ptr::<ObjectHeader, _>(|r| js_nanbox_pointer(r as i64)),
            f64::from_bits(JSValue::string_ptr(value as *mut _).bits()),
            getter.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        result.with_mut_ptr::<ObjectHeader, _>(|r| js_nanbox_pointer(r as i64))
    }
    #[test]
    fn native_step_hole_does_not_read_value_but_value_step_does() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            let iter = fixture(crate::array::ARRAY_ITERATOR_CLASS_ID);
            let next = js_nanbox_pointer(crate::closure::js_closure_alloc(
                crate::fn_info!(hole_next, 0),
                0,
            ) as i64);
            let skipped = crate::exception::js_call_catching(|| {
                crate::array::js_iterator_step(iter, next, std::ptr::null_mut()) as f64
            })
            .expect("an elision must not invoke the result value getter");
            assert_eq!(skipped, 0.0);
            let mut value = 0.0;
            let error = crate::exception::js_call_catching(|| {
                crate::array::js_iterator_step(iter, next, &mut value) as f64
            });
            assert_eq!(
                error,
                Err(1234.0),
                "IteratorStepValue must still read the getter"
            );
            build_iterator_prototypes();
        }
    }

    #[test]
    fn native_step_rest_keeps_the_record_and_completed_rest_never_steps() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            for kind in kinds() {
                let iter = fixture(kind);
                let next = crate::array::js_iterator_next_method(iter);
                let key = crate::string::intern_ascii_literal(b"next");
                let other = crate::closure::js_closure_alloc(crate::fn_info!(user_next, 0), 0);
                super::super::js_object_set_field_by_name(
                    crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader,
                    key,
                    js_nanbox_pointer(other as i64),
                );
                let rest = crate::exception::js_call_catching(|| {
                    crate::array::js_iterator_step_rest_to_array(
                        iter,
                        next,
                        f64::from_bits(crate::value::TAG_FALSE),
                    )
                })
                .expect("rest must use the captured callable next method");
                let arr =
                    crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader;
                assert_eq!(
                    crate::array::js_array_length(arr),
                    256,
                    "rest must call its captured next despite a later own replacement"
                );
                let thrower =
                    crate::closure::js_closure_alloc(crate::fn_info!(hole_value_getter, 0), 0);
                let result = crate::exception::js_call_catching(|| {
                    crate::array::js_iterator_step_rest_to_array(
                        iter,
                        js_nanbox_pointer(thrower as i64),
                        f64::from_bits(crate::value::TAG_TRUE),
                    )
                });
                let empty = result.expect("completed rest must not call next");
                assert_eq!(
                    crate::array::js_array_length(crate::value::js_nanbox_get_pointer(empty)
                        as *const crate::array::ArrayHeader),
                    0
                );
            }
            build_iterator_prototypes();
        }
    }

    #[test]
    fn native_step_noncallable_next_throws_before_result_validation() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            let iter = fixture(crate::array::ARRAY_ITERATOR_CLASS_ID);
            for next in [
                f64::from_bits(crate::value::TAG_UNDEFINED),
                f64::from_bits(crate::value::TAG_NULL),
                17.0,
            ] {
                let mut value = 0.0;
                let error = crate::exception::js_call_catching(|| {
                    crate::array::js_iterator_step(iter, next, &mut value) as f64
                })
                .expect_err("Call on a non-function must throw");
                let key = crate::string::intern_ascii_literal(b"message");
                let message = super::super::js_object_get_field_by_name_f64(
                    crate::value::js_nanbox_get_pointer(error) as *const ObjectHeader,
                    key,
                );
                let message = crate::value::js_jsvalue_to_string(message);
                assert!(
                    crate::string::string_as_str(message).contains("not a function"),
                    "must be Call's TypeError, not IteratorResult validation's TypeError"
                );
            }
            build_iterator_prototypes();
        }
    }

    extern "C" fn getter_seventeen(
        _c: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        17.0
    }

    #[test]
    fn native_step_next_get_preserves_collection_own_accessors() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            for kind in kinds() {
                let iter = fixture(kind);
                let getter =
                    crate::closure::js_closure_alloc(crate::fn_info!(getter_seventeen, 0), 0);
                let key = crate::string::intern_ascii_literal(b"next");
                super::super::js_object_define_accessor(
                    iter,
                    f64::from_bits(JSValue::string_ptr(key as *mut _).bits()),
                    js_nanbox_pointer(getter as i64),
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
                assert_eq!(
                    crate::array::js_iterator_next_method(iter),
                    17.0,
                    "own next must run its getter on every family"
                );
            }
            build_iterator_prototypes();
        }
    }

    extern "C" fn user_next(
        _c: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }

    #[test]
    fn native_step_saved_method_and_accessor_and_ancestor_guards() {
        unsafe {
            let _stable = crate::gc::GcSuppressScope::new();
            for kind in kinds() {
                let iter = fixture(kind);
                let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
                let saved = crate::array::js_iterator_next_method(iter);
                assert!(iterator_step_method_is_builtin(obj, saved));
                let bound = crate::closure::js_function_bind(saved, &iter, 1);
                assert!(
                    iterator_step_is_builtin(obj),
                    "binding must leave the receiver's shape pristine"
                );
                assert!(!iterator_step_method_is_builtin(obj, bound),
                    "a bound original must call its saved receiver, even after an own next is deleted");
                let other = crate::closure::js_closure_alloc(crate::fn_info!(user_next, 0), 0);
                assert!(
                    !iterator_step_method_is_builtin(obj, js_nanbox_pointer(other as i64)),
                    "a captured override cannot become native after next is restored"
                );
                for level in 0..3 {
                    for name in ["next", "return"] {
                        let iter = fixture(kind);
                        let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
                        let mut target = obj;
                        for _ in 0..level {
                            target = crate::value::js_nanbox_get_pointer(f64::from_bits(
                                super::super::shapes::object_prototype_word(target),
                            )) as *mut ObjectHeader;
                        }
                        let key = crate::string::intern_ascii_literal(name.as_bytes());
                        super::super::js_object_define_accessor(
                            js_nanbox_pointer(target as i64),
                            f64::from_bits(JSValue::string_ptr(key as *mut _).bits()),
                            js_nanbox_pointer(other as i64),
                            f64::from_bits(crate::value::TAG_UNDEFINED),
                        );
                        assert!(!iterator_step_is_builtin(obj), "accessor {level} {name}");
                    }
                }
                let iter = fixture(kind);
                let obj = crate::value::js_nanbox_get_pointer(iter) as *mut ObjectHeader;
                let family = crate::value::js_nanbox_get_pointer(f64::from_bits(
                    super::super::shapes::object_prototype_word(obj),
                )) as *mut ObjectHeader;
                let shared = crate::value::js_nanbox_get_pointer(f64::from_bits(
                    super::super::shapes::object_prototype_word(family),
                )) as *mut ObjectHeader;
                let ancestor = js_object_alloc(0, 0);
                let key = crate::string::intern_ascii_literal(b"return");
                super::super::js_object_set_field_by_name(ancestor, key, saved);
                chain_to(shared, ancestor);
                assert!(
                    !iterator_step_is_builtin(obj),
                    "an inherited return requires protocol"
                );
            }
            build_iterator_prototypes();
        }
    }
}

/// A captured method is eligible only if its immutable body is the same
/// body the current prototype shape proves. Bound wrappers stay on protocol.
pub(crate) unsafe fn iterator_step_method_is_builtin(
    obj: *const ObjectHeader,
    method: f64,
) -> bool {
    if !iterator_step_is_builtin(obj) {
        return false;
    }
    let method =
        crate::value::js_nanbox_get_pointer(method) as *const crate::closure::ClosureHeader;
    if !crate::closure::is_closure_ptr(method as usize) {
        return false;
    }
    let proto = crate::value::js_nanbox_get_pointer(f64::from_bits(
        super::shapes::object_prototype_word(obj),
    )) as *const ObjectHeader;
    let Some(shape) = super::shapes::object_shape_record(proto) else {
        return false;
    };
    let Some(info) = shape.constfn_info(0) else {
        return false;
    };
    (*method).info as usize as u64 == info
        || crate::closure::get_valid_func_ptr(method)
            == (*(info as *const crate::closure::JsFunctionInfo)).code
}
