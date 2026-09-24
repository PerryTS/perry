//! Compiler-declared, immutable box-slot layouts shared by function body.
//!
//! Only the owner address is retained per closure. The authoritative cells
//! stay in its capture payload; GC death pruning reads them before reclaiming
//! the payload, and relocation rekeys the owner before retiring from-space.

use super::{closure_capture_slots_mut, real_capture_count, ClosureHeader};
use std::cell::RefCell;

crate::perry_thread_local! {
    // Code addresses and Rust-owned bitmaps; no GC-managed pointers.
    static FUNCTION_BOX_LAYOUTS: RefCell<crate::fast_hash::PtrHashMap<usize, Box<[u64]>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    // Weak owners, deliberately NOT roots. Rekeyed by owner_moved and
    // pruned before either sweep or the copied-minor from-space reset.
    static BOX_LAYOUT_OWNERS: RefCell<crate::fast_hash::PtrHashSet<usize>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_set());
}

unsafe fn visit_mask(closure: *mut ClosureHeader, mask: &[u64], mut visit: impl FnMut(u32, usize)) {
    let count = real_capture_count((*closure).capture_count) as usize;
    let slots = closure_capture_slots_mut(closure);
    for (word_index, &word) in mask.iter().enumerate() {
        let mut bits = word;
        while bits != 0 {
            let index = word_index * 64 + bits.trailing_zeros() as usize;
            assert!(index < count, "box capture layout exceeds closure payload");
            visit(index as u32, *slots.add(index) as usize);
            bits &= bits - 1;
        }
    }
}

fn visit_cells(closure: *mut ClosureHeader, visit: impl FnMut(u32, usize)) {
    FUNCTION_BOX_LAYOUTS.with(|layouts| {
        let layouts = layouts.borrow();
        // SAFETY: callers hold a live closure or run in pre-reclamation GC pruning.
        unsafe {
            if let Some(mask) = layouts.get(&((*closure).func_ptr as usize)) {
                visit_mask(closure, mask, visit);
            }
        }
    });
}

/// Register initialized, compiler-proven box captures in one batch. The mask
/// is constant for a function body; every named slot must contain a live box.
/// No collection occurs here. Repeated registration of a cached singleton is
/// idempotent and needs only the owner-set probe.
///
/// # Safety
/// `closure` must be live, with initialized captures. `mask` must name `words`
/// readable words; set bits must name live box pointers within the payload.
/// All registrations for the same function must supply the same mask.
#[no_mangle]
pub unsafe extern "C" fn js_closure_register_box_layout(
    closure: *mut ClosureHeader,
    mask: *const u64,
    words: u32,
) {
    if closure.is_null() || words == 0 {
        return;
    }
    if super::box_captures::has_dynamic_box_captures(closure) {
        return;
    }
    if !BOX_LAYOUT_OWNERS.with(|owners| owners.borrow_mut().insert(closure as usize)) {
        return;
    }
    FUNCTION_BOX_LAYOUTS.with(|layouts| {
        let mut layouts = layouts.borrow_mut();
        let supplied = std::slice::from_raw_parts(mask, words as usize);
        let layout = layouts
            .entry((*closure).func_ptr as usize)
            .or_insert_with(|| supplied.into());
        debug_assert_eq!(
            layout.as_ref(),
            supplied,
            "box layout changed for a function body"
        );
        visit_mask(closure, layout, |_, cell| {
            super::box_captures::increment_cell_capture_count(cell, 1)
        });
    });
}

pub(super) fn visit_payloads(closure: usize, visit: &mut impl FnMut(*mut u64)) {
    if BOX_LAYOUT_OWNERS.with(|owners| owners.borrow().contains(&closure)) {
        visit_cells(closure as *mut ClosureHeader, |_, cell| {
            crate::r#box::visit_pending_captured_js_box_payload_slot(cell, visit);
        });
    }
}

