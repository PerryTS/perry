//! Lifetime bridge between GC closures and malloc-side async box cells.
//!
//! Codegen identifies raw box slots through the shared `box_layout` bitmap.
//! This module keeps per-cell counts and legacy dynamically declared edges. We
//! count only those declared edges before a box reaches terminal
//! `ReleaseBoxes`; arbitrary JS values must never be guessed to be boxes from
//! pointer-shaped bits. Once its async activation drains, the box runtime
//! publishes an unobserved cell immediately and leaves only a captured cell
//! pending. Closure moves rekey the per-closure index; authoritative GC death
//! pruning drops the corresponding per-cell counts. A cell released by the
//! ordinary frame that minted it (#10464) follows the same edges.

use super::ClosureHeader;
use std::cell::RefCell;

/// One closure's `(capture index, box address)` edges. Nearly every closure
/// declares one or two, and one is stored inline: a closure capturing a
/// reassigned local then costs no heap allocation for its lifetime record
/// (#10464 made those records a per-call cost of ordinary frames).
#[derive(Clone, Default)]
struct BoxCaptureSlots {
    first: Option<(u32, usize)>,
    rest: Vec<(u32, usize)>,
}

impl BoxCaptureSlots {
    fn take(&mut self, index: u32) -> Option<usize> {
        if self.first.is_some_and(|(slot, _)| slot == index) {
            let cell = self.first.take().map(|(_, cell)| cell);
            self.first = self.rest.pop();
            return cell;
        }
        let pos = self.rest.iter().position(|(slot, _)| *slot == index)?;
        Some(self.rest.swap_remove(pos).1)
    }

    fn push(&mut self, index: u32, cell: usize) {
        if self.first.is_none() {
            self.first = Some((index, cell));
        } else {
            self.rest.push((index, cell));
        }
    }

    /// `rest` is non-empty only while `first` is occupied.
    fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    fn cells(&self) -> impl Iterator<Item = usize> + '_ {
        self.first
            .iter()
            .chain(self.rest.iter())
            .map(|(_, cell)| *cell)
    }
}

crate::perry_thread_local! {
    /// Legacy/dynamically patched closures only. Generated closures use the
    /// shared function bitmap and weak owner set in `box_layout`.
    static CLOSURE_BOX_CELLS: RefCell<crate::fast_hash::PtrHashMap<usize, BoxCaptureSlots>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// Box address -> packed edge record: the number of capture slots naming
    /// it (`EDGE_COUNT_MASK`) plus, once the frame that minted the cell has
    /// released it (#10464), `FRAME_RELEASED` and the cell-kind tag. Keeping
    /// the release state in the record the capture edges already maintain
    /// makes a frame exit one probe, and lets the final edge's removal publish
    /// the cell without a second table.
    static BOX_CAPTURE_COUNTS: RefCell<crate::fast_hash::PtrHashMap<usize, usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

const FRAME_RELEASED: usize = 1 << 63;
const FRAME_RELEASE_TAG_SHIFT: u32 = 60;
const FRAME_RELEASE_TAG_MASK: usize = 0b11 << FRAME_RELEASE_TAG_SHIFT;
const EDGE_COUNT_MASK: usize = (1 << FRAME_RELEASE_TAG_SHIFT) - 1;

pub(super) fn increment_cell_capture_count(cell: usize, amount: usize) {
    BOX_CAPTURE_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        let record = counts.entry(cell).or_default();
        let count = (*record & EDGE_COUNT_MASK)
            .checked_add(amount)
            .filter(|count| *count <= EDGE_COUNT_MASK)
            .expect("box capture count overflow");
        *record = (*record & !EDGE_COUNT_MASK) | count;
    });
}

