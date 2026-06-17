//! Shared `%IteratorPrototype%`-style prototype singletons for the built-in
//! iterator objects (Array / Map / Set / String iterators).
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

// GC-rooted singleton slots. Scanned in `object/mod.rs::scan_object_cache_roots_mut`.
pub(crate) static ITERATOR_PROTOTYPE_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static ARRAY_ITERATOR_PROTOTYPE_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static MAP_ITERATOR_PROTOTYPE_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static SET_ITERATOR_PROTOTYPE_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static STRING_ITERATOR_PROTOTYPE_PTR: AtomicI64 = AtomicI64::new(0);
pub(crate) static REGEXP_STRING_ITERATOR_PROTOTYPE_PTR: AtomicI64 = AtomicI64::new(0);

/// Dispatch `method` on the implicit-`this` iterator instance, routing by class
/// id to the matching existing iterator dispatcher. Shared by the per-family
/// `next` thunks (read as a value or invoked via `.call`) and the parent
/// `[Symbol.iterator]` thunk. Returns a `{ value:undefined, done:true }`-ish
/// throw when `this` is not a recognized iterator (test262 `this-not-object` /
/// `does-not-have-...-internal-slots` brand checks).
unsafe fn dispatch_on_implicit_this(method: &str) -> f64 {
    let this = super::js_implicit_this_get();
    let jv = JSValue::from_bits(this.to_bits());
    if !jv.is_pointer() {
        return brand_type_error(method);
    }
    let obj = jv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if obj.is_null() || !super::is_valid_obj_ptr(obj as *const u8) {
        return brand_type_error(method);
    }
    let class_id = (*obj).class_id;
    match class_id {
        crate::array::ARRAY_ITERATOR_CLASS_ID => {
            crate::array::dispatch_array_iterator_method(obj, method)
        }
        crate::collection_iter_object::MAP_ITERATOR_CLASS_ID => {
            crate::collection_iter_object::dispatch_map_iterator_method(obj, method)
        }
        crate::collection_iter_object::SET_ITERATOR_CLASS_ID => {
            crate::collection_iter_object::dispatch_set_iterator_method(obj, method)
        }
        crate::string::STRING_ITERATOR_CLASS_ID => {
            crate::string::dispatch_string_iterator_method(obj, method)
        }
        #[cfg(feature = "regex-engine")]
        crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID => {
            crate::regex::dispatch_regexp_string_iterator_method(obj, method)
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
fn chain_to(child: *mut ObjectHeader, parent: *mut ObjectHeader) {
    let parent_bits = crate::value::js_nanbox_pointer(parent as i64).to_bits();
    super::prototype_chain::object_set_static_prototype(child as usize, parent_bits);
}

/// Build the shared `%IteratorPrototype%` and the four family prototypes,
/// storing them in the GC-rooted slots. Idempotent.
fn build_iterator_prototypes() {
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

    ITERATOR_PROTOTYPE_PTR.store(shared as i64, Ordering::Release);
    ARRAY_ITERATOR_PROTOTYPE_PTR.store(array_proto as i64, Ordering::Release);
    MAP_ITERATOR_PROTOTYPE_PTR.store(map_proto as i64, Ordering::Release);
    SET_ITERATOR_PROTOTYPE_PTR.store(set_proto as i64, Ordering::Release);
    STRING_ITERATOR_PROTOTYPE_PTR.store(string_proto as i64, Ordering::Release);
    REGEXP_STRING_ITERATOR_PROTOTYPE_PTR.store(regexp_string_proto as i64, Ordering::Release);
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
        _ => return,
    };
    let proto_ptr = slot.load(Ordering::Acquire);
    if proto_ptr == 0 {
        return;
    }
    chain_to(obj_ptr, proto_ptr as *mut ObjectHeader);
}
