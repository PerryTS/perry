//! Render loop: `run(component)` enters interactive mode, calls the
//! user's component closure each frame, paints the result, drains
//! input, and re-renders on state change. Exits when `exit()` is
//! called from a useInput handler.
//!
//! ```typescript
//! run(() => Box([Text("count: " + count.get())]));
//! ```

use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::closure::{js_closure_call0, ClosureHeader};
use crate::value::JSValue;

use super::input::{disable_raw_mode, drain_input, enable_raw_mode, EXIT_FLAG};
use super::state::STATE_DIRTY;

/// `run(component)` — enter the render loop. The component closure is
/// called every frame and must return a Widget handle.
///
/// Lifecycle:
///   1. Enable raw mode + spawn the byte reader.
///   2. Clear screen + home cursor.
///   3. Loop:
///      - Call component → Widget handle.
///      - Paint into back buffer + flush diff.
///      - Drain pending input bytes (dispatching to useInput handler).
///      - If state was set during dispatch, re-render immediately.
///      - Otherwise sleep ~16 ms (≈60 fps polling cap).
///      - Exit when EXIT_FLAG is set.
///   4. Restore cooked-mode termios + clear screen.
#[no_mangle]
pub extern "C" fn js_perry_tui_run(component: i64) -> f64 {
    if component == 0 {
        return f64::from_bits(JSValue::undefined().bits());
    }
    let component_closure = component as *const ClosureHeader;

    // Enter alt screen + clear + hide cursor. (Alt screen via DECSET 1049
    // is widely supported and means we don't pollute the user's primary
    // scrollback with our cell-grid output.)
    {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        let _ = h.write_all(b"\x1b[?1049h\x1b[2J\x1b[H\x1b[?25l");
        let _ = h.flush();
    }
    enable_raw_mode();

    loop {
        if EXIT_FLAG.load(Ordering::Acquire) {
            break;
        }

        // Reset hook index — ink-shape hooks (useState/useEffect/useMemo/
        // useRef) bind to a per-frame index, so the slot at position N
        // is the SAME slot the previous frame's Nth hook saw. Without
        // this reset the index would grow unboundedly and every render
        // would allocate fresh state. (#679 Phase 1.)
        super::hooks::reset_hook_index();

        // Call the component to get a fresh widget tree.
        let widget_v = js_closure_call0(component_closure);
        // Unbox the POINTER tag → raw handle (low 48 bits).
        let widget_handle = (widget_v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;

        // Paint the tree into the back buffer + flush.
        super::ffi::paint_root_for_run(widget_handle);

        // Clear the dirty flag *before* draining input — if the user's
        // handler calls state.set, dirty flips back on, and we'll
        // see it after drain returns.
        STATE_DIRTY.store(false, Ordering::Release);

        // Drain any pending input bytes; useInput handlers fire here.
        let _ = drain_input();

        if STATE_DIRTY.load(Ordering::Acquire) {
            // State changed — re-render without sleeping.
            continue;
        }

        // No state change → idle for one polling tick.
        std::thread::sleep(Duration::from_millis(16));
    }

    // Restore terminal state.
    disable_raw_mode();
    {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        // Show cursor + leave alt screen.
        let _ = h.write_all(b"\x1b[?25h\x1b[?1049l");
        let _ = h.flush();
    }

    f64::from_bits(JSValue::undefined().bits())
}
