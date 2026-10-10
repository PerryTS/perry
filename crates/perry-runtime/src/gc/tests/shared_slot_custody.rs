//! Shared descriptor words have one remembering authority: their root scanner.
//! Ordinary external buffers still need their individual owner's descriptor.

use super::super::*;
use super::support::*;
use crate::object::shapes;

const SIBLINGS: usize = 4096;

/// Old carriers of one YOUNG keys array. Numeric fields make the shared word
/// the only tracking edge, so per-parent pollution cannot hide in other work.
unsafe fn old_siblings(count: usize) -> (*mut crate::ArrayHeader, Vec<*mut GcHeader>, u32) {
    let keys = crate::array::js_array_alloc(2);
    js_shadow_slot_set(1, ptr_bits(keys as usize));
    for name in ["shared_slot_first_key", "shared_slot_second_key"] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let keys = (js_shadow_slot_get(1) & POINTER_MASK) as *mut crate::ArrayHeader;
        let next = crate::array::js_array_push(keys, crate::JSValue::string_ptr(key));
        js_shadow_slot_set(1, ptr_bits(next as usize));
    }
    let keys = (js_shadow_slot_get(1) & POINTER_MASK) as *const crate::ArrayHeader;
    assert!(crate::arena::pointer_in_nursery(keys as usize));
    let id = shapes::shape_descriptor_ensure(keys, 2, 2).expect("published shape");
    let (root, elements) = alloc_old_test_array(count as u32);
    js_shadow_slot_set(0, ptr_bits(root as usize));
    let mut headers = Vec::new();
    for i in 0..count {
        let (obj, fields) = alloc_old_test_object(2);
        shapes::stamp_object_shape_id_with_carrier_note(obj, id);
        *fields = (i as f64).to_bits();
        *fields.add(1) = 17.0f64.to_bits();
        *elements.add(i) = ptr_bits(obj as usize);
        layout_note_slot(root as usize, i, *elements.add(i));
        headers.push(header_from_user_ptr(obj.cast()));
    }
    js_shadow_slot_set(1, crate::value::TAG_UNDEFINED);
    assert!(shapes::shape_descriptor_by_id(id).unwrap().old_carrier);
    (root, headers, id)
}

#[test]
fn full_mark_does_not_remember_one_shared_keys_word_for_every_carrier() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    unsafe {
        let (_, headers, id) = old_siblings(SIBLINGS);
        let keys = shapes::shape_descriptor_by_id(id).unwrap().keys;
        let keys_slot = shapes::shape_descriptor_by_id(id)
            .unwrap()
            .keys_slot()
            .unwrap();
        let mut shared_words = 0;
        let mut sticky = StickyRememberedSet::default();
        let valid = build_valid_pointer_set();
        let mut worklist = Vec::new();
        for header in headers {
            visit_gc_rewrite_slots(header, |slot| {
                if slot.slot == keys_slot {
                    shared_words += 1;
                }
            });
            trace::trace_heap_rewrite_slots_remembering(
                header,
                &valid,
                &mut worklist,
                Some(&mut sticky),
            );
        }
        assert_eq!(
            shared_words, SIBLINGS,
            "every receiver reaches exactly the same word"
        );
        assert_ne!(
            (*header_from_user_ptr(keys as *const u8)).gc_flags & GC_FLAG_MARKED,
            0,
            "custody must not suppress tracing the shared edge"
        );
        assert!(
            sticky.external_pages.is_empty(),
            "the shared keys word acquired {} redundant parent entries",
            sticky.external_pages.len()
        );
        assert!(
            sticky.old_pages.is_empty(),
            "numeric fields need no remembering"
        );
        clear_marks();
        clear_mark_seeds();
    }
}

