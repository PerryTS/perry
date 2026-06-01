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

use perry_runtime::closure::{
    get_valid_func_ptr, js_closure_alloc, js_closure_call0, js_closure_call1, js_closure_call2,
    js_closure_get_capture_f64, js_closure_set_capture_f64, js_native_call_value, ClosureHeader,
};
use perry_runtime::object::{
    js_object_alloc_with_shape, js_object_get_field_by_name_f64, js_object_set_field, ObjectHeader,
};
use perry_runtime::string::{js_string_from_bytes, StringHeader};
use perry_runtime::value::{js_jsvalue_to_string, js_nanbox_pointer, JSValue};

/// Singleton handle for the legacy stdin-backed readline interface.
const STDIN_READLINE_HANDLE: i64 = 1;

#[derive(Clone)]
struct ReadlineInterfaceState {
    input: f64,
    output: f64,
    prompt: String,
    line: String,
    pending: String,
    line_callback: Option<i64>,
    close_callback: Option<i64>,
    question_callback: Option<i64>,
    terminal: bool,
    closed: bool,
    cursor_cols: i32,
    cursor_rows: i32,
    uses_custom_stream: bool,
}

impl ReadlineInterfaceState {
    fn new(
        input: f64,
        output: f64,
        prompt: String,
        terminal: bool,
        uses_custom_stream: bool,
    ) -> Self {
        Self {
            input,
            output,
            prompt,
            line: String::new(),
            pending: String::new(),
            line_callback: None,
            close_callback: None,
            question_callback: None,
            terminal,
            closed: false,
            cursor_cols: 0,
            cursor_rows: 0,
            uses_custom_stream,
        }
    }
}

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
/// `process.stdin.pause()` gates raw stdin event dispatch until resume.
static STDIN_PAUSED: AtomicBool = AtomicBool::new(false);
/// Ref/unref mirrors Node's event-loop liveness contract for stdin.
static STDIN_REFED: AtomicBool = AtomicBool::new(true);
/// Destroyed stdin clears listeners/queues and no longer keeps the loop alive.
static STDIN_DESTROYED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Main-thread-only state — callbacks are dispatched from the main thread
// only (where the GC/runtime are safe to touch), so thread_local is correct.
// ---------------------------------------------------------------------------

thread_local! {
    static READLINE_INTERFACES: RefCell<Vec<Option<ReadlineInterfaceState>>> =
        const { RefCell::new(Vec::new()) };
    static NEXT_READLINE_HANDLE: RefCell<i64> = const { RefCell::new(2) };
    /// One-shot callback registered by `rl.question(prompt, cb)`.
    static QUESTION_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Persistent callback registered by `rl.on('line', cb)`.
    static LINE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Persistent callback registered by `rl.on('close', cb)`.
    static CLOSE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Persistent callbacks registered by `process.stdin.on('data', cb)`.
    static DATA_CALLBACKS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    /// Persistent callbacks registered by `process.stdin.on('keypress', cb)`.
    static KEYPRESS_CALLBACKS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
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

fn undefined() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn boxed_str(bytes: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn value_to_string(value: f64) -> String {
    let ptr = js_jsvalue_to_string(value) as *const StringHeader;
    string_header_to_string(ptr)
}

fn object_ptr_from_value(value: f64) -> Option<*const ObjectHeader> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let ptr = js.as_pointer::<ObjectHeader>();
    if (ptr as usize) < 0x10000 {
        None
    } else {
        Some(ptr)
    }
}

fn raw_ptr_from_value(value: f64) -> Option<i64> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let raw = js.as_pointer::<u8>() as i64;
    if raw >= 0x10000 {
        Some(raw)
    } else {
        None
    }
}

fn key_ptr(key: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

fn object_field(value: f64, key: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let field = js_object_get_field_by_name_f64(obj, key_ptr(key));
    if JSValue::from_bits(field.to_bits()).is_undefined() {
        None
    } else {
        Some(field)
    }
}

fn is_callable(value: f64) -> bool {
    raw_ptr_from_value(value)
        .map(|raw| !get_valid_func_ptr(raw as *const ClosureHeader).is_null())
        .unwrap_or(false)
}

fn throw_type_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()))
}

