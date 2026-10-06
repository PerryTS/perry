use super::*;

extern "C" fn method(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    7.0
}

unsafe fn one_method(info: *const crate::closure::JsFunctionInfo) -> (*mut ObjectHeader, u32) {
    let closure = crate::closure::js_closure_alloc(info, 0);
    let obj = crate::object::js_object_alloc(0, 4);
    let key = b"constfn_site_method";
    let name = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_set_field_by_name(
        obj,
        name,
        crate::value::js_nanbox_pointer(closure as i64),
    );
    (obj, super::super::shapes::object_shape_stamp(obj))
}

unsafe fn primed_slot(obj: *mut ObjectHeader) -> u64 {
    let mut slot: MethodSiteSlot = std::ptr::null_mut();
    prime(
        &mut slot,
        crate::value::js_nanbox_pointer(obj as i64),
        b"constfn_site_method",
        0,
    );
    assert!(!slot.is_null(), "eligible method site must prime");
    let word = std::ptr::read(obj as *const u64);
    (*slot)
        .entries
        .iter()
        .find(|entry| entry.word == word)
        .expect("site entry for receiver shape")
        .slot
}

#[test]
fn constfn_site_uses_shape_body_fact_only_for_permanent_images() {
    if !run_with_fresh_worker_gate("constfn_site_uses_shape_body_fact_only_for_permanent_images") {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let _no_move = crate::gc::GcSuppressScope::new();
        let permanent =
            crate::fn_info!(method, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
        let (object, id) = one_method(permanent);
        let d = super::super::shapes::shape_descriptor_by_id(id).expect("shape");
        assert_eq!(d.special_constfn_mask, 1);
        assert_eq!(primed_slot(object), METHOD_SITE_CONSTFN);

        // An unloadable image has no ConstFn shape fact. It may still use
        // the existing guarded own-method entry, which validates the
        // closure's kind and body info on every hit.
        let transient = crate::fn_info!(method, 0);
        let (object, id) = one_method(transient);
        let d = super::super::shapes::shape_descriptor_by_id(id).expect("shape");
        assert_eq!(d.special_constfn_mask, 0);
        assert_eq!(primed_slot(object), 0);
    }
}

#[test]
fn constfn_static_captured_this_arrow_primes_and_rebinding_closure_refuses() {
    if !run_with_fresh_worker_gate(
        "constfn_static_captured_this_arrow_primes_and_rebinding_closure_refuses",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let _no_gc = crate::gc::GcSuppressScope::new();
        let arrow = crate::fn_info!(method, 0; with_flags(
            crate::codegen_abi::FN_PERMANENT_IMAGE | crate::closure::FN_ARROW
        ));
        let rebinding =
            crate::fn_info!(method, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
        let packed = b"constfn_site_method\0";
        let keys = super::super::static_shapes::canonical_keys_for_names(&[b"constfn_site_method"]);
        for (info, admitted) in [(arrow, true), (rebinding, false)] {
            let obj = crate::object::alloc_plain::alloc_plain_record_inline_keys_stamped(
                1,
                keys.arr() as *mut _,
                0,
            );
            let base = super::super::shapes::object_shape_stamp(obj);
            let birth = super::super::shapes::shape_descriptor_by_id(base).unwrap();
            assert_eq!(
                birth.object_kind,
                super::super::shapes::ShapeObjectKind::Ordinary
            );
            assert_eq!(birth.special_constfn_mask, 0, "allocation must stay Any");
            let c = crate::closure::js_closure_alloc(info, crate::closure::CAPTURES_THIS_FLAG | 1);
            crate::closure::js_closure_set_capture_bits(
                c,
                0,
                crate::JSValue::object_ptr(obj.cast()).bits(),
            );
            crate::object::store_object_field_slot(
                obj,
                0,
                crate::JSValue::object_ptr(c.cast()).bits(),
            );
            let entries = [super::super::static_shapes::ConstFnStaticEntry { slot: 0, info }];
            let finalized = super::super::static_shapes::js_object_finalize_constfn_static(
                obj as usize as u64,
                0,
                packed.as_ptr(),
                packed.len() as u32,
                1,
                1,
                0,
                super::super::field_rep::REP_SPECIAL,
                entries.as_ptr(),
                1,
            ) as usize as *mut ObjectHeader;
            let id = super::super::shapes::object_shape_stamp(finalized);
            let d = super::super::shapes::shape_descriptor_by_id(id).unwrap();
            assert_eq!(d.special_constfn_mask != 0, admitted);
            if admitted {
                assert_ne!(base, id, "ordinary allocation must finalize after stores");
                assert!(super::super::field_rep_store::final_shape_matches_birth(
                    id, base
                ));
                assert_eq!(primed_slot(finalized), METHOD_SITE_CONSTFN);
                assert_eq!(
                    crate::closure::js_closure_get_capture_bits(c, 0),
                    crate::JSValue::object_ptr(finalized.cast()).bits()
                );
            } else {
                assert_eq!(id, base, "captured-this rebinding remains excluded");
            }
        }
    }
}
