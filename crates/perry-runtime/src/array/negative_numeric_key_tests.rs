//! #13687: missing negative keys need neither formatting nor a property walk
//! when the receiver and both intrinsic prototypes prove their absence.
use super::*;
use crate::object::builtin_proto::{self, BuiltinProto};
use crate::value::TAG_UNDEFINED;

fn read(arr: *const ArrayHeader, key: f64) -> f64 {
    js_array_get_index_or_string(arr, key)
}

#[test]
fn negative_key_proof_requires_an_ordinary_array_without_own_named_properties() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let mut arr = js_array_alloc(4);
    arr = js_array_push_f64(arr, 19.0);
    assert!(negative_integer_key_is_absent(arr));
    for key in [
        -1.0,
        -4294967296.0,
        -1e21,
        f64::from_bits(crate::JSValue::int32(-1).bits()),
    ] {
        assert_eq!(read(arr, key).to_bits(), TAG_UNDEFINED);
    }
    assert_eq!(read(arr, -0.0), 19.0);
    let key = crate::string::js_string_from_bytes(b"-1".as_ptr(), 2);
    arr = js_array_set_string_key(arr, key, 37.0);
    assert!(!negative_integer_key_is_absent(arr));
    assert_eq!(read(arr, -1.0), 37.0);
    assert_eq!(read(arr, -2.0).to_bits(), TAG_UNDEFINED);

    let object = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_field_by_name(object, key, 41.0);
    assert!(!negative_integer_key_is_absent(object.cast()));
    assert_eq!(read(object.cast(), -1.0), 41.0);
    let string = crate::string::js_string_from_bytes(b"abc".as_ptr(), 3);
    assert!(!negative_integer_key_is_absent(string.cast()));
    assert!(!negative_integer_key_is_absent(std::ptr::null()));
}

#[test]
fn negative_key_proof_rechecks_custom_prototype_after_a_successful_read() {
    let _no_move = crate::gc::GcSuppressScope::new();
    let arr = js_array_alloc(4);
    assert!(negative_integer_key_is_absent(arr));
    assert_eq!(read(arr, -1.0).to_bits(), TAG_UNDEFINED);
    let proto = crate::object::js_object_alloc(0, 0);
    let key = crate::string::js_string_from_bytes(b"-1".as_ptr(), 2);
    crate::object::js_object_set_field_by_name(proto, key, 43.0);
    crate::object::js_object_set_prototype_of(
        crate::value::js_nanbox_pointer(arr as i64),
        crate::value::js_nanbox_pointer(proto as i64),
    );
    assert!(!negative_integer_key_is_absent(arr));
    assert_eq!(read(arr, -1.0), 43.0);
}

fn intrinsic_named_write_withdraws_proof(family: BuiltinProto) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let arr = js_array_alloc(4);
    struct RestoreIntrinsic {
        family: BuiltinProto,
        original: *const crate::object::ObjectHeader,
    }
    impl Drop for RestoreIntrinsic {
        fn drop(&mut self) {
            crate::agent_ptrs::publish_intrinsic_proto(
                self.family as usize,
                self.original.cast_mut(),
            );
        }
    }
    let _restore = RestoreIntrinsic {
        family,
        original: crate::agent_ptrs::intrinsic_proto(family as usize),
    };
    let proto = crate::object::js_object_alloc(0, 0);
    unsafe { builtin_proto::seal_object(proto, family) };
    crate::agent_ptrs::publish_intrinsic_proto(family as usize, proto);
    assert!(negative_integer_key_is_absent(arr));
    let key = crate::string::js_string_from_bytes(b"-1".as_ptr(), 2);
    crate::object::js_object_set_field_by_name(proto, key, 47.0);
    assert!(!builtin_proto::pristine(BuiltinProto::Array));
    assert!(!negative_integer_key_is_absent(arr));
}

#[test]
fn negative_key_proof_rechecks_array_prototype_named_writes() {
    intrinsic_named_write_withdraws_proof(BuiltinProto::Array);
}

#[test]
fn negative_key_proof_rechecks_object_prototype_named_writes() {
    intrinsic_named_write_withdraws_proof(BuiltinProto::Object);
}

#[test]
fn negative_key_proof_resolves_growth_and_completed_copying_evacuation() {
    let _copying = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _verify = crate::gc::knob_overrides::VerifyEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let initial = js_array_alloc(0);
    let mut head = initial;
    for i in 0..65 {
        head = js_array_push_f64(head, i as f64);
    }
    assert_ne!(head, initial);
    assert!(negative_integer_key_is_absent(initial));
    let scope = crate::gc::RuntimeHandleScope::new();
    let root = scope.root_raw_mut_ptr(head);
    let alias = scope.root_raw_const_ptr(initial);
    let cycles = crate::gc::copying_minor_cycles();
    let (_, moved) = root.across_mut::<ArrayHeader, _>(|| crate::gc::gc_collect_minor());
    assert!(crate::gc::copying_minor_cycles() > cycles);
    assert_ne!(moved, head, "the rooted live receiver must actually move");
    let relocated_alias = alias.get_raw_const_ptr::<ArrayHeader>();
    assert!(negative_integer_key_is_absent(relocated_alias));
    assert!(negative_integer_key_is_absent(moved));
    assert_eq!(read(relocated_alias, -1.0).to_bits(), TAG_UNDEFINED);
    // A named write after relocation must invalidate the receiver proof,
    // including reads through the original growth stub.
    let (key, moved) = root
        .across_mut::<ArrayHeader, _>(|| crate::string::js_string_from_bytes(b"-1".as_ptr(), 2));
    let updated = js_array_set_string_key(moved, key, 53.0);
    assert!(!negative_integer_key_is_absent(updated));
    let relocated_alias = alias.get_raw_const_ptr::<ArrayHeader>();
    assert!(!negative_integer_key_is_absent(relocated_alias));
    assert_eq!(read(relocated_alias, -1.0), 53.0);
}
