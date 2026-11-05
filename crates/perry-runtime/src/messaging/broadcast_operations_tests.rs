use super::*;

#[test]
fn fallback_private_name_and_closed_identity_survive_public_shadowing() {
    crate::gc::gc_init();
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(f64::from_bits(JSValue::string_ptr(key("fallback")).bits()));
    let channel =
        scope.root_raw_mut_ptr(broadcast_channel_object_with_id(0, name.get_nanbox_f64()));
    let prototype = scope.root_raw_mut_ptr(object::js_object_alloc(0, 0));
    install_core_methods(prototype.get_raw_mut_ptr());
    set_object_prototype(
        channel.get_raw_mut_ptr(),
        boxed_object(prototype.get_raw_mut_ptr()),
    );
    let receiver = || JsThis::from_f64(boxed_object(channel.get_raw_mut_ptr()));
    let name_key = key("name");
    assert_eq!(
        object::js_object_get_field_by_name_f64(channel.get_raw_mut_ptr(), name_key).to_bits(),
        name.get_nanbox_f64().to_bits()
    );
    assert_eq!(broadcast_channel_id(receiver().as_f64()), Some(0));
    for action in [ref_channel, unref_channel] {
        assert_eq!(
            action(std::ptr::null(), receiver()).to_bits(),
            receiver().as_f64().to_bits()
        );
    }
    let derived = scope.root_raw_mut_ptr(object::js_object_alloc(0, 0));
    set_object_prototype(derived.get_raw_mut_ptr(), receiver().as_f64());
    let derived_receiver = || JsThis::from_f64(boxed_object(derived.get_raw_mut_ptr()));
    assert_eq!(
        name_getter(std::ptr::null(), derived_receiver()).to_bits(),
        name.get_nanbox_f64().to_bits()
    );
    assert_eq!(
        ref_channel(std::ptr::null(), derived_receiver()).to_bits(),
        derived_receiver().as_f64().to_bits()
    );
    close(std::ptr::null(), receiver());
    close(std::ptr::null(), receiver());
    assert_eq!(
        name_getter(std::ptr::null(), receiver()).to_bits(),
        name.get_nanbox_f64().to_bits()
    );
    assert_eq!(
        CLOSED.read(receiver().as_f64()).map(f64::to_bits),
        Some(JSValue::bool(true).bits())
    );
    let fake = scope.root_raw_mut_ptr(object::js_object_alloc(0, 0));
    let spelling = key("#<perry:private-value:0:@0:[[BroadcastChannelId]]>");
    object::js_object_set_field_by_name(fake.get_raw_mut_ptr(), spelling, 0.0);
    assert_eq!(
        broadcast_channel_id(boxed_object(fake.get_raw_mut_ptr())),
        None
    );
    assert!(crate::exception::catch_js_throw(|| name_getter(
        std::ptr::null(),
        JsThis::from_f64(boxed_object(fake.get_raw_mut_ptr()))
    ))
    .is_err());
}

#[test]
fn private_name_keeps_the_original_boxed_string() {
    crate::gc::gc_init();
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(f64::from_bits(
        JSValue::string_ptr(crate::string::js_string_from_bytes(
            [0xed, 0xa0, 0x80].as_ptr(),
            3,
        ))
        .bits(),
    ));
    let channel =
        scope.root_raw_mut_ptr(broadcast_channel_object_with_id(0, name.get_nanbox_f64()));
    let actual = name_getter(
        std::ptr::null(),
        JsThis::from_f64(boxed_object(channel.get_raw_mut_ptr())),
    );
    assert_eq!(actual.to_bits(), name.get_nanbox_f64().to_bits());
}
