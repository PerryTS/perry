
use super::*;

#[test]
fn only_object_and_null_parent_prototypes_are_valid() {
    let object_ptr = crate::object::js_object_alloc(0, 0);
    assert!(!object_ptr.is_null());
    let object = crate::value::js_nanbox_pointer(object_ptr as i64);
    assert_eq!(class_parent_prototype_bits(object), Some(object.to_bits()));
    assert_eq!(
        class_parent_prototype_bits(f64::from_bits(crate::value::POINTER_TAG | 0x1234)),
        None
    );
    assert_eq!(
        class_parent_prototype_bits(f64::from_bits(crate::value::TAG_NULL)),
        Some(crate::value::TAG_NULL)
    );
    assert_eq!(
        class_parent_prototype_bits(f64::from_bits(crate::value::TAG_UNDEFINED)),
        None
    );
    assert_eq!(class_parent_prototype_bits(1.0), None);
}

/// #8760: a `class X extends <bound native EventEmitter export>` registered
/// through the dynamic-parent path (`import { EventEmitter as EE } from
/// "events"; class X extends EE {}` — cli.js 2.1.112's exact shape) must
/// link its declared prototype to EventEmitter's real, lazily-materialized
/// prototype instead of throwing "Class extends value does not have valid
/// prototype property". The raw `prototype` dynamic slot on the bound export
/// closure may be `undefined` (it is not any more: the slot already reads as
/// a pointer), so the resolution must work off the same value an
/// ordinary `EE.prototype` read uses.
#[test]
fn native_constructor_export_parent_links_declared_prototype() {
    const ISSUE_8760_CLASS_ID: u32 = 0x7d01_8760;
    const CLASS_NAME: &[u8] = b"Issue8760Subclass";

    // The bound `events.EventEmitter` export — a BOUND_METHOD closure whose
    // `.prototype` object is materialized on demand, not at mint time.
    let ee_ctor = crate::object::bound_native_callable_export_value("events", "EventEmitter");

    // The raw dynamic `prototype` slot used to be undefined here, which is
    // what made the dynamic-parent registration throw before the fix. The
    // slot now already reads as a pointer, so that precondition no
    // longer exists; the registration and the link below are what remain
    // to guard, whichever way the prototype is produced.
    unsafe {
        js_register_class_name(
            ISSUE_8760_CLASS_ID,
            CLASS_NAME.as_ptr(),
            CLASS_NAME.len() as u32,
        );
    }
    js_register_class_parent_dynamic(ISSUE_8760_CLASS_ID, ee_ctor);

    // Must not throw, and must materialize a real prototype object.
    let decl_proto = class_decl_prototype_value(ISSUE_8760_CLASS_ID);
    assert_eq!(
        decl_proto.to_bits() & crate::value::TAG_MASK,
        crate::value::POINTER_TAG,
        "the subclass prototype must materialize (no TypeError) for a native constructor parent"
    );

    // Its [[Prototype]] must be EventEmitter's canonical prototype — the same
    // object an ordinary `EE.prototype` read resolves to — so `instanceof`
    // and inherited `emit`/`on` work through the chain.
    let ee_proto = super::function_prototype::ordinary_function_prototype_value_for_read(
        crate::object::bound_native_callable_export_value("events", "EventEmitter"),
    )
    .expect("EventEmitter export must expose a prototype object");
    let decl_proto_addr = (decl_proto.to_bits() & crate::value::POINTER_MASK) as usize;
    let linked = super::super::prototype_chain::object_static_prototype(decl_proto_addr)
        .expect("the subclass prototype must have a linked [[Prototype]]");
    assert_eq!(
        linked & crate::value::POINTER_MASK,
        ee_proto.to_bits() & crate::value::POINTER_MASK,
        "the subclass prototype must inherit from EventEmitter.prototype"
    );
}
