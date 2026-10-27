//! The realm image's remembered set (WRT step 1, DESIGN §2.3).
//!
//! Image cells (`value::addr_class::realm`) are immortal and never move:
//! marking stops at the image boundary (no valid-pointer set holds them), and
//! sweep, compaction and the copying collector never visit the range. Their
//! edges into the heap come only from program stores, and every such store
//! reaches the write barrier: an image header has no `GC_FLAG_ARENA`, so the
//! emitted #7511 gate and `newborn_parent_needs_barrier` both take it.
//!
//! The barrier classifies an image parent like an old one
//! (`RememberingParent::Image`), but what it remembers differs in two ways,
//! because no collector ever traces an image cell:
//!
//! * every collectable child is remembered, not only young and malloc ones: a
//!   full collection does not mark from the regular remembered set (#9629: it
//!   traces old parents instead), so an image→old edge has to be a root;
//! * the entry is the cell, and it is kept across collections: the regular
//!   set is cleared after every cycle and rebuilt from children that are still
//!   young, which would drop the edge to a child a minor just promoted.
//!
//! Every collection therefore treats each remembered cell as a root object: a
//! minor marks or copies its young and malloc children, a full marks all of
//! them, a moving collection rewrites its slots, and the verifiers check every
//! image cell. A full trace also visits the cells whose shape record is not
//! static image data ([`root_cells`]). The set is bounded by the image's
//! cell count. Stores made before #7187's lazy arming are recovered by the
//! arming reconstruct, which walks the image.
//!
//! The set is per agent (thread). The primary agent uses the image in place;
//! a worker agent never reaches it, because its realm is an evacuated copy
//! ([`evacuate_realm_image`]).
use super::*;

crate::perry_thread_local! {
    /// Header addresses of the image cells that have held a collectable child
    /// on this agent. Never cleared: an entry lives as long as its cell.
    static IMAGE_REMEMBERED: RefCell<crate::fast_hash::PtrHashSet<usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_set());
}

/// Every image path visits a cell's slots through this one out-of-line,
/// non-generic instance of the layout visitor. The paths are cold (bounded by
/// the image) and sit inside hot collector functions; a generic visitor
/// inlined at each of them grew `.text` by about 20 KB.
#[inline(never)]
unsafe fn visit_cell_slots(header: *mut GcHeader, visit: &mut dyn FnMut(GcMutableSlot)) {
    visit_gc_rewrite_slots(header, |slot| visit(slot));
}

/// Is `child_addr` a cell some collector owns, so that an image slot holding
/// it must be a root: an arena cell of any generation, or a tracked malloc
/// cell? Image cells, image strings and other immortal cells are not.
#[inline]
pub(super) fn image_child_is_collectable(child_addr: usize) -> bool {
    !matches!(
        crate::arena::classify_heap_generation(child_addr),
        crate::arena::HeapGeneration::Unknown
    ) || (child_addr > GC_HEADER_SIZE
        && super::malloc::gc_malloc_header_is_tracked(
            (child_addr - GC_HEADER_SIZE) as *const GcHeader,
        ))
}

/// The barrier's image arm: a store into the image cell `parent_addr` (a user
/// address) published `child_addr`. Returns whether the cell is newly
/// remembered.
#[cold]
#[inline(never)]
pub(super) fn remember_image_parent(parent_addr: usize, child_addr: usize) -> bool {
    if !image_child_is_collectable(child_addr) {
        return false;
    }
    remember_image_cell(parent_addr)
}

/// Remember the image cell `parent_addr` (a user address) unconditionally:
/// the span and relocation funnels know a run of its slots was written but
/// not what each holds. An extra entry costs one cell scan per collection.
#[cold]
#[inline(never)]
pub(super) fn remember_image_cell(parent_addr: usize) -> bool {
    #[cfg(test)]
    if sabotage::knocked(sabotage::BARRIER) {
        return false;
    }
    let header = parent_addr - GC_HEADER_SIZE;
    IMAGE_REMEMBERED.with(|set| set.borrow_mut().insert(header))
}