/// The compatibility setter permits dynamic slot replacement. Move that rare
/// owner to the old exact-edge representation without changing any counts.
pub(super) fn take_dynamic_edges(closure: *mut ClosureHeader) -> Vec<(u32, usize)> {
    let mut edges = Vec::new();
    if BOX_LAYOUT_OWNERS.with(|owners| owners.borrow_mut().remove(&(closure as usize))) {
        visit_cells(closure, |index, cell| edges.push((index, cell)));
    }
    edges
}

pub(super) fn clone_owner(source: *const ClosureHeader, destination: *mut ClosureHeader) -> bool {
    if !BOX_LAYOUT_OWNERS.with(|owners| owners.borrow().contains(&(source as usize))) {
        return false;
    }
    // Runtime rebinding can patch a capture. Only unchanged layouts can stay
    // in the shared representation; the caller handles the exceptional case.
    let mut unchanged = true;
    visit_cells(source.cast_mut(), |index, cell| {
        unchanged &= super::js_closure_get_capture_bits(destination, index) as usize == cell;
    });
    if !unchanged {
        return false;
    }
    if BOX_LAYOUT_OWNERS.with(|owners| owners.borrow_mut().insert(destination as usize)) {
        visit_cells(destination, |_, cell| {
            super::box_captures::increment_cell_capture_count(cell, 1)
        });
    }
    true
}

pub(super) fn copy_edges(source: *const ClosureHeader) -> Vec<(u32, usize)> {
    let mut edges = Vec::new();
    if BOX_LAYOUT_OWNERS.with(|owners| owners.borrow().contains(&(source as usize))) {
        visit_cells(source.cast_mut(), |index, cell| edges.push((index, cell)));
    }
    edges
}

pub(super) fn owner_moved(old: usize, new: usize) {
    BOX_LAYOUT_OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        if owners.remove(&old) {
            owners.insert(new);
        }
    });
}

pub(super) fn prune(is_dead: &dyn Fn(usize) -> bool) {
    let mut cells = Vec::new();
    BOX_LAYOUT_OWNERS.with(|owners| {
        owners.borrow_mut().retain(|owner| {
            if !is_dead(*owner) {
                return true;
            }
            visit_cells(*owner as *mut ClosureHeader, |_, cell| cells.push(cell));
            false
        });
    });
    for cell in cells {
        super::box_captures::decrement_cell_capture_count(cell, 1);
    }
}

