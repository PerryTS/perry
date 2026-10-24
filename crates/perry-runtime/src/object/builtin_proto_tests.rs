use super::*;

#[test]
fn the_codegen_mirror_matches_the_family_slots() {
    // `perry-codegen/src/builtin_proto.rs` mirrors these discriminants.
    let expected: [(BuiltinProto, u8); FAMILIES] = [
        (BuiltinProto::Object, 0),
        (BuiltinProto::Function, 1),
        (BuiltinProto::Array, 2),
        (BuiltinProto::String, 3),
        (BuiltinProto::Number, 4),
        (BuiltinProto::Boolean, 5),
        (BuiltinProto::Symbol, 6),
        (BuiltinProto::BigInt, 7),
        (BuiltinProto::Map, 8),
        (BuiltinProto::Set, 9),
        (BuiltinProto::Date, 10),
        (BuiltinProto::RegExp, 11),
        (BuiltinProto::Promise, 12),
        (BuiltinProto::Error, 13),
        (BuiltinProto::WeakMap, 14),
        (BuiltinProto::WeakSet, 15),
    ];
    for (family, byte) in expected {
        assert_eq!(family as u8, byte);
        assert_eq!(BuiltinProto::ALL[byte as usize].0, family);
    }
}

/// A stand-in intrinsic of `family` for this test thread: an ordinary,
/// shaped object, sealed and published in the agent slot the fact reads.
unsafe fn sealed(family: BuiltinProto) -> *mut ObjectHeader {
    let proto = crate::object::js_object_alloc(0, 2);
    let key = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(proto, key, 1.0);
    assert!(
        crate::object::shapes::is_site_matchable_layout_word((*proto).parent_class_id),
        "premise: an ordinary layout word"
    );
    seal_object(proto, family);
    crate::agent_ptrs::publish_intrinsic_proto(family as usize, proto);
    proto
}

#[test]
fn a_sealed_object_withdraws_its_family_on_a_slot_store_and_never_reopens() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        assert!(pristine(BuiltinProto::Map), "starts pristine");
        let proto = sealed(BuiltinProto::Map);
        let set = sealed(BuiltinProto::Set);
        assert!(pristine(BuiltinProto::Map), "sealing is not a write");
        // A same-value store still withdraws the fact.
        crate::object::store_object_field_slot(proto, 0, 1.0f64.to_bits());
        assert!(!pristine(BuiltinProto::Map));
        assert!(pristine(BuiltinProto::Set), "other families stay pristine");
        assert!(super::super::layout_desc::word_is_shadowed(
            (*proto).parent_class_id
        ));
        assert!(!super::super::layout_desc::word_is_shadowed(
            (*set).parent_class_id
        ));
        crate::object::store_object_field_slot(proto, 0, 8.0f64.to_bits());
        assert!(!pristine(BuiltinProto::Map), "set-only");
    }
}

#[test]
fn object_prototype_withdraws_every_family() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let protos: Vec<_> = BuiltinProto::ALL.iter().map(|(f, _)| sealed(*f)).collect();
        crate::object::store_object_field_slot(protos[0], 0, 2.0f64.to_bits());
        for (family, _) in BuiltinProto::ALL {
            assert!(!pristine(family), "{family:?} inherits Object.prototype");
        }
    }
}

#[test]
fn an_unsealed_object_never_touches_the_fact() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let _ = sealed(BuiltinProto::Date);
        let obj = crate::object::js_object_alloc(0, 2);
        crate::object::store_object_field_slot(obj, 0, 1.0f64.to_bits());
        crate::object::store_object_field_slot(obj, 1, 2.0f64.to_bits());
        for (family, _) in BuiltinProto::ALL {
            assert!(pristine(family), "{family:?}");
        }
    }
}

#[test]
fn a_builtin_definition_does_not_withdraw_the_fact() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = sealed(BuiltinProto::Date);
        crate::object::own_override::as_builtin_definition(|| {
            crate::object::store_object_field_slot(proto, 0, 3.0f64.to_bits());
        });
        assert!(pristine(BuiltinProto::Date));
    }
}

extern "C" fn function_receiver_fixture(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    1.0
}

/// The replacement returns its own receiver's bits, so the assertion can see
/// both that it ran and which `this` it received.
extern "C" fn replaced_prototype_method_fixture(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    this.as_f64()
}

/// #9202 (`globals/function-tostring-override`): the generic `[[Get]]` the
/// failed-`Pristine` redirect performs cannot see a FUNCTION receiver's
/// method surface — `js_object_get_field_by_name_f64`'s closure arms report
/// `toString`/`valueOf`/`isPrototypeOf` absent because dedicated dispatch owns
/// them — so a program that replaces a method on the function's prototype made
/// `fn.method()` throw `TypeError: value is not a function` instead of calling
/// the replacement. `spec_get`'s function-receiver fallback must resolve the
/// name off the receiver's real `[[Prototype]]`.
#[test]
fn a_replaced_prototype_method_is_called_on_a_function_receiver() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(function_receiver_fixture, 0), 0)
                as i64,
        ));
        let proto = crate::object::js_object_alloc(0, 1);
        let replacement = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(
                crate::fn_info!(replaced_prototype_method_fixture, 0),
                0,
            ) as i64,
        ));
        let key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
        crate::object::js_object_set_field_by_name(proto, key, replacement.get_nanbox_f64());
        crate::object::js_object_set_prototype_of(
            receiver.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(proto as i64),
        );

        // Premise: the receiver IS a function, and the plain object it
        // inherits from really holds the replacement — a vacuous test below
        // would otherwise pass on a missing method.
        assert!(
            crate::closure::is_closure_ptr(crate::value::js_nanbox_get_pointer(
                receiver.get_nanbox_f64()
            ) as usize),
            "premise: the receiver must be a function"
        );
        assert_eq!(
            crate::object::js_object_get_field_by_name_f64(
                proto as *const crate::object::ObjectHeader,
                key
            )
            .to_bits(),
            replacement.get_nanbox_u64(),
            "premise: the prototype owns the replacement"
        );

        // The `Get` the redirect performs must answer with the replacement.
        let selected =
            crate::object::method_call::spec_get_value(receiver.get_nanbox_f64(), b"toString");
        assert_eq!(
            selected.to_bits(),
            replacement.get_nanbox_u64(),
            "the redirect's Get must see the inherited method on a function"
        );

        // And the redirect must call it with the function as `this`.
        let called = redirect_call(receiver.get_nanbox_f64(), b"toString", std::ptr::null(), 0);
        assert_eq!(
            called.map(|value| value.to_bits()),
            Some(receiver.get_nanbox_u64()),
            "the replacement must run with the function as `this`"
        );
    }
}

/// A detached function (`Object.setPrototypeOf(fn, null)`) has no inherited
/// method surface, so the fallback must find nothing there and leave the
/// redirect's non-callable verdict intact.
#[test]
fn a_detached_function_has_no_inherited_method_for_the_redirect() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(function_receiver_fixture, 0), 0)
                as i64,
        ));
        crate::object::js_object_set_prototype_of(
            receiver.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_NULL),
        );
        assert_eq!(
            crate::object::method_call::spec_get_value(receiver.get_nanbox_f64(), b"toString")
                .to_bits(),
            crate::value::TAG_UNDEFINED,
            "a detached function inherits no method"
        );
    }
}
