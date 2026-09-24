//! Shape-retention regression coverage for ordinary public class fields.

use super::*;

#[test]
fn repeated_class_field_defaults_do_not_mint_semantic_shapes() {
    let _side_table_lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"from".as_ptr(), 4));
    let packed_keys = b"from";
    let shape_id = 0x9942_0001;

    let define = |object: *mut ObjectHeader, value: f64| {
        let receiver = crate::value::js_nanbox_pointer(object as i64);
        let key_value = f64::from_bits(
            JSValue::string_ptr(key.get_raw_const_ptr::<crate::StringHeader>() as *mut _).bits(),
        );
        crate::object::js_class_field_add(receiver, key_value, value);
    };

    // The first descriptor operation in a process lazily installs the builtin
    // shape catalog. Warm that process-global machinery before measuring the
    // per-instance path; otherwise its fixed initialization cost is mistaken
    // for a semantic descriptor minted for this field.
    let warmup = js_object_alloc_with_shape(
        0x9942_0000,
        1,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    define(warmup, -1.0);

    // Materialize the shared structural shape before measuring. The first
    // class-field definition on this instance must not mint an additional
    // semantic descriptor for the ordinary all-true attributes.
    let first =
        js_object_alloc_with_shape(shape_id, 1, packed_keys.as_ptr(), packed_keys.len() as u32);
    let before_first_define = crate::object::shapes::test_shape_descriptor_count();
    define(first, 0.0);
    assert_eq!(
        crate::object::shapes::test_shape_descriptor_count(),
        before_first_define,
        "the first class field on a pre-shaped instance must not mint a semantic shape",
    );

    // This is the production pattern from MySqlSelectBuilder: every
    // construction gets a fresh object but reuses the class's installed keys.
    for i in 1..=256 {
        let object =
            js_object_alloc_with_shape(shape_id, 1, packed_keys.as_ptr(), packed_keys.len() as u32);
        define(object, i as f64);
        assert!(get_property_attrs(object as usize, "from").is_none());
    }

    assert_eq!(
        crate::object::shapes::test_shape_descriptor_count(),
        before_first_define,
        "fresh instances sharing class keys must not mint one semantic shape per field",
    );
}