pub(super) fn decrement_cell_capture_count(cell: usize, amount: usize) {
    // `Some(record)` when the final edge disappeared.
    let reached_zero = BOX_CAPTURE_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        let record = counts.get_mut(&cell)?;
        debug_assert!(*record & EDGE_COUNT_MASK >= amount);
        *record -= amount;
        if *record & EDGE_COUNT_MASK == 0 {
            counts.remove(&cell)
        } else {
            None
        }
    });
    match reached_zero {
        Some(record) if record & FRAME_RELEASED != 0 => crate::r#box::publish_frame_released_cell(
            cell,
            (record & FRAME_RELEASE_TAG_MASK) >> FRAME_RELEASE_TAG_SHIFT,
        ),
        Some(_) => crate::r#box::box_capture_count_reached_zero(cell),
        None => {}
    }
}

pub(crate) fn box_capture_count(cell: usize) -> usize {
    BOX_CAPTURE_COUNTS
        .with(|counts| {
            let counts = counts.borrow();
            if counts.is_empty() {
                None
            } else {
                counts.get(&cell).copied()
            }
        })
        .map_or(0, |record| record & EDGE_COUNT_MASK)
}

/// Outcome of [`note_frame_released_cell`].
pub(crate) enum FrameRelease {
    /// No closure edge names the cell: the frame was its only holder.
    Uncaptured,
    /// Captured; the final edge's removal now publishes it.
    Deferred,
    /// Captured and already released by its frame.
    AlreadyReleased,
}

/// #10464: the frame that minted `cell` (of kind `tag`, 1..=3) can no longer
/// name it. A captured cell is marked so its last capture edge publishes it.
pub(crate) fn note_frame_released_cell(cell: usize, tag: usize) -> FrameRelease {
    debug_assert!((1..=3).contains(&tag));
    BOX_CAPTURE_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        if counts.is_empty() {
            return FrameRelease::Uncaptured;
        }
        match counts.get_mut(&cell) {
            None => FrameRelease::Uncaptured,
            Some(record) if *record & FRAME_RELEASED != 0 => FrameRelease::AlreadyReleased,
            Some(record) => {
                *record |= FRAME_RELEASED | (tag << FRAME_RELEASE_TAG_SHIFT);
                FrameRelease::Deferred
            }
        }
    })
}

/// Whether `cell` is a frame-released, still-captured JSValue box: during a
/// full trace such a cell is reached only through its live closures.
pub(crate) fn frame_released_js_cell(cell: usize, js_tag: usize) -> bool {
    BOX_CAPTURE_COUNTS.with(|counts| {
        let counts = counts.borrow();
        !counts.is_empty()
            && counts.get(&cell).is_some_and(|record| {
                *record & FRAME_RELEASED != 0
                    && (*record & FRAME_RELEASE_TAG_MASK) >> FRAME_RELEASE_TAG_SHIFT == js_tag
            })
    })
}

/// Visit the JSValue payload slots reached through one live closure's exact
/// compiler-declared box captures. During a full trace drained async boxes are
/// no longer global roots: this is their ephemeron half, preserving
/// `live closure -> box payload` without allowing `box payload -> closure` to
/// keep an otherwise unreachable cycle alive.
pub(crate) fn visit_closure_box_payload_slots_mut(closure: usize, mut visit: impl FnMut(*mut u64)) {
    if closure == 0 || !crate::gc::full_trace_active() {
        return;
    }
    super::box_layout::visit_payloads(closure, &mut visit);
    CLOSURE_BOX_CELLS.with(|captures| {
        let captures = captures.borrow();
        let Some(cells) = captures.get(&closure) else {
            return;
        };
        for cell in cells.cells() {
            crate::r#box::visit_pending_captured_js_box_payload_slot(cell, &mut visit);
        }
    });
}

pub(super) fn has_dynamic_box_captures(closure: *mut ClosureHeader) -> bool {
    CLOSURE_BOX_CELLS.with(|all| {
        let all = all.borrow();
        !all.is_empty() && all.contains_key(&(closure as usize))
    })
}