/// Does the image cell hold a collectable child in a slot it owns? A shared
/// runtime-root word the layout also yields (a shape record's keys) has its
/// own root scanner; remembering and the old→young gate skip it the same way.
unsafe fn cell_holds_collectable_child(header: *mut GcHeader) -> bool {
    let mut holds = false;
    visit_cell_slots(header, &mut |slot| {
        if !holds && slot.is_parent_owned() {
            let child = decode_heap_addr(slot.read());
            holds = child != 0 && image_child_is_collectable(child);
        }
    });
    holds
}

/// The arming reconstruct's image half (#7187): before the first collection
/// arms it, the barrier records nothing, so every cell that already holds a
/// collectable child is remembered by one walk of the image. Returns the
/// number of cells it added.
#[cold]
#[inline(never)]
pub(super) fn reconstruct_image_remembered_set() -> usize {
    #[cfg(test)]
    if sabotage::knocked(sabotage::RECONSTRUCT) {
        return 0;
    }
    let mut added = 0usize;
    for_each_image_cell(|header| unsafe {
        if cell_holds_collectable_child(header)
            && IMAGE_REMEMBERED.with(|set| set.borrow_mut().insert(header as usize))
        {
            added += 1;
        }
    });
    added
}

/// Does this agent use the realm image in place? The primary agent does; a
/// worker agent's realm is an evacuated copy in its own heap, and the image's
/// shape words and heap pointers mean nothing to its collector, so a worker
/// collection neither roots, rewrites nor verifies image cells.
///
/// Every libtest thread is a primary agent, but the runtime tests' image is
/// one process-wide fixture: there the image belongs to the thread holding it
/// (`gc::tests::image_cells::ImageFixture`), so concurrent collections in
/// unrelated tests leave it alone.
#[inline]
pub(super) fn agent_uses_image() -> bool {
    #[cfg(test)]
    return TEST_IMAGE_HOLDER.with(std::cell::Cell::get);
    #[cfg(not(test))]
    return crate::agent::current_agent() == crate::agent::PRIMARY_AGENT;
}

