//! Unit tests for the readline/stdin module.
//!
//! Split out of `mod.rs` to keep it under the 2,000-line file gate.

use super::test_support::*;
use super::*;

struct ForcedEvacuationGuard {
    frame: u64,
    previous_force_evacuation: i32,
}

impl ForcedEvacuationGuard {
    fn new() -> Self {
        // Compiled programs initialize the runtime-handle scanner at startup.
        // This standalone crate test must do the equivalent before collecting.
        perry_runtime::gc::gc_init();
        let previous_force_evacuation = perry_runtime::gc::js_gc_force_evacuation_test_override(1);
        perry_runtime::gc::js_gc_write_barriers_emitted(1);
        let frame = perry_runtime::gc::js_shadow_frame_push(0);
        Self {
            frame,
            previous_force_evacuation,
        }
    }
}

impl Drop for ForcedEvacuationGuard {
    fn drop(&mut self) {
        perry_runtime::gc::js_shadow_frame_pop(self.frame);
        perry_runtime::gc::js_gc_write_barriers_emitted(0);
        perry_runtime::gc::js_gc_force_evacuation_test_override(self.previous_force_evacuation);
    }
}

#[test]
fn close_without_callbacks_is_noop() {
    let _g = reset();
    let h = js_readline_create_interface(0.0);
    assert_eq!(h, STDIN_READLINE_HANDLE);
    js_readline_close(h);
    assert!(STDIN_PAUSED.load(Ordering::Acquire));
    assert!(!EOF_REACHED.load(Ordering::Acquire));
    test_inject_line("late");
    assert_eq!(js_readline_process_pending(), 0);
    assert_eq!(PENDING_LINES.lock().unwrap().len(), 1);
    assert_eq!(js_readline_has_active(), 0);
    assert_eq!(js_readline_process_pending(), 0);
}

#[test]
fn stdin_close_pauses_delivery_until_explicit_resume() {
    let _g = reset();
    let h = js_readline_create_interface(0.0);
    let event = event_name("data");
    let cb = data_counter_callback();
    let _ = js_readline_stdin_on(event, cb);

    js_readline_close(h);
    test_inject_chunk(b"late");
    assert_eq!(js_readline_process_pending(), 0);
    assert_eq!(PENDING_DATA.lock().unwrap().len(), 1);
    assert!(!EOF_REACHED.load(Ordering::Acquire));

    let _ = js_readline_stdin_resume();
    assert_eq!(js_readline_process_pending(), 1);
    DATA_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
}

#[test]
fn new_stdin_interface_resumes_after_previous_interface_closed() {
    let _g = reset();
    let first = js_readline_create_interface(0.0);
    js_readline_close(first);
    assert!(STDIN_PAUSED.load(Ordering::Acquire));

    let second = js_readline_create_interface(0.0);
    assert_eq!(second, STDIN_READLINE_HANDLE);
    assert!(!STDIN_PAUSED.load(Ordering::Acquire));
    assert!(!EOF_REACHED.load(Ordering::Acquire));
}

#[test]
fn repeated_custom_close_does_not_mutate_stdin_state() {
    let _g = reset();
    let handle = allocate_interface(ReadlineInterfaceState::new(
        undefined(),
        undefined(),
        String::new(),
        false,
        true,
    ));
    QUESTION_CALLBACK.with(|cb| *cb.borrow_mut() = Some(123));

    js_readline_close(handle);
    assert!(!EOF_REACHED.load(Ordering::Acquire));
    QUESTION_CALLBACK.with(|cb| assert_eq!(*cb.borrow(), Some(123)));

    js_readline_close(handle);
    assert!(!EOF_REACHED.load(Ordering::Acquire));
    QUESTION_CALLBACK.with(|cb| assert_eq!(*cb.borrow(), Some(123)));
}

#[test]
fn injected_line_drains_via_test_helper() {
    let _g = reset();
    test_inject_line("hello");
    // No callback registered → drain consumes the line silently and
    // reports 0 callbacks fired.
    assert_eq!(js_readline_process_pending(), 0);
    assert_eq!(PENDING_LINES.lock().unwrap().len(), 0);
}

