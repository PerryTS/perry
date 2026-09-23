//! Shared slow constructor for wide synthetic record shapes. Its boxed ABI
//! still implements strict property assignment, including descriptor/prototype
//! changes. Constant literal sites normally bypass it via a descriptor.

/// `values` is an immediately consumed compiler stack buffer. Root every
/// operand before the first assignment, since a setter may enter user code
/// and collect. The keys address names an existing registered module root.
#[no_mangle]
pub extern "C" fn js_literal_shape_initialize(
    receiver: f64,
    keys_slot: *const u64,
    values: *const f64,
    count: u32,
) {
    if keys_slot.is_null() || values.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let keys = scope.root_raw_mut_ptr(unsafe { *keys_slot } as *mut crate::array::ArrayHeader);
    let values: Vec<_> = unsafe { std::slice::from_raw_parts(values, count as usize) }
        .iter()
        .map(|value| scope.root_nanbox_f64(*value))
        .collect();
    for (i, value) in values.iter().enumerate() {
        // Inspect the rooted key array without letting its raw pointer escape.
        // The returned value stays owned by that array until js_put_value_set
        // roots all three operands at entry; no allocation occurs in between.
        let key = keys.with_const_ptr::<crate::array::ArrayHeader, _>(|keys_array| {
            if keys_array.is_null() || i >= unsafe { (*keys_array).length } as usize {
                None
            } else {
                Some(unsafe { *crate::array::array_elements_ptr(keys_array).add(i) })
            }
        });
        let Some(key) = key else { return };
        let this = receiver.get_nanbox_f64();
        crate::proxy::js_put_value_set(this, f64::from_bits(key), value.get_nanbox_f64(), this, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn collecting_setter(_closure: *const crate::ClosureHeader, _value: f64) -> f64 {
        crate::gc::gc_collect_minor();
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }

    #[test]
    fn literal_constructor_roots_later_values_across_a_collecting_setter() {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        let _verification = crate::gc::knob_overrides::VerifyEvacuationTestGuard::on();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        // The isolation guard removes production's root registry. The keys
        // lists are young canonical arrays the setter's collection moves, so
        // the object-model rewrites must run or the receiver's shape keeps
        // naming a moved list's old address.
        crate::gc::gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
        crate::gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
        crate::gc::gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
        crate::gc::gc_register_mutable_root_scanner(
            crate::object::shapes::scan_shape_table_rekey_mut,
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let setter = crate::closure::js_closure_alloc(collecting_setter as *const u8, 0);
        let descriptor = crate::object::js_object_alloc(0, 0);
        let set_key = crate::js_string_from_bytes(b"set".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(
            descriptor,
            set_key,
            crate::value::js_nanbox_pointer(setter as i64),
        );
        let head_key = crate::js_string_from_bytes(b"head".as_ptr(), 4);
        crate::object::js_object_define_property(
            receiver.with_mut_ptr::<u8, _>(|ptr| crate::value::js_nanbox_pointer(ptr as i64)),
            crate::value::js_nanbox_string(head_key as i64),
            crate::value::js_nanbox_pointer(descriptor as i64),
        );
        let keys =
            crate::object::js_build_class_keys_array(1017302, 3, b"head\0text\0n\0".as_ptr(), 12)
                as u64;
        let text = b"later value must survive the first setter";
        let string = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        // The caller's plain buffer is deliberately NOT rooted. The helper
        // must transfer all values to handles before the first setter runs.
        let values = [1.0, crate::value::js_nanbox_string(string as i64), 42.0];
        let before = receiver.with_const_ptr::<crate::ObjectHeader, _>(|ptr| ptr as usize);
        let cycles = crate::gc::copying_minor_cycles();
        let (_, after) = receiver.across_const::<crate::ObjectHeader, _>(|| {
            let boxed =
                receiver.with_mut_ptr::<u8, _>(|ptr| crate::value::js_nanbox_pointer(ptr as i64));
            js_literal_shape_initialize(boxed, &keys, values.as_ptr(), 3);
        });
        assert!(crate::gc::copying_minor_cycles() > cycles);
        assert_ne!(after as usize, before);
        let key = crate::js_string_from_bytes(b"text".as_ptr(), 4);
        let actual =
            receiver.with_const_ptr(|ptr| crate::object::js_object_get_field_by_name(ptr, key));
        let mut scratch = [0; crate::value::SHORT_STRING_MAX_LEN];
        let (bytes, len) =
            crate::string::str_bytes_from_jsvalue(f64::from_bits(actual.bits()), &mut scratch)
                .expect("later string field must survive");
        assert_eq!(
            unsafe { std::slice::from_raw_parts(bytes, len as usize) },
            text
        );
        let key = crate::js_string_from_bytes(b"n".as_ptr(), 1);
        assert_eq!(
            receiver.with_const_ptr(|ptr| {
                crate::object::js_object_get_field_by_name(ptr, key).as_number()
            }),
            42.0
        );
    }
}