#[cfg(test)]
thread_local! {
    static TEST_IMAGE_HOLDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test-only: the calling thread takes (or releases) the process-wide image.
#[cfg(test)]
pub(crate) fn hold_image_for_tests(holds: bool) {
    TEST_IMAGE_HOLDER.with(|holder| holder.set(holds));
}

/// The remembered image cells, copied out so a visit may allocate or store.
pub(super) fn image_remembered_headers() -> Vec<*mut GcHeader> {
    if !agent_uses_image() {
        return Vec::new();
    }
    IMAGE_REMEMBERED.with(|set| {
        set.borrow()
            .iter()
            .map(|&header| header as *mut GcHeader)
            .collect()
    })
}

/// Is the image cell `header` remembered on this agent?
pub(crate) fn image_cell_remembered(header: *const GcHeader) -> bool {
    IMAGE_REMEMBERED.with(|set| set.borrow().contains(&(header as usize)))
}

/// The cells a collection visits as root objects. A minor needs only the
/// remembered ones: a cell holding no collectable child has nothing young to
/// keep. A full trace also visits the cells whose shape is not static image
/// data ([`cell_shape_needs_visit`]), because visiting an object does more
/// than mark its slots: the layout visitor notes the object as its shape's
/// carrier and emits the shape record's keys edge
/// (`visit_gc_layout_slot_descriptors_inline`). Shape records are weak with
/// respect to receivers, so an image object no trace visited would lose its
/// shape's keys (and the record) at the first full collection, even with no
/// heap child of its own. A static record with image keys (WRT step 2) is
/// never pruned and its keys never die, so a cell on it needs no visit.
fn root_cells(minor_only: bool) -> Vec<*mut GcHeader> {
    if minor_only {
        return image_remembered_headers();
    }
    let mut cells = image_remembered_headers();
    for_each_image_cell(|header| {
        if !image_cell_remembered(header) && unsafe { cell_shape_needs_visit(header) } {
            cells.push(header);
        }
    });
    cells
}

/// Does a full trace have to visit this cell for its shape's sake? Not when
/// the shape record is static and its keys are image data (or it has none):
/// such a record and its keys outlive every trace without a carrier. Every
/// other case (a dynamic shape with heap keys, a static record whose keys
/// are a heap list, an unknown word) is visited.
unsafe fn cell_shape_needs_visit(header: *mut GcHeader) -> bool {
    #[cfg(test)]
    if sabotage::knocked(sabotage::DYNAMIC) {
        return false;
    }
    if (*header).obj_type != GC_TYPE_OBJECT {
        return true;
    }
    let object = (header as *mut u8).add(GC_HEADER_SIZE) as *const crate::object::ObjectHeader;
    let word = crate::codegen_abi::LayoutWord((*object).parent_class_id);
    match crate::object::shapes::shape_record_for_word(word) {
        Some(record) => {
            let keys = record.keys();
            !(record.is_static_record()
                && (keys == 0 || crate::value::addr_class::realm::is_image_object(keys as usize)))
        }
        None => true,
    }
}

/// Test-only: the cells a full trace visits as roots.
#[cfg(test)]
pub(crate) fn full_trace_roots() -> Vec<*mut GcHeader> {
    root_cells(false)
}

/// Mark the children of the root cells ([`root_cells`]). A minor marks young
/// and malloc children only (an old child marked by a minor would keep a
/// stray mark that the minor's sweep never clears); a full marks every child
/// and observes native handles, as tracing any object does. Returns the
/// number of newly marked children.
#[cold]
#[inline(never)]
pub(super) fn mark_image_roots(valid_ptrs: &ValidPointerSet, minor_only: bool) -> usize {
    #[cfg(test)]
    if sabotage::knocked(sabotage::ROOTS) {
        return 0;
    }
    let mut marked = 0usize;
    for header in root_cells(minor_only) {
        unsafe {
            visit_cell_slots(header, &mut |slot| {
                let bits = slot.read();
                let newly = if minor_only {
                    try_mark_young_value_as_seed(bits, valid_ptrs)
                } else if bits >> 48 == 0 {
                    // A raw pointer word (a closure's `props`, a meta record):
                    // exact membership decides, as for a raw root.
                    bits != 0 && try_mark_raw_root_addr(bits as usize, valid_ptrs)
                } else {
                    try_mark_value(bits, valid_ptrs)
                };
                marked += usize::from(newly);
            });
        }
    }
    marked
}

/// The copying minor's root pass over the remembered image cells: each young
/// child is copied and the slot rewritten; nothing is re-remembered (the cell
/// stays in this set). Returns the number of slots that changed.
#[cold]
#[inline(never)]
pub(super) unsafe fn visit_image_roots_copying(collector: &mut CopyingNurseryCollector) -> usize {
    #[cfg(test)]
    if sabotage::knocked(sabotage::COPY) {
        return 0;
    }
    let mut rewritten = 0usize;
    for header in image_remembered_headers() {
        let mut changed = false;
        visit_cell_slots(header, &mut |slot| {
            let before = slot.read();
            collector.visit_image_root_slot(slot, header);
            changed |= slot.read() != before;
            rewritten += usize::from(slot.read() != before);
        });
        if changed {
            run_gc_rewrite_hook(
                (*header).obj_type,
                (header as *mut u8).add(GC_HEADER_SIZE) as usize,
            );
        }
    }
    rewritten
}

/// The copying minor's pinned-young preflight over the same slots.
#[cold]
#[inline(never)]
pub(super) unsafe fn preflight_image_roots(checker: &mut CopyingNurseryPreflight) {
    for header in image_remembered_headers() {
        visit_cell_slots(header, &mut |slot| checker.check_bits(slot.read()));
    }
}

/// After a collection that moved objects without the copying minor's own
/// rewrite (evacuating cycles, old-generation defragmentation, idle
/// compaction): rewrite every image cell's forwarded slots. Every cell, as a
/// full trace visits them: the visit also rewrites the shape record's keys
/// word an image object may be the only carrier of.
#[cold]
#[inline(never)]
pub(super) fn rewrite_image_roots(valid_ptrs: &ValidPointerSet) {
    #[cfg(test)]
    if sabotage::knocked(sabotage::REWRITE) {
        return;
    }
    for header in root_cells(false) {
        unsafe { rewrite_heap_object_fields(header, valid_ptrs) };
    }
}

/// Every image cell, remembered or not, for the verifiers: a verifier must
/// not trust the set it checks.
pub(super) fn for_each_image_cell(mut visit: impl FnMut(*mut GcHeader)) {
    if agent_uses_image() {
        crate::value::addr_class::realm::for_each_cell(&mut visit);
    }
}

/// The sabotage gate (`PERRY_GC_VERIFY_EVACUATION`'s old→young check): every
/// image cell holding a collectable child must be remembered, whatever the
/// child's generation. A store that bypassed the barrier leaves its cell out.
#[cold]
#[inline(never)]
pub(super) fn verify_image_cells_remembered(stats: &mut OldYoungEdgeVerifyStats) {
    for_each_image_cell(|header| unsafe {
        stats.checked_old_objects = stats.checked_old_objects.saturating_add(1);
        let remembered = image_cell_remembered(header);
        visit_cell_slots(header, &mut |slot| {
            if !slot.is_parent_owned() {
                return;
            }
            let child = decode_heap_addr(slot.read());
            if child == 0 || !image_child_is_collectable(child) {
                return;
            }
            stats.checked_old_to_young_edges = stats.checked_old_to_young_edges.saturating_add(1);
            if !remembered {
                let child_type = (*header_from_user_ptr(child as *const u8)).obj_type;
                stats.record_missing_diag(
                    header as usize,
                    slot.slot as usize,
                    child,
                    (*header).obj_type,
                    child_type,
                    false,
                    true,
                );
            }
        });
    });
}

/// Test-only: forget this agent's remembered image cells (the image is
/// process-wide and the tests restore its cells between them).
#[cfg(test)]
pub(crate) fn forget_for_tests() {
    IMAGE_REMEMBERED.with(|set| set.borrow_mut().clear());
}

/// Test-only knockouts: each disables one clause of the image contract so its
/// test can show the failure the clause prevents (a test must be able to
/// fail).
#[cfg(test)]
pub(crate) mod sabotage {
    use std::cell::Cell;

    pub(crate) const BARRIER: u8 = 1 << 0;
    pub(crate) const ROOTS: u8 = 1 << 1;
    pub(crate) const COPY: u8 = 1 << 2;
    pub(crate) const REWRITE: u8 = 1 << 3;
    pub(crate) const RECONSTRUCT: u8 = 1 << 4;
    pub(crate) const FORWARD: u8 = 1 << 5;
    /// The full trace's shape selection: no cell is visited for its shape.
    pub(crate) const DYNAMIC: u8 = 1 << 6;

    thread_local! {
        static KNOCKED: Cell<u8> = const { Cell::new(0) };
    }

    pub(crate) fn knocked(clause: u8) -> bool {
        KNOCKED.with(Cell::get) & clause != 0
    }

    pub(crate) struct Guard(u8);

    impl Guard {
        pub(crate) fn arm(clauses: u8) -> Self {
            Self(KNOCKED.with(|k| k.replace(k.get() | clauses)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let prior = self.0;
            KNOCKED.with(|k| k.set(prior));
        }
    }
}
