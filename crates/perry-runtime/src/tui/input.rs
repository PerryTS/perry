//! Raw-mode stdin reader + keypress dispatch for the TUI render loop.
//!
//! Conceptually the same shape as the readline reader (#347 Phase 2)
//! but lives in perry-runtime so perry/tui can own stdin without a
//! cross-crate dep on stdlib's readline. Once the render loop calls
//! `enable_raw_mode()`, every byte read from stdin is queued in
//! `PENDING_BYTES`; the loop drains the queue at every frame and
//! dispatches to the registered `useInput` handler.
//!
//! Mode toggle is process-wide and reversible — the cooked-mode
//! termios is saved on first enable so disable can restore.

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;

use crate::closure::{js_closure_call1, ClosureHeader};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

// ---------------------------------------------------------------------------
// Cross-thread state
// ---------------------------------------------------------------------------

/// Bytes waiting for the main thread to dispatch.
static PENDING_BYTES: Mutex<Vec<u8>> = Mutex::new(Vec::new());
/// Whether the reader thread has been spawned.
static READER_STARTED: AtomicBool = AtomicBool::new(false);
/// Whether we should keep reading. Cleared on `disable_raw_mode` so
/// the thread can exit cleanly (it'll observe this between bytes).
/// Note: `read()` is blocking so an in-flight read doesn't see the
/// flag until the next byte arrives — fine for our shutdown path
/// which is typically driven by the user pressing 'q' (which IS a
/// byte).
static READING: AtomicBool = AtomicBool::new(false);
/// Registered useInput handler — at most one for v1. Multiple-handler
/// dispatch lands in Phase 2.5. Stored as the raw closure pointer.
///
/// **This is a GC root, and must stay one (#7231).** `useInput(cb)` is
/// written inline in idiomatic ink style, so the arrow closure is a
/// nursery allocation that NOTHING else refers to once
/// `js_perry_tui_use_input` returns. Before
/// `scan_tui_input_handler_root_mut` the next collection reclaimed it and
/// `drain_input` called through a dangling `ClosureHeader*` on the first
/// keystroke.
static INPUT_HANDLER: AtomicI64 = AtomicI64::new(0);
static INPUT_BYTES_MODE: AtomicBool = AtomicBool::new(false);
/// Set when the user calls exit() — render loop checks this each frame.
pub static EXIT_FLAG: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Termios (Unix only)
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod termios_impl {
    use std::sync::Mutex;

    static SAVED: Mutex<Option<libc::termios>> = Mutex::new(None);

    pub fn enable() -> bool {
        unsafe {
            let mut current: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut current) != 0 {
                return false;
            }
            {
                let mut saved = SAVED.lock().unwrap();
                if saved.is_none() {
                    *saved = Some(current);
                }
            }
            let mut raw = current;
            raw.c_iflag &= !(libc::IGNBRK
                | libc::BRKINT
                | libc::PARMRK
                | libc::ISTRIP
                | libc::INLCR
                | libc::IGNCR
                | libc::ICRNL
                | libc::IXON);
            raw.c_oflag &= !libc::OPOST;
            raw.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
            raw.c_cflag &= !(libc::CSIZE | libc::PARENB);
            raw.c_cflag |= libc::CS8;
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSANOW, &raw) == 0
        }
    }

    pub fn disable() -> bool {
        unsafe {
            let saved = SAVED.lock().unwrap();
            if let Some(t) = saved.as_ref() {
                libc::tcsetattr(0, libc::TCSANOW, t) == 0
            } else {
                true
            }
        }
    }
}

#[cfg(windows)]
mod termios_impl {
    /// Flip stdin into byte-mode + virtual-terminal-input mode via the
    /// shared `win_console` helpers (#406; consolidated by the Windows
    /// TTY fix — the flag math and save/restore now live in one place,
    /// shared with `tty.rs`'s `setRawMode`). Output-side VT processing
    /// is enabled process-wide at startup (`js_gc_init` →
    /// `win_console::enable_vt_output`); `enable()` re-asserts it in
    /// case something reset the console mode in between.
    pub fn enable() -> bool {
        if !crate::win_console::set_stdin_raw(true) {
            return false;
        }
        crate::win_console::enable_vt_output();
        true
    }

    pub fn disable() -> bool {
        crate::win_console::set_stdin_raw(false)
    }
}

