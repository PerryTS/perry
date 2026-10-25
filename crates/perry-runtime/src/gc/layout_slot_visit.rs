//! GC slot-descriptor visitors, split out of `layout.rs` to keep it under the
//! 2000-line cap. These walk an object's payload and hand the collector either
//! a read-only view of each pointer slot (`visit_gc_layout_slot_descriptors`)
//! or a mutable one for evacuation rewriting (`visit_gc_rewrite_slot_*`).
//! No behaviour change; moved verbatim.

use super::*;

fn fixed_slot(slot: *mut u64) -> GcMutableSlotDescriptor {
    GcMutableSlotDescriptor::Slot(GcMutableSlot::new(slot, None))
}

fn pointer_slot(slot: *mut usize) -> GcMutableSlotDescriptor {
    GcMutableSlotDescriptor::Slot(GcMutableSlot::pointer(slot))
}

impl HeapChildSlotIterator {
    /// The payload mask WORD for a `Masked` selection whose mask is
    /// [`LayoutSlotMask::Inline`]: exactly the slot indices [`Self::next`]
    /// would yield, in the same ascending order, so the caller can walk them
    /// with `trailing_zeros` and `word &= word - 1` instead of re-entering the
    /// iterator once per slot. `None` for every other selection, which keeps
    /// iterating through `next`.
    ///
    /// `next` re-dispatched the selection, re-decoded the mask's niche and
    /// rebuilt the limit and cursor masks FOR EVERY SLOT, for about eight
    /// instructions of work.
    ///
    /// Equivalence, since this replaces the whole iteration:
    /// * `next` stops at `slot_count` and at 64 (an inline mask holds no bit
    ///   above 63), so the eligible set is the mask under both limits — which
    ///   is what this returns;
    /// * it takes the ONE-SHOT raw-numeric accounting with it, exactly as
    ///   `next`'s first call performs it, so the counters see one record per
    ///   traced object either way; and
    /// * it leaves the cursor at the end, so a later `next` yields nothing.
    ///
    /// The prefix and meta slots are NOT its business: every caller takes them
    /// with `take_prefix_child_slot` / `take_meta_child_slot{,2}` before it
    /// reaches the payload, so they are already `None` here. The debug
    /// assertion below is what keeps that true.
    pub(super) fn take_inline_mask_word(&mut self) -> Option<u64> {
        // Silent loss of a prefix/meta edge is the one way this can go wrong
        // without disagreeing with `next` on any payload index, so it is
        // asserted rather than argued.
        debug_assert!(
            self.prefix_slot.is_none() && self.meta_slot.is_none() && self.meta_slot2.is_none(),
            "the inline walk covers the PAYLOAD only; the caller takes the prefix and meta edges first"
        );
        let slot_count = self.payload.slot_count();
        let HeapPayloadSlotSelection::Masked {
            mask: LayoutSlotMask::Inline(bits),
            cursor,
            raw_numeric_object_slots,
            raw_numeric_recorded,
        } = &mut self.selection
        else {
            return None;
        };
        debug_assert_eq!(*cursor, 0, "the inline walk replaces the whole iteration");
        if !*raw_numeric_recorded {
            *raw_numeric_recorded = true;
            if *raw_numeric_object_slots != 0 {
                record_layout_raw_numeric_object_field_range_skipped(*raw_numeric_object_slots);
            }
        }
        let bits = *bits;
        *cursor = slot_count;
        let limit = slot_count.min(64);
        let limit_mask = if limit == 64 {
            u64::MAX
        } else {
            (1u64 << limit) - 1
        };
        let word = bits & limit_mask;
        #[cfg(test)]
        let word = inline_mask_sabotage::perturb(word);
        Some(word)
    }
}

pub(super) unsafe fn visit_gc_layout_slot_descriptors(
    header: *mut GcHeader,
    visit: &mut dyn FnMut(GcMutableSlotDescriptor),
) {
    visit_gc_layout_slot_descriptors_inline(header, visit);
}

