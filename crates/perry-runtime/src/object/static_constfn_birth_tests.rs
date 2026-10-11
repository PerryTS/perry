use super::*;

extern "C" fn body_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    11.0
}
extern "C" fn body_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    22.0
}

fn info_a() -> *const crate::closure::JsFunctionInfo {
    crate::fn_info!(body_a, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE | crate::codegen_abi::FN_COMPILED_BODY))
}
fn info_b() -> *const crate::closure::JsFunctionInfo {
    crate::fn_info!(body_b, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE | crate::codegen_abi::FN_COMPILED_BODY))
}
fn seed() -> u32 {
    let packed = b"qsbirths_capture\0qsbirths_number\0";
    let entries = [static_shapes::ConstFnStaticEntry {
        slot: 0,
        info: info_a(),
    }];
    static_shapes::js_shape_seed_plain_constfn(
        shapes::SHAPE_ID_BASE + 0x7981,
        packed.as_ptr(),
        packed.len() as u32,
        2,
        4,
        7,
        entries.as_ptr(),
        1,
    )
}
fn born(shape: u32, values: &[u64]) -> *mut ObjectHeader {
    let packed = b"qsbirths_capture\0qsbirths_number\0";
    let entries = [static_shapes::ConstFnStaticEntry {
        slot: 0,
        info: info_a(),
    }];
    js_object_alloc_constfn_static(
        shape,
        packed.as_ptr(),
        packed.len() as u32,
        2,
        4,
        0,
        7,
        entries.as_ptr(),
        1,
        values.as_ptr(),
    ) as usize as *mut ObjectHeader
}

#[test]
fn literal_birth_shares_shape_but_owns_fresh_closures_and_capture_values() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let id = seed();
    let mut objects = Vec::new();
    for capture in [13.0f64, 17.0] {
        let closure = crate::closure::js_closure_alloc(info_a(), 1);
        unsafe { crate::closure::js_closure_set_capture_bits(closure, 0, capture.to_bits()) };
        let values = [
            crate::value::POINTER_TAG | closure as usize as u64,
            capture.to_bits(),
        ];
        let obj = born(id, &values);
        unsafe {
            assert_eq!(
                shapes::object_shape_stamp(obj),
                id,
                "must actually birth on the final shape"
            );
            assert_eq!(crate::object::js_object_get_field(obj, 0).bits(), values[0]);
            assert_eq!(crate::object::js_object_get_field(obj, 1).bits(), values[1]);
            assert_eq!(
                crate::closure::js_closure_get_capture_bits(closure, 0),
                capture.to_bits()
            );
            assert_eq!(
                crate::object::js_object_get_field(obj, 3).bits(),
                crate::value::TAG_UNDEFINED
            );
        }
        objects.push(obj);
    }
    assert_ne!(objects[0], objects[1]);
    assert_ne!(
        crate::object::js_object_get_field(objects[0], 0).bits(),
        crate::object::js_object_get_field(objects[1], 0).bits()
    );
}

#[test]
fn literal_birth_revalidates_body_numeric_lane_and_complete_shape() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let id = seed();
    let record = birth_record(id, 2, 4, 7).unwrap();
    assert!(birth_record(id, 1, 4, 7).is_none());
    assert!(birth_record(id, 2, 3, 7).is_none());
    assert!(birth_record(id, 2, 4, 3).is_none());
    assert!(birth_record(0, 2, 4, 7).is_none());
    let a = crate::closure::js_closure_alloc(info_a(), 0);
    let b = crate::closure::js_closure_alloc(info_b(), 0);
    let good = [
        crate::value::POINTER_TAG | a as usize as u64,
        31.0f64.to_bits(),
    ];
    let wrong = [
        crate::value::POINTER_TAG | b as usize as u64,
        crate::value::TAG_NULL,
    ];
    unsafe {
        assert!(values_match(record, &good));
        assert!(
            !values_match(record, &wrong),
            "current values must validate each use"
        );
        let obj = born(id, &wrong);
        let actual = shapes::object_shape_stamp(obj);
        assert_ne!(
            actual, id,
            "refused values must resolve different compatible facts"
        );
        let d = shapes::shape_descriptor_by_id(actual).unwrap();
        assert_eq!(d.special_constfn_mask, 0);
        assert_eq!(d.rep, 0);
        assert_eq!(crate::object::js_object_get_field(obj, 0).bits(), wrong[0]);
        assert_eq!(crate::object::js_object_get_field(obj, 1).bits(), wrong[1]);
    }
}

#[test]
fn literal_reassignment_loses_body_facts_without_affecting_another_birth() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let id = seed();
    let a = crate::closure::js_closure_alloc(info_a(), 0);
    let values = [
        crate::value::POINTER_TAG | a as usize as u64,
        37.0f64.to_bits(),
    ];
    let first = born(id, &values);
    crate::object::js_object_set_field(first, 0, crate::JSValue::number(41.0));
    unsafe {
        let changed = shapes::shape_descriptor_by_id(shapes::object_shape_stamp(first)).unwrap();
        assert_eq!(changed.special_constfn_mask, 0);
        let second = born(id, &values);
        assert_eq!(
            crate::object::js_object_get_field(second, 0).bits(),
            values[0]
        );
        assert_eq!(
            shapes::shape_descriptor_by_id(shapes::object_shape_stamp(second))
                .unwrap()
                .special_constfn_mask,
            1
        );
    }
}
