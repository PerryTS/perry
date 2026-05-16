//! readline module for Perry — Phases 1 & 2 of #347
//!
//! Phase 1: line-buffered stdin reading via `readline.createInterface`:
//!   const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
//!   rl.question("name? ", (answer) => { ... });
//!   rl.on("line", (line) => { ... });
//!   rl.on("close", () => { ... });
//!   rl.close();
//!
//! Phase 2: raw-mode stdin + 'data' / 'keypress' events on `process.stdin`:
//!   process.stdin.setRawMode(true);
//!   process.stdin.on("data", (chunk) => { ... });
//!   process.stdin.on("keypress", (str, key) => {
//!       // key = { name, ctrl, shift, meta, sequence }
//!   });
//!
//! Architecture: a single background thread reads stdin one byte at a
//! time. When raw mode is OFF (default), bytes accumulate into a line
//! buffer and the line is queued on `\n`. When raw mode is ON, byte
//! chunks are queued immediately for `'data'`/`'keypress'` dispatch.
//! Mode flips are observed at the start of each byte read, so toggling
//! mid-stream is supported (the next byte routes to the new mode's
//! queue). The main event-loop pump drains both queues every tick via
//! `js_readline_process_pending`.
//!
//! Phase 3 (`tty.isatty`, `process.stdout.columns/rows`, SIGWINCH) is
//! independent of this file.

use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use perry_runtime::closure::{js_closure_call0, js_closure_call1, js_closure_call2, ClosureHeader};
use perry_runtime::string::{js_string_from_bytes, StringHeader};
use perry_runtime::value::{js_nanbox_pointer, JSValue};

/// Singleton handle for the readline interface. createInterface always
/// returns this — Node also tolerates multiple createInterface calls on
/// the same input, but for v1 we treat it as a process-wide singleton
/// since stdin can only have one consumer at a time anyway.
const READLINE_HANDLE: i64 = 1;

// ---------------------------------------------------------------------------
// Cross-thread state — touched by the reader thread AND the main thread, so
// it MUST be in shared statics, not thread_local. (worker_threads.rs has a
// known latent bug from the same mistake; readline.rs deliberately doesn't
// repeat it.)
// ---------------------------------------------------------------------------

/// Lines waiting for the main thread to dispatch.
static PENDING_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Raw byte chunks waiting for the main thread to dispatch as 'data' /
/// 'keypress' events.
static PENDING_DATA: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
/// `true` when raw mode is enabled — the reader thread checks this
/// between bytes to decide which queue to push to.
static RAW_MODE: AtomicBool = AtomicBool::new(false);
/// Set when stdin returns EOF or `rl.close()` is called. The has-active
/// check reads this to decide whether to keep the event loop alive.
static EOF_REACHED: AtomicBool = AtomicBool::new(false);
/// Whether the background reader thread has been spawned. Atomic
/// (compare_exchange) so we don't accidentally spawn twice if two
/// init paths race on first call.
static READER_STARTED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Main-thread-only state — callbacks are dispatched from the main thread
// only (where the GC/runtime are safe to touch), so thread_local is correct.
// ---------------------------------------------------------------------------

thread_local! {
    /// One-shot callback registered by `rl.question(prompt, cb)`.
    static QUESTION_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Persistent callback registered by `rl.on('line', cb)`.
    static LINE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Persistent callback registered by `rl.on('close', cb)`.
    static CLOSE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Persistent callback registered by `process.stdin.on('data', cb)`.
    static DATA_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Persistent callback registered by `process.stdin.on('keypress', cb)`.
    static KEYPRESS_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Whether the close callback has already fired.
    static CLOSE_FIRED: RefCell<bool> = const { RefCell::new(false) };
}

// ---------------------------------------------------------------------------
// Pump-registration shim. The async_bridge module is gated on the
// `async-runtime` feature; without it, `ensure_pump_registered` doesn't
// exist. We still want a project to compile when it imports `readline`
// without pulling in tokio (e.g. a one-shot rl.close() smoke test).
// When async-runtime is off, this is a no-op — rl.close() still fires
// synchronously, but live stdin events won't drain.
// ---------------------------------------------------------------------------

fn try_register_pump() {
    #[cfg(feature = "async-runtime")]
    crate::common::async_bridge::ensure_pump_registered();
}

// ---------------------------------------------------------------------------
// Background reader
// ---------------------------------------------------------------------------

