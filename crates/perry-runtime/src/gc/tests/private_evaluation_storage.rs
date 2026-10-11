//! #11163: private storage identities must survive evacuation with their class.
use super::super::*;
use super::support::*;

#[test]
fn private_storage_identity_and_value_survive_copied_minor() {
    private_storage_move(false);
}

#[test]
fn shape_evaluation_brand_and_private_storage_survive_copied_minor() {
    private_storage_move(true);
}

fn private_storage_move(on_shape: bool) {
    let _guard = CopyingNurseryTestGuard::new(3);
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
        if on_shape {
            crate::object::shapes::stamp_evaluation_holder(owner, js_shadow_slot_get(1));
            let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
            assert!(
                (*owner).meta.is_null(),
                "the shape brand must not allocate ObjectMeta"
            );
            // A changed [[Prototype]] cannot erase or change the birth evaluation.
            crate::object::shapes::transition_object_shape_prototype(
                owner,
                crate::object::shapes::PROTO_ID_NULL,
                crate::value::TAG_NULL,
            );
        } else {
            crate::object::stamp_private_evaluation_brand(
                owner,
                f64::from_bits(js_shadow_slot_get(1)),
            );
        }
        let field = crate::string::js_string_from_bytes(b"#v".as_ptr(), 2);
        crate::object::private_lexical_brand_push(f64::from_bits(js_shadow_slot_get(1)));
        crate::object::js_private_field_add(
            f64::from_bits(js_shadow_slot_get(0)),
            CID,
            crate::value::js_nanbox_string(field as i64),
            17.0,
        );
        crate::object::private_lexical_brand_pop();
        // Prime the qualified key and its shape/slot cache before evacuation.
        // The cache must continue reading through the relocated receiver and
        // class identity, without retaining either address (#10501).
        let request = format!("#<perry:private-value:{CID}:#v>");
        let key = crate::string::js_string_from_bytes(request.as_ptr(), request.len() as u32);
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ObjectHeader;
        assert_eq!(
            crate::object::js_object_get_field_by_name_f64(owner, key),
            17.0
        );
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
        if on_shape {
            let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
            assert!((*owner).meta.is_null());
            assert_eq!(
                crate::object::shapes::object_evaluation_brand(owner),
                js_shadow_slot_get(1)
            );
        }
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

#[test]
fn evaluation_holder_full_gc_negative_control() {
    assert!(
        evaluation_holder_survives_full(false),
        "the holder must survive through its shape edge"
    );
    assert!(
        !evaluation_holder_survives_full(true),
        "omitting the holder edge must lose the otherwise unreachable holder"
    );
}

fn evaluation_holder_survives_full(suppress: bool) -> bool {
    let _guard = CopyingNurseryTestGuard::new(3);
    let _no_native_roots = ConservativeScanDisabledGuard::new();
    let _age = super::super::tenuring::set_survivals_for_test(1);
    let _thresholds = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let owner = crate::object::js_object_alloc(0, 2);
        js_shadow_slot_set(0, ptr_bits(owner as usize));
        let class = crate::object::js_object_alloc(62_643, 0);
        js_shadow_slot_set(1, ptr_bits(class as usize));
        crate::object::js_object_mark_class(class as i64);
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
        crate::object::shapes::stamp_evaluation_holder(owner, js_shadow_slot_get(1));
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
        crate::object::shapes::transition_object_shape_prototype(
            owner,
            crate::object::shapes::PROTO_ID_NULL,
            crate::value::TAG_NULL,
        );
        // Full collection first drains the nursery, whose young-word log
        // deliberately roots every candidate. Promote the class while it is
        // explicitly rooted so the negative control isolates the full edge.
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(trace.copying_nursery.promoted_objects > 0);
        let class = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
        assert!(!crate::arena::pointer_in_nursery(class));
        // The class survives solely through the receiver's birth-shape edge.
        js_shadow_slot_set(1, 0);
        let _control =
            suppress.then(crate::object::shapes::TestKeysEdgeSuppression::without_descriptor_edge);
        let before = gc_collection_count();
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
        assert!(
            gc_collection_count() > before,
            "test must complete a full collection"
        );
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
        crate::object::shapes::object_evaluation_brand(owner) != 0
    }
}
