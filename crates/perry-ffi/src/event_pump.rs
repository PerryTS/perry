//! Event-pump surface — wakes the main thread when a tokio worker
//! (or other background thread) has produced an event that user
//! callbacks need to fire on.
//!
//! # Why
//!
//! Wrappers that expose `.on('event', cb)` semantics — `ws`, `http`,
//! `net`, `fastify` request/reply, `events.EventEmitter` (already
//! ported but in-process so no notify needed) — produce events on
//! tokio worker threads but must invoke user callbacks on the main
//! thread (perry-runtime's GC, NaN-boxing tag visibility, and
//! single-threaded JS semantics all assume main-thread callback
//! execution).
//!
//! The flow:
//!
//! ```text
//! tokio worker:        main thread:
//!   receive event        wait_for_event() →
//!   push onto queue          (woken by notify)
//!   notify_main_thread() → process_pending() drains queue
//!                          → JsClosure::call0/1/N for each listener
//! ```
//!
//! Wrappers manage their own per-module pending-events queue
//! (`Mutex<Vec<MyEvent>>` typical). At initialization they pass their
//! `process_pending` and `has_active` callbacks to
//! [`register_aux_event_pump`]. The runtime invokes those callbacks through
//! its registry, so neither codegen nor stdlib hard-references extension
//! symbols. The queue's producer side calls [`notify_main_thread`] after
//! pushing so the main loop wakes promptly instead of waiting on the
//! heartbeat cap.

#[cfg(any(not(test), feature = "runtime-link"))]
use std::sync::Once;

extern "C" {
    /// Wake the main thread from `js_wait_for_event`.
    ///
    /// Safe to call from any thread, including the main thread
    /// itself. Multiple notifies between consumer waits collapse to
    /// one wake — the main-loop tick drains every queue each pass
    /// regardless.
    fn js_notify_main_thread();
    #[cfg(any(not(test), feature = "runtime-link"))]
    fn js_register_aux_tick_begin(f: extern "C" fn());
    fn js_register_aux_pump(f: extern "C" fn() -> i32);
    fn js_register_aux_has_active(f: extern "C" fn() -> i32);
}

#[cfg(any(not(test), feature = "runtime-link"))]
static HANDLE_TICK_HOOK_REGISTRATION: Once = Once::new();

/// Install handle recycling before the registry can allocate its first id.
/// The runtime registration is itself idempotent, while `Once` keeps the hot
/// allocation path to a single atomic check after initialization.
pub(crate) fn ensure_handle_tick_hook_registered() {
    // Standalone perry-ffi unit tests do not link the runtime symbol. Product
    // binaries always resolve it when the compiler links libperry_runtime.
    #[cfg(any(not(test), feature = "runtime-link"))]
    HANDLE_TICK_HOOK_REGISTRATION.call_once(|| unsafe {
        js_register_aux_tick_begin(drain_handle_quarantine_at_tick_begin);
    });
}

/// Register an extension event pump and activity probe with the runtime.
/// Registration is idempotent for each function pointer.
pub fn register_aux_event_pump(pump: extern "C" fn() -> i32, has_active: extern "C" fn() -> i32) {
    ensure_handle_tick_hook_registered();
    unsafe {
        js_register_aux_pump(pump);
        js_register_aux_has_active(has_active);
    }
}

#[cfg(any(not(test), feature = "runtime-link"))]
extern "C" fn drain_handle_quarantine_at_tick_begin() {
    crate::handle::drain_quarantined_handles();
}

/// Wake the main thread so it picks up a pending event the calling
/// (worker) thread just pushed onto its wrapper's queue.
///
/// ```ignore
/// // Inside a tokio worker that received a websocket message:
/// MY_PENDING_EVENTS.lock().unwrap().push(event);
/// perry_ffi::notify_main_thread();
/// ```
pub fn notify_main_thread() {
    // SAFETY: the runtime entry is `extern "C"` and takes no args;
    // it can be called from any thread.
    unsafe { js_notify_main_thread() };
}

