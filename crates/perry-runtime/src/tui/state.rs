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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Set when ANY state.set() call writes a different value. Cleared by
/// the render loop at the start of each frame; checked at the bottom
/// to decide whether to re-render immediately.
pub static STATE_DIRTY: AtomicBool = AtomicBool::new(false);

/// Per-state storage. Each state() call appends a slot; the returned
/// handle is the slot's index. Slots hold raw NaN-boxed JSValue bits
/// so any JS value (number, string, bool, object handle) round-trips
/// cleanly.
static SLOTS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

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
    use std::sync::Mutex as StdMutex;

    /// Cargo runs tests in parallel by default. SLOTS + STATE_DIRTY are
    /// process-wide globals, so two tests racing see each other's writes.
    /// This mutex serialises tests within this module — same shape as
    /// `tui::hooks::tests::TEST_LOCK` (#862).
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    /// Acquire the test lock and reset all shared state. Returns the
    /// guard — drop at end of test. Binding the guard to `_g` keeps it
    /// alive for the duration of the test; making it the return value
    /// of `reset()` ensures no test can clear globals without first
    /// taking the lock.
    fn reset() -> std::sync::MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        crate::gc::lock_gc_root_registry(&SLOTS).clear();
        STATE_DIRTY.store(false, Ordering::Release);
        g
    }

    #[test]
    fn alloc_returns_sequential_handles() {
        let _g = reset();
        let h0 = js_perry_tui_state_alloc(0.0);
        let h1 = js_perry_tui_state_alloc(1.0);
        let h2 = js_perry_tui_state_alloc(2.0);
        assert_eq!(h0, 0);
        assert_eq!(h1, 1);
        assert_eq!(h2, 2);
    }

    #[test]
    fn get_returns_initial_value() {
        let _g = reset();
        let h = js_perry_tui_state_alloc(42.0);
        let v = js_perry_tui_state_get(h);
        assert_eq!(v.to_bits(), 42.0_f64.to_bits());
    }

    #[test]
    fn set_writes_and_get_reads_back() {
        let _g = reset();
        let h = js_perry_tui_state_alloc(1.0);
        js_perry_tui_state_set(h, 99.0);
        let v = js_perry_tui_state_get(h);
        assert_eq!(v.to_bits(), 99.0_f64.to_bits());
    }

    #[test]
    fn set_flips_dirty_flag_on_change() {
        let _g = reset();
        let h = js_perry_tui_state_alloc(5.0);
        assert!(!STATE_DIRTY.load(Ordering::Acquire));
        js_perry_tui_state_set(h, 6.0);
        assert!(STATE_DIRTY.load(Ordering::Acquire));
    }

    #[test]
    fn set_to_same_value_does_not_flip_dirty() {
        let _g = reset();
        let h = js_perry_tui_state_alloc(7.0);
        js_perry_tui_state_set(h, 7.0);
        // Same value → no dirty flag.
        assert!(!STATE_DIRTY.load(Ordering::Acquire));
    }

    #[test]
    fn out_of_range_handle_returns_undefined() {
        let _g = reset();
        let v = js_perry_tui_state_get(9_999);
        assert_eq!(v.to_bits(), 0x7FFC_0000_0000_0001);
    }
}
