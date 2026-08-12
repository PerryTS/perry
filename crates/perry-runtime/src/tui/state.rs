//! Reactive state container for perry/tui.
//!
//! TS surface:
//!
//! ```typescript
//! const count = state(0);
//! count.set(count.get() + 1);
//! ```
//!
//! `state(initial)` returns a NaN-boxed POINTER handle whose `.get()` /
//! `.set(v)` methods dispatch via the codegen NativeModSig table to
//! `js_perry_tui_state_get` / `js_perry_tui_state_set`.
//!
//! Setter writes flip a global STATE_DIRTY atomic flag that the render
//! loop reads at the bottom of each frame; if it's been flipped since
//! the last paint, the loop re-renders immediately instead of sleeping.
//! That's the "trigger re-render on state change" semantics, with no
//! reconciler / no fiber tree / no diffing of widget trees — just a
//! coarse "something changed, redo it" signal. Good enough for a TUI
//! whose paint cost is dominated by the cell-grid diff anyway.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// #7680: `STATE_DIRTY` and `SLOTS` used to be cleared by THREE different
// locks — this module's own private `TEST_LOCK` (in `reset()` below),
// `gc::tests::roots`'s `lock_safe_runtime_scanner_test_guard()` (10 call
// sites), and the global GC-guard lock via `CopyingNurseryTestGuard`
// (`gc/tests/cycle_state.rs`, `gc/tests/runtime_roots/callback_scanners.rs`).
// `alloc_returns_sequential_handles`'s `h0 == 0, h1 == 1, h2 == 2` is exactly
// the #7672 shape: a concurrent `SLOTS.clear()` or a concurrent `alloc()` on
// another test thread breaks it, and it fails as "handles are not
// sequential" rather than naming the interference. `tui::state` is not on
// the GC guards' clear list `reset_copying_nursery_runtime_test_state()`
// walks, so #7674's gate never saw this table either. Per-thread storage
// removes the need for all three locks — `gc/tests/roots.rs` and friends
// still take `lock_safe_runtime_scanner_test_guard()` for `tui::hooks`'s
// still-unconverted slot pool and for scanner-registration serialization,
// which is unrelated to this table's isolation.
per_test_global! {
    /// Set when ANY state.set() call writes a different value. Cleared by
    /// the render loop at the start of each frame; checked at the bottom
    /// to decide whether to re-render immediately.
    pub static STATE_DIRTY: AtomicBool = AtomicBool::new(false);

    /// Per-state storage. Each state() call appends a slot; the returned
    /// handle is the slot's index. Slots hold raw NaN-boxed JSValue bits
    /// so any JS value (number, string, bool, object handle) round-trips
    /// cleanly.
    static SLOTS: Mutex<Vec<u64>> = Mutex::new(Vec::new());
}

/// GC root scanner — emits every slot value so heap-allocated JS
/// arrays/objects/strings stashed via `state.set(...)` stay reachable
/// across collections. Same rationale as `hooks::scan_hook_slot_roots`.
/// Pre-fix only numeric-state demos worked; storing an array reference
/// then triggering allocation freed it (#679 follow-up).
pub fn scan_state_slot_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_state_slot_roots_mut(&mut visitor);
}

pub fn scan_state_slot_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    for bits in s.iter_mut() {
        visitor.visit_nanbox_u64_slot(bits);
    }
}

#[derive(Default)]
pub(crate) struct StateSlotRootScanState {
    index: usize,
}

pub(crate) fn new_state_slot_root_scan_state() -> Box<dyn Any> {
    Box::<StateSlotRootScanState>::default()
}

pub(crate) fn scan_state_slot_roots_mut_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut dyn Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<StateSlotRootScanState>()
        .expect("tui state root scanner state type");
    let mut slots = crate::gc::lock_gc_root_registry(&SLOTS);
    while *remaining > 0 && state.index < slots.len() {
        visitor.visit_nanbox_u64_slot(&mut slots[state.index]);
        state.index += 1;
        *remaining -= 1;
    }
    state.index >= slots.len()
}

/// Allocate a fresh state slot with the given initial value (NaN-boxed
/// JSValue bits). Returns the slot index as the handle.
#[no_mangle]
pub extern "C" fn js_perry_tui_state_alloc(initial: f64) -> i64 {
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    let h = s.len() as i64;
    s.push(initial.to_bits());
    h
}

/// Read a state slot. Returns the stored NaN-boxed value. Out-of-range
/// handles return undefined.
#[no_mangle]
pub extern "C" fn js_perry_tui_state_get(handle: i64) -> f64 {
    let s = crate::gc::lock_gc_root_registry(&SLOTS);
    match s.get(handle as usize) {
        Some(bits) => f64::from_bits(*bits),
        None => f64::from_bits(0x7FFC_0000_0000_0001), // TAG_UNDEFINED
    }
}