/// The ONE body of [`visit_gc_layout_slot_descriptors`], generic over the
/// visitor. The copying drain instantiates it directly; full marking and
/// retained-parent rebuilding use the shared `dyn` wrapper. Specializing those
/// two walks increased TS instructions in both isolated application modes
/// without reducing peak RSS. Enumeration remains one body in either case.
#[inline(always)]
pub(super) unsafe fn visit_gc_layout_slot_descriptors_inline<F>(
    header: *mut GcHeader,
    visit: &mut F,
) where
    F: FnMut(GcMutableSlotDescriptor) + ?Sized,
{
    #[cfg(target_pointer_width = "32")]
    if (*header).obj_type == GC_TYPE_REGEXP {
        for slot in crate::regex::regex_native_slots((header as *mut u8).add(GC_HEADER_SIZE)) {
            visit(pointer_slot(slot));
        }
        return;
    }
    let mut child_slots = gc_child_slots(header);
    // #8112: the authoritative ordered-keys edge, taken from the shape record
    // `gc_child_slots` already resolved for this receiver. It is the boxed
    // record's OWN `keys` word, so the collector marks through it and rewrites
    // it in place — the record is the root and the rewritable location.
    //
    // Never enumerate the HashMap BUCKET as a GC slot: dirty-page work may
    // retain enumerated slot addresses across budgeted resumptions, during
    // which descriptor insertion can reallocate the table. Boxing the record
    // is what answers that — the bucket moves, the record does not.
    //
    // Liveness is an ephemeron relation with two halves. A YOUNG carrier is
    // traced, so emitting the edge here marks the keys array exactly while
    // that receiver lives. An OLD carrier is not traced by a minor at all —
    // and because the record is SHARED, one sibling's rewrite creates an
    // old→young edge for a parent the minor never visits, which no per-parent
    // remembered-set page can describe. That half is the `old_carrier` gate
    // armed below and rooted by `shapes::scan_shape_table_rekey_mut`.
    // `PERRY_GC_VERIFY_EVACUATION` is what established the second half is
    // needed: without it the verifier aborts on a `slot_page_ever_dirty=false`
    // old→young edge through this word.
    let shape_prototype_edge = if (*header).obj_type == GC_TYPE_OBJECT {
        crate::object::gc_shape_prototype_edge_slot(child_slots.object_shape, full_trace_active())
    } else {
        None
    };
    let shape_keys_edge = if (*header).obj_type == GC_TYPE_OBJECT {
        // #9726: unlike the minor-rooting gate below, full-trace descriptor
        // liveness is generation-blind. Every reachable shaped receiver must
        // note the exact id it carries before synchronous-full pruning.
        if full_trace_active() {
            crate::object::shapes::note_full_trace_carrier(child_slots.object_shape);
        }
        // A receiver the minor will not enumerate for itself arms the table's
        // ephemeron gate. The test is "not in the nursery", not "in old-gen":
        // a `gc_malloc`'d large object and an immortal bootstrap resident are
        // both outside the young generation and both invisible to a minor, and
        // over-arming the gate only costs one extra rooted record until the
        // next full trace recomputes it. A to-space survivor IS nursery, so a
        // young carrier still relies on the edge emitted just below.
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        // The copying drain uses this same exact no-op test. Once a shape's
        // old-carrier flags are set for the epoch, another carrier changes
        // neither flag nor the candidate list. Avoid its generation lookup
        // and the repeated note; still enumerate the shared keys edge below.
        if !crate::object::shapes::old_generation_carrier_already_noted(child_slots.object_shape)
            && !crate::arena::pointer_in_nursery(user_ptr as usize)
        {
            crate::object::shapes::note_old_generation_carrier(child_slots.object_shape);
        }
        crate::object::gc_shape_keys_edge_slot(child_slots.object_shape)
    } else {
        None
    };
    if let Some(slot) = child_slots.take_prefix_child_slot() {
        visit(fixed_slot(slot).with_layout(HeapChildSlotReadKind::Prefix));
    }
    if let Some(slot) = shape_keys_edge {
        visit(
            GcMutableSlotDescriptor::Slot(GcMutableSlot::runtime_root(slot))
                .with_layout(HeapChildSlotReadKind::Prefix),
        );
    }
    // The receiver's [[Prototype]] when its shape names it: the identity's
    // shared word, marked through like the keys word.
    if let Some(slot) = shape_prototype_edge {
        visit(
            GcMutableSlotDescriptor::Slot(GcMutableSlot::runtime_root(slot))
                .with_layout(HeapChildSlotReadKind::Prefix),
        );
    }
    if (*header).obj_type == GC_TYPE_OBJECT {
        if let Some(slot) = crate::object::shapes::evaluation_holder_slot(
            child_slots.object_shape,
            full_trace_active(),
        ) {
            visit(
                GcMutableSlotDescriptor::Slot(GcMutableSlot::runtime_root(slot))
                    .with_layout(HeapChildSlotReadKind::Prefix),
            );
        }
    }
    if let Some(slot) = child_slots.take_meta_child_slot() {
        // The meta word of an object, closure or array is a raw machine
        // pointer (`ObjectHeader::meta`, `ClosureHeader::props`,
        // `ArrayHeader::named`), not an eight-byte JS value. On wasm32 it is
        // four bytes followed by padding, so reading it as a `u64` would fold
        // uninitialised padding into the address and hide the edge.
        let descriptor = if matches!(
            (*header).obj_type,
            GC_TYPE_OBJECT | GC_TYPE_CLOSURE | GC_TYPE_ARRAY
        ) {
            pointer_slot(slot.cast())
        } else {
            fixed_slot(slot)
        };
        visit(descriptor.with_layout(HeapChildSlotReadKind::Prefix));
    }
    if let Some(slot) = child_slots.take_meta_child_slot2() {
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
            visit(GcMutableSlotDescriptor::PointerFreeRange(range));
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
            // An inline mask's set bits ARE the slot indices, in ascending
            // order: take the word once and walk it, instead of re-entering
            // `next` per slot to re-dispatch the selection and rebuild the
            // same two masks. Every other mask — `Heap`, i.e. more than 64
            // payload slots — keeps the iterator.
            if let Some(mut word) = child_slots.take_inline_mask_word() {
                let payload = child_slots.payload;
                while word != 0 {
                    let index = word.trailing_zeros() as usize;
                    word &= word - 1;
                    visit(GcMutableSlotDescriptor::Slot(GcMutableSlot::new(
                        payload.slot(index),
                        Some(HeapChildSlotReadKind::Masked),
                    )));
                }
            } else {
                // Iterate by reference: `for .. in child_slots` moves the
                // iterator into the loop, a copy per traced object (#10362).
                for child_slot in &mut child_slots {
                    if let HeapChildSlot::Child(slot, layout_kind) = child_slot {
                        visit(GcMutableSlotDescriptor::Slot(GcMutableSlot::new(
                            slot,
                            Some(layout_kind),
                        )));
                    }
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
    visit: impl FnMut(GcMutableSlotDescriptor),
) {
    #[cfg(test)]
    if (*header).obj_type == crate::gc::GC_TYPE_NATIVE_HANDLE
        && crate::native_payload::callback_sabotage("rewrite")
    {
        return;
    }
    visit_gc_rewrite_slot_descriptors_with::<false>(header, visit);
}

/// The one body of [`visit_gc_rewrite_slot_descriptors`]. `INLINE_LAYOUT`
/// selects only HOW the layout-descriptor walk is called — through the shared
/// `dyn` copy, or instantiated for this visitor — never what it enumerates.
#[inline(always)]
unsafe fn visit_gc_rewrite_slot_descriptors_with<const INLINE_LAYOUT: bool>(
    header: *mut GcHeader,
    mut visit: impl FnMut(GcMutableSlotDescriptor),
) {
    macro_rules! layout_descriptors {
        () => {
            if INLINE_LAYOUT {
                visit_gc_layout_slot_descriptors_inline(header, &mut visit)
            } else {
                visit_gc_layout_slot_descriptors(header, &mut visit)
            }
        };
    }
    if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    match gc_type_rewrite_descriptor_kind((*header).obj_type) {
        GcRewriteDescriptorKind::Array => {
            layout_descriptors!();
            let array = user_ptr as *mut crate::array::ArrayHeader;
            if (*header)._reserved & crate::gc::GC_ARRAY_KEY_ATTRS != 0 {
                visit(fixed_slot(crate::array::key_attrs_slot(array)));
            }
        }
        GcRewriteDescriptorKind::Object => {
            // #6759 Phase B / #6812: the per-object meta record is a raw-
            // pointer child edge exactly like `keys_array`'s prefix slot.
            // Since the child-slot iterator gained the meta second-prefix
            // (so MARKING sees it too), the layout-descriptor visit below
            // already emits it — no explicit `gc_object_meta_slot` visit
            // here, or the rewrite pass would hand the same slot to the
            // visitor twice and double-count in verification statistics.
            layout_descriptors!();
        }
        GcRewriteDescriptorKind::RegExp => {
            layout_descriptors!();
        }
        GcRewriteDescriptorKind::Closure => {
            // Captures and the own-property bag edge (`ClosureCaptures`).
            layout_descriptors!();
        }
        GcRewriteDescriptorKind::Promise => {
            let promise = user_ptr as *mut crate::promise::Promise;
            visit(fixed_slot(&mut (*promise).value as *mut f64 as *mut u64));
            visit(fixed_slot(&mut (*promise).reason as *mut f64 as *mut u64));
            visit(pointer_slot(
                &mut (*promise).on_fulfilled as *mut _ as *mut usize,
            ));
            visit(pointer_slot(
                &mut (*promise).on_rejected as *mut _ as *mut usize,
            ));
            visit(pointer_slot(&mut (*promise).next as *mut _ as *mut usize));
            if crate::async_hooks::resource_state::get((*promise).async_id).is_some() {
                visit(pointer_slot(
                    std::ptr::addr_of_mut!((*promise).async_id).cast(),
                ));
            }
            // #6759 phase 1: the metadata edge (MARK path as well as rewrite).
            visit(pointer_slot(&mut (*promise).meta as *mut _ as *mut usize));
            visit(pointer_slot(
                &mut (*promise).callback_context as *mut _ as *mut usize,
            ));
        }
        GcRewriteDescriptorKind::Symbol => {
            let symbol = user_ptr as *mut crate::symbol::SymbolHeader;
            if !(*symbol).description.is_null() {
                visit(pointer_slot(
                    std::ptr::addr_of_mut!((*symbol).description).cast::<usize>(),
                ));
            }
        }
        GcRewriteDescriptorKind::AsyncResourceState => {
            let state = user_ptr as *mut crate::async_hooks::resource_state::ResourceState;
            visit(fixed_slot(std::ptr::addr_of_mut!((*state).resource).cast()));
            visit(fixed_slot(
                std::ptr::addr_of_mut!((*state).lease_resource).cast(),
            ));
            crate::async_context::for_each_snapshot_store_slot(
                &mut (*state).context,
                &mut |slot| {
                    visit(fixed_slot(slot.cast()));
                },
            );
        }
        GcRewriteDescriptorKind::AsyncSnapshot => {
            let holder = user_ptr as *mut crate::async_context::AsyncSnapshotHolder;
            crate::async_context::for_each_snapshot_store_slot(
                &mut (*holder).snapshot,
                &mut |slot| {
                    visit(fixed_slot(slot.cast::<u64>()));
                },
            );
        }
        GcRewriteDescriptorKind::Error => {
            let error = user_ptr as *mut crate::error::ErrorHeader;
            visit(pointer_slot(&mut (*error).message as *mut _ as *mut usize));
            visit(pointer_slot(&mut (*error).name as *mut _ as *mut usize));
            visit(pointer_slot(&mut (*error).stack as *mut _ as *mut usize));
            visit(fixed_slot(&mut (*error).cause as *mut f64 as *mut u64));
            visit(pointer_slot(&mut (*error).errors as *mut _ as *mut usize));
            // #6759 phase 1: the metadata edge. This arm is reached by
            // `trace_heap_rewrite_slots`, so visiting the slot here both MARKS
            // the meta record (keeping it, and anything reachable only through
            // it, alive) and rewrites the edge when evacuation moves it.
            visit(pointer_slot(&mut (*error).meta as *mut _ as *mut usize));
            // #9486: the captured-frames blob. Same shape as `stack` beside
            // it — a `StringHeader` edge, null until captured and null again
            // once `.stack` has been materialised — so it needs exactly this
            // one line and no new descriptor kind.
            visit(pointer_slot(&mut (*error).frames as *mut _ as *mut usize));
        }
        GcRewriteDescriptorKind::Map => {
            let map = user_ptr as *mut crate::map::MapHeader;
            visit(pointer_slot(std::ptr::addr_of_mut!((*map).store).cast()));
            visit(pointer_slot(std::ptr::addr_of_mut!((*map).meta).cast()));
        }
        GcRewriteDescriptorKind::MapStore => {
            if let Some(range) = crate::map::owned_entry_slot_range(user_ptr) {
                visit(GcMutableSlotDescriptor::Range {
                    range,
                    layout_kind: None,
                });
            }
        }
        GcRewriteDescriptorKind::Set => {
            let set = user_ptr as *mut crate::set::SetHeader;
            visit(pointer_slot(std::ptr::addr_of_mut!((*set).store).cast()));
            visit(pointer_slot(std::ptr::addr_of_mut!((*set).meta).cast()));
        }
        GcRewriteDescriptorKind::Proxy => {
            let proxy = user_ptr.cast::<crate::proxy::ProxyEntry>();
            visit(GcMutableSlotDescriptor::Range {
                range: HeapSlotRange::new(std::ptr::addr_of_mut!((*proxy).target).cast(), 2),
                layout_kind: None,
            });
            visit(pointer_slot(std::ptr::addr_of_mut!((*proxy).props).cast()));
        }
        GcRewriteDescriptorKind::SetStore => {
            if let Some(range) = crate::set::owned_element_slot_range(user_ptr) {
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
            visit(pointer_slot(&mut (*lazy).blob_str as *mut _ as *mut usize));
            visit(pointer_slot(
                &mut (*lazy).materialized as *mut _ as *mut usize,
            ));
            visit(pointer_slot(
                &mut (*lazy).materialized_elements as *mut _ as *mut usize,
            ));
            visit(pointer_slot(
                &mut (*lazy).materialized_bitmap as *mut _ as *mut usize,
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

        GcRewriteDescriptorKind::NativePodView => {
            let view = user_ptr as *mut crate::native_arena::NativePodViewHeader;
            visit(pointer_slot(&mut (*view).owner as *mut _ as *mut usize));
        }
        GcRewriteDescriptorKind::Overflow => {
            crate::object::overflow::visit_slots(user_ptr.cast(), |slot| visit(fixed_slot(slot)));
        }
        GcRewriteDescriptorKind::ObjectMeta => {
            // #6759 Phase B: the recorded custom `[[Prototype]]` is a live
            // reference (NaN-boxed pointer, raw pointer, or the TAG_NULL /
            // 0-unset sentinels, which the slot visitor ignores).
            let meta = user_ptr as *mut crate::object::ObjectMeta;
            let birth_word = ((*meta).instance_birth >> 32) as u32;
            if birth_word != 0 {
                let birth = crate::object::shapes::shape_record_for_word(birth_word);
                if full_trace_active() {
                    crate::object::shapes::note_full_trace_carrier(birth);
                }
                crate::object::shapes::note_old_generation_carrier(birth);
                if let Some(slot) = crate::object::gc_shape_keys_edge_slot(birth) {
                    visit(fixed_slot(slot));
                }
                if let Some(slot) = crate::object::gc_shape_prototype_edge_slot(birth, full_trace_active()) {
                    visit(fixed_slot(slot));
                }
            }
            visit(fixed_slot(&mut (*meta).prototype as *mut u64));
            // The private boxed primitive is a child edge (string, BigInt,
            // or Symbol), with scalars ignored by the ordinary slot visitor.
            // Most metadata records have no boxed payload; zero cannot name
            // a GC child (including the numeric payload +0).
            if (*meta).boxed_primitive != 0 {
                visit(fixed_slot(&mut (*meta).boxed_primitive as *mut u64));
            }
            if (*meta).reflect_metadata != 0 {
                visit(fixed_slot(&mut (*meta).reflect_metadata as *mut u64));
            }
            // #6812: the object-owned overflow buffer is a raw-pointer child
            // edge (0 = none), traced and rewritten exactly like `prototype`.
            visit(fixed_slot(&mut (*meta).spill as *mut u64));
            // #6759 phase 1: the named-property bag for a cell with no inline
            // slot layout (an Error, say). Reachable ONLY through this record,
            // so an unvisited edge here collects a live object's own
            // properties — the same shape as the spill hazard above (#6812).
            visit(fixed_slot(&mut (*meta).expando as *mut u64));
            // The Array-subclass elements store (0 = none): a raw-pointer child
            // edge traced and rewritten exactly like `spill`.
            visit(fixed_slot(&mut (*meta).elements as *mut u64));
            // #10868 step 2.5 stage 1: a dictionary-mode receiver's private
            // ordered key list. Reachable ONLY through this record, so an
            // unvisited edge here collects a live object's own property NAMES
            // — the same shape as the spill hazard above (#6812). This single
            // `visit` is mark, evacuation-rewrite and dirty-slot-rescan
            // coverage at once, because this function is the one enumerator
            // all three drive; `overflow_survive_a_moving_collection`
            // reddens if it is removed.
            visit(fixed_slot(&mut (*meta).overflow as *mut u64));
            // #11506: a sloppy mapped `arguments` object's alias array (a
            // NaN-boxed pointer; the unmapped states are scalars the slot
            // visitor skips). Reachable ONLY through this record, like
            // `overflow`; `arguments_mapping_survives_a_moving_collection`
            // reddens if this visit is removed.
            visit(fixed_slot(&mut (*meta).arguments as *mut u64));
            // A fresh class object stored as an instance's private evaluation
            // brand is a NaN-boxed child edge and moves with the meta record.
            visit(fixed_slot(
                &mut (*meta).private_evaluation_brand as *mut u64,
            ));
            if (*meta).owned_state != 0 {
                visit(fixed_slot(&mut (*meta).owned_state as *mut u64));
            }
            // Native payload and weak-collection `native_state` is its
            // POINTER_TAG-boxed owned cell, reachable ONLY through this
            // record, so it is a child edge exactly like `arguments`. A plain
            // stream's state record (an array) is the same kind of edge. Other
            // families pack POD into the same word (text bits, timer/tui ids,
            // a Set's malloc'd index, a class's private-storage serial) and
            // none of those words carries the pointer tag, so they are never
            // visited; `native_payload_cell_survives_a_moving_collection`
            // reddens if this visit is removed.
            if crate::native_payload::is_payload_state_word((*meta).native_state) {
                #[cfg(test)]
                if crate::node_stream::native_hooks::stream_sabotage("record_trace")
                    && crate::node_stream::is_stream_record_word((*meta).native_state)
                {
                    return;
                }
                visit(fixed_slot(&mut (*meta).native_state as *mut u64));
            }
        }
        GcRewriteDescriptorKind::MetaOnly => {
            // #6759 phase 1: the cell's only traced edge is its metadata
            // record. Reached by `trace_heap_rewrite_slots`, so this is the
            // MARK path as well as the rewrite path.
            if let Some(slot) = crate::object::cell_meta_slot(user_ptr as usize) {
                visit(pointer_slot(slot.cast()));
            }
        }
        GcRewriteDescriptorKind::Buffer => {
            #[cfg(test)]
            let trace_link = !crate::buffer::bytes::b4_sabotage("view_edge");
            #[cfg(not(test))]
            let trace_link = true;
            if trace_link {
                if let Some(slot) = crate::buffer::store::gc_link_slot(user_ptr as usize) {
                    visit(pointer_slot(slot));
                }
            }
        }
        GcRewriteDescriptorKind::WeakStorage => {
            let storage = user_ptr as *mut crate::weakref::storage::WeakStorage;
            for i in 0..(*storage).len {
                let entry = (*storage).entries().add(i as usize);
                // Free-list words are scalars, never JS edges.
                if (*entry).key != crate::value::TAG_UNDEFINED {
                    visit(fixed_slot(&mut (*entry).key));
                    visit(fixed_slot(&mut (*entry).value));
                }
            }
        }
        GcRewriteDescriptorKind::Box => visit(fixed_slot(user_ptr as *mut u64)),
        GcRewriteDescriptorKind::Scope => {
            let slots = user_ptr as *mut u64;
            for i in 0..crate::r#box::scope::scope_slot_count(header) {
                visit(fixed_slot(slots.add(i)));
            }
        }
        GcRewriteDescriptorKind::NativeHandle => {
            let cell = user_ptr as *mut crate::native_handle::NativeHandleHeader;
            // One enumerator serves mark, relocation and dirty-slot rescan.
            // Released cells keep tracing their owner; only finalized cells stop.
            if (*cell).finalized == 0 {
                if (*cell).owner != 0 {
                    visit(fixed_slot(&mut (*cell).owner as *mut u64));
                }
                #[cfg(test)]
                if crate::native_payload::callback_sabotage("callback_trace") {
                    return;
                }
                if let Some(slot) = crate::native_payload::callback_slot_address(cell) {
                    visit(fixed_slot(slot));
                }
            }
        }
        GcRewriteDescriptorKind::Leaf => {}
    }
}

pub(super) unsafe fn visit_gc_rewrite_slots(
    header: *mut GcHeader,
    mut visit: impl FnMut(GcMutableSlot),
) {
    #[cfg(test)]
    if (*header).obj_type == crate::gc::GC_TYPE_NATIVE_HANDLE
        && crate::native_payload::callback_sabotage("rewrite")
    {
        return;
    }
    visit_gc_rewrite_slot_descriptors(header, |descriptor| unsafe {
        descriptor.visit_slots(&mut visit);
    });
}

/// [`visit_gc_rewrite_slots`] with the whole enumeration instantiated for
/// `visit`: the copying drain. Indirect descriptor and slot visitor calls
/// were a measured share of that path's per-object
/// trace cost. Enumerates exactly the same slots, in the same order.
#[inline(always)]
pub(super) unsafe fn visit_gc_rewrite_slots_inline(
    header: *mut GcHeader,
    mut visit: impl FnMut(GcMutableSlot),
) {
    #[cfg(test)]
    if (*header).obj_type == crate::gc::GC_TYPE_NATIVE_HANDLE
        && crate::native_payload::callback_sabotage("rewrite")
    {
        return;
    }
    visit_gc_rewrite_slot_descriptors_with::<true>(header, |descriptor| unsafe {
        descriptor.visit_slots_inline(&mut visit);
    });
}

/// Test-only sabotage for the inline mask walk
/// ([`HeapChildSlotIterator::take_inline_mask_word`]): a fast path that
/// enumerates a DIFFERENT set than the iterator it replaces must be caught, so
/// the witnesses arm this and REQUIRE the failure. Its witnesses are
/// `gc::tests::layout_inline_mask`.
#[cfg(test)]
pub(crate) mod inline_mask_sabotage {
    use std::cell::Cell;

    /// Forget the mask's highest slot — the one a cursor-or-limit mistake
    /// loses, and the one no `0..slot_count` spot check would look at.
    pub(crate) const DROP_TOP: u8 = 1;

    thread_local! {
        static PERTURB: Cell<u8> = const { Cell::new(0) };
    }

    #[inline]
    pub(crate) fn perturb(word: u64) -> u64 {
        let armed = PERTURB.with(Cell::get);
        if armed & DROP_TOP != 0 && word != 0 {
            return word & !(1u64 << (63 - word.leading_zeros()));
        }
        word
    }

    pub(crate) struct Guard(u8);

    impl Guard {
        pub(crate) fn arm(what: u8) -> Self {
            Self(PERTURB.with(|p| p.replace(p.get() | what)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let prior = self.0;
            PERTURB.with(|p| p.set(prior));
        }
    }
}
