use super::*;
use crate::value::JSValue;

fn object() -> f64 {
    crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64)
}

fn key(name: &[u8]) -> f64 {
    crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        name.as_ptr(),
        name.len() as u32,
    ) as i64)
}

fn read(target: f64, key: f64) -> f64 {
    super::reflect::js_reflect_get(target, key, target)
}

#[test]
fn new_property_on_receiver_with_proxy_prototype_is_defined_once() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = object();
    let handler = object();
    let proxy = js_proxy_new(target, handler);
    let nested = js_proxy_new(proxy, handler);
    let name = key(b"new_property");
    for prototype in [proxy, nested] {
        let receiver = crate::object::js_object_create(prototype);
        assert!(
            crate::value::js_is_truthy(reflect_ordinary_set_with_receiver(
                prototype, name, 42.0, receiver,
            )) != 0
        );
        assert_eq!(read(receiver, name), 42.0);
        assert!(JSValue::from_bits(read(target, name).to_bits()).is_undefined());
        let descriptor = js_reflect_get_own_property_descriptor(receiver, name);
        for flag in [b"writable".as_slice(), b"enumerable", b"configurable"] {
            assert_eq!(read(descriptor, key(flag)).to_bits(), TAG_TRUE);
        }
        assert_eq!(read(descriptor, key(b"value")), 42.0);
        // Updating the newly created own property must keep its attributes.
        assert!(ordinary_set_with_receiver(receiver, name, 43.0, receiver));
        assert_eq!(read(receiver, name), 43.0);
    }
}

#[test]
fn proxy_prototype_does_not_allow_creation_on_nonextensible_receiver() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let proxy = js_proxy_new(object(), object());
    let receiver = crate::object::js_object_create(proxy);
    js_reflect_prevent_extensions(receiver);
    let name = key(b"new_property");
    assert!(!ordinary_set_with_receiver(proxy, name, 42.0, receiver));
    assert!(JSValue::from_bits(read(receiver, name).to_bits()).is_undefined());
}