/// Every `process.stdin.on("end", …)` listener must fire, not just the
/// last one registered.
///
/// These used to share the single-slot readline `CLOSE_CALLBACK`, so each
/// registration clobbered the previous one. Claude Code registers three
/// `stdin.on("end")` handlers; the one that resolved its read-stdin promise
/// was silently dropped, the promise never settled, and the process exited
/// 0 having printed nothing.
#[test]
fn every_stdin_end_listener_fires() {
    let _g = reset();
    let a = data_counter_callback();
    let b = data_counter_callback();
    let c = data_counter_callback();
    for cb in [a, b, c] {
        js_readline_stdin_on(event_name("end"), cb);
    }
    assert_eq!(
        STDIN_END_CALLBACKS.lock().map(|v| v.len()).unwrap_or(0),
        3,
        "all three end listeners must be retained"
    );
    EOF_REACHED.store(true, Ordering::Release);
    js_readline_process_pending();
    assert_eq!(
        DATA_COUNT.with(|n| *n.borrow()),
        3,
        "each registered end listener must be invoked exactly once"
    );
}

/// The PROVIDER path — `stdin_on_op` / `stdin_off_op` — must handle
/// `end`/`close` too, not just the syntactic `js_readline_stdin_on` extern
/// that `every_stdin_end_listener_fires` above covers.
///
/// The provider is what the stdin object's native `on`/`once`/`addListener`
/// delegate to, i.e. every registration that is NOT codegen's literal
/// `process.stdin.x(…)` shape: an alias, or stdin passed as a parameter.
/// Claude Code's print-mode reader is the parameter form — `X71(process.stdin,
/// 3000)` then `stream.once("end", …)` inside — so without these arms its
/// `race(once("end"), timeout(3000))` can never resolve via `end` and
/// `echo hi | claude -p …` never completes.
///
/// These two tests guard arms that have now been lost twice, which is why they
/// assert the provider entry points directly rather than going through the
/// extern.
#[test]
fn provider_path_registers_end_listeners() {
    let _g = reset();
    let a = data_counter_callback();
    let b = data_counter_callback();
    stdin_on_op(b"end".as_ptr(), 3, a, 0);
    stdin_on_op(b"close".as_ptr(), 5, b, 1);
    assert_eq!(
        STDIN_END_CALLBACKS.lock().map(|v| v.len()).unwrap_or(0),
        2,
        "aliased end/close registrations must reach the end-listener list"
    );
    EOF_REACHED.store(true, Ordering::Release);
    js_readline_process_pending();
    assert_eq!(
        DATA_COUNT.with(|n| *n.borrow()),
        2,
        "listeners registered through the provider must fire at EOF"
    );
}

#[test]
fn provider_path_removes_end_listeners() {
    let _g = reset();
    let cb = data_counter_callback();
    stdin_on_op(b"end".as_ptr(), 3, cb, 0);
    stdin_off_op(b"end".as_ptr(), 3, cb);
    assert_eq!(
        STDIN_END_CALLBACKS.lock().map(|v| v.len()).unwrap_or(0),
        0,
        "removing an aliased end listener must clear it"
    );
}

#[test]
fn listeners_provider_roots_readable_snapshot_across_array_allocation() {
    let _g = reset();
    let _gc = ForcedEvacuationGuard::new();
    ensure_gc_scanner_registered();

    let before = readable_counter_callback();
    stdin_on_op(b"readable".as_ptr(), 8, before, 0);
    let snapshot = stdin_listener_snapshot(b"readable".as_ptr(), 8);
    let listeners = listener_array_from_snapshot_impl(snapshot, || {
        perry_runtime::gc::js_gc_collect();
    });

    let current = READABLE_CALLBACKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())[0];
    assert_ne!(
        before, current,
        "the forced collection must relocate the callback to exercise the regression"
    );

    let array = JSValue::from_bits(listeners.to_bits())
        .as_pointer::<perry_runtime::array::ArrayHeader>()
        as *mut perry_runtime::array::ArrayHeader;
    let returned = perry_runtime::array::js_array_get_f64(array, 0);
    let returned = JSValue::from_bits(returned.to_bits()).as_pointer::<ClosureHeader>() as i64;
    assert_eq!(
        returned, current,
        "listeners() must return the collector-rewritten callback, not its stale snapshot"
    );
}

/// An `on("readable")` listener puts stdin in paused/pull mode, where the
/// fd-0 reader must buffer bytes for `process.stdin.read()` instead of
/// routing them to readline's line queue (which `read()` never drains).
#[test]
fn readable_listener_enables_pull_mode() {
    let _g = reset();
    assert!(!STDIN_PULL_MODE.load(Ordering::Acquire));
    let cb = readable_counter_callback();
    js_readline_stdin_on(event_name("readable"), cb);
    assert!(
        STDIN_PULL_MODE.load(Ordering::Acquire),
        "a readable listener must switch the reader into pull mode"
    );
    js_readline_stdin_remove_listener(event_name("readable"), cb);
    assert!(
        !STDIN_PULL_MODE.load(Ordering::Acquire),
        "removing the last readable listener must leave pull mode"
    );
}

