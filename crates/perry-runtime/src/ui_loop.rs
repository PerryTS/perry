//! Top-level UI event-loop takeover.
//!
//! A native UI backend (perry-ui-macos, …) can register a blocking event-loop
//! function. Perry's generated `main` calls [`js_ui_loop_take_over`] once, at
//! the true top level (right before the normal async event loop), so the UI
//! backend's `[NSApp run]` becomes the outermost loop on the main thread.
//!
//! Why this matters: if the UI loop is entered from a microtask / the post-main
//! drain instead (e.g. `Promise.resolve().then(() => appRunLoop())`), the macOS
//! window server never composites the app's windows (they exist but report
//! `onscreen=None`). Entering it at the top level — the same context as the
//! classic blocking `App({...})` call — fixes that. Used by the Electron-compat
//! `app.whenReady()` shell, which can't itself block at the top level because it
//! must first let the user's `main` register `ipcMain`/`whenReady` handlers.

use std::sync::atomic::{AtomicPtr, Ordering};

static UI_LOOP_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Register a blocking UI event-loop entry point. The UI backend calls this
/// (indirectly, when the program requests a windowed loop) so that
/// [`js_ui_loop_take_over`] can hand control to it. Passing a null-equivalent
/// is not expected; registration is one-shot for the process lifetime.
#[no_mangle]
pub extern "C" fn perry_runtime_register_ui_loop(f: extern "C" fn()) {
    UI_LOOP_FN.store(f as *mut (), Ordering::SeqCst);
}

/// Called once by generated `main` at the top level, just before the normal
/// async event loop. If a UI loop was registered (a windowed app requested it),
/// this blocks in that loop for the lifetime of the app (it returns only if the
/// UI loop itself returns, which a real app's `[NSApp run]` does not — it exits
/// via `terminate:`). If nothing registered a UI loop, this is a no-op and the
/// normal event loop proceeds.
#[no_mangle]
pub extern "C" fn js_ui_loop_take_over() {
    let p = UI_LOOP_FN.load(Ordering::SeqCst);
    if !p.is_null() {
        let f: extern "C" fn() = unsafe { std::mem::transmute(p) };
        f();
    }
}
