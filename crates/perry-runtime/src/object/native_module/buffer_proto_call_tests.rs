//! BUFEQ28: a builtin `Buffer.prototype` member called with an explicit
//! `this` (`Buffer.prototype.copy.call(src, dst)`) runs that member's body on
//! the receiver; a receiver that is not a `Uint8Array` view is a TypeError.
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// `Buffer.prototype[name]` of this thread's realm (building it).
fn member(name: &str) -> f64 {
    let proto = crate::object::native_module::buffer_original_prototype_value();
    let proto = JSValue::from_bits(proto.to_bits()).as_pointer::<ObjectHeader>();
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(proto, key)
}

fn value<T>(ptr: *mut T) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn call(name: &str, this: f64, args: &[f64]) -> Option<f64> {
    crate::collection_iter::call_with_this_capturing_throw(member(name), this, args).ok()
}

/// The realm is per thread; the typed-array group is installed as a program
/// that names `Buffer` does.
fn on_fresh_thread(body: impl FnOnce() + Send + 'static) {
    crate::typedarray_hook_slots::install();
    std::thread::spawn(body).join().unwrap();
}

#[test]
fn explicit_this_runs_the_member_on_a_buffer_and_a_uint8array() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        let src = crate::buffer::js_buffer_alloc(4, 7);
        let dst = crate::buffer::js_buffer_alloc(4, 0);
        assert_eq!(call("copy", value(src), &[value(dst), 1.0]), Some(3.0));
        assert_eq!(call("readUInt8", value(dst), &[1.0]), Some(7.0));
        assert_eq!(call("readUInt8", value(dst), &[0.0]), Some(0.0));
        let same = crate::buffer::js_buffer_alloc(4, 7);
        assert_eq!(
            call("equals", value(src), &[value(same)]).map(f64::to_bits),
            Some(JSValue::bool(true).bits())
        );
        assert_eq!(
            call("equals", value(src), &[value(dst)]).map(f64::to_bits),
            Some(JSValue::bool(false).bits())
        );
        // A plain Uint8Array receiver runs the same body (Node: any Uint8Array).
        let u8 = crate::buffer::js_uint8array_alloc(4);
        assert_eq!(call("copy", value(src), &[value(u8)]), Some(4.0));
        assert_eq!(
            call("equals", value(u8), &[value(same)]).map(f64::to_bits),
            Some(JSValue::bool(true).bits())
        );
        assert_eq!(call("writeUInt8", value(u8), &[9.0, 0.0]), Some(1.0));
        assert_eq!(call("readUInt8", value(u8), &[0.0]), Some(9.0));
    });
}

#[test]
fn explicit_this_that_is_not_a_uint8array_view_throws() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        let other = value(crate::buffer::js_buffer_alloc(2, 0));
        let plain = value(crate::object::js_object_alloc(0, 0));
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        for this in [plain, undefined, 3.0] {
            assert_eq!(call("equals", this, &[other]), None);
            assert_eq!(call("copy", this, &[other]), None);
        }
    });
}