#[test]
fn has_active_reflects_state() {
    let _g = reset();
    EOF_REACHED.store(true, Ordering::Release);
    CLOSE_FIRED.with(|f| *f.borrow_mut() = true);
    assert_eq!(js_readline_has_active(), 0);
    test_inject_line("x");
    assert_eq!(js_readline_has_active(), 1);
    PENDING_LINES.lock().unwrap().clear();
    assert_eq!(js_readline_has_active(), 0);
}

#[test]
fn injected_chunk_drains_via_data_queue() {
    let _g = reset();
    test_inject_chunk(b"a");
    // No data callback registered → drain consumes silently.
    assert_eq!(js_readline_process_pending(), 0);
    assert_eq!(PENDING_DATA.lock().unwrap().len(), 0);
}

#[test]
fn stdin_remove_listener_detaches_data_callback() {
    let _g = reset();
    let event = event_name("data");
    // Allocate the event string before the raw callback pointer. The real
    // JS caller roots both arguments; this unit test must not leave its
    // freshly allocated closure unrooted across `event_name`.
    let cb = data_counter_callback();
    let _ = js_readline_stdin_on(event, cb);
    let _ = js_readline_stdin_remove_listener(event, cb);
    test_inject_chunk(b"x");
    assert_eq!(js_readline_process_pending(), 0);
    DATA_COUNT.with(|count| assert_eq!(*count.borrow(), 0));
    assert_eq!(js_readline_has_active(), 0);
}

#[test]
fn stdin_pause_resume_gates_data_dispatch() {
    let _g = reset();
    let event = event_name("data");
    let cb = data_counter_callback();
    let _ = js_readline_stdin_on(event, cb);
    let _ = js_readline_stdin_pause();
    test_inject_chunk(b"x");
    assert_eq!(js_readline_process_pending(), 0);
    assert_eq!(PENDING_DATA.lock().unwrap().len(), 1);
    DATA_COUNT.with(|count| assert_eq!(*count.borrow(), 0));

    let _ = js_readline_stdin_resume();
    assert_eq!(js_readline_process_pending(), 1);
    assert_eq!(PENDING_DATA.lock().unwrap().len(), 0);
    DATA_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
}

#[test]
fn stdin_data_listener_flows_without_raw_mode() {
    // #5227: a 'data' listener attached in cooked (non-raw) mode must
    // switch stdin into flowing mode and keep the loop alive so the
    // reader can deliver chunks — previously only raw mode did.
    let _g = reset();
    READER_STARTED.store(true, Ordering::Release);
    assert!(!RAW_MODE.load(Ordering::Acquire));
    assert!(!STDIN_DATA_FLOWING.load(Ordering::Acquire));

    let event = event_name("data");
    let cb = data_counter_callback();
    let _ = js_readline_stdin_on(event, cb);
    assert!(STDIN_DATA_FLOWING.load(Ordering::Acquire));
    // Cooked-mode data listener keeps the event loop alive.
    assert_eq!(js_readline_has_active(), 1);

    // Cooked-mode chunks (delivered by the reader with the newline
    // included) drain to the 'data' callback.
    test_inject_chunk(b"hello world\n");
    assert_eq!(js_readline_process_pending(), 1);
    DATA_COUNT.with(|count| assert_eq!(*count.borrow(), 1));

    // Removing the last data listener clears flowing mode.
    let _ = js_readline_stdin_remove_listener(event, cb);
    assert!(!STDIN_DATA_FLOWING.load(Ordering::Acquire));
}

#[test]
fn stdin_unref_and_destroy_release_active_state() {
    let _g = reset();
    READER_STARTED.store(true, Ordering::Release);
    RAW_MODE.store(true, Ordering::Release);
    let _ = js_readline_stdin_on(event_name("data"), data_counter_callback());
    assert_eq!(js_readline_has_active(), 1);

    let _ = js_readline_stdin_unref();
    assert_eq!(js_readline_has_active(), 0);

    let _ = js_readline_stdin_ref();
    test_inject_chunk(b"x");
    assert_eq!(js_readline_has_active(), 1);
    let _ = js_readline_stdin_destroy();
    assert_eq!(js_readline_has_active(), 0);
    assert_eq!(PENDING_DATA.lock().unwrap().len(), 0);
    assert!(DATA_CALLBACKS.lock().map(|v| v.is_empty()).unwrap_or(true));
    assert!(STDIN_DESTROYED.load(Ordering::Acquire));
}

