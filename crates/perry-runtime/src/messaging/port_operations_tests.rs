use super::*;

#[test]
fn private_message_port_identity_survives_public_shadow_and_shape_changes() {
    crate::gc::gc_init();
    let scope = crate::gc::RuntimeHandleScope::new();
    let port = scope.root_raw_mut_ptr(message_port_object_with_id(17));
    let receiver =
        || crate::value::js_nanbox_pointer(port.get_raw_mut_ptr::<ObjectHeader>() as i64);
    assert_eq!(message_port_id(receiver()), Some(17));
    let spelling = key("#<perry:private-value:0:@0:[[MessagePortId]]>");
    object::js_object_set_field_by_name(port.get_raw_mut_ptr(), spelling, 33.0);
    assert_eq!(message_port_id(receiver()), Some(17));
    let fake = object::js_object_alloc(0, 0);
    object::js_object_set_field_by_name(fake, spelling, 17.0);
    assert_eq!(message_port_id(boxed_object(fake)), None);
    assert_eq!(message_port_id(js_undefined()), None);
}

#[test]
fn runtime_port_core_operations_preserve_closed_identity_without_retaining_state() {
    crate::gc::gc_init();
    let scope = crate::gc::RuntimeHandleScope::new();
    let first = scope.root_raw_mut_ptr(message_port_object_with_id(0));
    let second = scope.root_raw_mut_ptr(message_port_object_with_id(0));
    entangle_ports(first.get_raw_mut_ptr(), second.get_raw_mut_ptr());
    let receiver = |port: &crate::gc::RuntimeHandle<'_>| {
        JsThis::from_f64(boxed_object(port.get_raw_mut_ptr()))
    };
    assert_eq!(
        ref_port(std::ptr::null(), receiver(&first)).to_bits(),
        js_undefined().to_bits()
    );
    assert_eq!(
        has_ref_port(std::ptr::null(), receiver(&first)).to_bits(),
        js_bool(true).to_bits()
    );
    close(std::ptr::null(), receiver(&first), js_undefined());
    close(std::ptr::null(), receiver(&second), js_undefined());
    assert!(!with_port_states(|map| map.contains_key(
        &(first.get_raw_mut_ptr::<ObjectHeader>() as usize)
    )));
    start(std::ptr::null(), receiver(&first));
    assert_eq!(message_port_id(receiver(&first).as_f64()), Some(0));
    assert_eq!(
        has_ref_port(std::ptr::null(), receiver(&first)).to_bits(),
        js_bool(false).to_bits()
    );
    assert!(!with_port_states(|map| map.contains_key(
        &(first.get_raw_mut_ptr::<ObjectHeader>() as usize)
    )));
}