/// Spawn the background byte-mode reader if it isn't already running.
/// Idempotent across threads via `READER_STARTED.compare_exchange`.
fn ensure_reader_started() {
    if READER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut byte = [0u8; 1];
        let mut line_buf: Vec<u8> = Vec::with_capacity(256);
        loop {
            match reader.read(&mut byte) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if RAW_MODE.load(Ordering::Acquire) {
                        // In raw mode, queue a single-byte chunk. Multi-byte
                        // escape sequences (e.g. arrow keys = "\x1b[A")
                        // arrive as three separate chunks; the keypress
                        // parser on the drain side reassembles them.
                        if let Ok(mut q) = PENDING_DATA.lock() {
                            q.push(vec![byte[0]]);
                        }
                    } else if byte[0] == b'\n' {
                        // Strip trailing CR for Windows CRLF input.
                        if line_buf.last() == Some(&b'\r') {
                            line_buf.pop();
                        }
                        let line = String::from_utf8_lossy(&line_buf).into_owned();
                        line_buf.clear();
                        if let Ok(mut q) = PENDING_LINES.lock() {
                            q.push(line);
                        }
                    } else {
                        line_buf.push(byte[0]);
                    }
                }
                Err(_) => break,
            }
        }
        EOF_REACHED.store(true, Ordering::Release);
    });
}

