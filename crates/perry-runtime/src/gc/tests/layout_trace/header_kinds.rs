//! Birth, rebuild and store classify payloads without creating owner records.
use super::*;

#[test]
fn layout_birth_rebuild_and_store_never_mint_masks() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let child = crate::string::js_string_from_bytes(b"child".as_ptr(), 5);
    let arr = crate::array::js_array_alloc_with_length(8);
    crate::array::js_array_set_f64(arr, 1, crate::value::js_nanbox_string(child as i64));
    unsafe {
        let slots = crate::array::array_elements_ptr(arr);
        layout_rebuild_from_slots(arr.cast(), slots, 8);
        assert_eq!(
            (*header_from_user_ptr(arr.cast()))._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN
        );
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 8);
        let captures = crate::closure::closure_capture_slots_mut(closure);
        *captures.add(1) = string_bits(child as usize);
        assert!(layout_init_from_slots(closure.cast(), captures, 8));
    }
    assert_eq!(crate::gc::per_object_layout_table_sizes(), 0);
}