/// Preserve counted edges before a legacy caller mutates a shared-layout closure.
pub(super) fn prepare_dynamic_box_capture(closure: *mut ClosureHeader) {
    let edges = super::box_layout::take_dynamic_edges(closure);
    if !edges.is_empty() {
        CLOSURE_BOX_CELLS.with(|all| {
            let mut slots = BoxCaptureSlots::default();
            for (index, cell) in edges {
                slots.push(index, cell);
            }
            all.borrow_mut().insert(closure as usize, slots);
        });
    }
}

/// Record a compiler-declared boxed capture slot.
pub(super) fn set_closure_box_capture(
    closure: *mut ClosureHeader,
    index: u32,
    cell: Option<usize>,
) {
    if closure.is_null() {
        return;
    }
    let closure = closure as usize;
    let previous = CLOSURE_BOX_CELLS.with(|captures| {
        let mut captures = captures.borrow_mut();
        let slots = captures.entry(closure).or_default();
        let previous = slots.take(index);
        if let Some(cell) = cell {
            slots.push(index, cell);
        }
        if slots.is_empty() {
            captures.remove(&closure);
        }
        previous
    });
    if previous == cell {
        return;
    }
    if let Some(cell) = cell {
        increment_cell_capture_count(cell, 1);
    }
    if let Some(previous) = previous {
        decrement_cell_capture_count(previous, 1);
    }
}

/// Copy exact boxed-slot metadata when runtime code clones a closure.
pub(crate) fn clone_closure_box_captures(
    source: *const ClosureHeader,
    destination: *mut ClosureHeader,
) {
    if source.is_null() || destination.is_null() || source.cast_mut() == destination {
        return;
    }
    if super::box_layout::clone_owner(source, destination) {
        return;
    }
    let shared_edges = super::box_layout::copy_edges(source);
    if !shared_edges.is_empty() {
        for (index, _) in shared_edges {
            let cell = crate::r#box::registered_box_capture_addr(
                super::js_closure_get_capture_bits(destination, index) as usize,
            );
            set_closure_box_capture(destination, index, cell);
        }
        return;
    }
    let source = source as usize;
    let destination = destination as usize;
    let copied = CLOSURE_BOX_CELLS.with(|all| {
        let mut all = all.borrow_mut();
        debug_assert!(!all.contains_key(&destination));
        let copied = all.get(&source).cloned().unwrap_or_default();
        if !copied.is_empty() {
            all.insert(destination, copied.clone());
        }
        copied
    });
    for cell in copied.cells() {
        increment_cell_capture_count(cell, 1);
    }
}

pub(crate) fn closure_box_captures_owner_moved(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner {
        return;
    }
    super::box_layout::owner_moved(old_owner, new_owner);
    CLOSURE_BOX_CELLS.with(|all| {
        let mut all = all.borrow_mut();
        if let Some(cells) = all.remove(&old_owner) {
            debug_assert!(!all.contains_key(&new_owner));
            all.insert(new_owner, cells);
        }
    });
}

pub(crate) fn prune_dead_closure_box_capture_owners(is_dead_closure: &dyn Fn(usize) -> bool) {
    super::box_layout::prune(is_dead_closure);
    // One pass: dropping a dead owner and collecting its edges together avoids
    // a second probe per dead closure. Counts (and any publication they
    // trigger) are settled after the table borrow ends.
    let mut dead_cells = Vec::new();
    CLOSURE_BOX_CELLS.with(|captures| {
        captures.borrow_mut().retain(|owner, slots| {
            if is_dead_closure(*owner) {
                dead_cells.extend(slots.cells());
                false
            } else {
                true
            }
        });
    });
    for cell in dead_cells {
        decrement_cell_capture_count(cell, 1);
    }
}

#[cfg(test)]
pub(crate) fn test_clear_closure_box_capture_indexes() {
    super::box_layout::clear();
    CLOSURE_BOX_CELLS.with(|all| all.borrow_mut().clear());
    BOX_CAPTURE_COUNTS.with(|all| all.borrow_mut().clear());
}
