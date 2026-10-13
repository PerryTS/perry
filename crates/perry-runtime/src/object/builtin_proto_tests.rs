use super::*;

#[test]
fn the_codegen_mirror_matches_the_family_bytes() {
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

#[test]
fn a_sealed_object_withdraws_its_family_on_a_slot_store_and_never_reopens() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 2);
        crate::object::store_object_field_slot(proto, 0, 7.0f64.to_bits());
        assert!(pristine(BuiltinProto::Map), "starts pristine");
        seal_object(proto, BuiltinProto::Map);
        assert!(pristine(BuiltinProto::Map), "sealing is not a write");
        // A same-value store still withdraws the fact.
        crate::object::store_object_field_slot(proto, 0, 7.0f64.to_bits());
        assert!(!pristine(BuiltinProto::Map));
        assert!(pristine(BuiltinProto::Set), "other families stay pristine");
        assert!(!all_pristine());
        crate::object::store_object_field_slot(proto, 0, 8.0f64.to_bits());
        assert!(!pristine(BuiltinProto::Map), "set-only");
    }
}

#[test]
fn object_prototype_withdraws_every_family() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 1);
        seal_object(proto, BuiltinProto::Object);
        crate::object::store_object_field_slot(proto, 0, 1.0f64.to_bits());
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
        let obj = crate::object::js_object_alloc(0, 2);
        crate::object::store_object_field_slot(obj, 0, 1.0f64.to_bits());
        crate::object::store_object_field_slot(obj, 1, 2.0f64.to_bits());
        assert!(all_pristine());
    }
}

#[test]
fn a_builtin_definition_does_not_withdraw_the_fact() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let proto = crate::object::js_object_alloc(0, 2);
        seal_object(proto, BuiltinProto::Date);
        crate::object::own_override::as_builtin_definition(|| {
            crate::object::store_object_field_slot(proto, 0, 3.0f64.to_bits());
        });
        assert!(pristine(BuiltinProto::Date));
    }
}

#[test]
fn a_shape_fact_dies_when_a_prototype_method_is_replaced_in_place() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    use crate::object::global_this_builtin_noop_thunk;
    unsafe {
        let proto = crate::object::js_object_alloc(0, 2);
        let _ = crate::object::proto_validity::mark_object_as_prototype(proto as usize);
        let f1 =
            crate::closure::js_closure_alloc(crate::fn_info!(global_this_builtin_noop_thunk, 1), 0);
        let f2 =
            crate::closure::js_closure_alloc(crate::fn_info!(global_this_builtin_noop_thunk, 1), 0);
        let m1 = crate::value::js_nanbox_pointer(f1 as i64).to_bits();
        let m2 = crate::value::js_nanbox_pointer(f2 as i64).to_bits();
        crate::object::store_object_field_slot(proto, 0, m1);
        let word = receiver_word(proto);
        let fact = PristineReceiverFact::Shape {
            receiver_word: word,
            proof_word: word,
            proof_epoch: proof_epoch(),
        };
        assert!(fact.holds());
        // A data store on the prototype does not move the epoch.
        crate::object::store_object_field_slot(proto, 1, 5.0f64.to_bits());
        assert!(fact.holds(), "a data overwrite keeps the proof");
        // Replacing the method in place, on the same shape, does.
        crate::object::store_object_field_slot(proto, 0, m2);
        assert_eq!(receiver_word(proto), word, "the shape did not change");
        assert!(!fact.holds(), "a replaced method kills the proof");
    }
}
