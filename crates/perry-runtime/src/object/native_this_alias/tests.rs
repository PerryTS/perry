use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

// Test-only id in the native handle band; the extension declines every other id.
const HANDLE: i64 = 0xe1725;
per_test_global! {
    static NATIVE_VALUE: AtomicU64 = AtomicU64::new(0);
}

unsafe extern "C" fn property(handle: i64, name: *const u8, len: usize, out: *mut f64) -> i32 {
    if handle != HANDLE || std::slice::from_raw_parts(name, len) != b"socket" {
        return 0;
    }
    *out = f64::from_bits(NATIVE_VALUE.load(Ordering::Relaxed));
    1
}

#[test]
fn inline_cache_reads_native_alias_state_without_caching_a_miss() {
    unsafe {
        super::super::class_handles::js_register_handle_property_dispatch_extension(property)
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(super::super::js_object_alloc(0, 0));
    let sibling = scope.root_raw_mut_ptr(super::super::js_object_alloc(0, 0));
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"socket".as_ptr(), 6));
    let read = |obj: &crate::gc::RuntimeHandle<'_>| {
        obj.with_const_ptr(|o: *const super::super::ObjectHeader| {
            key.with_const_ptr(|k| {
                let front = unsafe {
                    match crate::string::existing_property_atom(k) {
                        Some(atom) => crate::object::layout_probe::perry_probe_get(
                            super::super::shapes::ordinary_dir_addr(),
                            (o as i64).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR as i64),
                            atom as u64 | crate::value::STRING_TAG,
                            crate::string::property_atom_hash(atom),
                        ),
                        None => f64::from_bits(crate::value::TAG_HOLE),
                    }
                };
                if front.to_bits() != crate::value::TAG_HOLE {
                    return front;
                }
                crate::object::js_object_get_field_slow(o as i64, k)
            })
        })
    };
    // Prime a miss before the object becomes native-backed.
    assert_eq!(read(&object).to_bits(), crate::value::TAG_UNDEFINED);
    object.with_const_ptr(|o: *const super::super::ObjectHeader| {
        register_this_to_handle_alias(
            crate::value::js_nanbox_pointer(o as i64),
            crate::value::js_nanbox_pointer(HANDLE),
            true,
        )
    });
    for value in [41.0_f64, 42.0] {
        NATIVE_VALUE.store(value.to_bits(), Ordering::Relaxed);
        assert_eq!(read(&object), value);
        assert_eq!(read(&sibling).to_bits(), crate::value::TAG_UNDEFINED);
    }
    // An ordinary own property must still take precedence over native state.
    object.with_mut_ptr(|o| {
        key.with_const_ptr(|k| {
            super::super::js_object_set_field_by_name(o, k, 99.0);
        })
    });
    assert_eq!(read(&object), 99.0);
}
