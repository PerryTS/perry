//! Lifetime bridge between GC closures and malloc-side async box cells.
//!
//! Codegen identifies the capture slots which contain raw box addresses. We
//! count only those declared edges before a box reaches terminal
//! `ReleaseBoxes`; arbitrary JS values must never be guessed to be boxes from
//! pointer-shaped bits. Once its async activation drains, the box runtime
//! publishes an unobserved cell immediately and leaves only a captured cell
//! pending. Closure moves rekey the per-closure index; authoritative GC death
//! pruning drops the corresponding per-cell counts.

use super::ClosureHeader;
use std::cell::RefCell;

type BoxCaptureSlots = Vec<(u32, usize)>;

crate::perry_thread_local! {
    /// Closure address -> compiler-declared `(capture index, box address)` edges.
    static CLOSURE_BOX_CELLS: RefCell<crate::fast_hash::PtrHashMap<usize, BoxCaptureSlots>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// Box address -> total number of capture slots naming it.
    static BOX_CAPTURE_COUNTS: RefCell<crate::fast_hash::PtrHashMap<usize, usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

fn increment_cell_capture_count(cell: usize, amount: usize) {
    BOX_CAPTURE_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        let count = counts.entry(cell).or_default();
        *count = count
            .checked_add(amount)
            .expect("box capture count overflow");
    });
}

fn decrement_cell_capture_count(cell: usize, amount: usize) {
    let reached_zero = BOX_CAPTURE_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        let Some(count) = counts.get_mut(&cell) else {
            return false;
        };
        debug_assert!(*count >= amount);
        *count -= amount;
        if *count == 0 {
            counts.remove(&cell);
            true
        } else {
            false
        }
    });
    if reached_zero {
        crate::r#box::box_capture_count_reached_zero(cell);
    }
}

pub(crate) fn box_capture_count(cell: usize) -> usize {
    BOX_CAPTURE_COUNTS
        .with(|counts| counts.borrow().get(&cell).copied())
        .unwrap_or(0)
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
    CLOSURE_BOX_CELLS.with(|captures| {
        let captures = captures.borrow();
        let Some(cells) = captures.get(&closure) else {
            return;
        };
        for &(_, cell) in cells {
            crate::r#box::visit_pending_captured_js_box_payload_slot(cell, &mut visit);
        }
    });
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
        let previous = slots
            .iter()
            .position(|(slot, _)| *slot == index)
            .map(|pos| slots.swap_remove(pos).1);
        if let Some(cell) = cell {
            slots.push((index, cell));
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
    for (_, cell) in copied {
        increment_cell_capture_count(cell, 1);
    }
}

pub(crate) fn closure_box_captures_owner_moved(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner {
        return;
    }
    CLOSURE_BOX_CELLS.with(|all| {
        let mut all = all.borrow_mut();
        if let Some(cells) = all.remove(&old_owner) {
            debug_assert!(!all.contains_key(&new_owner));
            all.insert(new_owner, cells);
        }
    });
}

pub(crate) fn prune_dead_closure_box_capture_owners(is_dead_closure: &dyn Fn(usize) -> bool) {
    let dead_keys = CLOSURE_BOX_CELLS.with(|captures| {
        captures
            .borrow()
            .keys()
            .copied()
            .filter(|owner| is_dead_closure(*owner))
            .collect::<Vec<_>>()
    });
    for closure in dead_keys {
        let cells = CLOSURE_BOX_CELLS
            .with(|captures| captures.borrow_mut().remove(&closure))
            .unwrap_or_default();
        for (_, cell) in cells {
            decrement_cell_capture_count(cell, 1);
        }
    }
}

#[cfg(test)]
pub(crate) fn test_clear_closure_box_capture_indexes() {
    CLOSURE_BOX_CELLS.with(|all| all.borrow_mut().clear());
    BOX_CAPTURE_COUNTS.with(|all| all.borrow_mut().clear());
}
