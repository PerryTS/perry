//! #11523 test hooks: plant work inside the typed-feedback registry's locked
//! region, and reach the private bodies of two `CannotCollect` helpers.

use std::cell::Cell;

thread_local! {
    static PLANTED: Cell<Option<fn()>> = const { Cell::new(None) };
}

/// Run `hook` inside every typed-feedback registry critical section on this
/// thread until cleared with `None`.
pub(crate) fn plant_in_locked_region(hook: Option<fn()>) {
    PLANTED.with(|planted| planted.set(hook));
}

pub(super) fn run_planted_locked_region_hook() {
    if let Some(hook) = PLANTED.with(Cell::get) {
        hook();
    }
}

/// The whole body of `js_typed_feedback_record_guard_pass`. The extern itself
/// cannot be driven here: a panic cannot unwind out of an `extern "C"` fn.
pub(crate) fn record_guard_pass(site_id: u64) {
    super::record_guard_pass(site_id);
}

/// What `js_gc_note_slot_layout` / `js_closure_set_capture_bits` reach through
/// `layout_mark_unknown` when a typed-layout object goes unknown.
pub(crate) fn invalidate_representation_change(obj_addr: usize) {
    super::invalidate_representation_change_when(obj_addr, true);
}
