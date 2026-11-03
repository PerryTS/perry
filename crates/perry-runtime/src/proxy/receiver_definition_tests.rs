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

per_test_global! {
    static DELETE_TRAP_KEY_WAS_STRING: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
}

extern "C" fn delete_trap_records_key(
    _: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _target: f64,
    key: f64,
) -> f64 {
    let tag = key.to_bits() & crate::value::TAG_MASK;
    DELETE_TRAP_KEY_WAS_STRING.store(
        tag == crate::value::STRING_TAG || tag == crate::value::SHORT_STRING_TAG,
        std::sync::atomic::Ordering::SeqCst,
    );
    f64::from_bits(TAG_TRUE)
}

/// `delete proxy[0]` lowers with the raw index: the deleteProperty trap must
/// still receive the property key "0" (ToPropertyKey), like get/set traps
/// (Web IDL observable arrays compare the key as an array-index string).
#[test]
fn delete_trap_receives_a_property_key_for_a_numeric_index() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::gc::gc_init();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let handler = object();
    let trap = crate::closure::js_closure_alloc(crate::fn_info!(delete_trap_records_key, 2), 0);
    let trap = crate::value::js_nanbox_pointer(trap as i64);
    crate::object::property_ops::reflect::js_reflect_set(
        handler,
        key(b"deleteProperty"),
        trap,
        handler,
    );
    let proxy = js_proxy_new(object(), handler);
    DELETE_TRAP_KEY_WAS_STRING.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(js_proxy_delete(proxy, 0.0).to_bits(), TAG_TRUE);
    assert!(DELETE_TRAP_KEY_WAS_STRING.load(std::sync::atomic::Ordering::SeqCst));
}
