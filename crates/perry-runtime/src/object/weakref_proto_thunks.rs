//! Weak-wrapper method dispatch: `WeakMap` / `WeakSet` (moved here from
//! `weakref.rs`, which is at the 2000-line gate) plus the `WeakRef` /
//! `FinalizationRegistry` arms and prototype thunks added by #7947.
//!
//! ## Why this module exists (#7947)
//!
//! `WeakMap.prototype.get` & friends have always had two independent routes: an
//! HIR fast path that folds `wm.get(k)` straight to `js_weakmap_get`, and this
//! dynamic route, reached from `js_native_call_method` whenever the receiver is
//! anything the fold could not recognise. `WeakRef.prototype.deref` and
//! `FinalizationRegistry.prototype.register`/`.unregister` had **only** the
//! fold, and the fold keys on a bare local NAME recorded by
//! `pre_scan_weakref_locals`. Every other receiver shape — an array element, an
//! object property, a call result, a `for…of` binding, a function parameter, or
//! a `const r = new WeakRef(x)` inside an *arrow function* (the pre-scan
//! descends into function DECLARATIONS only) — resolved nothing and threw
//! `TypeError: deref is not a function`. Two of twenty receiver shapes worked.
//!
//! Three additions close that:
//!
//! * `try_weak_method_dispatch` gains `CLASS_ID_WEAKREF` /
//!   `CLASS_ID_FINALIZATION_REGISTRY` arms, so a *call* on any receiver shape
//!   reaches the runtime helper;
//! * `install_weakref_proto_methods` installs brand-checking
//!   `WeakRef.prototype.deref` / `FinalizationRegistry.prototype.{register,
//!   unregister}` thunks, so the reflective path (`.call`/`.apply`, method
//!   extraction, `typeof wr.deref`) works and brand-checks `this`;
//! * `dispatch_foreign_weak_receiver` gives the *fold* a safe landing when its
//!   name-keyed guess was wrong — see below.
//!
//! ## The fold's landing pad (#7948)
//!
//! `pre_scan_weakref_locals` is name-keyed and scope-blind, so a module that
//! binds `const r = new WeakRef(x)` anywhere folds EVERY `r.deref()` in that
//! module onto `js_weakref_deref` — including an `r` that is a plain object, a
//! user class instance, or a function parameter. `js_weakref_deref` used to
//! read its internal slot by name off whatever it was handed and answer
//! `undefined`: a silent wrong answer, exit code 0. The helpers now brand-check
//! their receiver and hand a foreign one to `dispatch_foreign_weak_receiver`,
//! which re-enters ordinary dynamic method dispatch. A mis-fold therefore
//! degrades to the correct slow path instead of a wrong answer. Recursion is
//! impossible: `js_native_call_method` only routes back into these helpers when
//! the receiver's `class_id` IS the weak wrapper's, which is exactly the case
//! the brand check accepts.

use super::*;
use crate::weakref::{
    js_finreg_register, js_finreg_unregister, js_weakmap_delete, js_weakmap_get, js_weakmap_has,
    js_weakmap_set, js_weakref_deref, js_weakset_add, CLASS_ID_FINALIZATION_REGISTRY,
    CLASS_ID_WEAKMAP, CLASS_ID_WEAKREF, CLASS_ID_WEAKSET,
};

