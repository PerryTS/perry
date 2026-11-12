//! BUFEQ24: a Buffer's `[[Prototype]]` is a fact of the receiver (its bag,
//! `buffer::store::PROTOTYPE_KEY`). Once the program replaces it, the name
//! dispatch and the named Get on THAT receiver read its real chain; every
//! other Buffer keeps the builtin body, and `__proto__` reads the link.
use crate::object::builtin_proto::{prototype_unwritten, BuiltinProto};
use crate::object::ObjectHeader;
use crate::value::JSValue;

extern "C" fn patched_member(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    42.0
}

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn boxed(buf: *mut crate::buffer::BufferHeader) -> f64 {
    crate::value::js_nanbox_pointer(buf as i64)
}

unsafe fn read_u8(buf: *mut crate::buffer::BufferHeader) -> f64 {
    let zero = 0.0f64;
    crate::object::dispatch_buffer_method(buf as usize, "readUInt8", &zero, 1)
}

unsafe fn seven() -> *mut crate::buffer::BufferHeader {
    crate::buffer::js_buffer_alloc(1, 7)
}

fn named_get(buf: *mut crate::buffer::BufferHeader, name: &str) -> f64 {
    crate::object::js_object_get_field_by_name_f64(buf as *const ObjectHeader, key(name))
}

fn set_proto(buf: *mut crate::buffer::BufferHeader, proto: f64) {
    crate::object::js_object_set_prototype_of(boxed(buf), proto);
}

/// An ordinary object `{ readUInt8: () => 42 }`.
fn patch_object() -> (f64, f64) {
    let c = crate::closure::js_closure_alloc(crate::fn_info!(patched_member, 0), 0);
    let f = crate::value::js_nanbox_pointer(c as i64);
    let obj = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field_by_name(obj, key("readUInt8"), f);
    (crate::value::js_nanbox_pointer(obj as i64), f)
}

fn buffer_prototype() -> f64 {
    crate::object::native_module::buffer_original_prototype_value()
}

fn on_fresh_thread(body: impl FnOnce() + Send + 'static) {
    crate::typedarray_hook_slots::install();
    std::thread::spawn(body).join().unwrap();
}

#[test]
fn a_replaced_prototype_is_this_receivers_only() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let _ = buffer_prototype();
            let (x, f) = patch_object();
            let a = seven();
            let b = seven();
            set_proto(a, x);
            assert!(
                prototype_unwritten(BuiltinProto::Buffer),
                "a receiver's link is not a write to Buffer.prototype"
            );
            assert_eq!(read_u8(a), 42.0, "dispatch calls X.readUInt8");
            assert_eq!(named_get(a, "readUInt8").to_bits(), f.to_bits());
            assert!(JSValue::from_bits(named_get(a, "readInt8").to_bits()).is_undefined());
            assert_eq!(named_get(a, "__proto__").to_bits(), x.to_bits());
            assert_eq!(read_u8(b), 7.0, "another Buffer keeps the builtin");
            assert!(JSValue::from_bits(named_get(b, "readInt8").to_bits()).is_pointer());
        }
    });
}

#[test]
fn proto_reads_and_restore() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let proto = buffer_prototype();
            let a = seven();
            assert_eq!(named_get(a, "__proto__").to_bits(), proto.to_bits());
            let (x, _) = patch_object();
            set_proto(a, x);
            set_proto(a, proto);
            assert_eq!(read_u8(a), 7.0, "Buffer.prototype again: the builtin");
            assert_eq!(named_get(a, "__proto__").to_bits(), proto.to_bits());
        }
    });
}

#[test]
fn null_and_uint8array_prototype_hide_buffer_members() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let _ = buffer_prototype();
            let a = seven();
            set_proto(a, f64::from_bits(crate::value::TAG_NULL));
            assert!(JSValue::from_bits(named_get(a, "readUInt8").to_bits()).is_undefined());
            assert!(JSValue::from_bits(named_get(a, "__proto__").to_bits()).is_undefined());
            let u8_proto = crate::object::builtin_prototype_value("Uint8Array");
            let b = seven();
            set_proto(b, u8_proto);
            assert!(JSValue::from_bits(named_get(b, "readUInt8").to_bits()).is_undefined());
            assert!(JSValue::from_bits(named_get(b, "at").to_bits()).is_pointer());
            assert_eq!(named_get(b, "__proto__").to_bits(), u8_proto.to_bits());
        }
    });
}
