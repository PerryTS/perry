//! Conditional edges discovered by a collection, never a mutator registry.
//! The seed queue is the weak-table counterpart of MARK_SEEDS: visitors put
//! traced tables here and the active collector immediately takes ownership.
//! Budgeted non-moving cycles retain their work in GcCycleState; copying
//! cycles retain it in CopyingNurseryCollector. Neither roots a key.
use super::*;
use crate::weakref::storage::WeakStorage;
use std::cell::RefCell;

/// Mark verification follows a value only when its key independently lives.
/// Key words remain weak; rewrite and generational coverage still visit both.
pub(super) unsafe fn value_slot_is_enabled(
    header: *mut GcHeader,
    slot: *mut u64,
    minor: bool,
) -> bool {
    if (*header).obj_type != GC_TYPE_WEAK_STORAGE {
        return false;
    }
    let table = (header as *mut u8)
        .add(GC_HEADER_SIZE)
        .cast::<WeakStorage>();
    let offset = (slot as usize).wrapping_sub((*table).entries() as usize);
    let stride = std::mem::size_of::<crate::weakref::storage::Entry>();
    if offset >= (*table).len as usize * stride || offset % stride != 8 {
        return false;
    }
    let key = (*(*table).entries().add(offset / stride)).key;
    if key == crate::value::TAG_UNDEFINED {
        return false;
    }
    let valid = hot_incremental_mark_valid_ptrs().get();
    if !valid.is_null() {
        return !crate::weakref::weak_target_should_clear(key, &*valid, minor);
    }
    let addr = barrier::decode_heap_addr(key);
    if crate::value::addr_class::is_proxy_id_band(addr) {
        return crate::proxy::gc_weak_key_is_live(key, minor);
    }
    if let Some((addr, key_header)) = barrier::current_heap_header_for_heap_word(key, None) {
        return (minor && !crate::arena::pointer_in_nursery(addr))
            || (*key_header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0;
    }
    // Occupied entries have already passed CanBeHeldWeakly. Immediate class
    // references and runtime handles have no collectible heap header.
    crate::value::is_js_handle(f64::from_bits(key))
        || key & crate::value::TAG_MASK != crate::value::POINTER_TAG
        || crate::symbol::is_well_known_symbol(addr)
}

/// Publishing a weak pair owes generational coverage and conditional work,
/// never unconditional insertion shading. In particular, growth copies old
/// weak pairs whose keys are not live mutator arguments.
pub(crate) unsafe fn weak_collection_store_barrier(
    table: *mut WeakStorage,
    entry: *mut crate::weakref::storage::Entry,
) {
    if !incremental_mark_barrier_globally_idle()
        && !hot_incremental_mark_valid_ptrs().get().is_null()
    {
        discover(header_from_user_ptr(table.cast()));
    }
    if !barrier::write_barriers_enabled() || !barrier::barrier_remembering_active() {
        return;
    }
    for slot in [
        &mut (*entry).key as *mut u64,
        &mut (*entry).value as *mut u64,
    ] {
        let bits = *slot;
        if barrier_store::barrier_scalar_child_skips(bits) {
            continue;
        }
        let child = barrier::decode_heap_addr(bits);
        if child != 0 {
            barrier::write_barrier_decoded_parent(table as usize, slot as usize, child, false);
        }
    }
}

thread_local! {
    static EPHEMERON_SEEDS: RefCell<Vec<*mut WeakStorage>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn clear_seeds() {
    let _ = EPHEMERON_SEEDS.try_with(|seeds| seeds.borrow_mut().clear());
}

#[inline]
pub(super) unsafe fn discover(header: *mut GcHeader) {
    if (*header).obj_type == GC_TYPE_WEAK_STORAGE {
        EPHEMERON_SEEDS.with(|seeds| {
            seeds
                .borrow_mut()
                .push((header as *mut u8).add(GC_HEADER_SIZE).cast())
        });
    }
}

#[derive(Default)]
pub(super) struct Ephemerons {
    pub(super) tables: Vec<*mut WeakStorage>,
    seen: crate::fast_hash::PtrHashSet<usize>,
    // Conditional edges already processed by this cycle, not mutator state.
    enabled: crate::fast_hash::PtrHashSet<usize>,
    ready_table: usize,
    ready_entry: u32,
    ready_changed: bool,
    ready_revision: usize,
    clear_table: usize,
    clear_entry: u32,
}

impl Ephemerons {
    pub(super) fn add(&mut self, table: *mut WeakStorage) {
        if self.seen.insert(table as usize) {
            self.tables.push(table);
        }
    }
    pub(super) fn absorb(&mut self) {
        let seeds = EPHEMERON_SEEDS.with(|seeds| std::mem::take(&mut *seeds.borrow_mut()));
        for table in seeds {
            self.add(table);
        }
    }

    pub(super) fn has_pending_work(&self) -> bool {
        EPHEMERON_SEEDS.with(|seeds| !seeds.borrow().is_empty())
            || (!self.tables.is_empty()
                && crate::proxy::gc_trace_mark_revision() != self.ready_revision)
    }

    pub(super) fn restart_ready_scan(&mut self) {
        self.ready_table = 0;
        self.ready_entry = 0;
        self.ready_changed = false;
    }

    pub(super) fn note_strong_progress(&mut self) {
        // Finish the current bounded round before rescanning. Resetting its
        // cursor for each newly shaded value would make a one-unit walk O(n²).
        if !self.tables.is_empty() {
            self.ready_changed = true;
        }
    }

    /// One entry or table boundary per work unit. New strong marks and newly
    /// enabled handle edges force another round before any weak decision.
    pub(super) fn shade_ready_step(
        &mut self,
        valid: &ValidPointerSet,
        minor: bool,
        mut sticky: Option<&mut StickyRememberedSet>,
        budget: usize,
    ) -> (bool, usize) {
        self.absorb();
        let revision = crate::proxy::gc_trace_mark_revision();
        if revision != self.ready_revision {
            self.ready_changed = true;
            self.ready_revision = revision;
        }
        let mut used = 0;
        while self.ready_table < self.tables.len() && used < budget {
            let table = self.tables[self.ready_table];
            used += 1;
            unsafe {
                if self.ready_entry >= (*table).len {
                    self.ready_table += 1;
                    self.ready_entry = 0;
                    continue;
                }
                let entry = (*table).entries().add(self.ready_entry as usize);
                self.ready_entry += 1;
                if (*entry).key == crate::value::TAG_UNDEFINED {
                    continue;
                }
                if !crate::weakref::weak_target_should_clear((*entry).key, valid, minor) {
                    // Proxy observation can enable another key without adding
                    // heap work when its endpoints are already black.
                    self.ready_changed |= self.enabled.insert(entry as usize);
                    try_mark_value((*entry).value, valid);
                }
                if let Some(sticky) = sticky.as_deref_mut() {
                    let header = header_from_user_ptr(table.cast());
                    remember_evacuated_old_to_young_slot(sticky, header, &mut (*entry).key);
                    remember_evacuated_old_to_young_slot(sticky, header, &mut (*entry).value);
                }
            }
        }
        if self.ready_table < self.tables.len() {
            return (false, used);
        }
        let revision = crate::proxy::gc_trace_mark_revision();
        let done = !self.ready_changed && revision == self.ready_revision;
        self.ready_revision = revision;
        self.restart_ready_scan();
        (done, used)
    }

    /// Non-moving clearing uses the table's delete path, so each slice leaves
    /// a valid index and free chain. Moving passes rebuild atomically below.
    pub(super) fn finish_step(
        &mut self,
        valid: &ValidPointerSet,
        minor: bool,
        budget: usize,
    ) -> bool {
        self.absorb();
        let mut used = 0;
        while self.clear_table < self.tables.len() && used < budget {
            let table = self.tables[self.clear_table];
            used += 1;
            unsafe {
                if self.clear_entry >= (*table).len {
                    self.clear_table += 1;
                    self.clear_entry = 0;
                    continue;
                }
                let slot = self.clear_entry;
                self.clear_entry += 1;
                let entry = (*table).entries().add(slot as usize);
                if (*entry).key != crate::value::TAG_UNDEFINED
                    && crate::weakref::weak_target_should_clear((*entry).key, valid, minor)
                {
                    (*table).remove(slot);
                }
            }
        }
        self.clear_table == self.tables.len()
    }
}

impl CopyingNurseryCollector {
    /// Values are copied only after their key is live through another edge.
    pub(super) unsafe fn shade_ephemerons(&mut self) {
        self.ephemerons.absorb();
        // Visiting a value can grow the collector, but cannot move a table
        // already scanned (it is in to-space or old/malloc storage).
        for n in 0..self.ephemerons.tables.len() {
            let table = self.ephemerons.tables[n];
            let header = header_from_user_ptr(table.cast());
            for i in 0..(*table).len {
                let entry = (*table).entries().add(i as usize);
                if let Some(bits) = self.rewrite_value_bits((*entry).key) {
                    (*entry).key = bits;
                }
                if (*entry).key != crate::value::TAG_UNDEFINED
                    && !crate::weakref::weak_target_should_clear_copied((*entry).key, &self.ptrs)
                {
                    // Bypass the weak-slot skip for this proven conditional edge.
                    self.visit_slot_with_weak_fact(&mut (*entry).value, header, false, false);
                }
            }
        }
    }

    pub(super) unsafe fn finish_ephemerons(&mut self) {
        self.ephemerons.absorb();
        for &table in &self.ephemerons.tables {
            let header = header_from_user_ptr(table.cast());
            for i in 0..(*table).len {
                let entry = (*table).entries().add(i as usize);
                if (*entry).key == crate::value::TAG_UNDEFINED {
                    continue;
                }
                if crate::weakref::weak_target_should_clear_copied((*entry).key, &self.ptrs) {
                    (*entry).key = crate::value::TAG_UNDEFINED;
                    (*entry).value = crate::value::TAG_UNDEFINED;
                } else {
                    if let Some(bits) = self.rewrite_value_bits((*entry).key) {
                        (*entry).key = bits;
                    }
                    if let Some(bits) = self.rewrite_value_bits((*entry).value) {
                        (*entry).value = bits;
                    }
                    if !self.skip_remembering {
                        remember_evacuated_old_to_young_slot(
                            &mut self.sticky,
                            header,
                            &mut (*entry).key,
                        );
                        remember_evacuated_old_to_young_slot(
                            &mut self.sticky,
                            header,
                            &mut (*entry).value,
                        );
                    }
                }
            }
            (*table).rebuild();
        }
    }
}

impl CopyingNurseryPreflight {
    /// Roots and dirty parents share one fixed point: a rooted key can enable
    /// a conditional pinned value in an old dirty table.
    pub(super) fn check_dirty_roots(&mut self) {
        let snapshot = remembered_dirty_snapshot();
        scan_remembered_dirty_slots_copying(&snapshot, None, |slot, _, _, _| unsafe {
            self.check_bits_with_reason(*slot, CopiedMinorFallbackReason::PinnedYoungDirtySlot);
        });
    }

    pub(super) unsafe fn check_ephemerons(&mut self) {
        self.ephemerons.absorb();
        for n in 0..self.ephemerons.tables.len() {
            let table = self.ephemerons.tables[n];
            for i in 0..(*table).len {
                let entry = (*table).entries().add(i as usize);
                if (*entry).key == crate::value::TAG_UNDEFINED {
                    continue;
                }
                let live = match self.ptrs().decode_bits_for_preflight((*entry).key) {
                    Ok(Some((addr, ptr))) => {
                        !crate::arena::pointer_in_nursery(addr)
                            || self.seen.contains(&(ptr.header as usize))
                    }
                    // Permanent symbols and runtime handle identities are not GC objects.
                    Ok(None) => true,
                    Err(_) => false,
                };
                if live {
                    self.check_bits_with_reason(
                        (*entry).value,
                        CopiedMinorFallbackReason::PinnedYoungTransitive,
                    );
                }
            }
        }
    }
}

/// Currency only, just like WEAK_HOLDERS. This queue is GC work, never a
/// strong owner: the map's meta edge is the sole source of storage liveness.
pub(crate) fn scan_ephemeron_seeds_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    EPHEMERON_SEEDS.with(|seeds| {
        for table in seeds.borrow_mut().iter_mut() {
            let mut address = *table as usize;
            visitor.visit_metadata_usize_slot(&mut address);
            *table = address as *mut WeakStorage;
        }
    });
}
