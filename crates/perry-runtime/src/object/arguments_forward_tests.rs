use super::*;

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn born(values: &[f64], mapped: u32) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let values = scope.root_nanbox_f64_slice(values);
    arguments_object_alloc_rooted(
        &values,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
        mapped,
    )
}

#[test]
fn born_arguments_forward_slots_and_shape_mutation_control() {
    let _guard = crate::gc::global_side_table_test_lock();
    crate::object::js_get_global_this();
    let args = born(&[3.0, 4.0], 0);
    unsafe {
        assert!((*args).meta.is_null());
        assert_eq!(arguments_object_born_slice(args).unwrap(), &[3.0, 4.0]);
        assert_eq!(arguments_object_get_index(args, 1), Some(4.0));
        let prototype = js_object_get_prototype_of(crate::value::js_nanbox_pointer(args as i64));
        assert_ne!(
            prototype.to_bits(),
            crate::value::js_nanbox_pointer(args as i64).to_bits()
        );
        let object = crate::object::builtin_prototype_value("Object");
        assert_eq!(prototype.to_bits(), object.to_bits());
        let inherited = js_object_get_field_by_name(args, key("toString"));
        assert!(crate::closure::is_closure_ptr(
            crate::value::js_nanbox_get_pointer(f64::from_bits(inherited.bits())) as usize
        ));
        assert_eq!(
            js_arguments_length_value(crate::value::js_nanbox_pointer(args as i64)),
            2.0
        );
        // Negative control: an indexed delete revokes the exact birth proof.
        assert_eq!(js_object_delete_field(args, key("1")), 1);
        assert!(arguments_object_born_slice(args).is_none());
        let values = arguments_object_to_vec(args).unwrap();
        assert_eq!(values[0], 3.0);
        assert_eq!(values[1].to_bits(), crate::value::TAG_UNDEFINED);
    }
}

#[test]
fn birth_shape_proves_indices_beyond_the_arity_cache() {
    let _guard = crate::gc::global_side_table_test_lock();
    crate::object::js_get_global_this();
    let values: Vec<_> = (0..65).map(|i| i as f64).collect();
    let args = born(&values, 0);
    unsafe {
        assert!((*args).meta.is_null());
        assert_eq!(arguments_object_born_slice(args).unwrap(), values.as_slice());
        assert_eq!(arguments_object_get_index(args, 64), Some(64.0));
    }
}

#[test]
fn mapped_aliases_and_length_values_are_not_raw_forwarded() {
    let _guard = crate::gc::global_side_table_test_lock();
    crate::object::js_get_global_this();
    let args = born(&[1.0, 2.0], 1);
    let cell = crate::r#box::js_box_alloc(9.0);
    js_arguments_object_map_index(args, 0, cell);
    unsafe {
        assert!((*args).meta.is_null());
        assert!(arguments_object_born_slice(args).is_none());
        assert_eq!(arguments_object_to_vec(args).unwrap(), vec![9.0, 2.0]);
        crate::r#box::js_box_set(cell, 12.0);
        assert_eq!(arguments_object_get_index(args, 0), Some(12.0));
        js_object_set_field_by_name(
            args,
            key("length"),
            crate::value::js_nanbox_string(key("1") as i64),
        );
        assert_eq!(arguments_object_to_vec(args).unwrap(), vec![12.0]);
        assert_eq!(arguments_object_length(args), 1);
    }
}

#[test]
fn escaping_arguments_thread_copy_keeps_data_without_private_identity() {
    let _guard = crate::gc::global_side_table_test_lock();
    crate::object::js_get_global_this();
    let args = born(&[3.0, 4.0], 0);
    unsafe {
        let data = crate::thread::serialize_nanbox_for_thread(
            crate::value::js_nanbox_pointer(args as i64).to_bits(),
        );
        let copied = crate::thread::deserialize_nanbox_on_current_thread(&data);
        let copied = (copied & crate::value::POINTER_MASK) as *const ObjectHeader;
        assert_eq!((*copied).class_id, 0);
        assert!(!is_arguments_object(copied));
        assert_eq!(js_object_get_field_by_name_f64(copied, key("0")), 3.0);
        assert_eq!(js_object_get_field_by_name_f64(copied, key("1")), 4.0);
    }
}
