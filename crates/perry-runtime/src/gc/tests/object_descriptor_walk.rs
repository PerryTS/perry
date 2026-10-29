//! Compare the direct ObjectFields enumeration to its iterator reference,
//! including representation masks, both sides of their width limit and meta.
use super::super::*;
use super::support::*;

unsafe fn slots(header: *mut GcHeader, direct: bool) -> Vec<(usize, bool, bool, u8)> {
    let mut out = Vec::new();
    let mut visit = |descriptor| match descriptor {
        GcMutableSlotDescriptor::Slot(slot) => out.push((
            slot.slot as usize,
            slot.is_parent_owned(),
            slot.is_pointer_width(),
            slot.layout_kind.map_or(255, |kind| kind as u8),
        )),
        GcMutableSlotDescriptor::Range { range, layout_kind } => {
            for i in 0..range.slot_count() {
                out.push((
                    range.slot(i) as usize,
                    true,
                    false,
                    layout_kind.map_or(255, |kind| kind as u8),
                ));
            }
        }
        GcMutableSlotDescriptor::PointerFreeRange(range) => {
            out.push((range.slots() as usize, true, false, 254));
        }
    };
    if direct {
        super::super::layout_slot_visit::visit_gc_layout_slot_descriptors_inline::<{ cfg!(test) }, _>(
            header, &mut visit,
        );
    } else {
        super::super::layout_slot_visit::visit_gc_layout_slot_descriptors_generic::<
            { cfg!(test) },
            _,
        >(header, &mut visit);
    }
    out
}

#[test]
fn direct_objects_preserve_slot_order_custody_width_and_representation_masks() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        for width in [0u32, 1, 2, 7, 31, 32, 33, 63, 64, 65, 96] {
            for pattern in [0u64, 1, 0x5555_5555_5555_5555, 0x1111_1111_1111_1111] {
                let (obj, fields) = alloc_nursery_test_object(width);
                // Every F64 lane contains an actual double; other lanes may
                // contain any JSValue. Numeric values are sufficient here.
                for i in 0..width as usize {
                    *fields.add(i) = (i as f64).to_bits();
                }
                // The zero-slot support fixture deliberately has no ShapeId.
                if width != 0 {
                    restamp_with_rep(obj, pattern);
                }
                let header = header_from_user_ptr(obj.cast()) as *mut GcHeader;
                assert_eq!(
                    slots(header, true),
                    slots(header, false),
                    "width {width} rep {pattern:x}"
                );
            }
        }
        let (obj, _) = alloc_nursery_test_object(2);
        crate::object::object_meta_ensure(
            crate::object::MetaObj::runtime_born(obj),
            crate::object::MetaReason::RuntimeHandle,
        );
        let header = header_from_user_ptr(obj.cast()) as *mut GcHeader;
        assert_eq!(slots(header, true), slots(header, false));
    })
    .join()
    .unwrap();
}

#[test]
fn shared_keys_skip_only_an_already_marked_nonforwarded_exact_array() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let arr = crate::array::js_array_alloc(0);
        let mut bits = arr as usize as u64;
        let slot = GcMutableSlot::runtime_root(&mut bits);
        let header = header_from_user_ptr(arr.cast()) as *mut GcHeader;
        let original = (*header).gc_flags;
        for extra in [
            0,
            GC_FLAG_MARKED,
            GC_FLAG_FORWARDED,
            GC_FLAG_MARKED | GC_FLAG_FORWARDED,
        ] {
            (*header).gc_flags = (original & !(GC_FLAG_MARKED | GC_FLAG_FORWARDED)) | extra;
            assert_eq!(
                slot.shared_keys_already_marked(bits),
                extra == GC_FLAG_MARKED
            );
            assert!(!GcMutableSlot::new(&mut bits, None).shared_keys_already_marked(bits));
            assert!(!slot.shared_keys_already_marked(POINTER_TAG | bits));
            assert!(!slot.shared_keys_already_marked(0));
        }
        (*header).gc_flags = original;
    })
    .join()
    .unwrap();
}
