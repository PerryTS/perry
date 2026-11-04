//! The raw-Int32 iter-result transport hands a generic reader a NUMBER.
//!
//! An INT32 word equal to a registered class id still decodes as that class's
//! legacy constructor immediate (`object/class_value.rs`, #11414), so the
//! generic read must publish the number in its f64 form. Before, `await` of an
//! async method returning `2` resolved to the class with id 2.
use super::{js_iter_result_get_value, js_iter_result_set_i32};

#[test]
fn raw_i32_iter_result_reads_back_as_the_number_not_a_class() {
    let cid = 0x6B02;
    unsafe { crate::object::js_register_class_id(cid) };
    js_iter_result_set_i32(cid as i32, 1);
    let v = js_iter_result_get_value();
    assert_eq!(v.to_bits(), (cid as f64).to_bits(), "the number, f64 form");
    assert_eq!(crate::object::class_value::class_value_id(v), None, "not a class");
    js_iter_result_set_i32(-7, 0);
    assert_eq!(js_iter_result_get_value(), -7.0);
}
