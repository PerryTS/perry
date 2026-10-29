//! First dynamic-key growth publishes Object -> ObjectMeta -> overflow edges.
//! Both holder generations matter independently; a copying minor must follow
//! and rewrite the whole chain even when young stores skip remembering.
use super::super::*;
use super::support::*;

fn owner() -> *mut crate::object::ObjectHeader {
    (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader
}

fn check_first_dynamic_key(old_owner: bool, old_meta: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    let obj = crate::object::js_object_alloc(0, 0);
    js_shadow_slot_set(0, ptr_bits(obj as usize));
    if old_meta {
        unsafe {
            crate::object::object_meta_ensure(
                crate::object::MetaObj::runtime_born(obj),
                crate::object::MetaReason::RuntimeHandle,
            )
        };
    }
    if old_owner {
        for _ in 0..8 {
            if crate::arena::pointer_in_old_gen(owner() as usize) {
                break;
            }
            let trace = collect_minor_trace(GcTriggerKind::Direct);
            assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        }
        assert!(crate::arena::pointer_in_old_gen(owner() as usize));
    }
    unsafe {
        let obj = owner();
        if old_meta {
            assert!(crate::arena::pointer_in_old_gen((*obj).meta as usize));
        } else {
            assert!(
                (*obj).meta.is_null(),
                "the add must install the first meta edge"
            );
        }
        let child = crate::object::js_object_alloc(0, 0);
        assert!(crate::arena::pointer_in_nursery(child as usize));
        let key = crate::string::js_string_from_bytes(b"dynamicGrowth".as_ptr(), 13);
        let atom = crate::string::property_atom_from_value(crate::JSValue::string_ptr(key).bits());
        reset_remembered_set();
        let slot = crate::object::overflow::insert_key_bits(obj, atom, ptr_bits(child as usize), 0);
        let meta = (*obj).meta;
        let table = (*meta).overflow as usize;
        assert!(crate::arena::pointer_in_nursery(table));
        if old_owner {
            assert!(
                remembered_set_size() > 0,
                "the old holder must remember its new young edge"
            );
        } else {
            assert!(crate::arena::pointer_in_nursery(meta as usize));
        }
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        let after = owner();
        assert_eq!(after == obj, old_owner);
        if old_meta {
            assert_eq!((*after).meta, meta);
        } else {
            assert_ne!((*after).meta, meta, "young metadata must be rewritten");
        }
        assert_ne!(
            (*(*after).meta).overflow as usize,
            table,
            "young overflow must move"
        );
        let child_after = crate::object::overflow::get(after, slot).unwrap() & POINTER_MASK;
        assert_ne!(
            child_after, child as u64,
            "the table must trace and rewrite its value"
        );
        assert!(crate::arena::pointer_in_nursery(child_after as usize));
    }
}

#[test]
fn fresh_young_dynamic_growth_rewrites_the_whole_chain() {
    check_first_dynamic_key(false, false);
}

#[test]
fn old_receiver_remembers_its_first_metadata_edge() {
    check_first_dynamic_key(true, false);
}

#[test]
fn old_metadata_remembers_its_first_overflow_edge() {
    check_first_dynamic_key(true, true);
}
