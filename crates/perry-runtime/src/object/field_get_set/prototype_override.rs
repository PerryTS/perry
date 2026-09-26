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
    /// #11391: the recorded chain was walked and does not carry the key, AND
    /// it is not the chain the receiver's class id names any more, so the
    /// class-id prototype arms must not answer either. See
    /// `prototype_chain::class_default_prototype_superseded`.
    Superseded,
}

impl InheritedRead {
    /// Whether the recorded prototype chain has already been walked.
    pub(super) fn walked(&self) -> bool {
        !matches!(self, InheritedRead::NotWalked)
    }

    /// Whether the class-id prototype walk may still answer this read: false
    /// once the receiver's own `[[Prototype]]` has replaced the object that
    /// walk reads (#11391).
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
/// A non-`Hit` answer means either no override, or an override that does not
/// carry this key — in both cases the caller keeps its existing fallback.
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
    if individual && class_prototype_declares_own_getter(obj, key) {
        return InheritedRead::NotWalked;
    }
    if let Some(value) = crate::object::prototype_chain::resolve_inherited_field(obj as usize, key)
    {
        return InheritedRead::Hit(value);
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
        return InheritedRead::Hit(JSValue::undefined());
    }
    if superseded {
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

/// A class prototype object's ClassBody getters are not stored on the object:
/// they live only in its template's vtable, which the tail consults after this
/// override. A per-evaluation prototype (`ClassExprFresh`, #9502/#11043) also
/// carries an individual `[[Prototype]]`: the evaluated parent's prototype.
/// Walking that chain first let an ancestor's accessor shadow the class's own
/// one, so `class F extends Base { get type() {…} }` declared in a function
/// answered `new F().type` with `Base`'s getter. luxon's zones hit this
/// (`FixedOffsetZone.utcInstance.type` threw "Zone is an abstract class") once
/// an in-body `new FixedOffsetZone()` constructed through the evaluation (#11142).
///
/// Only the class's OWN vtable is consulted. An inherited getter must still come
/// from the evaluated heritage chain, which can differ between evaluations of
/// one template.
fn class_prototype_declares_own_getter(
    obj: *const ObjectHeader,
    key: *const crate::string::StringHeader,
) -> bool {
    let Some(class_id) =
        crate::object::class_registry::class_id_for_decl_prototype_object(obj as usize)
    else {
        return false;
    };
    let key_copy = unsafe { super::HeapKeyBytes::copy_of_key(key) };
    let Ok(name) = std::str::from_utf8(key_copy.as_bytes()) else {
        return false;
    };
    let Ok(guard) = crate::object::class_registry::CLASS_VTABLE_REGISTRY.read() else {
        return false;
    };
    guard
        .as_ref()
        .and_then(|registry| registry.get(&class_id))
        .is_some_and(|vtable| vtable.declares_getter(name))
}
