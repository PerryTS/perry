//! #9131: a per-instance `[[Prototype]]` override wins over the class vtable.
//!
//! Split out of `get_field_by_name_tail.rs`, which is at the 2000-line cap.
//! Both of that file's own-key misses — the keyless arm and the shaped-receiver
//! arm — ask the same question, so it lives here once instead of twice.

use crate::object::ObjectHeader;
use crate::value::JSValue;

/// An explicit per-instance `[[Prototype]]` REPLACES the class's declaration
/// prototype, so when the own-key scan misses, that chain is what decides —
/// `Some(value)` here is authoritative and the caller must not fall back to the
/// class vtable.
///
/// A miss on the custom chain returns `None`, NOT `Some(undefined)`. #9131
/// originally returned `Some(undefined)` to avoid resurrecting the old class
/// surface, but the arms BELOW both call sites are not only the class vtable:
/// they are also everything Perry *synthesizes* rather than stores on a real
/// prototype — a plain-function `.prototype`, the boxed-wrapper builtins, the
/// iterator helpers. Swallowing the miss made those unreachable for every
/// flagged receiver, which is #9244 (`Object(true).valueOf()` →
/// `called on incompatible receiver`, `FooObj.prototype` → `undefined`).
///
/// This is the same polarity `canonical_shape_excludes_own_property` uses: a
/// question we cannot answer here defers to the tail rather than fabricating a
/// verdict. Evaluated class prototypes also take this path (#9502): their
/// heritage can differ between evaluations of one template. Other internal
/// runtime wiring retains its existing fallback behavior.
///
/// `None` therefore means either no override, or an override that does not
/// carry this key — in both cases the caller keeps its existing fallback.
pub(super) fn inherited_field_if_overridden(
    obj: *const ObjectHeader,
    key: *const crate::string::StringHeader,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    if !crate::object::prototype_chain::object_has_individual_class_prototype(obj as usize) {
        return None;
    }
    if let Some(value) = crate::object::prototype_chain::resolve_inherited_field(obj as usize, key)
    {
        return Some(value);
    }
    // #10827: the two reasons this walk can miss are not the same reason.
    //
    // If the chain ENDS IN AN EXPLICIT NULL, the miss is the final answer and
    // it is `undefined` — there is nothing above this receiver to synthesize
    // from, and the arms below would go and ask the class surface anyway.
    // That is how `Object.setPrototypeOf(o, null); o.a` kept answering from
    // the prototype `o` was born with, while `"a" in o` correctly said false:
    // the read and the `in` disagreed, which is the whole bug.
    //
    // Every other miss still returns `None` and defers, which is what #9244
    // requires: the arms below are not only the class vtable, they are also
    // everything Perry SYNTHESIZES rather than stores on a real prototype (a
    // plain function's `.prototype`, the boxed-wrapper builtins, the iterator
    // helpers), and swallowing those made them unreachable.
    if crate::object::prototype_chain::prototype_chain_ends_in_explicit_null(obj as usize) {
        return Some(JSValue::undefined());
    }
    None
}
