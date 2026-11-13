/// A Map or Set that took its own `[[Prototype]]` does not read its family's
/// built-ins: the receiver proof every folded site asks is false for it.
#[test]
fn collection_with_own_prototype_is_not_pristine() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    for (family, cell) in [
        (
            crate::object::builtin_proto::BuiltinProto::Map,
            crate::map::js_map_alloc(0) as i64,
        ),
        (
            crate::object::builtin_proto::BuiltinProto::Set,
            crate::set::js_set_alloc(0) as i64,
        ),
    ] {
        let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(cell));
        assert_eq!(
            crate::object::builtin_proto::receiver_pristine(receiver.get_nanbox_f64()),
            crate::object::builtin_proto::pristine(family),
            "premise: a fresh family {} receiver is answered by its family",
            family as usize
        );
        let proto = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        crate::object::js_object_set_prototype_of(
            receiver.get_nanbox_f64(),
            proto.get_nanbox_f64(),
        );
        assert!(
            !crate::object::builtin_proto::receiver_pristine(receiver.get_nanbox_f64()),
            "family {} with its own [[Prototype]] must not prove Pristine",
            family as usize
        );
    }
}
