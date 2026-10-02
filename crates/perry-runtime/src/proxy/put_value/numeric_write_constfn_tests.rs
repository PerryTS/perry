//! The whole-loop guard licenses bare numeric stores, so every targeted
//! SPECIAL lane must decline while unrelated Any/F64 lanes remain usable.
use super::*;
use crate::object::field_rep::{with_slot_rep, REP_ANY, REP_F64, REP_SPECIAL};
use crate::object::shapes;

extern "C" fn body(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    17.0
}

fn boxed(ptr: *const u8) -> f64 {
    f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK))
}

/// Assert every exported ABI that draws its raw-store authority from the
/// same supplier. A refusal must leave the keytable's output buffer untouched.
fn assert_family(array: f64, keys: [f64; 2], lanes: u64) {
    assert_eq!(
        js_object_array_numeric_write_guard(array, keys[0], keys[1], 0.0, 0.0, 2, 2),
        lanes
    );
    assert_eq!(
        js_object_array_numeric_write_range_guard(array, keys[0], keys[1], 0.0, 0.0, 2, 0, 2),
        lanes
    );
    let wide_lanes = (lanes & 0xffff) | (((lanes >> 16) & 0xffff) << 32);
    assert_eq!(
        js_object_array_numeric_write2_guard(array, keys[0], keys[1], 2),
        wide_lanes
    );
    let table = crate::array::js_array_from_f64(keys.as_ptr(), 2);
    let mut output = [-7i64; 2];
    assert_eq!(
        js_object_array_keytable_write_guard(array, boxed(table.cast()), 2, 2, output.as_mut_ptr()),
        i32::from(lanes != 0)
    );
    if lanes == 0 {
        assert_eq!(output, [-7; 2]);
    } else {
        assert_eq!(
            output,
            [(lanes & 0xffff) as i64, ((lanes >> 16) & 0xffff) as i64]
        );
    }
}

#[test]
fn numeric_write_guards_refuse_constfn_targets_and_preserve_mixed_shapes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_gc = crate::gc::GcSuppressScope::new();
    crate::object::descriptor_state::test_reset_class_field_inline_guard();
    const CLASS: u32 = 0x5c68_09;
    let names = b"cf_numeric_method\0cf_numeric_any\0cf_numeric_f64\0";
    let keys =
        crate::object::js_build_class_keys_array(CLASS, 3, names.as_ptr(), names.len() as u32, 0);
    let base_rep = with_slot_rep(REP_ANY, 2, REP_F64);
    let birth = shapes::class_birth_shape_ensure(keys, 3, 3, CLASS, base_rep, None).unwrap();
    let info = crate::fn_info!(body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let final_rep = with_slot_rep(base_rep, 0, REP_SPECIAL);
    let completed = shapes::final_shape_ensure_constfn(
        keys,
        3,
        3,
        CLASS,
        final_rep,
        &[shapes::ConstFnSlotInfo {
            slot: 0,
            info: info as usize as u64,
        }],
        None,
    )
    .unwrap();
    let mut objects = Vec::new();
    let mut values = Vec::new();
    let mut method_bits = Vec::new();
    for number in [1.0f64, 2.0] {
        let object = crate::object::js_object_alloc_class_inline_keys(CLASS, 0, 3, keys);
        let closure = crate::closure::js_closure_alloc(info, 0);
        let method = boxed(closure.cast()).to_bits();
        unsafe {
            crate::object::store_object_field_slot(object, 0, method);
            crate::object::store_object_field_slot(object, 1, number.to_bits());
            crate::object::store_object_field_slot(object, 2, number.to_bits());
            shapes::stamp_object_shape_id_with_carrier_note(object, birth);
        }
        objects.push(object);
        values.push(boxed(object.cast()));
        method_bits.push(method);
    }
    let array = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    let array_box = boxed(array.cast());
    let key = |index| {
        let raw = crate::array::js_array_get(keys, index).as_string_ptr();
        f64::from_bits(crate::value::STRING_TAG | (raw as u64 & POINTER_MASK))
    };
    let method = key(0);
    let any = key(1);
    let f64_key = key(2);
    assert_family(array_box, [method, any], 1 | (2 << 16));
    assert_family(array_box, [any, f64_key], 2 | (3 << 16));
    for object in &objects {
        unsafe { shapes::stamp_object_shape_id_with_carrier_note(*object, completed) };
    }
    let facts = shapes::shape_descriptor_by_id(completed).unwrap();
    assert_eq!(facts.object_kind, shapes::ShapeObjectKind::Ordinary);
    assert_eq!(
        facts.special_constfn_mask, 1,
        "the refusal must exercise a live ConstFn shape"
    );
    assert!(
        crate::object::field_rep_store::final_shape_matches_birth(completed, birth),
        "the completed shape remains compatible for read-only field access"
    );
    assert_family(array_box, [method, any], 0);
    assert_family(array_box, [any, method], 0);
    assert_family(array_box, [any, f64_key], 2 | (3 << 16));
    for (index, object) in objects.iter().enumerate() {
        assert_eq!(unsafe { shapes::object_shape_stamp(*object) }, completed);
        let slot = unsafe {
            (*object as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const u64
        };
        assert_eq!(
            unsafe { *slot },
            method_bits[index],
            "preflight cannot change the closure"
        );
        crate::object::js_object_set_field_by_name(
            *object,
            (method.to_bits() & POINTER_MASK) as *mut _,
            99.0,
        );
        let now = unsafe { shapes::object_shape_stamp(*object) };
        assert_ne!(
            now, completed,
            "ordinary replacement must retire the body fact first"
        );
        assert!(crate::object::field_rep_store::shape_slot_is_any(now, 0));
        assert_eq!(unsafe { *slot }, 99.0f64.to_bits());
    }
    assert_family(array_box, [method, any], 1 | (2 << 16));
}