/// Write a state slot. If the new value differs from the old, flips
/// STATE_DIRTY so the render loop re-renders next frame. Out-of-range
/// handles silently no-op.
#[no_mangle]
pub extern "C" fn js_perry_tui_state_set(handle: i64, value: f64) -> f64 {
    let mut s = crate::gc::lock_gc_root_registry(&SLOTS);
    if let Some(slot) = s.get_mut(handle as usize) {
        let new_bits = value.to_bits();
        if *slot != new_bits {
            *slot = new_bits;
            STATE_DIRTY.store(true, Ordering::Release);
        }
    }
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[cfg(test)]
pub(crate) fn test_reset_state_slots() {
    crate::gc::lock_gc_root_registry(&SLOTS).clear();
    STATE_DIRTY.store(false, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_with_state_slots_locked<R>(f: impl FnOnce() -> R) -> R {
    let _slots = crate::gc::lock_gc_root_registry(&SLOTS);
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    // #7680: no lock needed here anymore. `SLOTS` and `STATE_DIRTY` are
    // `per_test_global!`, so this thread's `reset()` and `alloc`/`get`/`set`
    // calls touch only this thread's own instances — a concurrent test on
    // another thread cannot land a `SLOTS.clear()` or a concurrent `alloc()`
    // between the three `js_perry_tui_state_alloc` calls below, which is
    // exactly the #7672 shape `alloc_returns_sequential_handles` is named
    // for (a wrong VALUE — non-sequential handles — not a hang).

    /// Reset this thread's state for a fresh test.
    fn reset() {
        crate::gc::lock_gc_root_registry(&SLOTS).clear();
        STATE_DIRTY.store(false, Ordering::Release);
    }

    #[test]
    fn alloc_returns_sequential_handles() {
        reset();
        let h0 = js_perry_tui_state_alloc(0.0);
        let h1 = js_perry_tui_state_alloc(1.0);
        let h2 = js_perry_tui_state_alloc(2.0);
        assert_eq!(h0, 0);
        assert_eq!(h1, 1);
        assert_eq!(h2, 2);
    }

    #[test]
    fn get_returns_initial_value() {
        reset();
        let h = js_perry_tui_state_alloc(42.0);
        let v = js_perry_tui_state_get(h);
        assert_eq!(v.to_bits(), 42.0_f64.to_bits());
    }

    #[test]
    fn set_writes_and_get_reads_back() {
        reset();
        let h = js_perry_tui_state_alloc(1.0);
        js_perry_tui_state_set(h, 99.0);
        let v = js_perry_tui_state_get(h);
        assert_eq!(v.to_bits(), 99.0_f64.to_bits());
    }

    #[test]
    fn set_flips_dirty_flag_on_change() {
        reset();
        let h = js_perry_tui_state_alloc(5.0);
        assert!(!STATE_DIRTY.load(Ordering::Acquire));
        js_perry_tui_state_set(h, 6.0);
        assert!(STATE_DIRTY.load(Ordering::Acquire));
    }

    #[test]
    fn set_to_same_value_does_not_flip_dirty() {
        reset();
        let h = js_perry_tui_state_alloc(7.0);
        js_perry_tui_state_set(h, 7.0);
        // Same value → no dirty flag.
        assert!(!STATE_DIRTY.load(Ordering::Acquire));
    }

    #[test]
    fn out_of_range_handle_returns_undefined() {
        reset();
        let v = js_perry_tui_state_get(9_999);
        assert_eq!(v.to_bits(), 0x7FFC_0000_0000_0001);
    }

    /// #7680: plants the #7672 shape directly — allocate a slot on THIS
    /// thread, clear `SLOTS` (what all three pre-fix lock domains eventually
    /// did) on ANOTHER thread, and assert the slot survived. Revert the
    /// `per_test_global!` conversion above (back to a bare `static`) and this
    /// fails: a foreign thread's clear empties the slot this thread just
    /// allocated, and a subsequent `js_perry_tui_state_alloc` on this thread
    /// hands out handle `0` again — the exact non-sequential-handle shape
    /// `alloc_returns_sequential_handles` is named for.
    #[test]
    fn state_slots_survive_a_foreign_clear() {
        reset();
        let h = js_perry_tui_state_alloc(7680.0);
        assert_eq!(
            js_perry_tui_state_get(h).to_bits(),
            7680.0_f64.to_bits(),
            "the probe installed nothing, so survived-vs-wiped would be vacuous"
        );

        std::thread::spawn(|| {
            crate::gc::lock_gc_root_registry(&SLOTS).clear();
            STATE_DIRTY.store(false, Ordering::Release);
        })
        .join()
        .expect("the clearing thread panicked");

        assert_eq!(
            js_perry_tui_state_get(h).to_bits(),
            7680.0_f64.to_bits(),
            "a state slot written on this thread was destroyed by a foreign thread's \
             clear (#7680). Per-thread storage (`per_test_global!`) is what prevents \
             this."
        );
        let h_next = js_perry_tui_state_alloc(1.0);
        assert_eq!(
            h_next,
            h + 1,
            "this thread's slot count must not have been reset by the foreign clear"
        );
        reset();
    }
}
