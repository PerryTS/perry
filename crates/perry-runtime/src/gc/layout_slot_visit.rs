//! GC slot-descriptor visitors, split out of `layout.rs` to keep it under the
//! 2000-line cap. These walk an object's payload and hand the collector either
//! a read-only view of each pointer slot (`visit_gc_layout_slot_descriptors`)
//! or a mutable one for evacuation rewriting (`visit_gc_rewrite_slot_*`).
//! No behaviour change; moved verbatim.

use super::*;

fn fixed_slot(slot: *mut u64) -> GcMutableSlotDescriptor {
    GcMutableSlotDescriptor::Slot(GcMutableSlot::new(slot, None))
}

pub(super) unsafe fn visit_gc_layout_slot_descriptors(
    header: *mut GcHeader,
    visit: &mut dyn FnMut(GcMutableSlotDescriptor),
) {
    let mut child_slots = gc_child_slots(header);
    // Capture the authoritative descriptor facts. `gc_child_slots` has already
    // copied the descriptor's keys edge into the compatibility header scratch
    // slot; a copying visit can rewrite that slot, after which the descriptor
    // table is updated below.
    let object_shape_facts = if (*header).obj_type == GC_TYPE_OBJECT {
        let obj = (header as *mut u8).add(GC_HEADER_SIZE) as *mut crate::object::ObjectHeader;
        let descriptor = crate::object::shapes::object_shape_descriptor(obj);
        let old_keys = descriptor
            .map(|facts| facts.keys as usize as *mut crate::array::ArrayHeader)
            .unwrap_or((*obj).keys_array);
        let live_inline_slot_count = descriptor
            .map(|facts| facts.live_inline_slot_count)
            .unwrap_or((*obj).field_count);
        if old_keys.is_null() {
            Some((obj, 0, 0, live_inline_slot_count))
        } else if crate::value::addr_class::try_read_tracked_gc_header(old_keys as usize)
            .is_some_and(|keys_header| (*keys_header.as_ptr()).obj_type == GC_TYPE_ARRAY)
        {
            // A forwarded tracked array still carries GC_TYPE_ARRAY in
            // its from-space header. The length helper follows that stub,
            // so a sibling whose shared keys edge was already rewritten
            // can still validate against the descriptor's new pointer.
            Some((
                obj,
                old_keys as u64,
                descriptor
                    .map(|facts| facts.logical_key_count)
                    .unwrap_or_else(|| {
                        crate::array::keys_array_len_capped_to_capacity(old_keys) as u32
                    }),
                live_inline_slot_count,
            ))
        } else {
            // Do not dereference corrupt/unmapped header words merely because
            // their sibling word happens to look like a ShapeId. The
            // authoritative header edge below is still enumerated; only
            // redundant descriptor synchronization is skipped.
            None
        }
    } else {
        None
    };
    if let Some(slot) = child_slots.take_prefix_child_slot() {
        visit(fixed_slot(slot).with_layout(HeapChildSlotReadKind::Prefix));
    }
    // #8067: the header keys slot above is the sole strong edge. Once its
    // visitor callback has run, mirror an immediate rewrite into the weak
    // descriptor. Never enumerate the HashMap bucket as a GC slot: dirty-page
    // work may retain enumerated slot addresses across budgeted resumptions,
    // during which descriptor insertion can reallocate the table. A deferred
    // visitor leaves old==new here; the metadata forwarding pass repairs it
    // after copying. RegExp uses its dedicated GC slot kind and never enters
    // the ObjectHeader branch above.
    if let Some((obj, old_keys, logical_key_count, live_inline_slot_count)) = object_shape_facts {
        let new_keys = (*obj).keys_array as u64;
        // Mark, verify, and deferred dirty scans leave the header edge
        // unchanged. Only a copying rewrite needs to borrow and update the
        // weak descriptor table.
        if new_keys != old_keys {
            crate::object::shapes::synchronize_live_object_shape_descriptor_after_header_visit(
                obj,
                old_keys,
                new_keys,
                logical_key_count,
                live_inline_slot_count,
            );
        }
    }
    if let Some(slot) = child_slots.take_meta_child_slot() {
        visit(fixed_slot(slot).with_layout(HeapChildSlotReadKind::Prefix));
    }

    match child_slots.payload_scan() {
        HeapPayloadSlotScan::Empty => {}
        HeapPayloadSlotScan::PointerFree {
            raw_numeric_array,
            raw_numeric_object_slots,
        } => {
            let range = child_slots.payload;
            record_layout_pointer_free_range_skipped(range.slot_count());
            if raw_numeric_array {
                record_layout_raw_numeric_array_range_skipped(range.slot_count());
            }
            if raw_numeric_object_slots != 0 {
                record_layout_raw_numeric_object_field_range_skipped(raw_numeric_object_slots);
            }
            visit(GcMutableSlotDescriptor::PointerFreeRange);
        }
        HeapPayloadSlotScan::AllPointers {
            raw_numeric_object_slots,
        } => {
            if raw_numeric_object_slots != 0 {
                record_layout_raw_numeric_object_field_range_skipped(raw_numeric_object_slots);
            }
            // Same slot set the `Masked` arm below would emit one-at-a-time
            // (`AllPointers` yields every index in `0..slot_count`), handed over
            // as a contiguous range so `scan_dirty_object_slots` can intersect
            // it with the dirty-page set instead of probing that set per slot.
            visit(GcMutableSlotDescriptor::Range {
                range: child_slots.payload,
                layout_kind: Some(HeapChildSlotReadKind::Masked),
            });
        }
        HeapPayloadSlotScan::Masked => {
            for child_slot in child_slots {
                if let HeapChildSlot::Child(slot, layout_kind) = child_slot {
                    visit(GcMutableSlotDescriptor::Slot(GcMutableSlot::new(
                        slot,
                        Some(layout_kind),
                    )));
                }
            }
        }
        HeapPayloadSlotScan::All(range) => visit(GcMutableSlotDescriptor::Range {
            range,
            layout_kind: Some(HeapChildSlotReadKind::Unknown),
        }),
    }
}