#[test]
fn old_same_shape_graph_survives_copying_minors_and_full_cycles_without_external_replay() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trace = TestGcTraceCaptureGuard::force_enabled();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let _tenuring =
        crate::gc::tenuring::set_survivals_for_test(crate::gc::tenuring::GC_TENURING_SURVIVALS_MAX);
    gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
    unsafe {
        let (_, _, id) = old_siblings(512);
        let expected = b"shared-slot-payload-survives";
        for round in 0..4 {
            let mut old_values = Vec::new();
            for i in 0..512 {
                // The first carrier dies after the first full trace. Coverage
                // must not depend on the lifetime of a chosen representative.
                if round > 0 && i == 0 {
                    continue;
                }
                let root = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ArrayHeader;
                let obj = crate::array::js_array_get(root, i).as_pointer::<crate::ObjectHeader>();
                let fields = (obj as *mut u8)
                    .add(std::mem::size_of::<crate::ObjectHeader>())
                    .cast::<u64>();
                let value =
                    crate::string::js_string_from_bytes(expected.as_ptr(), expected.len() as u32);
                *fields.add(1) = string_bits(value as usize);
                layout_note_slot(obj as usize, 1, *fields.add(1));
                runtime_write_barrier_slot(obj as usize, fields.add(1) as usize, *fields.add(1));
                old_values.push((fields.add(1), value as usize));
            }
            for _ in 0..2 {
                let before: Vec<_> = old_values.iter().map(|(slot, _)| **slot).collect();
                let trace = collect_minor_trace(GcTriggerKind::Direct);
                assert!(
                    trace.copying_nursery.copied_objects > 0,
                    "a real copying minor is required"
                );
                for ((slot, _), before) in old_values.iter().zip(before) {
                    assert_ne!(**slot, before, "the ordinary field edge must actually move");
                    assert_string_bytes(
                        (**slot & POINTER_MASK) as *const crate::StringHeader,
                        expected,
                    );
                }
                assert!(
                    EXTERNAL_DIRTY_SLOT_PAGES.with(|p| p.borrow().is_empty()),
                    "shared shape words must not enter the per-parent external log"
                );
                let descriptor = shapes::shape_descriptor_by_id(id).expect("live shape");
                let keys = descriptor.keys as *const crate::ArrayHeader;
                assert_eq!((*keys).length, 2);
                assert_string_bytes(
                    crate::array::js_array_get(keys, 0).as_string_ptr(),
                    b"shared_slot_first_key",
                );
            }
            if round == 0 {
                let root = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ArrayHeader;
                let elements = crate::array::array_elements_ptr(root) as *mut u64;
                *elements = crate::value::TAG_UNDEFINED;
            }
            let full = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(
                GcTriggerKind::Direct,
            ));
            let full_trace = full.trace.expect("a real full cycle is required");
            let restore = full_trace
                .phase_us
                .get("remembered_set_restore")
                .expect("full replay must have an attributed phase");
            assert!(
                full_trace.phase_us["reclaim"]
                    >= *restore + full_trace.phase_us["remembered_set_clear"],
                "reclaim must include clear and restore, even on the synchronous full path"
            );
            let valid = build_valid_pointer_set();
            let descriptor = shapes::shape_descriptor_by_id(id).expect("live shape after full");
            assert!(
                valid.contains(&(descriptor.keys as usize)),
                "full must retain the keys array"
            );
            let root = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::ArrayHeader;
            assert_eq!((*root).length, 512);
            for i in 1..512 {
                let obj = crate::array::js_array_get(root, i).as_pointer::<crate::ObjectHeader>();
                assert!(
                    valid.contains(&(obj as usize)),
                    "full must retain every remaining receiver"
                );
                assert_eq!((*obj).parent_class_id, id);
                let fields = (obj as *const u8)
                    .add(std::mem::size_of::<crate::ObjectHeader>())
                    .cast::<u64>();
                assert_eq!(f64::from_bits(*fields), i as f64);
                let value = (*fields.add(1) & POINTER_MASK) as usize;
                assert!(
                    valid.contains(&value),
                    "full must retain ordinary field targets"
                );
                assert_string_bytes(value as *const crate::StringHeader, expected);
            }
        }
    }
}
