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

fn run_frames(mut frame: impl FnMut()) {
    while !EXIT_FLAG.load(Ordering::Acquire) {
        crate::event_pump::perry_poll();
        if EXIT_FLAG.load(Ordering::Acquire) {
            break;
        }
        STATE_DIRTY.store(false, Ordering::Release);
        super::hooks::reset_hook_index();
        frame();
        if EXIT_FLAG.load(Ordering::Acquire) {
            break;
        }
        let _ = drain_input();
        if EXIT_FLAG.load(Ordering::Acquire) {
            break;
        }
        if !STATE_DIRTY.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(16));
        }
    }
}

/// `run(component)` — enter the render loop. The component closure is
/// called every frame and must return a Widget handle.
///
/// Lifecycle:
///   1. Enable raw mode + spawn the byte reader.
///   2. Clear screen + home cursor.
///   3. Loop:
///      - Advance async work outside component construction; stop on exit.
///      - Clear the previous dirty flag and reset the hook index.
///      - Call component → Widget handle.
///      - Paint into back buffer + flush diff.
///      - Drain pending input bytes (dispatching to useInput handler).
///      - If state was set during dispatch, re-render immediately.
///      - Otherwise sleep ~16 ms (≈60 fps polling cap).
///      - Exit when EXIT_FLAG is set.
///   4. Restore cooked-mode termios, cursor and the primary screen.
#[no_mangle]
pub extern "C" fn js_perry_tui_run(component: i64) -> f64 {
    if component == 0 {
        return f64::from_bits(JSValue::undefined().bits());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let component_closure = scope.root_raw_const_ptr(component as *const ClosureHeader);

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

    run_frames(|| {
        let widget_v = component_closure
            .across_const::<ClosureHeader, _>(|| {
                component_closure.with_const_ptr(|pointer| {
                    js_closure_call0(pointer, crate::closure::plain_call_receiver())
                })
            })
            .0;
        let widget_handle = super::handle_object::tui_widget_id_from_bits(widget_v.to_bits());
        super::ffi::paint_root_for_run(widget_handle);
    });

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static CALLBACKS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn update_state(
        _closure: *const ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        CALLBACKS.fetch_add(1, Ordering::AcqRel);
        STATE_DIRTY.store(true, Ordering::Release);
        f64::from_bits(JSValue::undefined().bits())
    }

    fn reset() {
        crate::gc::initialize_runtime_fixture();
        EXIT_FLAG.store(false, Ordering::Release);
        STATE_DIRTY.store(false, Ordering::Release);
        CALLBACKS.store(0, Ordering::Release);
    }

    #[test]
    fn timers_progress_without_input_between_frames() {
        let _serial = crate::event_pump::tests::serial();
        let _gc = crate::gc::global_side_table_test_lock();
        reset();
        let callback = crate::closure::js_closure_alloc(crate::fn_info!(update_state, 0), 0);
        let timeout = crate::timer::js_set_timeout_callback(callback as i64, 0.0);
        let interval = unsafe {
            crate::timer::js_set_interval_callback_args(callback as i64, 0.0, std::ptr::null(), 0)
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut frames = 0;
        run_frames(|| {
            frames += 1;
            if CALLBACKS.load(Ordering::Acquire) >= 3 || std::time::Instant::now() > deadline {
                super::super::input::js_perry_tui_exit();
            }
        });
        crate::timer::js_clear_timeout_value(crate::value::js_nanbox_pointer(timeout));
        crate::timer::js_clear_interval_value(crate::value::js_nanbox_pointer(interval));
        assert!(CALLBACKS.load(Ordering::Acquire) >= 3);
        assert!(frames > 0);
    }

    #[test]
    fn promise_microtasks_progress_before_building_a_frame() {
        let _serial = crate::event_pump::tests::serial();
        let _gc = crate::gc::global_side_table_test_lock();
        reset();
        extern "C" fn settled(
            _closure: *const ClosureHeader,
            _this: crate::closure::JsThis,
            _value: f64,
        ) -> f64 {
            CALLBACKS.fetch_add(1, Ordering::AcqRel);
            f64::from_bits(JSValue::undefined().bits())
        }
        let callback = crate::closure::js_closure_alloc(crate::fn_info!(settled, 1), 0);
        let promise = crate::promise::js_promise_resolved(1.0);
        crate::promise::js_promise_then(promise, callback, std::ptr::null());
        run_frames(|| {
            assert_eq!(CALLBACKS.load(Ordering::Acquire), 1);
            super::super::input::js_perry_tui_exit();
        });
    }

    #[test]
    fn pump_exit_does_not_build_another_frame() {
        let _serial = crate::event_pump::tests::serial();
        let _gc = crate::gc::global_side_table_test_lock();
        reset();
        extern "C" fn exit(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
            super::super::input::js_perry_tui_exit()
        }
        let callback = crate::closure::js_closure_alloc(crate::fn_info!(exit, 0), 0);
        crate::timer::js_set_timeout_callback(callback as i64, 0.0);
        std::thread::sleep(Duration::from_millis(10));
        run_frames(|| panic!("pump requested exit before the frame"));
        assert!(EXIT_FLAG.load(Ordering::Acquire));
    }

    #[test]
    fn dirty_updates_during_a_frame_are_not_erased() {
        let _serial = crate::event_pump::tests::serial();
        let _gc = crate::gc::global_side_table_test_lock();
        reset();
        run_frames(|| {
            STATE_DIRTY.store(true, Ordering::Release);
            super::super::input::js_perry_tui_exit();
        });
        assert!(STATE_DIRTY.load(Ordering::Acquire));
    }
}