fn bool_to_f64(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn is_true_value(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_bool() && js.as_bool()
}

fn stream_is_readable(value: f64) -> bool {
    is_true_value(perry_runtime::node_stream::js_node_stream_is_readable(
        value,
    ))
}

fn stream_is_writable(value: f64) -> bool {
    is_true_value(perry_runtime::node_stream::js_node_stream_is_writable(
        value,
    ))
}

fn call_write_value(output: f64, text: &str) {
    let chunk = boxed_str(text.as_bytes());
    if stream_is_writable(output) {
        if let Some(raw) = raw_ptr_from_value(output) {
            let _ = perry_runtime::node_stream::js_node_stream_method_write(
                raw,
                chunk,
                undefined(),
                undefined(),
            );
            return;
        }
    }
    if let Some(write) = object_field(output, b"write").filter(|v| is_callable(*v)) {
        let args = [chunk];
        unsafe {
            let _ = js_native_call_value(write, args.as_ptr(), args.len());
        }
        return;
    }
    let stdout = io::stdout();
    let mut h = stdout.lock();
    let _ = h.write_all(text.as_bytes());
    let _ = h.flush();
}

fn allocate_interface(state: ReadlineInterfaceState) -> i64 {
    READLINE_INTERFACES.with(|interfaces| {
        let mut interfaces = interfaces.borrow_mut();
        let handle = if state.uses_custom_stream {
            NEXT_READLINE_HANDLE.with(|next| {
                let handle = *next.borrow();
                *next.borrow_mut() = handle + 1;
                handle
            })
        } else {
            STDIN_READLINE_HANDLE
        };
        let index = handle as usize;
        if interfaces.len() <= index {
            interfaces.resize_with(index + 1, || None);
        }
        interfaces[index] = Some(state);
        handle
    })
}

fn with_interface_mut<R>(
    handle: i64,
    f: impl FnOnce(&mut ReadlineInterfaceState) -> R,
) -> Option<R> {
    READLINE_INTERFACES.with(|interfaces| {
        let mut interfaces = interfaces.borrow_mut();
        interfaces
            .get_mut(handle as usize)
            .and_then(|slot| slot.as_mut())
            .map(f)
    })
}

fn with_interface<R>(handle: i64, f: impl FnOnce(&ReadlineInterfaceState) -> R) -> Option<R> {
    READLINE_INTERFACES.with(|interfaces| {
        let interfaces = interfaces.borrow();
        interfaces
            .get(handle as usize)
            .and_then(|slot| slot.as_ref())
            .map(f)
    })
}

fn callback_arg(line: &str) -> f64 {
    boxed_str(line.as_bytes())
}

fn fire_line_or_question(state: &mut ReadlineInterfaceState, line: String) {
    state.line.clear();
    let arg = callback_arg(&line);
    if let Some(cb_i64) = state.question_callback.take() {
        js_closure_call1(cb_i64 as *const ClosureHeader, arg);
        return;
    }
    if let Some(cb_i64) = state.line_callback {
        js_closure_call1(cb_i64 as *const ClosureHeader, arg);
    }
}

fn close_custom_interface(handle: i64) {
    let cb = with_interface_mut(handle, |state| {
        if state.closed {
            None
        } else {
            state.closed = true;
            state.close_callback.take()
        }
    })
    .flatten();
    if let Some(cb_i64) = cb {
        js_closure_call0(cb_i64 as *const ClosureHeader);
    }
}

fn append_custom_input(handle: i64, chunk: f64) {
    let text = value_to_string(chunk);
    with_interface_mut(handle, |state| {
        state.pending.push_str(&text);
        while let Some(pos) = state.pending.find('\n') {
            let mut line: String = state.pending.drain(..=pos).collect();
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }
            fire_line_or_question(state, line);
        }
    });
}

extern "C" fn custom_input_data(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let handle = js_closure_get_capture_f64(closure, 0) as i64;
    append_custom_input(handle, chunk);
    undefined()
}

extern "C" fn custom_input_close(closure: *const ClosureHeader) -> f64 {
    let handle = js_closure_get_capture_f64(closure, 0) as i64;
    close_custom_interface(handle);
    undefined()
}

fn attach_custom_input(handle: i64, input: f64) {
    let Some(raw) = raw_ptr_from_value(input) else {
        return;
    };
    let data = js_closure_alloc(custom_input_data as *const u8, 1);
    js_closure_set_capture_f64(data, 0, handle as f64);
    let data_value = f64::from_bits(JSValue::pointer(data as *const u8).bits());
    let close = js_closure_alloc(custom_input_close as *const u8, 1);
    js_closure_set_capture_f64(close, 0, handle as f64);
    let close_value = f64::from_bits(JSValue::pointer(close as *const u8).bits());
    let data_event = boxed_str(b"data");
    let end_event = boxed_str(b"end");
    let close_event = boxed_str(b"close");
    let _ = perry_runtime::node_stream::js_node_stream_method_on(raw, data_event, data_value);
    let _ = perry_runtime::node_stream::js_node_stream_method_on(raw, end_event, close_value);
    let _ = perry_runtime::node_stream::js_node_stream_method_on(raw, close_event, close_value);
}

