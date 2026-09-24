//! #11163: private storage identities must survive evacuation with their class.
use super::super::*;
use super::support::*;

#[test]
fn private_storage_identity_and_value_survive_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(2);
    gc_register_mutable_root_scanner(crate::object::scan_private_lexical_brand_roots_mut);
    unsafe {
        const CID: u32 = 62_534;
        let (owner, _) = alloc_nursery_test_object(0);
        js_shadow_slot_set(0, ptr_bits(owner as usize));
        let (class, _) = alloc_nursery_test_object(0);
        (*class).class_id = CID;
        js_shadow_slot_set(1, ptr_bits(class as usize));
        crate::object::js_object_mark_class(class as i64);
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
        crate::object::stamp_private_evaluation_brand(owner, f64::from_bits(js_shadow_slot_get(1)));
        let field = crate::string::js_string_from_bytes(b"#v".as_ptr(), 2);
        crate::object::private_lexical_brand_push(f64::from_bits(js_shadow_slot_get(1)));
        crate::object::js_private_field_add(
            f64::from_bits(js_shadow_slot_get(0)),
            CID,
            crate::value::js_nanbox_string(field as i64),
            17.0,
        );
        crate::object::private_lexical_brand_pop();
        let old_class = (js_shadow_slot_get(1) & POINTER_MASK) as *mut crate::ObjectHeader;
        let identity = (*(*old_class).meta).native_state;
        assert_ne!(identity, 0);
        crate::object::js_private_guard(
            f64::from_bits(js_shadow_slot_get(0)),
            f64::from_bits(js_shadow_slot_get(1)),
            CID,
            b"#v".as_ptr(),
            2,
            0,
            0,
        );
        let _ = gc_collect_minor();
        let class = (js_shadow_slot_get(1) & POINTER_MASK) as *mut crate::ObjectHeader;
        assert_ne!(class, old_class, "test premise: the class must move");
        assert_eq!((*(*class).meta).native_state, identity);
        assert_eq!(
            crate::object::field_get_set::test_pending_private_access_owner(),
            Some(js_shadow_slot_get(1)),
            "the pending guard owner must be rewritten too"
        );
        let request = format!("#<perry:private-value:{CID}:#v>");
        let key = crate::string::js_string_from_bytes(request.as_ptr(), request.len() as u32);
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ObjectHeader;
        // No lexical stack entry: direct compiled calls use the receiver's
        // evaluation, which the GC has also rewritten.
        assert_eq!(
            crate::object::js_object_get_field_by_name_f64(owner, key),
            17.0
        );
        js_shadow_slot_set(0, 0);
        js_shadow_slot_set(1, 0);
    }
}
