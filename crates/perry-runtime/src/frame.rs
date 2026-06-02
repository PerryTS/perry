//! Display-link frame callbacks for `perry/ui` `onFrame` / `cancelFrame`.
//!
//! One-shot per registration, like `requestAnimationFrame`. The callback fires
//! the next time `js_frame_tick(timestamp_ms)` is invoked by the platform's
//! display-link driver (CADisplayLink on Apple, Choreographer on Android,
//! GTK4 tick callback on Linux, DwmFlush thread on Windows,
//! `requestAnimationFrame` in WASM). Subscribers fire in registration order.
//!
//! Per-subscriber `deltaMs` is computed off the most recent fire time
//! recorded for the same closure pointer — so the idiomatic
//! `function loop(t, dt) { ...; onFrame(loop); }` pattern gets accurate
//! deltas without bookkeeping. The very first call to a closure-pointer
//! gets `deltaMs = 0`.

use crate::closure::ClosureHeader;
use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::Mutex;

struct FrameCallback {
    id: i64,
    callback: i64,
    context: crate::async_context::AsyncContextSnapshot,
    cleared: bool,
}

// SAFETY: closure pointers point to global compiled code / GC-rooted data.
unsafe impl Send for FrameCallback {}

static FRAME_CALLBACKS: Mutex<Vec<FrameCallback>> = Mutex::new(Vec::new());
static NEXT_FRAME_ID: Mutex<i64> = Mutex::new(1);
static LAST_FIRE_BY_CLOSURE: Mutex<Option<HashMap<i64, f64>>> = Mutex::new(None);

fn next_frame_id() -> i64 {
    let mut next = NEXT_FRAME_ID.lock().unwrap();
    let current = *next;
    *next += 1;
    current
}

fn with_frame_uncaught_trap<F: FnOnce()>(f: F) {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    if jumped == 0 {
        f();
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::os::emit_process_uncaught_exception(exc);
    }
    crate::exception::js_try_end();
}

/// Register a one-shot frame callback. Returns an id usable with
/// `js_cancel_frame`. The callback is invoked as `cb(timestampMs, deltaMs)`
/// from `js_frame_tick`.
#[no_mangle]
pub extern "C" fn js_on_frame_callback(callback: i64) -> i64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cb_handle = scope.root_raw_const_ptr(callback as *const ClosureHeader);

    let id = next_frame_id();

    FRAME_CALLBACKS.lock().unwrap().push(FrameCallback {
        id,
        callback: cb_handle.get_raw_const_ptr::<ClosureHeader>() as i64,
        context: crate::async_context::capture_context(),
        cleared: false,
    });

    id
}

/// Cancel a previously-registered frame callback. No-op if `id` is unknown
/// or already fired.
#[no_mangle]
pub extern "C" fn js_cancel_frame(id: i64) {
    let mut queue = FRAME_CALLBACKS.lock().unwrap();
    for cb in queue.iter_mut() {
        if cb.id == id {
            cb.cleared = true;
            break;
        }
    }
    queue.retain(|c| !c.cleared);
}

/// Whether any frame callbacks are currently pending. Lets the platform's
/// display-link driver park itself when there's nothing to do.
#[no_mangle]
pub extern "C" fn js_frame_has_pending() -> i32 {
    let q = FRAME_CALLBACKS.lock().unwrap();
    if q.iter().any(|t| !t.cleared) {
        1
    } else {
        0
    }
}

/// Fire all pending frame callbacks once with the supplied vsync timestamp
/// (milliseconds, monotonic since app start). Returns the number of
/// callbacks that fired.
#[no_mangle]
pub extern "C" fn js_frame_tick(timestamp_ms: f64) -> i32 {
    use crate::closure::js_closure_call2;

    let pending: Vec<FrameCallback> = {
        let mut queue = FRAME_CALLBACKS.lock().unwrap();
        queue.drain(..).filter(|t| !t.cleared).collect()
    };

    let mut fired = 0;
    for cb in pending {
        let scope = crate::gc::RuntimeHandleScope::new();
        let cb_handle = scope.root_raw_const_ptr(cb.callback as *const ClosureHeader);

        let delta_ms = {
            let mut slot = LAST_FIRE_BY_CLOSURE.lock().unwrap();
            let map = slot.get_or_insert_with(HashMap::new);
            let prev = map.insert(cb.callback, timestamp_ms);
            match prev {
                Some(p) if timestamp_ms >= p => timestamp_ms - p,
                _ => 0.0,
            }
        };

        let previous = crate::async_context::enter_context(&cb.context);
        let mut previous = previous;
        let previous_roots = crate::async_context::root_snapshot(&scope, &previous);
        let cb_ptr = cb_handle.get_raw_const_ptr::<ClosureHeader>();
        with_frame_uncaught_trap(|| {
            js_closure_call2(cb_ptr, timestamp_ms, delta_ms);
        });
        crate::async_context::refresh_snapshot_from_roots(&mut previous, &previous_roots);
        crate::async_context::restore_context(previous);
        fired += 1;
    }

    fired
}

/// Convenience pump: tick using `js_timer_now()` for the timestamp. Useful
/// for platforms whose driver doesn't supply its own vsync timestamp
/// (Windows DwmFlush, Android Choreographer fallback) and for tests.
#[no_mangle]
pub extern "C" fn js_frame_pump_default() -> i32 {
    js_frame_tick(crate::timer::js_timer_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests share the global FRAME_CALLBACKS queue, so serialize them.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn clear_state() {
        FRAME_CALLBACKS.lock().unwrap().clear();
        if let Some(map) = LAST_FIRE_BY_CLOSURE.lock().unwrap().as_mut() {
            map.clear();
        }
    }

    #[test]
    fn registration_assigns_unique_ids_and_marks_pending() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_state();
        // Use distinct dummy callback bits — `js_frame_tick` is not invoked
        // here so the pointers never get dereferenced.
        let id_a = js_on_frame_callback(0x1000);
        let id_b = js_on_frame_callback(0x2000);
        assert_ne!(id_a, id_b);
        assert_eq!(js_frame_has_pending(), 1);
        clear_state();
        assert_eq!(js_frame_has_pending(), 0);
    }

    #[test]
    fn cancel_frame_removes_pending_callback() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_state();
        let id = js_on_frame_callback(0x3000);
        assert_eq!(js_frame_has_pending(), 1);
        js_cancel_frame(id);
        assert_eq!(js_frame_has_pending(), 0);
    }

    #[test]
    fn cancel_unknown_id_is_noop() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_state();
        js_on_frame_callback(0x4000);
        js_cancel_frame(999_999);
        assert_eq!(js_frame_has_pending(), 1);
        clear_state();
    }
}