fn prompt_from_options(opts: f64) -> String {
    object_field(opts, b"prompt")
        .map(value_to_string)
        .unwrap_or_else(|| "> ".to_string())
}

fn terminal_from_options(opts: f64) -> bool {
    object_field(opts, b"terminal")
        .map(|v| perry_runtime::value::js_is_truthy(v) != 0)
        .unwrap_or(false)
}

fn create_interface_from_options(opts: f64) -> i64 {
    let Some(_) = object_ptr_from_value(opts) else {
        return allocate_interface(ReadlineInterfaceState::new(
            undefined(),
            undefined(),
            "> ".to_string(),
            false,
            false,
        ));
    };
    let Some(input) = object_field(opts, b"input") else {
        throw_type_error("input.on is not a function");
    };
    if !stream_is_readable(input) && !object_field(input, b"on").is_some_and(is_callable) {
        throw_type_error("input.on is not a function");
    }
    let output = object_field(opts, b"output").unwrap_or_else(undefined);
    let prompt = prompt_from_options(opts);
    let terminal = terminal_from_options(opts);
    let uses_custom_stream = stream_is_readable(input);
    let handle = allocate_interface(ReadlineInterfaceState::new(
        input,
        output,
        prompt,
        terminal,
        uses_custom_stream,
    ));
    if uses_custom_stream {
        attach_custom_input(handle, input);
    }
    handle
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
                    if STDIN_DESTROYED.load(Ordering::Acquire) {
                        break;
                    }
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
/// pointing at an interface handle. Explicit Node stream inputs are
/// wired through their data/end events; the legacy no-options path keeps
/// the stdin-backed singleton behavior.
#[no_mangle]
pub extern "C" fn js_readline_create_interface(opts: f64) -> i64 {
    CLOSE_FIRED.with(|f| *f.borrow_mut() = false);
    CLOSE_CALLBACK.with(|cb| *cb.borrow_mut() = None);
    try_register_pump();
    let handle = create_interface_from_options(opts);
    if !with_interface(handle, |state| state.uses_custom_stream).unwrap_or(false) {
        ensure_reader_started();
    }
    handle
}

/// rl.question(prompt, callback) — write `prompt` to stdout (no
/// trailing newline) and register `callback` as a one-shot to fire with
/// the next line read.
#[no_mangle]
pub extern "C" fn js_readline_question(
    handle: i64,
    prompt_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let prompt = string_header_to_string(prompt_ptr);
    if with_interface_mut(handle, |state| {
        if state.uses_custom_stream {
            call_write_value(state.output, &prompt);
            state.question_callback = Some(callback);
            true
        } else {
            false
        }
    })
    .unwrap_or(false)
    {
        return undefined();
    }
    if !prompt.is_empty() {
        let stdout = io::stdout();
        let mut h = stdout.lock();
        let _ = h.write_all(prompt.as_bytes());
        let _ = h.flush();
    }
    QUESTION_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
    try_register_pump();
    ensure_reader_started();
    undefined()
}

/// rl.on(event, callback) — register a persistent callback for the
/// `'line'` or `'close'` event.
#[no_mangle]
pub extern "C" fn js_readline_on(
    handle: i64,
    event_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    if event_ptr.is_null() {
        return undefined();
    }
    let event = string_header_to_string(event_ptr);
    if with_interface_mut(handle, |state| {
        if !state.uses_custom_stream {
            return false;
        }
        match event.as_str() {
            "line" => state.line_callback = Some(callback),
            "close" => state.close_callback = Some(callback),
            _ => {}
        }
        true
    })
    .unwrap_or(false)
    {
        return undefined();
    }
    match event.as_str() {
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
    undefined()
}

/// rl.close() — synchronously fire the close callback (matching Node's
/// `Interface.close()` semantics) and mark the interface as EOF.
#[no_mangle]
pub extern "C" fn js_readline_close(_handle: i64) -> f64 {
    if with_interface(_handle, |state| state.uses_custom_stream).unwrap_or(false) {
        close_custom_interface(_handle);
        return undefined();
    }
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
    undefined()
}

#[no_mangle]
pub extern "C" fn js_readline_pause(handle: i64) -> i64 {
    if let Some(input) = with_interface(handle, |state| state.input) {
        if let Some(raw) = raw_ptr_from_value(input) {
            let _ = perry_runtime::node_stream::js_node_stream_method_pause(raw);
        }
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_readline_resume(handle: i64) -> i64 {
    if let Some(input) = with_interface(handle, |state| state.input) {
        if let Some(raw) = raw_ptr_from_value(input) {
            let _ = perry_runtime::node_stream::js_node_stream_method_resume(raw);
        }
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_readline_prompt(handle: i64) -> f64 {
    with_interface_mut(handle, |state| {
        call_write_value(state.output, &state.prompt);
        state.cursor_cols = state.prompt.chars().count() as i32;
    });
    undefined()
}

#[no_mangle]
pub extern "C" fn js_readline_set_prompt(handle: i64, prompt_ptr: *const StringHeader) -> f64 {
    let prompt = string_header_to_string(prompt_ptr);
    with_interface_mut(handle, |state| {
        state.prompt = prompt;
    });
    undefined()
}

#[no_mangle]
pub extern "C" fn js_readline_get_prompt(handle: i64) -> *mut StringHeader {
    let prompt = with_interface(handle, |state| state.prompt.clone()).unwrap_or_default();
    js_string_from_bytes(prompt.as_ptr(), prompt.len() as u32)
}

#[no_mangle]
pub extern "C" fn js_readline_write(handle: i64, chunk: f64) -> f64 {
    let text = value_to_string(chunk);
    with_interface_mut(handle, |state| {
        state.cursor_cols = state.cursor_cols.max(text.chars().count() as i32);
    });
    undefined()
}

#[no_mangle]
pub extern "C" fn js_readline_get_cursor_pos(handle: i64) -> i64 {
    let (cols, rows) =
        with_interface(handle, |state| (state.cursor_cols, state.cursor_rows)).unwrap_or((0, 0));
    let packed = b"cols\0rows\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF49, 2, packed.as_ptr(), packed.len() as u32);
    js_object_set_field(obj, 0, JSValue::number(cols as f64));
    js_object_set_field(obj, 1, JSValue::number(rows as f64));
    obj as i64
}

#[no_mangle]
pub extern "C" fn js_readline_line(handle: i64) -> *mut StringHeader {
    let line = with_interface(handle, |state| state.line.clone()).unwrap_or_default();
    js_string_from_bytes(line.as_ptr(), line.len() as u32)
}

#[no_mangle]
pub extern "C" fn js_readline_terminal(handle: i64) -> f64 {
    bool_to_f64(with_interface(handle, |state| state.terminal).unwrap_or(false))
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
    if STDIN_DESTROYED.load(Ordering::Acquire) {
        throw_type_error("process.stdin.setRawMode cannot be used after process.stdin.destroy()");
    }
    let truthy = perry_runtime::value::js_is_truthy(enabled) != 0;
    if truthy {
        let _ = termios_impl::enable();
        RAW_MODE.store(true, Ordering::Release);
    } else {
        let _ = termios_impl::disable();
        RAW_MODE.store(false, Ordering::Release);
    }
    perry_runtime::os::set_process_stdin_raw_state(truthy);
    try_register_pump();
    ensure_reader_started();
    // Return a pointer-tagged handle so the chain `process.stdin.setRawMode(true)`
    // could be extended later (Node returns `this`); for now any non-undefined
    // value is fine.
    js_nanbox_pointer(STDIN_READLINE_HANDLE)
}

/// process.stdin.on(event, callback) — register a callback for raw-mode
/// stdin events. Supported events: "data" (raw byte chunk as a string),
/// "keypress" (parsed key info — see below), "end" (alias for the
/// readline 'close' event since Node fires 'end' on stdin EOF).
#[no_mangle]
pub extern "C" fn js_readline_stdin_on(event_ptr: *const StringHeader, callback: i64) -> f64 {
    if event_ptr.is_null() {
        return undefined();
    }
    if STDIN_DESTROYED.load(Ordering::Acquire) {
        throw_type_error("process.stdin.on cannot be used after process.stdin.destroy()");
    }
    let event = string_header_to_string(event_ptr);
    match event.as_str() {
        "data" => {
            DATA_CALLBACKS.with(|cb| cb.borrow_mut().push(callback));
            try_register_pump();
            ensure_reader_started();
        }
        "keypress" => {
            KEYPRESS_CALLBACKS.with(|cb| cb.borrow_mut().push(callback));
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
    undefined()
}

#[no_mangle]
pub extern "C" fn js_readline_stdin_remove_listener(
    event_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    if event_ptr.is_null() {
        return js_nanbox_pointer(STDIN_READLINE_HANDLE);
    }
    let event = string_header_to_string(event_ptr);
    match event.as_str() {
        "data" => DATA_CALLBACKS.with(|callbacks| {
            callbacks
                .borrow_mut()
                .retain(|registered| *registered != callback);
        }),
        "keypress" => KEYPRESS_CALLBACKS.with(|callbacks| {
            callbacks
                .borrow_mut()
                .retain(|registered| *registered != callback);
        }),
        "end" | "close" => CLOSE_CALLBACK.with(|cb| {
            let mut cb = cb.borrow_mut();
            if *cb == Some(callback) {
                *cb = None;
            }
        }),
        _ => {}
    }
    js_nanbox_pointer(STDIN_READLINE_HANDLE)
}

#[no_mangle]
pub extern "C" fn js_readline_stdin_pause() -> f64 {
    STDIN_PAUSED.store(true, Ordering::Release);
    js_nanbox_pointer(STDIN_READLINE_HANDLE)
}

#[no_mangle]
pub extern "C" fn js_readline_stdin_resume() -> f64 {
    if !STDIN_DESTROYED.load(Ordering::Acquire) {
        STDIN_PAUSED.store(false, Ordering::Release);
        try_register_pump();
        ensure_reader_started();
    }
    js_nanbox_pointer(STDIN_READLINE_HANDLE)
}

#[no_mangle]
pub extern "C" fn js_readline_stdin_unref() -> f64 {
    STDIN_REFED.store(false, Ordering::Release);
    js_nanbox_pointer(STDIN_READLINE_HANDLE)
}

#[no_mangle]
pub extern "C" fn js_readline_stdin_ref() -> f64 {
    if !STDIN_DESTROYED.load(Ordering::Acquire) {
        STDIN_REFED.store(true, Ordering::Release);
    }
    js_nanbox_pointer(STDIN_READLINE_HANDLE)
}

#[no_mangle]
pub extern "C" fn js_readline_stdin_destroy() -> f64 {
    STDIN_DESTROYED.store(true, Ordering::Release);
    STDIN_REFED.store(false, Ordering::Release);
    STDIN_PAUSED.store(true, Ordering::Release);
    RAW_MODE.store(false, Ordering::Release);
    EOF_REACHED.store(true, Ordering::Release);
    let _ = termios_impl::disable();
    if let Ok(mut q) = PENDING_DATA.lock() {
        q.clear();
    }
    if let Ok(mut q) = PENDING_LINES.lock() {
        q.clear();
    }
    DATA_CALLBACKS.with(|cb| cb.borrow_mut().clear());
    KEYPRESS_CALLBACKS.with(|cb| cb.borrow_mut().clear());
    QUESTION_CALLBACK.with(|cb| *cb.borrow_mut() = None);
    LINE_CALLBACK.with(|cb| *cb.borrow_mut() = None);
    CLOSE_CALLBACK.with(|cb| *cb.borrow_mut() = None);
    CLOSE_FIRED.with(|f| *f.borrow_mut() = true);
    perry_runtime::os::mark_process_stdin_destroyed();
    js_nanbox_pointer(STDIN_READLINE_HANDLE)
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
    let chunks: Vec<Vec<u8>> = if STDIN_DESTROYED.load(Ordering::Acquire) {
        if let Ok(mut q) = PENDING_DATA.lock() {
            q.clear();
        }
        Vec::new()
    } else if STDIN_PAUSED.load(Ordering::Acquire) {
        Vec::new()
    } else {
        let mut q = match PENDING_DATA.lock() {
            Ok(g) => g,
            Err(_) => return fired,
        };
        std::mem::take(&mut *q)
    };
    for chunk in chunks {
        // 'data' callback receives the raw bytes as a string.
        let data_callbacks = DATA_CALLBACKS.with(|cb| cb.borrow().clone());
        for cb_i64 in data_callbacks {
            let s = js_string_from_bytes(chunk.as_ptr(), chunk.len() as u32);
            let arg = f64::from_bits(JSValue::string_ptr(s).bits());
            let closure = cb_i64 as *const ClosureHeader;
            js_closure_call1(closure, arg);
            fired += 1;
        }
        // 'keypress' callback receives (sequence_string, key_object).
        let keypress_callbacks = KEYPRESS_CALLBACKS.with(|cb| cb.borrow().clone());
        for cb_i64 in keypress_callbacks {
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
    // #3962: a TUI that tore down stdin (`process.stdin.destroy()/.pause()/
    // .unref()`) no longer pins the event loop, so the process can quiesce.
    if perry_runtime::os::stdin_is_detached() {
        return 0;
    }
    let started = READER_STARTED.load(Ordering::Acquire);
    let eof = EOF_REACHED.load(Ordering::Acquire);
    let destroyed = STDIN_DESTROYED.load(Ordering::Acquire);
    let paused = STDIN_PAUSED.load(Ordering::Acquire);
    let refed = STDIN_REFED.load(Ordering::Acquire);
    let has_lines = PENDING_LINES.lock().map(|q| !q.is_empty()).unwrap_or(false);
    let has_data = PENDING_DATA.lock().map(|q| !q.is_empty()).unwrap_or(false);
    let has_stdin_callbacks = DATA_CALLBACKS.with(|c| !c.borrow().is_empty())
        || KEYPRESS_CALLBACKS.with(|c| !c.borrow().is_empty());
    let has_line_callbacks = QUESTION_CALLBACK.with(|c| c.borrow().is_some())
        || LINE_CALLBACK.with(|c| c.borrow().is_some());
    let has_close_cb =
        !CLOSE_FIRED.with(|f| *f.borrow()) && CLOSE_CALLBACK.with(|c| c.borrow().is_some());
    let has_dispatchable_data = has_data && has_stdin_callbacks && !paused;
    let reader_keeps_alive = started
        && !eof
        && !destroyed
        && refed
        && !paused
        && ((RAW_MODE.load(Ordering::Acquire) && has_stdin_callbacks)
            || has_line_callbacks
            || has_close_cb);
    if !destroyed
        && refed
        && (has_lines || has_dispatchable_data || has_close_cb || reader_keeps_alive)
    {
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
    thread_local! {
        static DATA_COUNT: RefCell<usize> = const { RefCell::new(0) };
    }

    extern "C" fn count_data_callback(_closure: *const ClosureHeader, _chunk: f64) -> f64 {
        DATA_COUNT.with(|count| *count.borrow_mut() += 1);
        undefined()
    }

    fn data_counter_callback() -> i64 {
        js_closure_alloc(count_data_callback as *const u8, 0) as i64
    }

    fn event_name(name: &str) -> *mut StringHeader {
        js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    fn reset() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        DATA_COUNT.with(|count| *count.borrow_mut() = 0);
        QUESTION_CALLBACK.with(|c| *c.borrow_mut() = None);
        LINE_CALLBACK.with(|c| *c.borrow_mut() = None);
        CLOSE_CALLBACK.with(|c| *c.borrow_mut() = None);
        DATA_CALLBACKS.with(|c| c.borrow_mut().clear());
        KEYPRESS_CALLBACKS.with(|c| c.borrow_mut().clear());
        PENDING_LINES.lock().unwrap().clear();
        PENDING_DATA.lock().unwrap().clear();
        EOF_REACHED.store(false, Ordering::Release);
        STDIN_PAUSED.store(false, Ordering::Release);
        STDIN_REFED.store(true, Ordering::Release);
        STDIN_DESTROYED.store(false, Ordering::Release);
        CLOSE_FIRED.with(|f| *f.borrow_mut() = false);
        RAW_MODE.store(false, Ordering::Release);
        READLINE_INTERFACES.with(|interfaces| interfaces.borrow_mut().clear());
        NEXT_READLINE_HANDLE.with(|next| *next.borrow_mut() = 2);
        // READER_STARTED stays sticky once set in a test process.
        guard
    }

    #[test]
    fn close_without_callbacks_is_noop() {
        let _g = reset();
        let h = js_readline_create_interface(0.0);
        assert_eq!(h, STDIN_READLINE_HANDLE);
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
    fn stdin_remove_listener_detaches_data_callback() {
        let _g = reset();
        let cb = data_counter_callback();
        let event = event_name("data");
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
        let cb = data_counter_callback();
        let _ = js_readline_stdin_on(event_name("data"), cb);
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
        DATA_CALLBACKS.with(|callbacks| assert!(callbacks.borrow().is_empty()));
        assert!(STDIN_DESTROYED.load(Ordering::Acquire));
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
