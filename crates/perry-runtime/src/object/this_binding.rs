//! `this` / `new.target` / static-`this` binding state for method dispatch
//! (split out of `object/mod.rs`, behavior-preserving).

use super::*;

use std::cell::Cell;

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
    pub(crate) static IMPLICIT_THIS: Cell<u64> = const { Cell::new(crate::value::TAG_UNDEFINED) };
    pub(crate) static NEW_TARGET: Cell<u64> = const { Cell::new(crate::value::TAG_UNDEFINED) };
    // One-shot receiver override for STATIC method bodies. A compiled static
    // method's `this` slot used to be a compile-time class-ref literal, so
    // `C.m.call({})` / `D.m()` (inherited) ran with `this === C` and static
    // private brand checks could never throw (test262 class/elements
    // static-private-*). Armed by the dynamic dispatch paths that know the
    // real receiver (`js_class_static_method_call`, the Function.prototype
    // call/apply arms for a static bound-method value); consumed (take
    // semantics) by `js_static_this_resolve` in the static-method prologue.
    // Direct compiled calls never arm it, so they keep the lexical class-ref.
    static STATIC_THIS_OVERRIDE: Cell<(bool, u64)> =
        const { Cell::new((false, crate::value::TAG_UNDEFINED)) };
}

/// Arm the static-`this` override unconditionally (used by the call/apply
/// receiver paths, which take precedence over the inner dynamic dispatch).
pub(crate) fn static_this_arm(value: f64) {
    STATIC_THIS_OVERRIDE.with(|c| c.set((true, value.to_bits())));
}

/// Arm the static-`this` override only when no outer caller has already armed
/// it — `js_class_static_method_call` runs INSIDE the call/apply plumbing, and
/// the outermost receiver (the `.call(x)` thisArg) must win.
pub(crate) fn static_this_arm_if_unarmed(value: f64) {
    STATIC_THIS_OVERRIDE.with(|c| {
        if !c.get().0 {
            c.set((true, value.to_bits()));
        }
    });
}

/// Disarm without consuming (paired with arm sites as a safety net in case
/// the invoked target never reached a static-method prologue).
pub(crate) fn static_this_disarm() {
    STATIC_THIS_OVERRIDE.with(|c| c.set((false, crate::value::TAG_UNDEFINED)));
}

/// Arm the static-`this` override with a class constructor ref. Emitted by
/// codegen immediately before a direct call to an INHERITED static method
/// (`D.f()` where `f` lives on a parent class) so the body sees the dispatch
/// base (`this === D`) instead of the lexical defining class — spec
/// OrdinaryCallBindThis for `D.f()`, and what makes static-private brand
/// checks on subclass receivers throw (test262 static-private-method-
/// subclass-receiver).
// #1561-style force-keep: only generated IR calls this.
#[used]
static KEEP_JS_STATIC_THIS_ARM_CLASSREF: extern "C" fn(u32) = js_static_this_arm_classref;

#[no_mangle]
pub extern "C" fn js_static_this_arm_classref(class_id: u32) {
    if class_id != 0 {
        static_this_arm(native_module::class_constructor_ref_value(class_id));
    }
}

/// Arm the static-`this` override with an arbitrary receiver value. Emitted
/// by the codegen static-dispatch tower (`D.f()` where the receiver is a
/// class-ref expression and the method resolves on a parent class at compile
/// time) right before the direct call.
// #1561-style force-keep: only generated IR calls this.
#[used]
static KEEP_JS_STATIC_THIS_ARM_VALUE: extern "C" fn(f64) = js_static_this_arm_value;

#[no_mangle]
pub extern "C" fn js_static_this_arm_value(value: f64) {
    static_this_arm(value);
}

/// Static-method prologue `this` resolution: take the armed override if any,
/// else the lexical class-ref the codegen passes in.
// #1561-style force-keep: only generated IR calls this.
#[used]
static KEEP_JS_STATIC_THIS_RESOLVE: extern "C" fn(f64) -> f64 = js_static_this_resolve;

#[no_mangle]
pub extern "C" fn js_static_this_resolve(default_this: f64) -> f64 {
    STATIC_THIS_OVERRIDE.with(|c| {
        let (armed, bits) = c.get();
        if armed {
            c.set((false, crate::value::TAG_UNDEFINED));
            f64::from_bits(bits)
        } else {
            default_this
        }
    })
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
        return crate::builtins::js_boxed_string_new(value, 1);
    }
    // #5515: a class reference is an INT32-tagged class id, but it is
    // conceptually the class constructor OBJECT, not a primitive number.
    // `C.viaFn()` / `f.call(C)` bind `this` to the class ref; boxing it as a
    // Number here (the `is_int32()` arm below) makes a regular-function static
    // data property observe `this !== C` and lose access to the static chain.
    // Return the class ref unchanged so `this === C` and `this.staticData` work.
    if class_ref_id(value).is_some() {
        return value;
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

/// Read the current `new.target` value for ordinary function bodies.
#[no_mangle]
pub extern "C" fn js_new_target_get() -> f64 {
    NEW_TARGET.with(|c| f64::from_bits(c.get()))
}

/// Set `new.target` and return the previous value.
#[no_mangle]
pub extern "C" fn js_new_target_set(value: f64) -> f64 {
    NEW_TARGET.with(|c| f64::from_bits(c.replace(value.to_bits())))
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
    NEW_TARGET.with(|c| {
        let mut bits = c.get();
        if visitor.visit_nanbox_u64_slot(&mut bits) {
            c.set(bits);
        }
    });
    STATIC_THIS_OVERRIDE.with(|c| {
        let (armed, mut bits) = c.get();
        if visitor.visit_nanbox_u64_slot(&mut bits) {
            c.set((armed, bits));
        }
    });
}
