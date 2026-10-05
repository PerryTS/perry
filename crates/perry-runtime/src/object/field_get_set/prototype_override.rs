//! #9131: a per-instance `[[Prototype]]` override wins over the class vtable.
//!
//! Split out of `get_field_by_name_tail.rs`, which is at the 2000-line cap.
//! Both of that file's own-key misses — the keyless arm and the shaped-receiver
//! arm — ask the same question, so it lives here once instead of twice.

use crate::object::ObjectHeader;
use crate::value::JSValue;

/// What [`inherited_field_if_overridden`] learned about an own-key miss.
pub(super) enum InheritedRead {
    /// Authoritative: return this value.
    Hit(JSValue),
    /// The recorded chain was walked and does not carry the key. The caller's
    /// own `resolve_inherited_field` would repeat that walk — skip it.
    Missed,
    /// No walk happened; the caller's fallbacks run unchanged.
    NotWalked,
    /// The recorded chain missed, and replaces the class-id surface: an
    /// evaluated class (#12029) or a displaced default prototype (#11391).
    /// The class-id prototype arms must not answer either.
    Superseded,
}

impl InheritedRead {
    /// Whether the recorded prototype chain has already been walked.
    pub(super) fn walked(&self) -> bool {
        !matches!(self, InheritedRead::NotWalked)
    }

    /// Whether the class-id prototype walk may still answer this read: false
    /// when the receiver's recorded chain owns the whole class surface.
    pub(super) fn class_prototype_answers(&self) -> bool {
        !matches!(self, InheritedRead::Superseded)
    }
}

/// An explicit per-instance `[[Prototype]]` REPLACES the class's declaration
/// prototype, so when the own-key scan misses, that chain is what decides —
/// `Hit(value)` here is authoritative and the caller must not fall back to the
/// class vtable.
///
/// A miss on the custom chain is `Missed`, NOT `Hit(undefined)`. #9131
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
/// An evaluated class's recorded chain owns its whole surface: a miss disables
/// class-id fallbacks too (#12029). Synthesized intrinsic fallbacks remain.
///
/// #10877: the two non-`Hit` answers differ in whether the recorded chain was
/// already walked. Both callers end in a `resolve_inherited_field` of their
/// own, for receivers WITHOUT the override flag. After a `Missed` they must
/// skip it: it is the same walk from the same receiver, so it cannot find
/// anything this one did not, and each hop of that walk re-enters the generic
/// getter on the prototype, which asks this function again. Walking twice per
/// level made an absent read cost `2^depth` generic-getter entries on an
/// `Object.create` chain (536,047 instructions at 8 hops), and ran an
/// inherited getter that returned `undefined` twice.
pub(super) fn inherited_field_if_overridden(
    obj: *const ObjectHeader,
    key: *const crate::string::StringHeader,
) -> InheritedRead {
    if key.is_null() {
        return InheritedRead::NotWalked;
    }
    // #11391: a class-default link is authoritative too, once the class id no
    // longer names the object it links to.
    let individual =
        crate::object::prototype_chain::object_has_individual_class_prototype(obj as usize);
    let superseded = !individual
        && crate::object::prototype_chain::class_default_prototype_superseded(obj as usize);
    if !individual && !superseded {
        return InheritedRead::NotWalked;
    }
    if let Some(value) = crate::object::prototype_chain::resolve_inherited_field(obj as usize, key)
    {
        return InheritedRead::Hit(value);
    }
    // #10827: the two reasons this walk can miss are not the same reason.
    //
    // If the chain ENDS IN AN EXPLICIT NULL before Object.prototype, the miss
    // is the final answer: `undefined`. There is nothing above it to synthesize
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
    if crate::object::prototype_chain::prototype_chain_ends_in_null_before_object_prototype(
        obj as usize,
    ) {
        return InheritedRead::Hit(JSValue::undefined());
    }
    // #12029: a class evaluation owns its entire prototype surface. A miss
    // on that chain must never fall through to the template's declaration
    // prototype (the first evaluation), even for a newly added data property.
    // ClassBody accessors, like methods, are physical keys on the prototype.
    let evaluated = individual
        && crate::object::private_evaluation_brand_value(crate::value::js_nanbox_pointer(
            obj as i64,
        ))
        .is_some_and(crate::object::class_registry::is_class_object_value);
    if superseded || evaluated {
        return InheritedRead::Superseded;
    }
    InheritedRead::Missed
}

/// #10877: whether `read_miss` — reported by
/// `class_registry::resolve_proto_chain_field_noting_miss` — is `obj`'s
/// recorded prototype. That prototype was then already read in full and
/// answered `undefined`, so `resolve_inherited_field(obj, key)` would repeat
/// the read and cannot find anything it did not.
pub(super) fn static_prototype_already_read(obj: *const ObjectHeader, read_miss: u64) -> bool {
    if read_miss == 0 {
        return false;
    }
    let Some(proto_bits) = crate::object::prototype_chain::object_static_prototype(obj as usize)
    else {
        return false;
    };
    let noted = JSValue::from_bits(read_miss);
    let proto = JSValue::from_bits(proto_bits);
    proto.is_pointer() && noted.is_pointer() && proto.as_pointer::<u8>() == noted.as_pointer::<u8>()
}