/// Dynamic-dispatch entry point for weak-wrapper method calls (issues
/// #1757/#1758 for WeakMap/WeakSet, #7947 for WeakRef/FinalizationRegistry).
/// `js_native_call_method` calls this for any heap object; it returns
/// `Some(result)` only when `obj` carries one of the reserved weak `class_id`s
/// and `method_name` is one of *that class's own* methods, and `None` otherwise
/// so the caller falls through to its normal dispatch. `receiver` is the
/// NaN-boxed f64 the `js_weak*` / `js_finreg_*` helpers expect.
///
/// A method that isn't one of the receiver's own (e.g. `"add"` on a WeakMap,
/// `"deref"` on a WeakSet, or any name outside the per-class sets) falls
/// through to `None` so the ordinary property lookup resolves it — correctly
/// missing and raising `TypeError: ... is not a function` on a call, rather
/// than this function silently answering `undefined`.
///
/// # Safety
/// `obj` must be a valid, readable `ObjectHeader` pointer (the caller has
/// already validated it as a live heap object).
pub unsafe fn try_weak_method_dispatch(
    obj: *const ObjectHeader,
    receiver: f64,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let class_id = (*obj).class_id;
    if !matches!(
        class_id,
        CLASS_ID_WEAKMAP | CLASS_ID_WEAKSET | CLASS_ID_WEAKREF | CLASS_ID_FINALIZATION_REGISTRY
    ) {
        return None;
    }
    let args: &[f64] = if !args_ptr.is_null() && args_len > 0 {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    // #5834: dispatch regardless of arg count, padding missing positions with
    // `undefined` — mirrors calling the real thunks reflectively. Arity-gating
    // these arms let `s.add()` (zero args) fall through to a no-op, skipping
    // `js_weakset_add`'s CanBeHeldWeakly check entirely (it must throw
    // `TypeError` since `undefined` cannot be held weakly).
    //
    // Also gate each method by the receiver's actual class: `"set"`/`"get"`
    // only exist on WeakMap, `"add"` only on WeakSet, `"deref"` only on
    // WeakRef, `"register"`/`"unregister"` only on FinalizationRegistry
    // (`"has"`/`"delete"` are shared by WeakMap and WeakSet). Without this a
    // WeakMap receiver could reach `js_weakset_add` for a `.add(...)` call (and
    // vice versa) instead of falling through to the ordinary property lookup,
    // which correctly resolves the missing method to `undefined` and throws
    // `TypeError: ... is not a function`.
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg = |i: usize| args.get(i).copied().unwrap_or(undef);
    let result = match (method_name, class_id) {
        ("set", CLASS_ID_WEAKMAP) => js_weakmap_set(receiver, arg(0), arg(1)),
        ("add", CLASS_ID_WEAKSET) => js_weakset_add(receiver, arg(0)),
        ("get", CLASS_ID_WEAKMAP) => js_weakmap_get(receiver, arg(0)),
        ("has", CLASS_ID_WEAKMAP | CLASS_ID_WEAKSET) => js_weakmap_has(receiver, arg(0)),
        ("delete", CLASS_ID_WEAKMAP | CLASS_ID_WEAKSET) => js_weakmap_delete(receiver, arg(0)),
        // #7947: `deref` / `register` / `unregister` previously existed only as
        // the name-keyed HIR fold, so every receiver shape it could not name
        // threw `TypeError: deref is not a function`.
        ("deref", CLASS_ID_WEAKREF) => js_weakref_deref(receiver),
        ("register", CLASS_ID_FINALIZATION_REGISTRY) => {
            js_finreg_register(receiver, arg(0), arg(1), arg(2))
        }
        ("unregister", CLASS_ID_FINALIZATION_REGISTRY) => js_finreg_unregister(receiver, arg(0)),
        _ => return None,
    };
    Some(result)
}

/// Return the reserved WeakMap/WeakSet `class_id` of `receiver` if it is one
/// of those collections, else `None`. Backs the reflective
/// `WeakMap.prototype.*` / `WeakSet.prototype.*` thunks so they can perform
/// the spec brand check (`TypeError` on a non-Weak* receiver) before
/// dispatching.
///
/// Deliberately does NOT admit `CLASS_ID_WEAKREF` /
/// `CLASS_ID_FINALIZATION_REGISTRY` — callers branch on "WeakMap else WeakSet",
/// so widening it would let a `WeakRef` pass a `WeakSet` brand check. Use
/// [`weak_wrapper_class_id`] for the four-way question.
pub fn weak_class_id_from_receiver(receiver: f64) -> Option<u32> {
    match weak_wrapper_class_id(receiver) {
        Some(cid @ (CLASS_ID_WEAKMAP | CLASS_ID_WEAKSET)) => Some(cid),
        _ => None,
    }
}

/// Return the reserved weak-wrapper `class_id` of `receiver` — WeakMap,
/// WeakSet, WeakRef or FinalizationRegistry — else `None`.
///
/// The `GcHeader.obj_type == GC_TYPE_OBJECT` pre-filter ensures the pointer is
/// an `ObjectHeader`-backed allocation before `class_id` is read, so a
/// `Set`/`Map` pointer (different `obj_type`) or a primitive
/// (`js_nanbox_get_pointer` yields 0) safely resolves to `None`.
pub fn weak_wrapper_class_id(receiver: f64) -> Option<u32> {
    let addr = crate::value::js_nanbox_get_pointer(receiver) as usize;
    // #4004: reject the small-handle band (Web Fetch / node:http / timer ids
    // are NaN-boxed POINTER_TAG values, not heap addresses) before
    // dereferencing the GC header. The weak wrappers are ObjectHeader-backed
    // allocations above the cutoff. See `value::addr_class` for the band map.
    unsafe {
        match crate::value::addr_class::try_read_gc_header(addr) {
            Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT => {}
            _ => return None,
        }
        let cid = (*(addr as *const ObjectHeader)).class_id;
        if matches!(
            cid,
            CLASS_ID_WEAKMAP | CLASS_ID_WEAKSET | CLASS_ID_WEAKREF | CLASS_ID_FINALIZATION_REGISTRY
        ) {
            return Some(cid);
        }
    }
    None
}

/// True when `receiver` is a genuine instance of the weak wrapper `class_id`.
/// The brand check the folded fast-path helpers run before trusting the HIR's
/// name-keyed guess (#7948).
pub fn is_weak_wrapper(receiver: f64, class_id: u32) -> bool {
    weak_wrapper_class_id(receiver) == Some(class_id)
}

/// The `WeakMap`/`WeakSet` form of the brand-check-and-delegate above, as one
/// call: returns `Some(result_of_the_receivers_own_method)` when `receiver` is
/// NOT a genuine weak collection, and `None` when the caller should proceed.
///
/// `WeakMap` and `WeakSet` share three helpers (`js_weakset_has`/`_delete`
/// delegate to `js_weakmap_has`/`_delete`, and `js_weakset_add` to
/// `js_weakmap_set`), so the check admits either class id rather than the exact
/// one — the per-class method gating lives in [`try_weak_method_dispatch`] and
/// in the prototype thunks, both of which run before these helpers.
pub fn delegate_if_not_weak_collection(
    receiver: f64,
    method_name: &str,
    args: &[f64],
) -> Option<f64> {
    match weak_wrapper_class_id(receiver) {
        Some(CLASS_ID_WEAKMAP | CLASS_ID_WEAKSET) => None,
        _ => Some(dispatch_foreign_weak_receiver(receiver, method_name, args)),
    }
}

/// Re-dispatch `method_name` on a receiver the HIR fold mis-identified as a
/// weak wrapper (#7948). Routes back through ordinary dynamic method dispatch,
/// which resolves the receiver's own method — or, when there is none, throws
/// the same `TypeError: <method> is not a function` node throws.
///
/// Not reachable in a loop: `js_native_call_method` routes into the weak
/// helpers only via [`try_weak_method_dispatch`], which requires the reserved
/// `class_id` the brand check already rejected.
pub fn dispatch_foreign_weak_receiver(receiver: f64, method_name: &str, args: &[f64]) -> f64 {
    unsafe {
        super::js_native_call_method(
            receiver,
            method_name.as_ptr() as *const i8,
            method_name.len(),
            if args.is_empty() {
                std::ptr::null()
            } else {
                args.as_ptr()
            },
            args.len(),
        )
    }
}

// --- prototype thunks (#7947) --------------------------------------------

fn throw_incompatible(proto: &str, method: &str) -> ! {
    let msg = format!("Method {proto}.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

/// Resolve `IMPLICIT_THIS` to a receiver of the expected weak-wrapper class id,
/// or throw a `TypeError`. Mirrors `collection_proto_thunks`'
/// `weak_receiver_or_throw` for the WeakRef/FinalizationRegistry pair.
fn wrapper_receiver_or_throw(expected: u32, proto: &str, method: &str) -> f64 {
    let receiver = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if is_weak_wrapper(receiver, expected) {
        receiver
    } else {
        throw_incompatible(proto, method)
    }
}

pub(super) extern "C" fn weakref_proto_deref_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let r = wrapper_receiver_or_throw(CLASS_ID_WEAKREF, "WeakRef.prototype", "deref");
    js_weakref_deref(r)
}

pub(super) extern "C" fn finreg_proto_register_thunk(
    _c: *const crate::closure::ClosureHeader,
    target: f64,
    held: f64,
    token: f64,
) -> f64 {
    let r = wrapper_receiver_or_throw(
        CLASS_ID_FINALIZATION_REGISTRY,
        "FinalizationRegistry.prototype",
        "register",
    );
    js_finreg_register(r, target, held, token)
}

pub(super) extern "C" fn finreg_proto_unregister_thunk(
    _c: *const crate::closure::ClosureHeader,
    token: f64,
) -> f64 {
    let r = wrapper_receiver_or_throw(
        CLASS_ID_FINALIZATION_REGISTRY,
        "FinalizationRegistry.prototype",
        "unregister",
    );
    js_finreg_unregister(r, token)
}

/// Install the brand-checking `.prototype` methods for `WeakRef` /
/// `FinalizationRegistry`. Returns `true` when `builtin_name` is one of those —
/// the caller then adds the shared `OBJECT_PROTO_METHODS` — and `false`
/// otherwise. Called from `global_this::populate_builtin_prototype_methods`.
///
/// Arities are the spec `.length` values: `deref` 0, `register` 2 (the
/// unregister token is optional and does not count), `unregister` 1.
pub(super) fn install_weakref_proto_methods(
    builtin_name: &str,
    proto_obj: *mut ObjectHeader,
) -> bool {
    use super::global_this::install_proto_method as ipm;
    match builtin_name {
        "WeakRef" => {
            ipm(
                proto_obj,
                "deref",
                weakref_proto_deref_thunk as *const u8,
                0,
            );
        }
        "FinalizationRegistry" => {
            ipm(
                proto_obj,
                "register",
                finreg_proto_register_thunk as *const u8,
                2,
            );
            ipm(
                proto_obj,
                "unregister",
                finreg_proto_unregister_thunk as *const u8,
                1,
            );
        }
        _ => return false,
    }
    true
}

/// Resolve a `WeakRef`/`FinalizationRegistry` prototype method to the SAME
/// brand-checking thunk value installed on `<Builtin>.prototype`, so a VALUE
/// read off an *instance* (`typeof wr.deref`, `wr.deref.bind(wr)`,
/// `const d = wr.deref`) yields a function rather than `undefined`. Mirrors
/// `collection_proto_thunks::collection_proto_method_value`, which does this
/// for WeakMap/WeakSet.
///
/// Returns `None` for a receiver that is not one of the two wrappers, or a
/// name that is not one of its methods, so callers keep their existing path.
pub(crate) fn weakref_proto_method_value_for(receiver_cid: u32, method_name: &str) -> Option<f64> {
    let builtin = match (receiver_cid, method_name) {
        (CLASS_ID_WEAKREF, "deref") => "WeakRef",
        (CLASS_ID_FINALIZATION_REGISTRY, "register" | "unregister") => "FinalizationRegistry",
        _ => return None,
    };
    let proto = super::global_this::builtin_prototype_value(builtin);
    if proto.to_bits() == crate::value::TAG_UNDEFINED {
        return None;
    }
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *mut ObjectHeader;
    if proto_ptr.is_null() {
        return None;
    }
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    unsafe { super::own_data_field_by_name(proto_ptr, key) }
        .map(|value| f64::from_bits(value.bits()))
        .filter(|v| v.to_bits() != crate::value::TAG_UNDEFINED)
}