impl GcMutableSlotDescriptor {
    #[inline]
    fn with_layout(self, layout_kind: HeapChildSlotReadKind) -> Self {
        match self {
            GcMutableSlotDescriptor::Slot(mut slot) => {
                slot.layout_kind = Some(layout_kind);
                GcMutableSlotDescriptor::Slot(slot)
            }
            other => other,
        }
    }
}

pub(super) unsafe fn visit_gc_rewrite_slot_descriptors(
    header: *mut GcHeader,
    mut visit: impl FnMut(GcMutableSlotDescriptor),
) {
    if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match gc_type_rewrite_descriptor_kind((*header).obj_type) {
        GcRewriteDescriptorKind::Array => {
            visit_gc_layout_slot_descriptors(header, &mut visit);
        }
        GcRewriteDescriptorKind::Object => {
            // #6759 Phase B / #6812: the per-object meta record is a raw-
            // pointer child edge exactly like `keys_array`'s prefix slot.
            // Since the child-slot iterator gained the meta second-prefix
            // (so MARKING sees it too), the layout-descriptor visit below
            // already emits it — no explicit `gc_object_meta_slot` visit
            // here, or the rewrite pass would hand the same slot to the
            // visitor twice and double-count in verification statistics.
            visit_gc_layout_slot_descriptors(header, &mut visit);
            crate::object::visit_overflow_field_slots_mut(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
            // #2820: the recorded `Object.setPrototypeOf` value is a live
            // reference; rewrite it if the prototype object moved.
            crate::object::prototype_chain::visit_object_static_prototype_slot_mut(
                user_ptr as usize,
                |slot| {
                    visit(fixed_slot(slot));
                },
            );
        }
        GcRewriteDescriptorKind::RegExp => {
            visit_gc_layout_slot_descriptors(header, &mut visit);
        }
        GcRewriteDescriptorKind::Closure => {
            visit_gc_layout_slot_descriptors(header, &mut visit);
            crate::closure::visit_closure_dynamic_prop_value_slots_mut(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
            crate::closure::visit_closure_static_prototype_slot_mut(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
        }
        GcRewriteDescriptorKind::Promise => {
            let promise = user_ptr as *mut crate::promise::Promise;
            visit(fixed_slot(&mut (*promise).value as *mut f64 as *mut u64));
            visit(fixed_slot(&mut (*promise).reason as *mut f64 as *mut u64));
            visit(fixed_slot(
                &mut (*promise).on_fulfilled as *mut _ as *mut u64,
            ));
            visit(fixed_slot(
                &mut (*promise).on_rejected as *mut _ as *mut u64,
            ));
            visit(fixed_slot(&mut (*promise).next as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::Error => {
            let error = user_ptr as *mut crate::error::ErrorHeader;
            visit(fixed_slot(&mut (*error).message as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).name as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).stack as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).cause as *mut f64 as *mut u64));
            visit(fixed_slot(&mut (*error).errors as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::Map => {
            let map = user_ptr as *mut crate::map::MapHeader;
            let size = (*map).size;
            let capacity = (*map).capacity;
            // Corruption guard only: mirror Set's 16M bound (set.rs
            // gc_element_slot_range). Every GC walk (mark, copy, rewrite,
            // dirty-scan, verify) funnels through this descriptor, so a
            // lower cap makes larger maps invisible to the collector —
            // entries reachable only through a >cap map would be swept
            // while live and never rewritten after a move.
            if size > capacity || size > 16_000_000 || (*map).entries.is_null() {
                return;
            }
            visit(GcMutableSlotDescriptor::Range {
                range: HeapSlotRange::new((*map).entries as *mut u64, size as usize * 2),
                layout_kind: None,
            });
        }
        GcRewriteDescriptorKind::Set => {
            let set = user_ptr as *mut crate::set::SetHeader;
            if let Some(range) = crate::set::gc_element_slot_range(set) {
                visit(GcMutableSlotDescriptor::Range {
                    range,
                    layout_kind: None,
                });
            }
        }
        GcRewriteDescriptorKind::LazyArray => {
            let lazy = user_ptr as *mut crate::json_tape::LazyArrayHeader;
            if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
                return;
            }
            visit(fixed_slot(&mut (*lazy).blob_str as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*lazy).materialized as *mut _ as *mut u64));
            visit(fixed_slot(
                &mut (*lazy).materialized_elements as *mut _ as *mut u64,
            ));
            visit(fixed_slot(
                &mut (*lazy).materialized_bitmap as *mut _ as *mut u64,
            ));

            let cached_length = (*lazy).cached_length as usize;
            let cache = (*lazy).materialized_elements;
            let bitmap = (*lazy).materialized_bitmap;
            if cache.is_null() || bitmap.is_null() || cached_length == 0 {
                return;
            }
            let bitmap_words = cached_length.div_ceil(64);
            for w in 0..bitmap_words {
                let word = *bitmap.add(w);
                if word == 0 {
                    continue;
                }
                let base_idx = w * 64;
                for b in 0..64usize {
                    if word & (1u64 << b) == 0 {
                        continue;
                    }
                    let i = base_idx + b;
                    if i >= cached_length {
                        break;
                    }
                    visit(fixed_slot(cache.add(i) as *mut u64));
                }
            }
        }
        GcRewriteDescriptorKind::NativeTypedView => {
            let view = user_ptr as *mut crate::native_arena::NativeTypedViewHeader;
            visit(fixed_slot(&mut (*view).owner as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::NativePodView => {
            let view = user_ptr as *mut crate::native_arena::NativePodViewHeader;
            visit(fixed_slot(&mut (*view).owner as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::ObjectMeta => {
            // #6759 Phase B: the recorded custom `[[Prototype]]` is a live
            // reference (NaN-boxed pointer, raw pointer, or the TAG_NULL /
            // 0-unset sentinels, which the slot visitor ignores).
            let meta = user_ptr as *mut crate::object::ObjectMeta;
            visit(fixed_slot(&mut (*meta).prototype as *mut u64));
            // #6812: the object-owned overflow buffer is a raw-pointer child
            // edge (0 = none), traced and rewritten exactly like `prototype`.
            visit(fixed_slot(&mut (*meta).spill as *mut u64));
        }
        GcRewriteDescriptorKind::Leaf => {}
    }
}

pub(super) unsafe fn visit_gc_rewrite_slots(
    header: *mut GcHeader,
    mut visit: impl FnMut(GcMutableSlot),
) {
    visit_gc_rewrite_slot_descriptors(header, |descriptor| unsafe {
        descriptor.visit_slots(&mut visit);
    });
}