#[cfg(test)]
pub(super) fn clear() {
    BOX_LAYOUT_OWNERS.with(|owners| owners.borrow_mut().clear());
    FUNCTION_BOX_LAYOUTS.with(|layouts| layouts.borrow_mut().clear());
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER_BOX_LAYOUT: unsafe extern "C" fn(*mut ClosureHeader, *const u64, u32) =
    js_closure_register_box_layout;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::closure::{box_capture_count, js_closure_alloc, js_closure_set_capture_bits};
    use crate::r#box::{js_box_alloc_bits, js_box_scope_release, test_clear_box_registry};

    fn is_registered_box_ptr(cell: *mut crate::r#box::Box) -> bool {
        crate::r#box::registered_box_capture_addr(cell as usize).is_some()
    }

    fn closure(
        func: usize,
        captures: u32,
        slots: &[(u32, usize)],
        mask: &[u64],
    ) -> *mut ClosureHeader {
        let owner = js_closure_alloc(func as *const u8, captures);
        for &(index, cell) in slots {
            js_closure_set_capture_bits(owner, index, cell as u64);
        }
        unsafe {
            js_closure_register_box_layout(owner, mask.as_ptr(), mask.len() as u32);
        }
        owner
    }

    #[test]
    fn shared_box_layout_wide_sparse_duplicate_edges_and_singleton() {
        test_clear_box_registry();
        let cell = js_box_alloc_bits(42);
        // Slot 1 contains the SAME pointer-shaped word but is not a box edge.
        let slots = [(0, cell as usize), (1, cell as usize), (65, cell as usize)];
        let mask = [1, 2];
        let a = closure(123, 66, &slots, &mask);
        let b = closure(123, 66, &slots, &mask);
        unsafe {
            js_closure_register_box_layout(a, mask.as_ptr(), 2);
        }
        assert_eq!(box_capture_count(cell as usize), 4);
        assert!(!super::super::box_captures::has_dynamic_box_captures(a));
        FUNCTION_BOX_LAYOUTS.with(|layouts| assert_eq!(layouts.borrow().len(), 1));
        BOX_LAYOUT_OWNERS.with(|owners| assert_eq!(owners.borrow().len(), 2));
        js_box_scope_release(cell);
        prune(&|owner| owner == a as usize);
        assert_eq!(box_capture_count(cell as usize), 2);
        assert!(is_registered_box_ptr(cell));
        prune(&|owner| owner == b as usize);
        assert!(!is_registered_box_ptr(cell));
    }

    #[test]
    fn shared_box_layout_move_clone_and_dynamic_replacement() {
        test_clear_box_registry();
        let first = js_box_alloc_bits(41);
        let second = js_box_alloc_bits(42);
        let a = closure(123, 1, &[(0, first as usize)], &[1]);
        let b = js_closure_alloc(123 as *const u8, 1);
        js_closure_set_capture_bits(b, 0, first as u64);
        crate::closure::clone_closure_box_captures(a, b);
        assert_eq!(box_capture_count(first as usize), 2);
        let moved = js_closure_alloc(123 as *const u8, 1);
        js_closure_set_capture_bits(moved, 0, first as u64);
        crate::closure::closure_box_captures_owner_moved(a as usize, moved as usize);
        prune(&|owner| owner == a as usize);
        assert_eq!(box_capture_count(first as usize), 2);
        crate::closure::js_closure_set_box_capture_ptr(b, 0, second as i64);
        assert_eq!(box_capture_count(first as usize), 1);
        assert_eq!(box_capture_count(second as usize), 1);
        js_box_scope_release(first);
        js_box_scope_release(second);
        crate::closure::prune_dead_closure_box_capture_owners(&|_| true);
        assert!(!is_registered_box_ptr(first));
        assert!(!is_registered_box_ptr(second));
    }

    #[test]
    fn shared_box_layout_rebound_clone_uses_actual_destination_edges() {
        test_clear_box_registry();
        let cell = js_box_alloc_bits(42);
        let source = closure(123, 2, &[(0, cell as usize), (1, cell as usize)], &[3]);
        let dest = js_closure_alloc(123 as *const u8, 2);
        js_closure_set_capture_bits(dest, 0, crate::value::TAG_UNDEFINED);
        js_closure_set_capture_bits(dest, 1, cell as u64);
        crate::closure::clone_closure_box_captures(source, dest);
        assert_eq!(box_capture_count(cell as usize), 3);
        js_box_scope_release(cell);
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == source as usize);
        assert_eq!(box_capture_count(cell as usize), 1);
        crate::closure::prune_dead_closure_box_capture_owners(&|owner| owner == dest as usize);
        assert!(!is_registered_box_ptr(cell));
    }
    #[test]
    fn shared_box_layout_traces_released_payload_through_live_closure() {
        test_clear_box_registry();
        let cell = js_box_alloc_bits(crate::value::TAG_UNDEFINED as i64);
        let owner = closure(123, 1, &[(0, cell as usize)], &[1]);
        let bits = crate::value::js_nanbox_pointer(owner as i64).to_bits();
        crate::r#box::js_box_set_bits(cell, bits as i64);
        js_box_scope_release(cell);
        crate::gc::begin_full_trace();
        let mut slots = Vec::new();
        crate::closure::visit_closure_box_payload_slots_mut(owner as usize, |slot| {
            slots.push(slot as usize)
        });
        crate::gc::finish_full_trace();
        assert!(
            slots.contains(&(cell as usize)),
            "live closure must reach the released box payload"
        );
        prune(&|_| true);
        assert!(!is_registered_box_ptr(cell));
    }
}