#[cfg(all(test, feature = "runtime-link"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

    extern "C" {
        fn js_run_stdlib_pump();
    }

    static LIFECYCLE_PHASE: AtomicUsize = AtomicUsize::new(0);
    static CALLBACK_HANDLE: AtomicI64 = AtomicI64::new(0);
    static NESTED_HANDLE: AtomicI64 = AtomicI64::new(0);
    static CALLBACK_DROPPED: AtomicBool = AtomicBool::new(false);

    extern "C" fn lifecycle_probe() -> i32 {
        // Record observations, then assert outside the C callback boundary.
        match LIFECYCLE_PHASE.fetch_add(1, Ordering::SeqCst) {
            0 => {
                let handle = crate::register_handle(30_u64);
                CALLBACK_HANDLE.store(handle, Ordering::SeqCst);
                CALLBACK_DROPPED.store(crate::drop_handle(handle), Ordering::SeqCst);
                // A callback re-entering the real runtime pump remains in
                // the same outer tick, so its newly retired id stays parked.
                unsafe { js_run_stdlib_pump() };
            }
            1 => {
                NESTED_HANDLE.store(crate::register_handle(40_u64), Ordering::SeqCst);
            }
            _ => {}
        }
        0
    }

    extern "C" fn lifecycle_idle() -> i32 {
        0
    }

    #[test]
    fn outer_ticks_recycle_handles_before_callbacks_without_http() {
        const CHILD: &str = "PERRY_TEST_OUTER_TICK_LIFECYCLE_CHILD";
        if std::env::var_os(CHILD).as_deref() != Some(std::ffi::OsStr::new("1")) {
            // Other handle tests share the process-wide freelist. A fresh
            // test process makes the exact reuse observations deterministic
            // and starts with no HTTP pump or auxiliary registry entries.
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "event_pump::tests::outer_ticks_recycle_handles_before_callbacks_without_http",
                ])
                .env(CHILD, "1")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut timed_out = false;
            while child.try_wait().unwrap().is_none() {
                if std::time::Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap();
            assert!(
                !timed_out && output.status.success(),
                "pump lifecycle child timed_out={timed_out}, status={}\n{}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            return;
        }

        // Exercise the public FFI registration seam and the actual runtime
        // entry point. Never call drain_quarantined_handles directly here.
        register_aux_event_pump(lifecycle_probe, lifecycle_idle);
        let prior_tick = crate::register_handle(10_u64);
        assert!(crate::drop_handle(prior_tick));
        let held = crate::register_handle(20_u64);
        assert_ne!(held, prior_tick, "retired ids wait for the next outer tick");

        unsafe { js_run_stdlib_pump() };
        assert_eq!(LIFECYCLE_PHASE.load(Ordering::SeqCst), 2);
        assert_eq!(CALLBACK_HANDLE.load(Ordering::SeqCst), prior_tick);
        assert!(CALLBACK_DROPPED.load(Ordering::SeqCst));
        let nested = NESTED_HANDLE.load(Ordering::SeqCst);
        assert_ne!(
            nested, prior_tick,
            "nested pumps must not promote this tick's ids"
        );
        assert_eq!(*crate::get_handle::<u64>(nested).unwrap(), 40);

        unsafe { js_run_stdlib_pump() };
        assert_eq!(LIFECYCLE_PHASE.load(Ordering::SeqCst), 3);
        let next_tick = crate::register_handle(50_u64);
        assert_eq!(
            next_tick, prior_tick,
            "a later outer tick makes the id reusable"
        );
        for handle in [held, nested, next_tick] {
            assert!(crate::drop_handle(handle));
        }
        unsafe { js_run_stdlib_pump() };
    }

    #[test]
    fn notify_main_thread_does_not_panic() {
        // The pump is initialized lazily by perry-runtime; calling
        // notify before any consumer has waited is a no-op success.
        // We just want to confirm the extern symbol is wired.
        notify_main_thread();
    }
}
