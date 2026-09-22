//! #10481: a Symbol-keyed accessor found on a PROTOTYPE runs with the original
//! receiver as `this`, for `[[Get]]` and `[[Set]]` alike.

use super::*;
use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use std::cell::Cell;

thread_local! {
    static SETTER_CALL: Cell<Option<(u64, u64)>> = const { Cell::new(None) };
}

/// Getter body: answers the `this` it was invoked with.
extern "C" fn this_getter(_closure: *const ClosureHeader) -> f64 {
    crate::object::js_implicit_this_get()
}

/// Setter body: records `(this, value)`.
extern "C" fn recording_setter(_closure: *const ClosureHeader, value: f64) -> f64 {
    let this = crate::object::js_implicit_this_get();
    SETTER_CALL.with(|c| c.set(Some((this.to_bits(), value.to_bits()))));
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn closure_bits(f: *const u8, arity: u32) -> u64 {
    js_register_closure_arity(f, arity);
    crate::value::js_nanbox_pointer(js_closure_alloc(f, 0) as i64).to_bits()
}

unsafe fn plain_object() -> f64 {
    crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64)
}

/// `proto` carrying a `[sym]` accessor, `child = Object.create(proto)` and
/// `grandchild = Object.create(child)`.
unsafe fn fixture() -> (f64, f64, f64, f64) {
    let sym = super::constructors::js_symbol_new_empty();
    let proto = plain_object();
    set_symbol_accessor_property(
        proto,
        sym,
        closure_bits(this_getter as *const u8, 0),
        closure_bits(recording_setter as *const u8, 1),
    );
    let child = crate::object::js_object_create(proto);
    let grandchild = crate::object::js_object_create(child);
    (sym, proto, child, grandchild)
}

#[test]
fn inherited_symbol_getter_receives_the_original_receiver() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        crate::gc::gc_suppress();
        let (sym, proto, child, grandchild) = fixture();
        let own = js_object_get_symbol_property(proto, sym);
        let one_up = js_object_get_symbol_property(child, sym);
        let two_up = js_object_get_symbol_property(grandchild, sym);
        let other = plain_object();
        let reflected = js_object_get_symbol_property_with_receiver(grandchild, sym, other);
        let tag = inherited_symbol_property(child, sym);
        crate::gc::gc_unsuppress();

        assert_eq!(
            own.to_bits(),
            proto.to_bits(),
            "an own accessor read on the holder itself sees the holder as `this`"
        );
        assert_eq!(
            one_up.to_bits(),
            child.to_bits(),
            "a one-level-inherited getter must see the ORIGINAL receiver, not the prototype that holds it"
        );
        assert_eq!(
            two_up.to_bits(),
            grandchild.to_bits(),
            "a two-level-inherited getter must still see the original receiver"
        );
        assert_eq!(
            reflected.to_bits(),
            other.to_bits(),
            "an explicit receiver (Reflect.get-shaped call) must reach the getter unchanged"
        );
        assert_eq!(
            tag.unwrap().to_bits(),
            child.to_bits(),
            "inherited_symbol_property (the Symbol.toStringTag walk) runs the getter with `this === obj`"
        );
    }
}

#[test]
fn inherited_symbol_setter_receives_the_receiver_and_the_value() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        crate::gc::gc_suppress();
        let (sym, proto, child, grandchild) = fixture();
        let value = 42.0_f64;

        SETTER_CALL.with(|c| c.set(None));
        let ret = js_object_set_symbol_property(child, sym, value);
        let recorded_child = SETTER_CALL.with(|c| c.get());

        SETTER_CALL.with(|c| c.set(None));
        let value2 = 43.0_f64;
        js_object_set_symbol_property(grandchild, sym, value2);
        let recorded_grandchild = SETTER_CALL.with(|c| c.get());

        SETTER_CALL.with(|c| c.set(None));
        let value3 = 44.0_f64;
        js_object_set_symbol_property(proto, sym, value3);
        let recorded_own = SETTER_CALL.with(|c| c.get());
        crate::gc::gc_unsuppress();

        assert_eq!(
            ret.to_bits(),
            value.to_bits(),
            "the setter's return value is the assigned value"
        );
        assert_eq!(
            recorded_child,
            Some((child.to_bits(), value.to_bits())),
            "a one-level-inherited setter must run with the write's receiver, not the holder"
        );
        assert_eq!(
            recorded_grandchild,
            Some((grandchild.to_bits(), value2.to_bits())),
            "a two-level-inherited setter must still run with the original receiver"
        );
        assert_eq!(
            recorded_own,
            Some((proto.to_bits(), value3.to_bits())),
            "an own accessor write runs with the object written to as `this`"
        );
    }
}

#[test]
fn own_data_property_shadows_an_inherited_accessor_for_read_and_write() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        crate::gc::gc_suppress();
        let (sym, _proto, child, _grandchild) = fixture();
        let data_value = 99.0_f64;
        define_symbol_data_property(child, sym, data_value);

        SETTER_CALL.with(|c| c.set(None));
        let read = js_object_get_symbol_property(child, sym);
        let read_did_not_invoke_getter = SETTER_CALL.with(|c| c.get()).is_none();

        let new_value = 100.0_f64;
        let written = js_object_set_symbol_property(child, sym, new_value);
        let write_invoked_setter = SETTER_CALL.with(|c| c.get()).is_some();
        let read_after = js_object_get_symbol_property(child, sym);
        crate::gc::gc_unsuppress();

        assert_eq!(
            read.to_bits(),
            data_value.to_bits(),
            "a nearer own DATA property must shadow the inherited accessor on read"
        );
        assert!(
            read_did_not_invoke_getter,
            "reading a shadowing own data property must not run the inherited getter"
        );
        assert_eq!(
            written.to_bits(),
            new_value.to_bits(),
            "writing a shadowed key returns the assigned value"
        );
        assert!(
            !write_invoked_setter,
            "writing a nearer own data property must not run the inherited setter"
        );
        assert_eq!(
            read_after.to_bits(),
            new_value.to_bits(),
            "the own data property must be updated in place, not routed to the inherited accessor"
        );
    }
}

#[test]
fn symbol_may_have_accessor_is_false_until_an_accessor_is_installed() {
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        crate::gc::gc_suppress();
        let sym = super::constructors::js_symbol_new_empty();
        let sym_key = sym_key_from_f64(sym);
        let before = super::accessors::symbol_may_have_accessor(sym_key);
        let bits_before = super::accessors::test_symbol_accessor_id_bits_set();

        let holder = plain_object();
        set_symbol_accessor_property(holder, sym, closure_bits(this_getter as *const u8, 0), 0);
        let after = super::accessors::symbol_may_have_accessor(sym_key);
        let bits_after = super::accessors::test_symbol_accessor_id_bits_set();
        crate::gc::gc_unsuppress();

        assert!(
            !before,
            "a freshly minted symbol must not read as accessor-bearing before any accessor exists"
        );
        assert!(
            after,
            "installing an accessor under a symbol must flip its filter bit"
        );
        assert!(
            bits_after >= bits_before,
            "the filter's population count must never decrease"
        );
    }
}