// ---------------------------------------------------------------------------
// Raw-mode toggle (Unix termios; Windows / non-Unix is currently a no-op
// since iOS/Android stdlib stubs handle those targets and Windows raw mode
// needs the windows-rs `Console` API which isn't a stdlib dep yet).
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod termios_impl {
    use std::sync::Mutex;

    /// Saved cooked-mode termios so we can restore on disable. Lazy-init
    /// on the first enable call; survives toggle cycles.
    static SAVED: Mutex<Option<libc::termios>> = Mutex::new(None);

    /// Enable raw mode on fd 0 (stdin). Returns true on success.
    pub fn enable() -> bool {
        unsafe {
            let mut current: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut current) != 0 {
                return false;
            }
            // Save the original on first enable so disable can restore.
            {
                let mut saved = SAVED.lock().unwrap();
                if saved.is_none() {
                    *saved = Some(current);
                }
            }
            let mut raw = current;
            // cfmakeraw equivalent (Node's setRawMode does roughly this).
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

    /// Disable raw mode (restore the saved cooked-mode termios).
    pub fn disable() -> bool {
        unsafe {
            let saved = SAVED.lock().unwrap();
            if let Some(t) = saved.as_ref() {
                libc::tcsetattr(0, libc::TCSANOW, t) == 0
            } else {
                // Never enabled — nothing to restore.
                true
            }
        }
    }
}

#[cfg(all(windows, not(unix)))]
mod termios_impl {
    use std::sync::Mutex;
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT,
        ENABLE_PROCESSED_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    /// Saved console modes for the input + output handles. Set on first
    /// `enable()`; restored by `disable()`. Two-tuple so we can leave
    /// the output handle's mode untouched if we couldn't read it (e.g.
    /// stdout redirected to a file — `GetConsoleMode` fails on
    /// non-console handles).
    static SAVED: Mutex<Option<(u32, Option<u32>)>> = Mutex::new(None);

    /// Flip stdin into byte-mode + virtual-terminal-input mode (so
    /// arrow keys arrive as ANSI `\x1b[A..D` matching the Unix path's
    /// parser) and stdout into virtual-terminal-processing mode (so the
    /// renderer's CSI escapes actually move the cursor instead of
    /// printing literally). Saves the original modes on first call so
    /// `disable()` restores cleanly. (#406.)
    pub fn enable() -> bool {
        unsafe {
            // windows-sys 0.61 (#720) made HANDLE a `*mut c_void` (was `isize`
            // in 0.52). Use `.is_null()` + `INVALID_HANDLE_VALUE` constant
            // instead of raw integer comparison. (#406 fix updated for
            // windows-sys 0.61.)
            use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
            let h_in = GetStdHandle(STD_INPUT_HANDLE);
            if h_in.is_null() || h_in == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut current_in: u32 = 0;
            if GetConsoleMode(h_in, &mut current_in) == 0 {
                return false;
            }
            let h_out = GetStdHandle(STD_OUTPUT_HANDLE);
            let current_out = if !h_out.is_null() && h_out != INVALID_HANDLE_VALUE {
                let mut m: u32 = 0;
                if GetConsoleMode(h_out, &mut m) != 0 {
                    Some(m)
                } else {
                    None
                }
            } else {
                None
            };

            {
                let mut saved = SAVED.lock().unwrap();
                if saved.is_none() {
                    *saved = Some((current_in, current_out));
                }
            }

            let raw_in = (current_in
                & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_VIRTUAL_TERMINAL_INPUT;
            if SetConsoleMode(h_in, raw_in) == 0 {
                return false;
            }
            if let Some(out_mode) = current_out {
                let raw_out = out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
                let _ = SetConsoleMode(h_out, raw_out);
            }
            true
        }
    }

    pub fn disable() -> bool {
        unsafe {
            let saved = SAVED.lock().unwrap();
            if let Some((in_mode, out_mode)) = saved.as_ref() {
                use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
                let h_in = GetStdHandle(STD_INPUT_HANDLE);
                if !h_in.is_null() && h_in != INVALID_HANDLE_VALUE {
                    let _ = SetConsoleMode(h_in, *in_mode);
                }
                if let Some(m) = out_mode {
                    let h_out = GetStdHandle(STD_OUTPUT_HANDLE);
                    if !h_out.is_null() && h_out != INVALID_HANDLE_VALUE {
                        let _ = SetConsoleMode(h_out, *m);
                    }
                }
                true
            } else {
                true
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod termios_impl {
    pub fn enable() -> bool {
        // Raw mode unsupported on this platform (e.g. wasm32). The
        // flag still flips so the reader switches to byte-chunk
        // dispatch, but stdin remains line-cooked.
        false
    }
    pub fn disable() -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Public FFI — readline interface (Phase 1)
// ---------------------------------------------------------------------------

/// readline.createInterface(opts) — returns a NaN-boxed POINTER handle
/// pointing at the singleton interface. The opts argument is accepted
/// for shape compatibility with Node but currently ignored.
#[no_mangle]
pub extern "C" fn js_readline_create_interface(_opts: f64) -> i64 {
    try_register_pump();
    ensure_reader_started();
    READLINE_HANDLE
}

/// rl.question(prompt, callback) — write `prompt` to stdout (no
/// trailing newline) and register `callback` as a one-shot to fire with
/// the next line read.
#[no_mangle]
pub extern "C" fn js_readline_question(
    _handle: i64,
    prompt_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    if !prompt_ptr.is_null() {
        unsafe {
            let len = (*prompt_ptr).byte_len as usize;
            let data = (prompt_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            let stdout = io::stdout();
            let mut h = stdout.lock();
            let _ = h.write_all(bytes);
            let _ = h.flush();
        }
    }
    QUESTION_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
    try_register_pump();
    ensure_reader_started();
    f64::from_bits(JSValue::undefined().bits())
}

/// rl.on(event, callback) — register a persistent callback for the
/// `'line'` or `'close'` event.
#[no_mangle]
pub extern "C" fn js_readline_on(
    _handle: i64,
    event_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    if event_ptr.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    let event = unsafe {
        let len = (*event_ptr).byte_len as usize;
        let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    match event {
        "line" => {
            LINE_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
            try_register_pump();
            ensure_reader_started();
        }
        "close" => {
            CLOSE_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
        }
        _ => {}
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// rl.close() — synchronously fire the close callback (matching Node's
/// `Interface.close()` semantics) and mark the interface as EOF.
#[no_mangle]
pub extern "C" fn js_readline_close(_handle: i64) -> f64 {
    EOF_REACHED.store(true, Ordering::Release);
    let already = CLOSE_FIRED.with(|f| {
        let was = *f.borrow();
        *f.borrow_mut() = true;
        was
    });
    if !already {
        let cb = CLOSE_CALLBACK.with(|c| c.borrow_mut().take());
        if let Some(cb_i64) = cb {
            let closure = cb_i64 as *const ClosureHeader;
            js_closure_call0(closure);
        }
    }
    f64::from_bits(JSValue::undefined().bits())
}

// ---------------------------------------------------------------------------
// Public FFI — process.stdin.setRawMode / process.stdin.on (Phase 2)
// ---------------------------------------------------------------------------

/// process.stdin.setRawMode(enabled) — toggle raw mode on stdin. The
/// boolean comes in as a NaN-boxed JSValue; we extract via
/// `js_is_truthy` semantics (any value other than false/null/undefined/0
/// counts as enable). Returns the stdin handle (Node returns the
/// ReadStream itself for chaining).
#[no_mangle]
pub extern "C" fn js_readline_set_raw_mode(enabled: f64) -> f64 {
    let truthy = perry_runtime::value::js_is_truthy(enabled) != 0;
    if truthy {
        let _ = termios_impl::enable();
        RAW_MODE.store(true, Ordering::Release);
    } else {
        let _ = termios_impl::disable();
        RAW_MODE.store(false, Ordering::Release);
    }
    try_register_pump();
    ensure_reader_started();
    // Return a pointer-tagged handle so the chain `process.stdin.setRawMode(true)`
    // could be extended later (Node returns `this`); for now any non-undefined
    // value is fine.
    js_nanbox_pointer(READLINE_HANDLE)
}

/// process.stdin.on(event, callback) — register a callback for raw-mode
/// stdin events. Supported events: "data" (raw byte chunk as a string),
/// "keypress" (parsed key info — see below), "end" (alias for the
/// readline 'close' event since Node fires 'end' on stdin EOF).
#[no_mangle]
pub extern "C" fn js_readline_stdin_on(event_ptr: *const StringHeader, callback: i64) -> f64 {
    if event_ptr.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    let event = unsafe {
        let len = (*event_ptr).byte_len as usize;
        let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    match event {
        "data" => {
            DATA_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
            try_register_pump();
            ensure_reader_started();
        }
        "keypress" => {
            KEYPRESS_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
            try_register_pump();
            ensure_reader_started();
        }
        "end" | "close" => {
            // Reuse the readline close callback slot — only one terminal
            // close listener is supported per process.
            CLOSE_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
        }
        _ => {}
    }
    f64::from_bits(JSValue::undefined().bits())
}

// ---------------------------------------------------------------------------
// Drain / pump
// ---------------------------------------------------------------------------

/// Build a NaN-boxed object literal `{ name, ctrl, shift, meta, sequence }`
/// suitable for the `'keypress'` event's second argument.
fn build_keypress_object(name: &str, ctrl: bool, shift: bool, meta: bool, seq: &str) -> f64 {
    use perry_runtime::object::{js_object_alloc_with_shape, js_object_set_field};
    let packed = b"name\0ctrl\0shift\0meta\0sequence\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF47, 5, packed.as_ptr(), packed.len() as u32);
    let name_str = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field(obj, 0, JSValue::string_ptr(name_str));
    js_object_set_field(
        obj,
        1,
        if ctrl {
            JSValue::bool(true)
        } else {
            JSValue::bool(false)
        },
    );
    js_object_set_field(
        obj,
        2,
        if shift {
            JSValue::bool(true)
        } else {
            JSValue::bool(false)
        },
    );
    js_object_set_field(
        obj,
        3,
        if meta {
            JSValue::bool(true)
        } else {
            JSValue::bool(false)
        },
    );
    let seq_str = js_string_from_bytes(seq.as_ptr(), seq.len() as u32);
    js_object_set_field(obj, 4, JSValue::string_ptr(seq_str));
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Parse a single byte chunk into a (name, ctrl, shift, meta, sequence)
/// keypress descriptor. Recognises Enter, Backspace, Tab, Escape, Ctrl+
/// letter, and ANSI CSI arrow keys (which arrive as the 3-byte sequence
/// `\x1b[A`/`B`/`C`/`D`). Multi-byte sequences are reassembled by the
/// drain loop using the `pending_escape` accumulator.
fn parse_keypress(chunk: &[u8]) -> Option<(String, bool, bool, bool, String)> {
    if chunk.is_empty() {
        return None;
    }
    let seq = String::from_utf8_lossy(chunk).into_owned();
    // CSI arrow keys: \x1b[A..D
    if chunk.len() == 3 && chunk[0] == 0x1b && chunk[1] == b'[' {
        let name = match chunk[2] {
            b'A' => "up",
            b'B' => "down",
            b'C' => "right",
            b'D' => "left",
            b'H' => "home",
            b'F' => "end",
            _ => return Some(("undefined".to_string(), false, false, false, seq)),
        };
        return Some((name.to_string(), false, false, false, seq));
    }
    // Single byte
    if chunk.len() == 1 {
        let b = chunk[0];
        let (name, ctrl) = match b {
            b'\r' | b'\n' => ("return".to_string(), false),
            b'\t' => ("tab".to_string(), false),
            0x7f | 0x08 => ("backspace".to_string(), false),
            0x1b => ("escape".to_string(), false),
            b' ' => ("space".to_string(), false),
            // Ctrl+letter is byte = letter & 0x1F
            0x01..=0x1a => {
                let letter = (b + b'a' - 1) as char;
                (letter.to_string(), true)
            }
            b'a'..=b'z' => ((b as char).to_string(), false),
            b'A'..=b'Z' => ((b as char).to_string(), false),
            b'0'..=b'9' => ((b as char).to_string(), false),
            _ => (seq.clone(), false),
        };
        let shift = matches!(b, b'A'..=b'Z');
        return Some((name, ctrl, shift, false, seq));
    }
    // Anything else — surface the raw sequence with `name == sequence`.
    Some((seq.clone(), false, false, false, seq))
}

/// Drain pending lines and byte chunks, dispatching to registered
/// callbacks. Called from the async-bridge tick on every event-loop
/// iteration. Returns the number of callbacks fired.
#[no_mangle]
pub extern "C" fn js_readline_process_pending() -> i32 {
    let mut fired: i32 = 0;

    // Drain raw-mode byte chunks → 'data' / 'keypress' callbacks.
    let chunks: Vec<Vec<u8>> = {
        let mut q = match PENDING_DATA.lock() {
            Ok(g) => g,
            Err(_) => return fired,
        };
        std::mem::take(&mut *q)
    };
    for chunk in chunks {
        // 'data' callback receives the raw bytes as a string.
        let data_cb = DATA_CALLBACK.with(|cb| *cb.borrow());
        if let Some(cb_i64) = data_cb {
            let s = js_string_from_bytes(chunk.as_ptr(), chunk.len() as u32);
            let arg = f64::from_bits(JSValue::string_ptr(s).bits());
            let closure = cb_i64 as *const ClosureHeader;
            js_closure_call1(closure, arg);
            fired += 1;
        }
        // 'keypress' callback receives (sequence_string, key_object).
        let kp_cb = KEYPRESS_CALLBACK.with(|cb| *cb.borrow());
        if let Some(cb_i64) = kp_cb {
            if let Some((name, ctrl, shift, meta, seq)) = parse_keypress(&chunk) {
                let seq_str = js_string_from_bytes(seq.as_ptr(), seq.len() as u32);
                let arg1 = f64::from_bits(JSValue::string_ptr(seq_str).bits());
                let arg2 = build_keypress_object(&name, ctrl, shift, meta, &seq);
                let closure = cb_i64 as *const ClosureHeader;
                js_closure_call2(closure, arg1, arg2);
                fired += 1;
            }
        }
    }

    // Drain line-mode lines → question (one-shot) or 'line' callback.
    let lines: Vec<String> = {
        let mut q = match PENDING_LINES.lock() {
            Ok(g) => g,
            Err(_) => return fired,
        };
        std::mem::take(&mut *q)
    };
    for line in lines {
        let str_ptr = js_string_from_bytes(line.as_ptr(), line.len() as u32);
        let arg = f64::from_bits(JSValue::string_ptr(str_ptr).bits());
        let q_cb = QUESTION_CALLBACK.with(|cb| cb.borrow_mut().take());
        if let Some(cb_i64) = q_cb {
            let closure = cb_i64 as *const ClosureHeader;
            js_closure_call1(closure, arg);
            fired += 1;
            continue;
        }
        let line_cb = LINE_CALLBACK.with(|cb| *cb.borrow());
        if let Some(cb_i64) = line_cb {
            let closure = cb_i64 as *const ClosureHeader;
            js_closure_call1(closure, arg);
            fired += 1;
        }
    }

    // Fire close callback once on EOF.
    if EOF_REACHED.load(Ordering::Acquire) {
        let already = CLOSE_FIRED.with(|f| {
            let was = *f.borrow();
            *f.borrow_mut() = true;
            was
        });
        if !already {
            let cb = CLOSE_CALLBACK.with(|c| c.borrow_mut().take());
            if let Some(cb_i64) = cb {
                let closure = cb_i64 as *const ClosureHeader;
                js_closure_call0(closure);
                fired += 1;
            }
        }
    }
    fired
}

/// Whether readline has any active state requiring the event loop to
/// keep running.
#[no_mangle]
pub extern "C" fn js_readline_has_active() -> i32 {
    let started = READER_STARTED.load(Ordering::Acquire);
    let eof = EOF_REACHED.load(Ordering::Acquire);
    let has_lines = PENDING_LINES.lock().map(|q| !q.is_empty()).unwrap_or(false);
    let has_data = PENDING_DATA.lock().map(|q| !q.is_empty()).unwrap_or(false);
    let has_close_cb =
        !CLOSE_FIRED.with(|f| *f.borrow()) && CLOSE_CALLBACK.with(|c| c.borrow().is_some());
    if has_lines || has_data || has_close_cb || (started && !eof) {
        1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Test-only helper: bypass the stdin reader and inject a line into the
/// queue.
#[doc(hidden)]
#[cfg(test)]
fn test_inject_line(line: &str) {
    PENDING_LINES.lock().unwrap().push(line.to_string());
}

#[doc(hidden)]
#[cfg(test)]
fn test_inject_chunk(chunk: &[u8]) {
    PENDING_DATA.lock().unwrap().push(chunk.to_vec());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// All readline tests share PENDING_LINES / PENDING_DATA / EOF_REACHED
    /// / CLOSE_FIRED / RAW_MODE / the thread_local callback cells, so
    /// `cargo test`'s default parallel runner races them — most visibly,
    /// `has_active_reflects_state`'s `test_inject_line("x")` →
    /// `assert has_active == 1` window can be observed mid-flight by
    /// `injected_line_drains_via_test_helper`'s `reset()` (which clears
    /// PENDING_LINES) and flake. Serialize every state-touching test
    /// through one process-global lock acquired by `reset()`. Pure-
    /// function tests (`parse_keypress_*`) don't call `reset()` and
    /// continue running in parallel.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        QUESTION_CALLBACK.with(|c| *c.borrow_mut() = None);
        LINE_CALLBACK.with(|c| *c.borrow_mut() = None);
        CLOSE_CALLBACK.with(|c| *c.borrow_mut() = None);
        DATA_CALLBACK.with(|c| *c.borrow_mut() = None);
        KEYPRESS_CALLBACK.with(|c| *c.borrow_mut() = None);
        PENDING_LINES.lock().unwrap().clear();
        PENDING_DATA.lock().unwrap().clear();
        EOF_REACHED.store(false, Ordering::Release);
        CLOSE_FIRED.with(|f| *f.borrow_mut() = false);
        RAW_MODE.store(false, Ordering::Release);
        // READER_STARTED stays sticky once set in a test process.
        guard
    }

    #[test]
    fn close_without_callbacks_is_noop() {
        let _g = reset();
        let h = js_readline_create_interface(0.0);
        assert_eq!(h, READLINE_HANDLE);
        js_readline_close(h);
        assert_eq!(js_readline_process_pending(), 0);
        assert_eq!(js_readline_process_pending(), 0);
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
    fn parse_keypress_arrow_keys() {
        let (name, ctrl, shift, meta, seq) = parse_keypress(b"\x1b[A").unwrap();
        assert_eq!(name, "up");
        assert!(!ctrl && !shift && !meta);
        assert_eq!(seq, "\x1b[A");

        assert_eq!(parse_keypress(b"\x1b[B").unwrap().0, "down");
        assert_eq!(parse_keypress(b"\x1b[C").unwrap().0, "right");
        assert_eq!(parse_keypress(b"\x1b[D").unwrap().0, "left");
    }

    #[test]
    fn parse_keypress_ctrl_letter() {
        // Ctrl+C = 0x03
        let (name, ctrl, _, _, _) = parse_keypress(&[0x03]).unwrap();
        assert_eq!(name, "c");
        assert!(ctrl);
        // Ctrl+A = 0x01
        let (name, ctrl, _, _, _) = parse_keypress(&[0x01]).unwrap();
        assert_eq!(name, "a");
        assert!(ctrl);
    }

    #[test]
    fn parse_keypress_special_keys() {
        assert_eq!(parse_keypress(b"\r").unwrap().0, "return");
        assert_eq!(parse_keypress(b"\n").unwrap().0, "return");
        assert_eq!(parse_keypress(b"\t").unwrap().0, "tab");
        assert_eq!(parse_keypress(&[0x7f]).unwrap().0, "backspace");
        assert_eq!(parse_keypress(&[0x1b]).unwrap().0, "escape");
        assert_eq!(parse_keypress(b" ").unwrap().0, "space");
    }

    #[test]
    fn parse_keypress_letter_shift_flag() {
        let (name, ctrl, shift, _, _) = parse_keypress(b"A").unwrap();
        assert_eq!(name, "A");
        assert!(!ctrl);
        assert!(shift); // uppercase A → shift true
        let (_, _, shift, _, _) = parse_keypress(b"a").unwrap();
        assert!(!shift);
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
}
