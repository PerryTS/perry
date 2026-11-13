//! `raw_ptr_from_value` decodes a VALUE: an object is named by its tag.
//!
//! The decoder (and its copies in the node:stream, events, dgram, util.diff,
//! stream/consumers, trace_events, subclass and fs.writeFile entries) used to
//! return a word with clear top 16 bits as an address. Such a word is a Number
//! (a denormal double); the Number whose bits equal a live array's address
//! must not be that array.

use super::property_ops::set::{array_ptr_from_value, raw_ptr_from_value};

#[test]
fn a_forged_number_names_no_address() {
    let arr = crate::array::js_array_alloc(4);
    let arr = crate::array::js_array_push_f64(arr, 9.0);
    let forged = f64::from_bits(arr as u64);
    assert_eq!(forged.to_bits() >> 48, 0, "an address is a denormal");
    assert_eq!(raw_ptr_from_value(forged), None, "a Number decoded");
    assert!(
        array_ptr_from_value(forged).is_none(),
        "a Number is the array"
    );
    // Every other denormal, and a word in the handle band.
    assert_eq!(raw_ptr_from_value(f64::from_bits(0x1000_0000)), None);
    assert_eq!(raw_ptr_from_value(f64::from_bits(0x1234)), None);
    assert_eq!(raw_ptr_from_value(f64::from_bits(1)), None);

    let tagged = crate::value::js_nanbox_pointer(arr as i64);
    assert_eq!(raw_ptr_from_value(tagged), Some(arr as usize));
    assert_eq!(array_ptr_from_value(tagged), Some(arr));
}
