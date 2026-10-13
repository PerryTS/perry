//! Invalid UTF-8 (WTF-8 surrogate) method keys must survive method-call
//! dispatch, while ordinary UTF-8 names continue through the existing tower.

use super::*;

extern "C" fn return_this(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    this.as_f64()
}

fn string_key(bytes: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn assert_method_call_binds_receiver(name: &[u8]) {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 1) as i64,
    ));
    let key = scope.root_string_ptr(string_key(name));
    let method = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(return_this, 0; with_declared(0)), 0)
            as i64,
    ));
    key.with_const_ptr::<crate::StringHeader, _>(|key| {
        let object = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64())
            as *mut crate::object::ObjectHeader;
        crate::object::js_object_set_field_by_name(object, key, method.get_nanbox_f64());
    });

    let result = unsafe {
        super::js_native_call_method(
            receiver.get_nanbox_f64(),
            name.as_ptr().cast(),
            name.len(),
            std::ptr::null(),
            0,
        )
    };
    assert_eq!(
        result.to_bits(),
        receiver.get_nanbox_u64(),
        "method call for key {name:?} must preserve its receiver"
    );
}

fn runtime_throw(f: impl FnOnce()) -> bool {
    let env = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut std::os::raw::c_int) };
    if jumped == 0 {
        f();
        crate::exception::js_try_end();
        false
    } else {
        crate::exception::js_try_end();
        crate::exception::js_clear_exception();
        true
    }
}

#[test]
fn wtf8_method_call_preserves_exact_key_and_receiver() {
    // U+D800 encoded as WTF-8, which is intentionally invalid standard UTF-8.
    assert_method_call_binds_receiver(b"\xED\xA0\x80");
}

#[test]
fn utf8_method_call_stays_on_the_existing_dispatch_path() {
    assert_method_call_binds_receiver(b"ascii");
}

#[test]
fn wtf8_non_callable_does_not_fall_through_to_replacement_character_key() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 2) as i64,
    ));
    let exact_key = scope.root_string_ptr(string_key(b"\xED\xA0\x80"));
    let replacement_key = scope.root_string_ptr(string_key("\u{FFFD}".as_bytes()));
    let replacement = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(return_this, 0; with_declared(0)), 0)
            as i64,
    ));
    exact_key.with_const_ptr::<crate::StringHeader, _>(|key| {
        let object = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64())
            as *mut crate::object::ObjectHeader;
        crate::object::js_object_set_field_by_name(object, key, 17.0);
    });
    replacement_key.with_const_ptr::<crate::StringHeader, _>(|key| {
        let object = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64())
            as *mut crate::object::ObjectHeader;
        crate::object::js_object_set_field_by_name(object, key, replacement.get_nanbox_f64());
    });

    assert!(runtime_throw(|| unsafe {
        super::js_native_call_method(
            receiver.get_nanbox_f64(),
            b"\xED\xA0\x80".as_ptr().cast(),
            3,
            std::ptr::null(),
            0,
        );
    }));
}