#[cfg(not(any(unix, windows)))]
mod termios_impl {
    pub fn enable() -> bool {
        false
    }
    pub fn disable() -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Enable raw mode + spawn the reader thread (idempotent). Called from
/// the render-loop entry.
pub fn enable_raw_mode() {
    let _ = termios_impl::enable();
    // WASI cannot spawn the reader thread: no terminal input.
    if cfg!(target_os = "wasi") {
        return;
    }
    READING.store(true, Ordering::Release);
    if READER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut handle = stdin.lock();
            let mut byte = [0u8; 1];
            while READING.load(Ordering::Acquire) {
                match handle.read(&mut byte) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(mut q) = PENDING_BYTES.lock() {
                            q.push(byte[0]);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }
}

/// Restore cooked-mode termios. The reader thread is left running
/// (can't safely cancel a blocking `read`); subsequent enable_raw_mode
/// calls will pick it back up.
pub fn disable_raw_mode() {
    let _ = termios_impl::disable();
    READING.store(false, Ordering::Release);
}

/// Root + rewrite the registered `useInput` handler closure.
///
/// Process-global rather than thread-local, matching the slot it scans. A
/// `perry/thread` agent registering its own handler would already be
/// clobbering the main thread's, which is a pre-existing single-handler
/// limitation and not something this scanner changes: it visits whatever
/// address is currently published, and a foreign-arena address is rejected by
/// the visitor's own heap-attribution check rather than rewritten.
pub(crate) fn scan_tui_input_handler_root_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    visitor.visit_atomic_i64_slot(&INPUT_HANDLER, Ordering::Acquire, Ordering::Release);
}

/// Thread-exit release (#11471): clear the published `useInput` handler if it
/// is a closure in the exiting thread's freed arena blocks. Nothing restricts
/// `js_perry_tui_use_input` to the render thread, and `drain_input` would
/// otherwise call through the dangling pointer once the address is reused.
/// A compare-exchange, so a handler published concurrently by a live thread
/// is never cleared.
pub(crate) fn release_tui_input_handler_in_freed_ranges(
    freed: &crate::arena::thread_exit::FreedRanges,
) {
    let current = INPUT_HANDLER.load(Ordering::Acquire);
    if freed.holds_i64(current) {
        let _ = INPUT_HANDLER.compare_exchange(current, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// Test probe (#11471): the currently published `useInput` handler word.
#[doc(hidden)]
pub fn tui_input_handler_for_test() -> i64 {
    INPUT_HANDLER.load(Ordering::Acquire)
}

/// Register the user's `useInput` handler. Replaces any prior handler
/// — v1 supports a single handler.
#[no_mangle]
pub extern "C" fn js_perry_tui_use_input(handler: i64) -> f64 {
    // #11541: registered here, before the slot can hold an address, rather
    // than named in `arena::thread_exit`'s dispatcher (which linked it into
    // every binary).
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        crate::arena::thread_exit::register_thread_exit_range_hook(
            release_tui_input_handler_in_freed_ranges,
        );
    });
    INPUT_HANDLER.store(handler, Ordering::Release);
    INPUT_BYTES_MODE.store(false, Ordering::Release);
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub extern "C" fn js_perry_tui_use_input_bytes(handler: i64) -> f64 {
    let result = js_perry_tui_use_input(handler);
    INPUT_BYTES_MODE.store(true, Ordering::Release);
    result
}

fn dispatch_input_bytes(handler: i64, bytes: &[u8]) {
    if handler == 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handler = scope.root_raw_const_ptr(handler as *const ClosureHeader);
    for chunk in bytes.chunks(i32::MAX as usize) {
        let (array, _) = handler.across_const::<ClosureHeader, _>(|| {
            crate::typedarray::js_typed_array_new_empty(
                i32::from(crate::typedarray::KIND_UINT8),
                chunk.len() as i32,
            )
        });
        let array = scope.root_raw_mut_ptr(array);
        array.with_mut_ptr(|pointer| {
            for (index, byte) in chunk.iter().enumerate() {
                crate::typedarray::js_uint8array_set(pointer, index as i32, i32::from(*byte));
            }
        });
        let arg = array.with_const_ptr(|pointer| f64::from_bits(JSValue::pointer(pointer).bits()));
        handler.across_const::<ClosureHeader, _>(|| {
            handler.with_const_ptr(|pointer| {
                js_closure_call1(pointer, crate::closure::plain_call_receiver(), arg)
            })
        });
    }
}

/// User-facing `exit()` — sets the flag the render loop polls.
#[no_mangle]
pub extern "C" fn js_perry_tui_exit() -> f64 {
    EXIT_FLAG.store(true, Ordering::Release);
    f64::from_bits(JSValue::undefined().bits())
}

/// Drain pending bytes and dispatch to the registered handler. Called
/// from the render loop at every frame. Returns the number of bytes
/// dispatched (mostly for diagnostic — the loop just re-renders if
/// any state changed in the dispatch).
///
/// Special-cases:
/// - `\x09` (TAB) → calls `focus_next()` from the hooks module, then
///   re-dispatches the byte to the user handler too. Matches ink: Tab
///   cycles focus AND the active widget can still observe it.
/// - `ESC-[Z` (Shift-Tab) → calls `focus_previous()`, then dispatches.
pub fn drain_input() -> i32 {
    let bytes: Vec<u8> = {
        let mut q = match PENDING_BYTES.lock() {
            Ok(g) => g,
            Err(_) => return 0,
        };
        std::mem::take(&mut *q)
    };
    if bytes.is_empty() {
        return 0;
    }
    let handler = INPUT_HANDLER.load(Ordering::Acquire);
    if INPUT_BYTES_MODE.load(Ordering::Acquire) {
        dispatch_input_bytes(handler, &bytes);
        return bytes.len().min(i32::MAX as usize) as i32;
    }
    let mut count: i32 = 0;
    // Group consecutive bytes that look like a single ANSI escape
    // sequence (start-with-ESC, length 1..=8) so arrow keys etc.
    // dispatch as one event rather than three. v1 just emits the raw
    // byte chunk as a string; semantic key parsing is the caller's
    // problem (matches Node's process.stdin 'data' shape).
    let mut i = 0;
    while i < bytes.len() {
        let chunk_end = if bytes[i] == 0x1b {
            // ESC — consume up to 8 bytes or until next ESC.
            let mut j = i + 1;
            while j < bytes.len() && j - i < 8 && bytes[j] != 0x1b {
                j += 1;
            }
            j
        } else {
            i + 1
        };
        let chunk = &bytes[i..chunk_end];

        // Focus-cycle keys (#679 Phase 3).
        if chunk == b"\x09" {
            super::hooks::js_perry_tui_focus_next();
        } else if chunk == b"\x1b[Z" {
            super::hooks::js_perry_tui_focus_previous();
        }

        if handler != 0 {
            let s_ptr = js_string_from_bytes(chunk.as_ptr(), chunk.len() as u32);
            let arg = f64::from_bits(JSValue::string_ptr(s_ptr).bits());
            let closure = handler as *const ClosureHeader;
            js_closure_call1(closure, crate::closure::plain_call_receiver(), arg);
        }
        count += 1;
        i = chunk_end;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Cargo runs tests in parallel by default. PENDING_BYTES +
    /// INPUT_HANDLER + EXIT_FLAG are process-wide globals, so two tests
    /// racing see each other's writes. This mutex serialises tests
    /// within this module — same shape as `tui::hooks::tests::TEST_LOCK`
    /// (#862).
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    /// Acquire the test lock and reset all shared state. Returns the
    /// guard — drop at end of test. Binding the guard to `_g` keeps it
    /// alive for the duration of the test; making it the return value
    /// of `reset()` ensures no test can clear globals without first
    /// taking the lock.
    fn reset() -> std::sync::MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        PENDING_BYTES.lock().unwrap().clear();
        INPUT_HANDLER.store(0, Ordering::Release);
        INPUT_BYTES_MODE.store(false, Ordering::Release);
        EXIT_FLAG.store(false, Ordering::Release);
        g
    }

    #[test]
    fn drain_with_no_handler_still_drains_bytes() {
        let _g = reset();
        // Inject a byte via direct push since the reader thread isn't
        // running in tests.
        PENDING_BYTES.lock().unwrap().push(b'a');
        // Post-#679 Phase 3: drain_input always counts processed chunks
        // because the focus-cycle dispatch (Tab/Shift-Tab) runs regardless
        // of whether a user useInput handler is registered. Returning 1
        // here matches the new "always drain" contract.
        assert_eq!(drain_input(), 1);
        // Bytes consumed even when no handler registered.
        assert!(PENDING_BYTES.lock().unwrap().is_empty());
    }

    #[test]
    fn exit_flag_flips_via_ffi() {
        let _g = reset();
        assert!(!EXIT_FLAG.load(Ordering::Acquire));
        js_perry_tui_exit();
        assert!(EXIT_FLAG.load(Ordering::Acquire));
    }

    #[test]
    fn use_input_stores_handler() {
        let _g = reset();
        js_perry_tui_use_input(0xDEADBEEF);
        assert_eq!(INPUT_HANDLER.load(Ordering::Acquire), 0xDEADBEEF);
    }

    extern "C" fn capture_bytes(
        closure: *const ClosureHeader,
        _this: crate::closure::JsThis,
        value: f64,
    ) -> f64 {
        let array =
            JSValue::from_bits(value.to_bits()).as_pointer::<crate::typedarray::TypedArrayHeader>();
        let length = crate::typedarray::js_typed_array_length(array);
        let bytes = (0..length)
            .map(|index| crate::typedarray::js_typed_array_get(array, index) as u8)
            .collect();
        CAPTURED.lock().unwrap().push(bytes);
        crate::closure::js_closure_set_capture_f64(closure as *mut ClosureHeader, 0, value);
        f64::from_bits(JSValue::undefined().bits())
    }

    static CAPTURED: StdMutex<Vec<Vec<u8>>> = StdMutex::new(Vec::new());

    #[test]
    fn input_bytes_preserves_every_stream_split_and_owned_storage() {
        let _guard = crate::gc::global_side_table_test_lock();
        let _input_guard = reset();
        crate::gc::initialize_runtime_fixture();
        let scope = crate::gc::RuntimeHandleScope::new();
        let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(capture_bytes, 1),
            1,
        ));
        js_perry_tui_use_input_bytes(callback.get_raw_mut_ptr::<ClosureHeader>() as i64);
        super::super::state::STATE_DIRTY.store(false, Ordering::Release);
        let payload = "你好😀\x1b[A\x1b[200~a\tb\x1b[201~".as_bytes();
        for split in 0..=payload.len() {
            CAPTURED.lock().unwrap().clear();
            for part in [&payload[..split], &payload[split..]] {
                PENDING_BYTES.lock().unwrap().extend_from_slice(part);
                assert_eq!(drain_input(), part.len() as i32);
            }
            assert_eq!(CAPTURED.lock().unwrap().concat(), payload);
        }
        let retained = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(
            callback.get_raw_const_ptr(),
            0,
        ));
        PENDING_BYTES.lock().unwrap().extend_from_slice(b"new");
        drain_input();
        let retained = JSValue::from_bits(retained.get_nanbox_f64().to_bits())
            .as_pointer::<crate::typedarray::TypedArrayHeader>();
        assert_eq!(crate::typedarray::js_typed_array_get(retained, 0), 228.0);
        assert!(!super::super::state::STATE_DIRTY.load(Ordering::Acquire));
        js_perry_tui_use_input(0);
    }

