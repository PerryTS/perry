//! BUFEQ: `Buffer.prototype` is intrinsic family `Buffer`, sealed when it is
//! built; the name dispatch and the named Get on a Buffer read its own shadow
//! bit and, once the program wrote it, find the member through the chain.
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

fn stand_in() -> bool {
    std::ptr::eq(
        crate::agent_ptrs::intrinsic_proto(BuiltinProto::Buffer as usize).cast::<u64>(),
        crate::agent_ptrs::PERRY_PRISTINE_PROTO.as_ptr(),
    )
}

/// `Buffer.prototype` of this thread's realm (building it).
fn buffer_prototype() -> *mut ObjectHeader {
    let proto = crate::object::native_module::buffer_original_prototype_value();
    JSValue::from_bits(proto.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader
}

/// A one-byte Buffer holding 7, and what `readUInt8(0)` dispatches to.
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

fn patch() -> f64 {
    let c = crate::closure::js_closure_alloc(crate::fn_info!(patched_member, 0), 0);
    crate::value::js_nanbox_pointer(c as i64)
}

/// Run `body` on a fresh thread: the realm, its `Buffer.prototype` and the
/// shadow bit are per thread, so no test sees another one's writes. The
/// typed-array group is installed as a program that names `Buffer` does.
fn on_fresh_thread(body: impl FnOnce() + Send + 'static) {
    crate::typedarray_hook_slots::install();
    std::thread::spawn(body).join().unwrap();
}

#[test]
fn unbuilt_and_untouched_buffer_prototype_reads_unwritten() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        // Before Buffer.prototype exists the slot names the stand-in.
        assert!(crate::object::native_module::cached_buffer_intrinsic_prototype_value().is_none());
        assert!(stand_in());
        assert!(prototype_unwritten(BuiltinProto::Buffer));
        unsafe {
            let proto = buffer_prototype();
            assert!(!stand_in(), "the build publishes Buffer.prototype");
            assert!(std::ptr::eq(
                crate::agent_ptrs::intrinsic_proto(BuiltinProto::Buffer as usize),
                proto
            ));
            assert!(
                prototype_unwritten(BuiltinProto::Buffer),
                "the build itself writes nothing"
            );
            let buf = seven();
            assert_eq!(read_u8(buf), 7.0);
            // The named Get still mints the bound builtin.
            assert!(JSValue::from_bits(named_get(buf, "readUInt8").to_bits()).is_pointer());
        }
    });
}

#[test]
fn a_write_to_buffer_prototype_reaches_dispatch_and_get() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let proto = buffer_prototype();
            let f = patch();
            crate::object::js_object_set_field_by_name(proto, key("readUInt8"), f);
            assert!(!prototype_unwritten(BuiltinProto::Buffer));
            let buf = seven();
            assert_eq!(read_u8(buf), 42.0, "dispatch calls the replacement");
            assert_eq!(
                named_get(buf, "readUInt8").to_bits(),
                f.to_bits(),
                "Get finds it"
            );
            // An untouched member is still the builtin.
            let zero = 0.0f64;
            let other = crate::object::dispatch_buffer_method(buf as usize, "readInt8", &zero, 1);
            assert_eq!(other, 7.0);
        }
    });
}

#[test]
fn a_definition_on_buffer_prototype_sets_the_bit() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let proto = buffer_prototype();
            let f = patch();
            let bag = crate::object::js_object_alloc(0, 1);
            crate::object::js_object_set_field_by_name(bag, key("value"), f);
            crate::object::js_object_define_property(
                crate::value::js_nanbox_pointer(proto as i64),
                crate::value::js_nanbox_string(key("readUInt8") as i64),
                crate::value::js_nanbox_pointer(bag as i64),
            );
            assert!(!prototype_unwritten(BuiltinProto::Buffer));
            assert_eq!(read_u8(seven()), 42.0);
        }
    });
}

#[test]
fn a_delete_on_buffer_prototype_sets_the_bit() {
    on_fresh_thread(|| {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let proto = buffer_prototype();
            crate::object::js_object_delete_field(proto, key("readUInt8"));
            assert!(!prototype_unwritten(BuiltinProto::Buffer));
            // Node: nothing up the chain names `readUInt8`.
            assert_eq!(
                named_get(seven(), "readUInt8").to_bits(),
                crate::value::TAG_UNDEFINED
            );
        }
    });
}
