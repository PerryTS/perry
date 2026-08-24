//! GC root custody for the WinUI 3 (Fluent) backend.
//!
//! Every persistent JavaScript callback this crate keeps is stored as a RAW
//! CLOSURE POINTER (`js_nanbox_get_pointer(value) as usize`), so each stored
//! slot is a GC root. An unregistered holder is not an intermittent bug: it
//! goes bad at collection #0 and stays bad, surfacing later as
//! `TypeError: value is not a function` (see
//! `docs/src/internals/gc-rooting-invariant.md`). Same shape and same fix as
//! the sibling backends (`perry-ui-macos/src/gc.rs`, `perry-ui-ios/src/gc.rs`,
//! #8713).
//!
//! Registration also chains `perry-ui-windows`: on the Fluent path this crate
//! shadows `app_create`, so that crate's own `ensure_gc_scanner_registered()`
//! call never runs even though its Win32 tables (menu/tray/toolbar/window/
//! drag_drop/media_playback/pointer/widgets) are still live behind the
//! re-exports in `lib.rs`.

use std::cell::Cell;

use perry_ffi::{gc_register_mutable_root_scanner_named, GcRootVisitor};

thread_local! {
    /// Per-thread, NOT a process-global `Once`.
    ///
    /// `gc_register_mutable_root_scanner_named` installs its trampoline
    /// through a thread-local registry
    /// (`MUTABLE_ROOT_SCANNER_TRAMPOLINES_REGISTERED`, perry-ffi/src/handle.rs),
    /// so a process-global latch would let the first thread consume it and
    /// leave every later heap running without this scanner — the #8530 shape
    /// that `scripts/check_gc_scanner_latches.py` exists to reject. The
    /// registration itself is already idempotent per thread; this latch only
    /// keeps the common path off the registry mutex.
    static GC_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// Register this crate's mutable root scanner once per thread.
///
/// Called from the two funnels through which a JavaScript callback can first
/// reach this backend: `app::app_create` and `widgets::register`.
pub(crate) fn ensure_registered() {
    // NOTE: `perry-ui-windows` still latches on a process-global `Once`, as do
    // the other seven UI backends from #8713. Chaining it here is strictly
    // better than the status quo (on the Fluent path nothing registered it at
    // all); converting that family to per-thread latches is a separate change.
    perry_ui_windows::gc::ensure_gc_scanner_registered();
    GC_REGISTERED.with(|registered| {
        if registered.replace(true) {
            return;
        }
        gc_register_mutable_root_scanner_named("perry-ui-windows-winui", scan_roots);
    });
}

fn scan_roots(visitor: &mut GcRootVisitor<'_>) {
    crate::app::scan_winui_app_gc_roots(visitor);
    // `state.rs` is compiled INTO this crate via `#[path]`, so this crate has
    // its own instance of `STATES` / `FOR_EACH_BINDINGS` / `ON_CHANGE_BINDINGS`
    // that `perry-ui-windows`' scanner cannot reach.
    crate::state::scan_windows_state_gc_roots(visitor);
    crate::widgets::scan_winui_widgets_gc_roots(visitor);
}
