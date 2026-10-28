//! Shared ordinary property operations. Proxy dispatch uses installer slots.
use super::*;

/// Is `value` a callable function value: a closure, a class-ref constructor, or
/// a (possibly callable) proxy? Distinct from `is_callable`, which treats *any*
/// pointer-tagged value as callable — that's too loose for trap validation,
/// where a present-but-non-callable trap (e.g. `apply: {}`) must throw a
/// `TypeError` rather than be silently invoked as a no-op.
pub(crate) fn is_callable_function(value: f64) -> bool {
    let bits = value.to_bits();
    // Class-ref constructors (INT32-tagged, top16 == 0x7FFE) are callable.
    if (bits >> 48) == 0x7FFE || crate::object::class_value::class_value_id(value).is_some() {
        return crate::object::class_ref_id(value).is_some();
    }
    // A proxy whose target is callable is itself callable.
    if lookup(value).is_some() {
        return proxy_wraps_callable(value);
    }
    // Capturing/per-evaluation classes are heap class objects, not closures
    // or INT32 class refs. They still have [[Call]] (which throws when used
    // without `new`), so a Proxy must retain their callable classification.
    if crate::object::is_class_object_value(value) {
        return true;
    }
    // A POINTER_TAG value is callable only if it points at a closure.
    (bits & !POINTER_MASK) == POINTER_TAG && crate::closure::is_closure_bits(bits)
}

pub(crate) fn is_constructor_function(value: f64) -> bool {
    if lookup(value).is_some() {
        return proxy_is_constructor(value);
    }
    // #11229: a per-evaluation class object (a class that captures its
    // enclosing scope -- every such class in a CommonJS module, and any class
    // returned from a factory) is a heap object, not a closure, so the
    // callable check below rejected it and `Reflect.construct(C, args)` threw
    // `is not a constructor` while `new C(...args)` worked. It is always a
    // constructor, and `js_new_function_construct_with_new_target` already
    // replays its constructor with its own captures.
    if crate::object::is_class_object_value(value) {
        return true;
    }
    if !is_callable_function(value) {
        return false;
    }
    if crate::object::builtin_closure_is_non_constructable_value(value) {
        return false;
    }
    // #2768: arrow functions have no [[Construct]]. The deep construct path
    // already rejects an arrow *target* ("Arrow function is not a
    // constructor"), but `Reflect.construct`'s up-front constructor checks —
    // for both the target and the `newTarget` operand — must reject them too.
    // Without this, `Reflect.construct(C, args, arrowFn)` silently proceeded
    // instead of throwing the spec TypeError (newTarget is never itself
    // constructed, so the deep path never fires for it).
    // A POINTER_TAG value is only a closure if `is_closure_ptr` confirms it —
    // a callable Proxy is also POINTER_TAG but its lower 48 bits are a proxy
    // id, not a `ClosureHeader*`, so `closure_is_arrow` (which dereferences the
    // header via `get_valid_func_ptr`) must not run on it. Mirror the guard in
    // `is_callable_function`.
    let bits = value.to_bits();
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let raw = (bits & POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(raw)
            && crate::closure::closure_is_arrow(raw as *const crate::closure::ClosureHeader)
        {
            return false;
        }
    }
    true
}
