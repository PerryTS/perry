//! GC slot-descriptor visitors, split out of `layout.rs` to keep it under the
//! 2000-line cap. These walk an object's payload and hand the collector either
//! a read-only view of each pointer slot (`visit_gc_layout_slot_descriptors`)
//! or a mutable one for evacuation rewriting (`visit_gc_rewrite_slot_*`).
//! No behaviour change; moved verbatim.

use super::*;

fn fixed_slot(slot: *mut u64) -> GcMutableSlotDescriptor {
    GcMutableSlotDescriptor::Slot(GcMutableSlot::new(slot, None))
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
    let mut child_slots = gc_child_slots(header);
    // #8213: drained async box cells are weak registry entries during a full
    // trace. A closure proven live by the mark set is their owner, so enumerate
    // the malloc-side JSValue payload as an external child slot. Requiring a
    // marked/pinned closure prevents generic descriptor walks over dead old
    // objects from accidentally resurrecting the cycle this edge is meant to
    // break.
    if (*header).obj_type == GC_TYPE_CLOSURE
        && (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0
        && full_trace_active()
    {
        let closure = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        crate::closure::visit_closure_box_payload_slots_mut(closure, |slot| {
            visit(fixed_slot(slot));
        });
    }
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
        if !crate::arena::pointer_in_nursery(user_ptr as usize) {
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
        visit(fixed_slot(slot).with_layout(HeapChildSlotReadKind::Prefix));
    }
    if let Some(slot) = child_slots.take_meta_child_slot() {
        visit(fixed_slot(slot).with_layout(HeapChildSlotReadKind::Prefix));
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
    mut visit: impl FnMut(GcMutableSlotDescriptor),
) {
    if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let obj_type = (*header).obj_type;
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    // An explicit `Object.setPrototypeOf` value recorded in the residual
    // registry is a child edge of its owner, whatever the owner's kind: marking
    // retains it and a moving collection rewrites it after
    // `gc/layout/transfer.rs` rekeyed the entry. It used to be emitted from the
    // array and ordinary-object arms only, so a Map, Set, Error, Promise, Date,
    // RegExp, Temporal cell, lazy JSON array or closure owner kept a stale
    // prototype address once the prototype moved. First, ahead of the kind
    // arms, so no arm's early return can skip it.
    if crate::object::prototype_chain::object_static_prototypes_maybe_nonempty()
        && crate::object::prototype_chain::residual_prototype_owner_type(obj_type)
        // #10362: the per-OWNER half. The latch above is exact for a process
        // that never re-prototyped a non-object and useless for one that has —
        // it is what made a single `Object.setPrototypeOf(anArray, p)` charge
        // every traced cell of every owner-capable kind a global mutex and a
        // SipHash probe. This asks the owner's own header instead.
        && crate::object::prototype_chain::residual_entry_possible_for(header)
    {
        crate::object::prototype_chain::visit_object_static_prototype_slot_mut(
            user_ptr as usize,
            |slot| visit(fixed_slot(slot)),
        );
    }
    match gc_type_rewrite_descriptor_kind((*header).obj_type) {
        GcRewriteDescriptorKind::Array => {
            visit_gc_layout_slot_descriptors(header, &mut visit);
            // #10166 (brief 4): an array's named properties live in reserve
            // slots in front of logical element 0 (`array/named_props.rs`):
            // a pairs pointer, or inline exec-result values. Those words sit
            // outside every layout range, so emit them as fixed child slots:
            // marking retains the values, evacuation and compaction rewrite
            // them, and the remembered-set scan finds them through the pages
            // the store barriers dirtied.
            if (*header)._reserved & crate::gc::GC_ARRAY_NAMED_PROPS != 0 {
                crate::array::visit_array_named_props_slots(
                    user_ptr as *const crate::array::ArrayHeader,
                    |slot| visit(fixed_slot(slot)),
                );
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
            visit_gc_layout_slot_descriptors(header, &mut visit);
            crate::object::visit_overflow_field_slots_mut(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
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
            // #6759 phase 1: the metadata edge (MARK path as well as rewrite).
            visit(fixed_slot(&mut (*promise).meta as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::Error => {
            let error = user_ptr as *mut crate::error::ErrorHeader;
            visit(fixed_slot(&mut (*error).message as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).name as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).stack as *mut _ as *mut u64));
            visit(fixed_slot(&mut (*error).cause as *mut f64 as *mut u64));
            visit(fixed_slot(&mut (*error).errors as *mut _ as *mut u64));
            // #6759 phase 1: the metadata edge. This arm is reached by
            // `trace_heap_rewrite_slots`, so visiting the slot here both MARKS
            // the meta record (keeping it, and anything reachable only through
            // it, alive) and rewrites the edge when evacuation moves it.
            visit(fixed_slot(&mut (*error).meta as *mut _ as *mut u64));
            // #9486: the captured-frames blob. Same shape as `stack` beside
            // it — a `StringHeader` edge, null until captured and null again
            // once `.stack` has been materialised — so it needs exactly this
            // one line and no new descriptor kind.
            visit(fixed_slot(&mut (*error).frames as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::Map => {
            let map = user_ptr as *mut crate::map::MapHeader;
            let size = (*map).size;
            let used = (*map).used;
            let capacity = (*map).capacity;
            // Corruption guard only: mirror Set's 16M bound (set.rs
            // gc_element_slot_range). Every GC walk (mark, copy, rewrite,
            // dirty-scan, verify) funnels through this descriptor, so a
            // lower cap makes larger maps invisible to the collector —
            // entries reachable only through a >cap map would be swept
            // while live and never rewritten after a move.
            if size > used || used > capacity || used > 16_000_000 || (*map).entries.is_null() {
                return;
            }
            // Defensive tripwire (# fabricated-Map): if a fabricated Map
            // header ever slips past `plausible_gc_header`, the `entries`
            // field would be a NaN-boxed JSValue (top bits 0x7FFD…) or some
            // other non-pointer word read from the original object's
            // payload. Reject it rather than dereference a derived slot
            // range from a garbage base. This is a backstop; the primary
            // fix is the fixed-layout size check in `plausible_gc_header`.
            // Use the shared heap-address classifier so the bound is
            // platform-correct (a fixed `>> 47` cutoff would false-reject
            // genuine entries on aarch64 Linux, where user space reaches
            // bit 48) and so low / handle-band garbage is rejected too,
            // not only the NaN-box signature.
            let entries_addr = (*map).entries as usize;
            if !crate::value::addr_class::is_plausible_heap_addr(entries_addr) {
                if crate::gc::gc_diag_enabled() {
                    eprintln!(
                        "[gc-tripwire] Map entries is not a plausible heap address: {:#x} (fabricated Map?)",
                        entries_addr
                    );
                }
                return;
            }
            visit(GcMutableSlotDescriptor::Range {
                range: HeapSlotRange::new((*map).entries as *mut u64, used as usize * 2),
                layout_kind: None,
            });
            // #6759 phase 1: the metadata edge. This arm is the MARK path as
            // well as the rewrite path (`trace_heap_rewrite_slots` drives it),
            // so visiting here keeps the record — and anything reachable only
            // through it — alive.
            visit(fixed_slot(&mut (*map).meta as *mut _ as *mut u64));
        }
        GcRewriteDescriptorKind::Set => {
            let set = user_ptr as *mut crate::set::SetHeader;
            if let Some(range) = crate::set::gc_element_slot_range(set) {
                visit(GcMutableSlotDescriptor::Range {
                    range,
                    layout_kind: None,
                });
            }
            // #6759 phase 1: the metadata edge (MARK path as well as rewrite).
            visit(fixed_slot(&mut (*set).meta as *mut _ as *mut u64));
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
            // all three drive; `dictionary_keys_survive_a_moving_collection`
            // reddens if it is removed.
            visit(fixed_slot(&mut (*meta).dictionary_keys as *mut u64));
            // A fresh class object stored as an instance's private evaluation
            // brand is a NaN-boxed child edge and moves with the meta record.
            visit(fixed_slot(
                &mut (*meta).private_evaluation_brand as *mut u64,
            ));
        }
        GcRewriteDescriptorKind::MetaOnly => {
            // #6759 phase 1: the cell's only traced edge is its metadata
            // record. Reached by `trace_heap_rewrite_slots`, so this is the
            // MARK path as well as the rewrite path.
            if let Some(slot) = crate::object::cell_meta_slot(user_ptr as usize) {
                visit(fixed_slot(slot as *mut u64));
            }
        }
        GcRewriteDescriptorKind::Buffer => {
            crate::buffer::view::visit_backing_slot(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
            crate::buffer::visit_ab_alias_slot(user_ptr as usize, |slot| {
                visit(fixed_slot(slot));
            });
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