#[test]
fn complete_escape_sequence_reassembles_to_single_keypress() {
    // The raw-mode reader queues one byte per chunk, so an arrow key
    // arrives as `\x1b`, `[`, `A` in three chunks. The pump must
    // reassemble them into ONE 'up' keypress, not escape + [ + A.
    let _g = reset();
    let event = event_name("keypress");
    let cb = keypress_recorder_callback();
    let _ = js_readline_stdin_on(event, cb);
    test_inject_chunk(b"\x1b");
    test_inject_chunk(b"[");
    test_inject_chunk(b"A");
    let fired = js_readline_process_pending();
    KEYPRESS_NAMES.with(|names| assert_eq!(*names.borrow(), vec!["up".to_string()]));
    assert_eq!(fired, 1);
}

#[test]
fn split_escape_sequence_reassembles_before_timeout() {
    // ESC can arrive in one read and the rest of an arrow in a later read.
    // The completing bytes cancel the one-shot instead of observing a tick as
    // an implicit timeout and tearing the arrow apart.
    let _g = reset();
    let event = event_name("keypress");
    let cb = keypress_recorder_callback();
    let _ = js_readline_stdin_on(event, cb);
    test_inject_chunk(b"\x1b");
    assert_eq!(js_readline_process_pending(), 0);
    assert!(js_readline_next_wake_ms() > 0.0);
    test_inject_chunk(b"[");
    test_inject_chunk(b"C");
    assert_eq!(js_readline_process_pending(), 1);
    KEYPRESS_NAMES.with(|names| assert_eq!(*names.borrow(), vec!["right".to_string()]));
    assert_eq!(js_readline_next_wake_ms(), -1.0);
}

#[test]
fn bare_escape_flushes_only_after_escape_code_timeout() {
    // A lone ESC can't be distinguished from the start of a sequence. Pump
    // turns before the deadline must leave it held; expiry emits one keypress.
    let _g = reset();
    let event = event_name("keypress");
    let cb = keypress_recorder_callback();
    let _ = js_readline_stdin_on(event, cb);
    test_inject_chunk(b"\x1b");
    assert_eq!(js_readline_process_pending(), 0);
    // The held prefix keeps the loop alive so the flush tick runs.
    assert_eq!(js_readline_has_active(), 1);
    let remaining = js_readline_next_wake_ms();
    assert!((1.0..=500.0).contains(&remaining));
    assert_eq!(js_readline_process_pending(), 0);
    *PENDING_ESCAPE_DEADLINE.lock().unwrap() = Some(Instant::now() - Duration::from_millis(1));
    assert_eq!(js_readline_process_pending(), 1);
    KEYPRESS_NAMES.with(|names| assert_eq!(*names.borrow(), vec!["escape".to_string()]));
    assert_eq!(js_readline_next_wake_ms(), -1.0);
}

#[test]
fn readable_only_fires_with_new_chunks() {
    // A registered 'readable' listener must not be invoked on ticks
    // that delivered no new data (that was a per-tick JS busy loop).
    let _g = reset();
    let event = event_name("readable");
    let cb = readable_counter_callback();
    let _ = js_readline_stdin_on(event, cb);
    assert_eq!(js_readline_process_pending(), 0);
    assert_eq!(js_readline_process_pending(), 0);
    test_inject_chunk(b"x");
    assert_eq!(js_readline_process_pending(), 1);
    DATA_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    // Queue drained again → quiet ticks stay quiet.
    assert_eq!(js_readline_process_pending(), 0);
}

#[test]
fn raw_mode_toggle_flips_atomic() {
    let _g = reset();
    assert!(!RAW_MODE.load(Ordering::Acquire));
    // Truthy → enable.
    let _ = js_readline_set_raw_mode(f64::from_bits(JSValue::bool(true).bits()));
    assert!(RAW_MODE.load(Ordering::Acquire));
    // Falsy → disable.
    let _ = js_readline_set_raw_mode(f64::from_bits(JSValue::bool(false).bits()));
    assert!(!RAW_MODE.load(Ordering::Acquire));
}
