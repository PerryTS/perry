//! #11201: class members reached through an ordinary object whose class id is
//! 0 — `Object.setPrototypeOf(Object.create(null), C.prototype)`, and every
//! `Object.create`d object once `Object.create` stops minting a synthetic
//! class id per call (#11166).
//!
//! * A class method VALUE (`C.prototype.m`, owner marker captured in slot 0)
//!   ran with the INT32 marker as `this` (`typeof this === "number"`), because
//!   `canonical_bound_method_receiver` only accepted receivers with a class id.
//! * An inherited class SETTER was skipped: the `[[Set]]` walk does not model
//!   vtable accessors, and the receiver-keyed vtable walk in
//!   `js_object_set_field_by_name` has no class id to start from.

use super::*;

const RECEIVER_TEST_CLASS_ID: u32 = 0x0011_2010;
const METHOD: &[u8] = b"whoAmI";
const ACCESSOR: &[u8] = b"level";

extern "C" fn return_receiver_11201(this: f64) -> f64 {
    this
}

thread_local! {
    // Per test thread, so no sibling test can observe or clobber it.
    static SETTER_SAW: std::cell::Cell<(u64, f64)> = const { std::cell::Cell::new((0, 0.0)) };
}

extern "C" fn record_setter_11201(this: f64, value: f64) -> f64 {
    SETTER_SAW.with(|c| c.set((this.to_bits(), value)));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

struct Scope {
    _suppress: crate::gc::GcSuppressScope,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Scope {
    fn new() -> Self {
        Self {
            _lock: crate::gc::global_side_table_test_lock(),
            _suppress: crate::gc::GcSuppressScope::new(),
        }
    }
}

/// Register the class and return `(C.prototype.whoAmI, C.prototype)`.
unsafe fn register_class() -> (f64, f64) {
    js_register_class_id(RECEIVER_TEST_CLASS_ID);
    super::class_registry::js_register_class_method(
        RECEIVER_TEST_CLASS_ID as i64,
        METHOD.as_ptr(),
        METHOD.len() as i64,
        return_receiver_11201 as *const () as usize as i64,
        0,
        0,
        0,
    );
    super::class_registry::js_register_class_setter(
        RECEIVER_TEST_CLASS_ID as i64,
        ACCESSOR.as_ptr(),
        ACCESSOR.len() as i64,
        record_setter_11201 as *const () as usize as i64,
    );
    let proto_ref = super::native_module::class_prototype_ref_value(RECEIVER_TEST_CLASS_ID);
    let method =
        super::native_module::js_class_method_bind(proto_ref, METHOD.as_ptr(), METHOD.len());
    // `class_decl_prototype_value` materializes `C.prototype` only for a
    // named class, exactly as codegen registers one.
    const CLASS_NAME: &[u8] = b"Receiver11201";
    js_register_class_name(
        RECEIVER_TEST_CLASS_ID,
        CLASS_NAME.as_ptr(),
        CLASS_NAME.len() as u32,
    );
    let proto = super::class_registry::class_decl_prototype_value(RECEIVER_TEST_CLASS_ID);
    (method, proto)
}

/// `method.call(receiver)`: IMPLICIT_THIS bound to `receiver` for the call.
unsafe fn call_with_this(method: f64, receiver: f64) -> f64 {
    let prev = crate::object::js_implicit_this_set(receiver);
    let result = crate::closure::js_native_call_value(method, std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev);
    result
}

/// `Object.setPrototypeOf(Object.create(null), proto)`.
unsafe fn class_id_zero_object_on(proto: f64) -> f64 {
    let obj = js_object_alloc(0, 0);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    crate::object::js_object_set_prototype_of(value, proto);
    assert_eq!(
        js_object_get_class_id(obj),
        0,
        "fixture: the receiver must keep class id 0, the case under test"
    );
    value
}

#[test]
fn class_method_value_runs_with_a_class_id_zero_receiver_as_this() {
    let _scope = Scope::new();
    unsafe {
        let (method, proto) = register_class();
        assert!(
            JSValue::from_bits(proto.to_bits()).is_pointer(),
            "fixture: C.prototype must be a heap object"
        );

        let inheriting = class_id_zero_object_on(proto);
        assert_eq!(
            call_with_this(method, inheriting).to_bits(),
            inheriting.to_bits(),
            "a class-id-0 object inheriting C.prototype must be `this`, not the owner marker"
        );

        // `C.prototype.m.call(o)` on an ordinary object that does not inherit
        // from C: `this` is still the receiver.
        let stranger = js_object_alloc(0, 0);
        let stranger_value = crate::value::js_nanbox_pointer(stranger as i64);
        assert_eq!(
            call_with_this(method, stranger_value).to_bits(),
            stranger_value.to_bits()
        );

        // Fast path unchanged: a genuine instance is `this`.
        let instance = js_object_alloc(RECEIVER_TEST_CLASS_ID, 0);
        let instance_value = crate::value::js_nanbox_pointer(instance as i64);
        assert_eq!(
            call_with_this(method, instance_value).to_bits(),
            instance_value.to_bits()
        );

        // Strict class methods preserve primitive receivers too.
        let number = 7.0f64;
        assert_eq!(call_with_this(method, number).to_bits(), number.to_bits());
    }
}

#[test]
fn inherited_class_setter_runs_for_a_class_id_zero_receiver() {
    let _scope = Scope::new();
    unsafe {
        let (_method, proto) = register_class();
        let receiver = class_id_zero_object_on(proto);
        SETTER_SAW.with(|c| c.set((0, 0.0)));

        let key = crate::string::js_string_from_bytes(ACCESSOR.as_ptr(), ACCESSOR.len() as u32);
        let key_value = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
        crate::proxy::js_put_value_set(receiver, key_value, 3.0, receiver, 1);

        assert_eq!(
            SETTER_SAW.with(|c| c.get()),
            (receiver.to_bits(), 3.0),
            "the inherited class setter must run with the original receiver"
        );
        let obj = JSValue::from_bits(receiver.to_bits()).as_pointer::<ObjectHeader>();
        assert!(
            js_object_get_field_by_name(obj, key).is_undefined(),
            "the write must not create an own data property that shadows the setter"
        );
    }
}

#[test]
fn class_method_value_preserves_primitive_and_array_receivers() {
    let _scope = Scope::new();
    unsafe {
        let (method, _) = register_class();
        let text = crate::string::js_string_from_bytes(b"receiver".as_ptr(), 8);
        let array = crate::array::js_array_alloc(0);
        let receivers = [
            f64::from_bits(crate::value::TAG_UNDEFINED),
            f64::from_bits(crate::value::TAG_NULL),
            f64::from_bits(crate::value::TAG_TRUE),
            f64::from_bits(crate::value::TAG_FALSE),
            0.0,
            -0.0,
            7.0,
            f64::NAN,
            f64::from_bits(1),
            crate::value::js_nanbox_string(text as i64),
            crate::value::js_nanbox_pointer(array as i64),
        ];
        for receiver in receivers {
            assert_eq!(
                call_with_this(method, receiver).to_bits(),
                receiver.to_bits(),
                "strict class method must preserve receiver bits {:#x}",
                receiver.to_bits()
            );
        }
    }
}