    #[test]
    fn input_registration_is_mutually_exclusive() {
        let _guard = reset();
        js_perry_tui_use_input_bytes(0x1234);
        assert!(INPUT_BYTES_MODE.load(Ordering::Acquire));
        assert_eq!(tui_input_handler_for_test(), 0x1234);
        js_perry_tui_use_input(0);
        assert!(!INPUT_BYTES_MODE.load(Ordering::Acquire));
    }

    #[test]
    fn input_bytes_handler_survives_moving_collection() {
        let _gc_guard = crate::gc::CopyingNurseryTestGuard::new(0);
        let _input_guard = reset();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _force = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        crate::gc::gc_register_mutable_root_scanner(scan_tui_input_handler_root_mut);
        let callback = crate::closure::js_closure_alloc(crate::fn_info!(capture_bytes, 1), 1);
        js_perry_tui_use_input_bytes(callback as i64);
        let before = crate::gc::copying_minor_cycles();
        crate::gc::gc_collect_minor();
        assert!(crate::gc::copying_minor_cycles() > before);
        assert_ne!(tui_input_handler_for_test(), callback as i64);
        CAPTURED.lock().unwrap().clear();
        PENDING_BYTES
            .lock()
            .unwrap()
            .extend_from_slice(&[0, 0x80, 0xff]);
        assert_eq!(drain_input(), 3);
        assert_eq!(*CAPTURED.lock().unwrap(), vec![vec![0, 0x80, 0xff]]);
        js_perry_tui_use_input(0);
    }

    #[test]
    fn input_bytes_handler_is_cleared_on_owner_thread_exit() {
        let _guard = reset();
        js_perry_tui_use_input_bytes(0x1234);
        release_tui_input_handler_in_freed_ranges(&crate::arena::thread_exit::FreedRanges::new(&[
            (0x1200, 0x1300),
        ]));
        assert_eq!(tui_input_handler_for_test(), 0);
    }
}
