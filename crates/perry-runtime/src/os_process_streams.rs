use crate::string::StringHeader;
use std::cell::RefCell;

/// Coerce a NaN-boxed JSValue to its display bytes, suitable for raw
/// stream writes. Used by `process.stdout.write` / `process.stderr.write`.
/// Mirrors Node's behavior: numbers/booleans/null/undefined coerce to
/// their string form; strings pass through verbatim.
fn jsvalue_to_write_bytes(value: f64) -> Vec<u8> {
    let s_ptr = crate::value::js_jsvalue_to_string(value);
    if s_ptr.is_null() {
        return Vec::new();
    }
    unsafe {
        let header = &*s_ptr;
        let len = header.byte_len as usize;
        let data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

/// `write` impl for process.stdout. Writes the value's display bytes to fd 1
/// without appending a newline, matching Node.js semantics.
extern "C" fn process_stdout_write_stub(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    use std::io::Write;
    let bytes = jsvalue_to_write_bytes(arg);
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(&bytes);
    let _ = handle.flush();
    f64::from_bits(crate::value::TAG_TRUE)
}

/// `write` impl for process.stderr. Same as stdout, targeting fd 2.
extern "C" fn process_stderr_write_stub(
    _closure: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    use std::io::Write;
    let bytes = jsvalue_to_write_bytes(arg);
    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    let _ = handle.write_all(&bytes);
    let _ = handle.flush();
    f64::from_bits(crate::value::TAG_TRUE)
}

/// `write` impl for process.stdin. Reading from stdin via `.write` is
/// nonsensical; keep it as a no-op that returns `true`.
extern "C" fn process_stdin_write_noop_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_TRUE)
}

extern "C" fn process_stream_emit_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_TRUE)
}

extern "C" fn process_stream_on_once_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `setEncoding` impl for `process.stdin`. A Readable's `setEncoding(enc)`
/// returns the stream itself so callers can chain
/// (`process.stdin.setEncoding("utf8").on("data", …)`). The receiver is the
/// `IMPLICIT_THIS` bound by the method-dispatch path, so returning it mirrors
/// Node's `this`-returning contract. Encoding-aware reads remain future work.
extern "C" fn process_stream_set_encoding_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    crate::object::js_implicit_this_get()
}

/// #3962: set when a TUI tears down stdin via `process.stdin.destroy()`,
/// `.pause()`, or `.unref()`. `perry-stdlib`'s readline `has_active` consults
/// `stdin_is_detached()` so the runtime stops holding the event loop open for
/// the stdin reader, letting the process quiesce after teardown without an
/// explicit `process.exit()`.
static STDIN_DETACHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True once `process.stdin` has been detached (`destroy`/`pause`/`unref`).
pub fn stdin_is_detached() -> bool {
    STDIN_DETACHED.load(std::sync::atomic::Ordering::Acquire)
}

/// `destroy`/`pause`/`unref` impl for `process.stdin` — releases the stdin
/// reader's hold on the event loop. No-op return (`undefined`).
extern "C" fn process_stdin_detach_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    STDIN_DETACHED.store(true, std::sync::atomic::Ordering::Release);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

thread_local! {
    static STDIN_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
    static STDOUT_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
    static STDERR_STREAM_SINGLETON: RefCell<usize> = const { RefCell::new(0) };
}

fn string_key(key: &[u8]) -> *mut StringHeader {
    crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

fn set_stdin_bool_field(name: &[u8], value: bool) {
    STDIN_STREAM_SINGLETON.with(|slot| {
        let obj = *slot.borrow() as *mut crate::object::ObjectHeader;
        if obj.is_null() {
            return;
        }
        crate::object::js_object_set_field_by_name(
            obj,
            string_key(name),
            f64::from_bits(crate::value::JSValue::bool(value).bits()),
        );
    });
}

pub fn set_process_stdin_raw_state(enabled: bool) {
    set_stdin_bool_field(b"isRaw", enabled);
}

pub fn mark_process_stdin_destroyed() {
    set_stdin_bool_field(b"readable", false);
    set_stdin_bool_field(b"readableEnded", true);
    set_stdin_bool_field(b"destroyed", true);
    set_stdin_bool_field(b"closed", true);
    set_stdin_bool_field(b"isRaw", false);
}

// ── #input: process.stdin as a functional raw-mode Readable ──────────────
// Node TUIs read the keyboard via `process.stdin` — real `ink` uses
// `setRawMode(true)` + `on("data", …)`, and the bundle uses
// `setRawMode(!0); on("readable", () => { let c = stdin.read(); while (c !==
// null) { …; c = stdin.read() } })`. Previously `on`/`read`/`resume` were
// no-op stubs ("encoding-aware reads remain future work"), so input was dead
// even though `perry/tui` had its own working reader. A dedicated reader
// thread reads fd 0, buffers the bytes and wakes the event loop; the loop
// pump (`pump_process_stdin`, called each tick from `js_callback_timer_tick`)
// drains the buffer and fires the registered `data`/`readable` listeners.
static STDIN_BUFFER: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());
static STDIN_DATA_LISTENERS: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());
static STDIN_READABLE_LISTENERS: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());
// `once()` listeners — fired exactly once then cleared, per EventEmitter.
static STDIN_DATA_ONCE: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());
static STDIN_READABLE_ONCE: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());
// `end`/`close` listeners. Node fires `'end'` on stdin EOF; code that reads a
// prompt via `process.stdin.once('end', …)` (racing a timeout) relies on it.
// These fire from the main-thread pump once the reader hits EOF and the byte
// buffer has drained (so `'data'` precedes `'end'`, per Node).
static STDIN_END_LISTENERS: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());
static STDIN_END_ONCE: std::sync::Mutex<Vec<i64>> = std::sync::Mutex::new(Vec::new());
// Set by the reader thread on fd-0 EOF; observed by the main-thread pump.
static STDIN_EOF_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
// Set once the `'end'`/`'close'` listeners have fired, so they fire at most once.
static STDIN_END_FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static STDIN_READER_STARTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn ensure_stdin_reader() {
    use std::sync::atomic::Ordering;
    // A previous reader may have exited (EOF, error, or detach via
    // `pause`/`unref`); its drop guard resets `STDIN_READER_STARTED` to false,
    // so a later `resume()`/`on(...)` can spin up a fresh reader.
    if STDIN_READER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        std::thread::spawn(|| {
            use std::io::Read;
            // On exit, clear STARTED so the reader can be restarted later.
            struct ReaderGuard;
            impl Drop for ReaderGuard {
                fn drop(&mut self) {
                    STDIN_READER_STARTED.store(false, std::sync::atomic::Ordering::Release);
                }
            }
            let _guard = ReaderGuard;
            let stdin = std::io::stdin();
            let mut handle = stdin.lock();
            // Read in chunks, not one byte at a time. A paste or a fast-typed
            // burst arrives as many bytes; the old `[0u8; 1]` read did one
            // `read` syscall + one STDIN_BUFFER lock + one main-thread notify
            // (and thus one event-loop wake + pump) PER BYTE, so an N-byte
            // burst paid N round trips. `read` still returns as soon as any
            // bytes are available (it does not wait to fill the buffer), so a
            // lone keystroke is unaffected — it returns 1 byte immediately —
            // while a burst collapses into one lock + one notify.
            let mut buf = [0u8; 4096];
            loop {
                if stdin_is_detached() {
                    break;
                }
                match handle.read(&mut buf) {
                    Ok(0) => {
                        // EOF: record it so the main-thread pump can fire JS
                        // `'end'`/`'close'` listeners after the buffer drains,
                        // and wake the loop so a final pump runs even when no
                        // more bytes arrive (e.g. `< /dev/null`).
                        STDIN_EOF_SEEN.store(true, std::sync::atomic::Ordering::Release);
                        crate::event_pump::js_notify_main_thread();
                        break;
                    }
                    Ok(n) => {
                        if let Ok(mut q) = STDIN_BUFFER.lock() {
                            q.extend_from_slice(&buf[..n]);
                        }
                        crate::event_pump::js_notify_main_thread();
                    }
                    Err(_) => break,
                }
            }
        });
    }
}

/// Append bytes to the buffer that `process.stdin.read()` drains.
///
/// `process.stdin.on(...)` / `.setRawMode(...)` / `.pause()` / `.resume()` do NOT
/// dispatch on this object — codegen lowers them to direct extern calls into
/// `perry-stdlib`'s readline, which runs its own fd-0 reader. `read()` has no such
/// route, so it stays a method here and drains `STDIN_BUFFER`. Paused-mode input
/// (`on("readable")` + `read()`) therefore needs readline's reader to deposit its
/// bytes here, or the two halves of that pattern would talk to different buffers
/// and `read()` would always return null.
/// `perry-stdlib`'s readline owns the `process.stdin` listener lists (codegen
/// lowers `stdin.on(...)` to a direct extern into it), but the stdin *object*
/// lives here — so `stdin.listeners(event)` cannot see them without a bridge.
/// stdlib registers a provider at init; the method below calls through it.
///
/// Node TUIs need this: they suspend the keyboard by reading
/// `stdin.listeners("readable")`, stashing them, and removing each one, then
/// restore them afterwards. With `listeners` missing, that call throws
/// `TypeError: listeners is not a function` and the restore never happens.
static STDIN_LISTENERS_FN: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub extern "C" fn js_register_stdin_listeners_provider(f: extern "C" fn(*const u8, usize) -> f64) {
    STDIN_LISTENERS_FN.store(f as *mut (), std::sync::atomic::Ordering::Release);
}

/// Registration ops, owned by perry-stdlib's readline for the same reason as the
/// listener list itself. `addListener`/`removeListener`/`off` on the stdin OBJECT
/// were no-op stubs, so a TUI that registers through an aliased binding —
/// `const {stdin} = props; stdin.addListener("readable", handler)`, which is what
/// real TUI libraries do — had its keyboard handler silently discarded, while the
/// direct `process.stdin.on(...)` form (lowered to a readline extern by codegen)
/// worked. Route both to the same registry so there is one listener list and one
/// fd-0 reader.
static STDIN_ON_FN: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static STDIN_OFF_FN: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Encoding set via `process.stdin.setEncoding(enc)`. `None` — Node's default —
/// means `data` chunks arrive as **Buffers**, not strings.
static STDIN_ENCODING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// True once `setEncoding` has been called; readline's pump consults this too, so
/// both stdin delivery paths agree on Buffer-vs-string.
pub fn stdin_has_encoding() -> bool {
    STDIN_ENCODING.lock().map(|e| e.is_some()).unwrap_or(false)
}

/// A `data` chunk as Node delivers it: a Buffer by default, a string once an
/// encoding is set.
pub fn stdin_chunk_jsvalue(chunk: &[u8]) -> f64 {
    if stdin_has_encoding() {
        let s = crate::string::js_string_from_bytes(chunk.as_ptr(), chunk.len() as u32);
        return f64::from_bits(crate::value::JSValue::string_ptr(s).bits());
    }
    let buf = crate::buffer::buffer_alloc(chunk.len() as u32);
    unsafe {
        let dst = crate::buffer::buffer_data_mut(buf);
        if !dst.is_null() && !chunk.is_empty() {
            // GC_STORE_AUDIT(POINTER_FREE): raw stdin bytes into a freshly
            // allocated Buffer's data area. The payload is bytes, never
            // JSValues, so the destination slots hold no GC references and no
            // write barrier is required. `buffer_alloc` returns before any
            // safepoint, so `dst` cannot have been moved between the
            // allocation and this copy.
            std::ptr::copy_nonoverlapping(chunk.as_ptr(), dst, chunk.len());
        }
    }
    f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
}

/// `process.stdin.setEncoding(enc)`. Was a no-op stub, which forced every `data`
/// chunk to be delivered as a string. Node delivers a **Buffer** unless an
/// encoding has been set — so code that does `Buffer.concat([buf, chunk])` on
/// stdin data (a normal pattern) got a string and threw. Record the encoding so
/// the reader can decide.
extern "C" fn process_stdin_set_encoding(
    _closure: *const crate::closure::ClosureHeader,
    encoding: f64,
) -> f64 {
    let name = stdin_event_name(encoding).unwrap_or_default();
    if let Ok(mut e) = STDIN_ENCODING.lock() {
        *e = if name.is_empty() { None } else { Some(name) };
    }
    stdin_this_value()
}

#[no_mangle]
pub extern "C" fn js_register_stdin_listener_ops(
    on: extern "C" fn(*const u8, usize, i64, i32),
    off: extern "C" fn(*const u8, usize, i64),
) {
    STDIN_ON_FN.store(on as *mut (), std::sync::atomic::Ordering::Release);
    STDIN_OFF_FN.store(off as *mut (), std::sync::atomic::Ordering::Release);
}

/// True when readline owns the stdin listener registry (it always does once
/// perry-stdlib is linked).
fn stdin_ops_provider() -> Option<(
    extern "C" fn(*const u8, usize, i64, i32),
    extern "C" fn(*const u8, usize, i64),
)> {
    let on = STDIN_ON_FN.load(std::sync::atomic::Ordering::Acquire);
    let off = STDIN_OFF_FN.load(std::sync::atomic::Ordering::Acquire);
    if on.is_null() || off.is_null() {
        return None;
    }
    unsafe {
        Some((
            std::mem::transmute::<*mut (), extern "C" fn(*const u8, usize, i64, i32)>(on),
            std::mem::transmute::<*mut (), extern "C" fn(*const u8, usize, i64)>(off),
        ))
    }
}

/// `process.stdin.addListener(event, cb)` / `.on(...)` reached as an object method.
extern "C" fn process_stdin_add_listener(
    closure: *const crate::closure::ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    if let Some((on, _)) = stdin_ops_provider() {
        let name = stdin_event_name(event).unwrap_or_default();
        let cb = stdin_callback_ptr(callback);
        if cb != 0 {
            on(name.as_ptr(), name.len(), cb, 0);
        }
        return stdin_this_value();
    }
    process_stdin_on(closure, event, callback)
}

/// `process.stdin.once(event, cb)` reached as an object method.
extern "C" fn process_stdin_add_listener_once(
    closure: *const crate::closure::ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    if let Some((on, _)) = stdin_ops_provider() {
        let name = stdin_event_name(event).unwrap_or_default();
        let cb = stdin_callback_ptr(callback);
        if cb != 0 {
            on(name.as_ptr(), name.len(), cb, 1);
        }
        return stdin_this_value();
    }
    process_stdin_once(closure, event, callback)
}

/// `process.stdin.removeListener(event, cb)` / `.off(...)`.
extern "C" fn process_stdin_remove_listener(
    _closure: *const crate::closure::ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    if let Some((_, off)) = stdin_ops_provider() {
        let name = stdin_event_name(event).unwrap_or_default();
        let cb = stdin_callback_ptr(callback);
        if cb != 0 {
            off(name.as_ptr(), name.len(), cb);
        }
    }
    stdin_this_value()
}

/// `process.stdin.listeners(event)` — the registered listeners for `event`,
/// as a real array (empty when there are none), like Node's EventEmitter.
extern "C" fn process_stdin_listeners(
    _closure: *const crate::closure::ClosureHeader,
    event: f64,
) -> f64 {
    let name = stdin_event_name(event).unwrap_or_default();
    let f = STDIN_LISTENERS_FN.load(std::sync::atomic::Ordering::Acquire);
    if !f.is_null() {
        let func: extern "C" fn(*const u8, usize) -> f64 = unsafe { std::mem::transmute(f) };
        return func(name.as_ptr(), name.len());
    }
    let arr = crate::array::js_array_alloc(0);
    f64::from_bits(crate::value::JSValue::array_ptr(arr).bits())
}

pub fn stdin_push_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if let Ok(mut buf) = STDIN_BUFFER.lock() {
        buf.extend_from_slice(bytes);
    }
}

/// The `process.stdin` stream object as a JS value, for `this`-binding during
/// listener dispatch (Node calls stream listeners with `this === stream`).
fn stdin_this_value() -> f64 {
    STDIN_STREAM_SINGLETON.with(|slot| {
        let obj = *slot.borrow();
        if obj == 0 {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
        }
    })
}

fn stdin_event_name(value: f64) -> Option<String> {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        return None;
    }
    unsafe {
        let header = &*ptr;
        let len = header.byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

fn stdin_callback_ptr(value: f64) -> i64 {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return 0;
    }
    (value.to_bits() & crate::value::POINTER_MASK) as i64
}

fn register_stdin_listener(
    event: f64,
    callback: f64,
    persistent: &std::sync::Mutex<Vec<i64>>,
    once: &std::sync::Mutex<Vec<i64>>,
    is_once: bool,
) {
    let cb = stdin_callback_ptr(callback);
    if cb == 0 {
        return;
    }
    let target = if is_once { once } else { persistent };
    match stdin_event_name(event).as_deref() {
        Some("data") | Some("readable") | Some("end") | Some("close") => {
            if let Ok(mut l) = target.lock() {
                // EventEmitter allows the same listener registered multiple
                // times; only `on` callers dedupe in practice, but each
                // `once` registration must fire independently, so don't dedupe
                // there.
                if is_once || !l.contains(&cb) {
                    l.push(cb);
                }
            }
            // Starting the reader lets it observe EOF, which drives `'end'`.
            ensure_stdin_reader();
        }
        _ => {}
    }
}

/// `process.stdin.on(event, cb)` — registers a persistent `data`/`readable`
/// listener and starts the reader. Returns `this` so callers can chain.
extern "C" fn process_stdin_on(
    _closure: *const crate::closure::ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    // `data` vs `readable` is selected inside the helper by event name; both
    // persistent registries are passed and the helper picks per event.
    let cb = stdin_callback_ptr(callback);
    if cb != 0 {
        match stdin_event_name(event).as_deref() {
            Some("data") => register_stdin_listener(
                event,
                callback,
                &STDIN_DATA_LISTENERS,
                &STDIN_DATA_ONCE,
                false,
            ),
            Some("readable") => register_stdin_listener(
                event,
                callback,
                &STDIN_READABLE_LISTENERS,
                &STDIN_READABLE_ONCE,
                false,
            ),
            Some("end") | Some("close") => register_stdin_listener(
                event,
                callback,
                &STDIN_END_LISTENERS,
                &STDIN_END_ONCE,
                false,
            ),
            _ => {}
        }
    }
    crate::object::js_implicit_this_get()
}

/// `process.stdin.once(event, cb)` — fires the listener exactly once.
extern "C" fn process_stdin_once(
    _closure: *const crate::closure::ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let cb = stdin_callback_ptr(callback);
    if cb != 0 {
        match stdin_event_name(event).as_deref() {
            Some("data") => register_stdin_listener(
                event,
                callback,
                &STDIN_DATA_LISTENERS,
                &STDIN_DATA_ONCE,
                true,
            ),
            Some("readable") => register_stdin_listener(
                event,
                callback,
                &STDIN_READABLE_LISTENERS,
                &STDIN_READABLE_ONCE,
                true,
            ),
            Some("end") | Some("close") => register_stdin_listener(
                event,
                callback,
                &STDIN_END_LISTENERS,
                &STDIN_END_ONCE,
                true,
            ),
            _ => {}
        }
    }
    crate::object::js_implicit_this_get()
}

/// `process.stdin.read([size])` — returns buffered input as a string (stdin is
/// `setEncoding("utf8")` in practice) or `null` when nothing is buffered, per
/// Node's `Readable.read()` contract.
extern "C" fn process_stdin_read(_closure: *const crate::closure::ClosureHeader, _arg: f64) -> f64 {
    let bytes = match STDIN_BUFFER.lock() {
        Ok(mut b) => std::mem::take(&mut *b),
        Err(_) => return f64::from_bits(crate::value::TAG_NULL),
    };
    if bytes.is_empty() {
        return f64::from_bits(crate::value::TAG_NULL);
    }
    let s = String::from_utf8_lossy(&bytes);
    let sh = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    // Nanbox as a STRING value (not a generic object pointer) so JS sees a
    // real string from `read()` — `typeof` / `+` / `!== null` all rely on this.
    f64::from_bits(crate::value::STRING_TAG | (sh as u64 & crate::value::POINTER_MASK))
}

/// `process.stdin.resume()` — flowing mode. Clears any prior detach (from
/// `pause`/`unref`) and (re)starts the reader, so a paused stdin can resume.
extern "C" fn process_stdin_resume(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    STDIN_DETACHED.store(false, std::sync::atomic::Ordering::Release);
    ensure_stdin_reader();
    crate::object::js_implicit_this_get()
}

/// Drain buffered stdin and fire `data`/`readable` listeners. Called once per
/// event-loop iteration from `js_callback_timer_tick` — a safe JS-execution
/// point (the same place timer callbacks fire), NOT from `js_wait_for_event`
/// (calling JS from the wait primitive reenters and wedges the runtime). Each
/// listener call is wrapped in a GC handle scope that roots the closure (and
/// any string arg) across the call, mirroring the timer dispatch. `data`
/// listeners (ink's flowing path) get the bytes directly; otherwise `readable`
/// listeners are notified and pull via `read()`.
pub fn pump_process_stdin() {
    // Deliver any buffered bytes as `'data'`/`'readable'` first, then — once the
    // reader has hit EOF and the buffer is empty — dispatch `'end'`/`'close'`.
    pump_stdin_data_chunks();
    maybe_fire_stdin_end();
}

fn pump_stdin_data_chunks() {
    let has_bytes = STDIN_BUFFER.lock().map(|b| !b.is_empty()).unwrap_or(false);
    if !has_bytes {
        return;
    }
    // `data` (flowing) takes precedence: a `data` listener consumes the bytes.
    // `once` listeners are drained so they fire exactly once.
    let mut data_listeners: Vec<i64> = STDIN_DATA_LISTENERS
        .lock()
        .map(|l| l.clone())
        .unwrap_or_default();
    let data_once: Vec<i64> = STDIN_DATA_ONCE
        .lock()
        .map(|mut l| std::mem::take(&mut *l))
        .unwrap_or_default();
    data_listeners.extend(&data_once);
    if !data_listeners.is_empty() {
        let bytes = STDIN_BUFFER
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default();
        if bytes.is_empty() {
            return;
        }
        let this = stdin_this_value();
        for cb in data_listeners {
            let scope = crate::gc::RuntimeHandleScope::new();
            let cb_handle = scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
            // Allocate the arg string inside the scope so GC during the call
            // can't free or move it out from under the callback.
            let arg = stdin_chunk_jsvalue(&bytes);
            let arg_handles = scope.root_nanbox_f64_slice(&[arg]);
            let a = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
            let closure = cb_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
            // Node calls stream listeners with `this === stream`.
            let prev_this = crate::object::js_implicit_this_set(this);
            crate::closure::js_closure_call1(closure, a[0]);
            crate::object::js_implicit_this_set(prev_this);
        }
        return;
    }
    let mut readable_listeners: Vec<i64> = STDIN_READABLE_LISTENERS
        .lock()
        .map(|l| l.clone())
        .unwrap_or_default();
    let readable_once: Vec<i64> = STDIN_READABLE_ONCE
        .lock()
        .map(|mut l| std::mem::take(&mut *l))
        .unwrap_or_default();
    readable_listeners.extend(&readable_once);
    let this = stdin_this_value();
    for cb in readable_listeners {
        let scope = crate::gc::RuntimeHandleScope::new();
        let cb_handle = scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
        let closure = cb_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
        let prev_this = crate::object::js_implicit_this_set(this);
        crate::closure::js_closure_call0(closure);
        crate::object::js_implicit_this_set(prev_this);
    }
}

/// Fire `process.stdin` `'end'`/`'close'` listeners once the reader has hit EOF
/// and all buffered bytes have drained (so `'data'` precedes `'end'`, per Node).
/// Runs on the main thread from the pump, so calling JS is safe here. Idempotent:
/// the `STDIN_END_FIRED` latch guarantees at-most-once, and we only latch when
/// there is at least one listener to fire so a listener attached shortly after
/// EOF (the prompt-reader race) still runs.
fn maybe_fire_stdin_end() {
    use std::sync::atomic::Ordering;
    if !STDIN_EOF_SEEN.load(Ordering::Acquire) || STDIN_END_FIRED.load(Ordering::Acquire) {
        return;
    }
    // Node emits `'end'` only after the readable side is fully consumed.
    let has_bytes = STDIN_BUFFER.lock().map(|b| !b.is_empty()).unwrap_or(false);
    if has_bytes {
        return;
    }
    let mut end_listeners: Vec<i64> = STDIN_END_LISTENERS
        .lock()
        .map(|l| l.clone())
        .unwrap_or_default();
    let has_once = STDIN_END_ONCE
        .lock()
        .map(|l| !l.is_empty())
        .unwrap_or(false);
    if end_listeners.is_empty() && !has_once {
        // No listener yet — leave EOF pending so a slightly-later `once('end')`
        // (racing the reader) still fires on a subsequent pump.
        return;
    }
    if STDIN_END_FIRED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let end_once: Vec<i64> = STDIN_END_ONCE
        .lock()
        .map(|mut l| std::mem::take(&mut *l))
        .unwrap_or_default();
    end_listeners.extend(&end_once);
    let this = stdin_this_value();
    for cb in end_listeners {
        let scope = crate::gc::RuntimeHandleScope::new();
        let cb_handle = scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
        let closure = cb_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
        let prev_this = crate::object::js_implicit_this_set(this);
        crate::closure::js_closure_call0(closure);
        crate::object::js_implicit_this_set(prev_this);
    }
}

/// Make a native-method closure value with the given arity registered, so the
/// dispatch path forwards the right number of arguments.
fn stdin_native_method(func_ptr: *const u8, name: &str, arity: u32) -> f64 {
    crate::closure::js_register_closure_arity(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc_singleton(func_ptr);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

pub fn scan_process_stream_singleton_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    {
        let mut visit_slot = |slot: &RefCell<usize>| {
            let mut value = slot.borrow_mut();
            if *value != 0 {
                let mut ptr = *value as *mut crate::object::ObjectHeader;
                if visitor.visit_raw_mut_ptr_slot(&mut ptr) {
                    *value = ptr as usize;
                }
            }
        };
        STDIN_STREAM_SINGLETON.with(&mut visit_slot);
        STDOUT_STREAM_SINGLETON.with(&mut visit_slot);
        STDERR_STREAM_SINGLETON.with(&mut visit_slot);
    }
    // The registered stdin listeners (raw closure addresses) are GC roots:
    // a TUI that registers an anonymous handler and drops its only JS
    // reference must not have that closure swept or relocated out from under
    // us before the next keypress fires it.
    for registry in [
        &STDIN_DATA_LISTENERS,
        &STDIN_READABLE_LISTENERS,
        &STDIN_DATA_ONCE,
        &STDIN_READABLE_ONCE,
        &STDIN_END_LISTENERS,
        &STDIN_END_ONCE,
    ] {
        if let Ok(mut listeners) = registry.lock() {
            for cb in listeners.iter_mut() {
                if *cb != 0 {
                    let mut ptr = *cb as *mut crate::object::ObjectHeader;
                    if visitor.visit_raw_mut_ptr_slot(&mut ptr) {
                        *cb = ptr as i64;
                    }
                }
            }
        }
    }
}

/// Build a stream object with a `write` field bound to the given stub.
fn build_stream_object_with_write(
    write_stub: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64,
    fd: f64,
    writable: f64,
) -> *mut crate::object::ObjectHeader {
    use crate::closure::js_closure_alloc;
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let fd_i = fd as i32;
    let is_tty = crate::tty::is_tty_fd(fd_i);
    if is_tty {
        crate::tty::attach_tty_constructor_prototype(
            crate::object::bound_native_callable_export_value(
                "tty",
                if fd_i == 0 {
                    "ReadStream"
                } else {
                    "WriteStream"
                },
            ),
            if fd_i == 0 {
                "ReadStream"
            } else {
                "WriteStream"
            },
        );
    }

    // #3962: EventEmitter listener-removal + lifecycle surface appended to the
    // stdin shapes. The TTY *write* stream keeps its existing shape; generic
    // non-TTY streams keep `main`'s no-op teardown surface.
    const STDIN_TEARDOWN_KEYS: &[u8] =
        b"addListener\0removeListener\0off\0removeAllListeners\0pause\0resume\0unref\0ref\0destroy\0setEncoding\0";
    const GENERIC_TEARDOWN_KEYS: &[u8] =
        b"addListener\0removeListener\0off\0removeAllListeners\0pause\0resume\0unref\0destroy\0";
    let is_stdin = fd_i == 0;
    let (class_id, packed, field_count, teardown_start): (u32, Vec<u8>, u32, Option<u32>) =
        if is_stdin {
            let mut keys = b"write\0fd\0emit\0on\0once\0writable\0readable\0readableEnded\0destroyed\0closed\0isRaw\0isTTY\0".to_vec();
            keys.extend_from_slice(STDIN_TEARDOWN_KEYS);
            keys.extend_from_slice(b"read\0"); // field 22: Readable.read()
            keys.extend_from_slice(b"listeners\0"); // field 23: EventEmitter.listeners()
            (
                if is_tty {
                    crate::tty::CLASS_ID_TTY_READ_STREAM
                } else {
                    0
                },
                keys,
                24,
                Some(12),
            )
        } else if is_tty {
            (
                crate::tty::CLASS_ID_TTY_WRITE_STREAM,
                b"write\0fd\0emit\0on\0once\0writable\0addListener\0removeListener\0off\0removeAllListeners\0".to_vec(),
                10,
                None,
            )
        } else {
            let mut keys = b"write\0fd\0emit\0on\0once\0writable\0".to_vec();
            keys.extend_from_slice(GENERIC_TEARDOWN_KEYS);
            (0, keys, 14, Some(6))
        };
    let obj = if class_id == 0 {
        // Shape ids must stay clear of NAVIGATOR_CLASS_ID (0x7FFF_FF22) — the
        // per-shape key registry is first-registration-wins, so sharing an id
        // with navigator made `process.stdout.write` resolve to undefined
        // whenever navigator was built first. stdin gets its own id because
        // its key layout diverges from stdout/stderr past field 5.
        let shape_id = if is_stdin { 0x7FFF_FF29 } else { 0x7FFF_FF23 };
        js_object_alloc_with_shape(shape_id, field_count, packed.as_ptr(), packed.len() as u32)
    } else {
        crate::object::js_object_alloc_class_with_keys(
            class_id,
            0,
            field_count,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    let closure = js_closure_alloc(write_stub as *const u8, 0);
    let cval = JSValue::pointer(closure as *const u8);
    js_object_set_field(obj, 0, cval);
    js_object_set_field(obj, 1, JSValue::number(fd));
    let emit = js_closure_alloc(process_stream_emit_stub as *const u8, 0);
    js_object_set_field(obj, 2, JSValue::pointer(emit as *const u8));
    if is_tty && fd_i != 0 {
        js_object_set_field(
            obj,
            3,
            JSValue::from_bits(crate::tty::tty_listener_on_value().to_bits()),
        );
        js_object_set_field(
            obj,
            4,
            JSValue::from_bits(crate::tty::tty_listener_on_value().to_bits()),
        );
    } else if is_stdin {
        // Real `on(event, cb)` so `process.stdin.on("data"/"readable", …)`
        // registers a keyboard listener instead of dropping it (#input).
        let on = stdin_native_method(process_stdin_on as *const u8, "on", 2);
        js_object_set_field(obj, 3, JSValue::from_bits(on.to_bits()));
        // `once` routes through the same registry as `on`/`addListener` so a
        // one-shot listener registered on an aliased binding is not dropped either.
        let once = stdin_native_method(process_stdin_add_listener_once as *const u8, "once", 2);
        js_object_set_field(obj, 4, JSValue::from_bits(once.to_bits()));
    } else {
        let on = js_closure_alloc(process_stream_on_once_stub as *const u8, 0);
        js_object_set_field(obj, 3, JSValue::pointer(on as *const u8));
        let once = js_closure_alloc(process_stream_on_once_stub as *const u8, 0);
        js_object_set_field(obj, 4, JSValue::pointer(once as *const u8));
    }
    js_object_set_field(obj, 5, JSValue::from_bits(writable.to_bits()));
    if fd_i == 0 {
        js_object_set_field(obj, 6, JSValue::from_bits(crate::value::TAG_TRUE));
        js_object_set_field(obj, 7, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(obj, 8, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(obj, 9, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(obj, 10, JSValue::from_bits(crate::value::TAG_FALSE));
        js_object_set_field(
            obj,
            11,
            JSValue::from_bits(if is_tty {
                crate::value::TAG_TRUE
            } else {
                crate::value::TAG_FALSE
            }),
        );
    } else if is_tty {
        js_object_set_field(
            obj,
            6,
            JSValue::from_bits(crate::tty::tty_listener_on_value().to_bits()),
        );
        js_object_set_field(
            obj,
            7,
            JSValue::from_bits(crate::tty::tty_listener_remove_value().to_bits()),
        );
        js_object_set_field(
            obj,
            8,
            JSValue::from_bits(crate::tty::tty_listener_remove_value().to_bits()),
        );
        js_object_set_field(
            obj,
            9,
            JSValue::from_bits(crate::tty::tty_listener_remove_all_value().to_bits()),
        );
    }
    // #3962: install the appended listener-removal + lifecycle methods.
    // `on`/`once` above are no-ops here, so `addListener`/`removeListener`/
    // `off`/`removeAllListeners`/`resume` are no-ops too. On *stdin* (fd 0),
    // `pause`/`unref`/`destroy` additionally detach the reader so the loop can
    // quiesce after TUI teardown; on stdout/stderr they stay no-ops.
    if let Some(start) = teardown_start {
        let set_field_with_stub =
            |idx: u32, stub: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64| {
                let c = js_closure_alloc(stub as *const u8, 0);
                js_object_set_field(obj, idx, JSValue::pointer(c as *const u8));
            };
        let lifecycle: extern "C" fn(*const crate::closure::ClosureHeader, f64) -> f64 = if is_stdin
        {
            process_stdin_detach_stub
        } else {
            process_stream_on_once_stub
        };
        // On stdin these must be REAL: a TUI registers its keyboard through an
        // aliased binding (`stdin.addListener("readable", handler)`), which lands
        // here rather than on codegen's direct `process.stdin.on(...)` extern. As
        // no-op stubs they silently discarded the handler.
        if is_stdin {
            let add =
                stdin_native_method(process_stdin_add_listener as *const u8, "addListener", 2);
            js_object_set_field(obj, start, JSValue::from_bits(add.to_bits()));
            let rm = stdin_native_method(
                process_stdin_remove_listener as *const u8,
                "removeListener",
                2,
            );
            js_object_set_field(obj, start + 1, JSValue::from_bits(rm.to_bits()));
            let off = stdin_native_method(process_stdin_remove_listener as *const u8, "off", 2);
            js_object_set_field(obj, start + 2, JSValue::from_bits(off.to_bits()));
        } else {
            set_field_with_stub(start, process_stream_on_once_stub); // addListener
            set_field_with_stub(start + 1, process_stream_on_once_stub); // removeListener
            set_field_with_stub(start + 2, process_stream_on_once_stub); // off
        }
        set_field_with_stub(start + 3, process_stream_on_once_stub); // removeAllListeners
        set_field_with_stub(start + 4, lifecycle); // pause
                                                   // resume: real flowing-mode start on stdin, no-op on stdout/stderr.
        set_field_with_stub(
            start + 5,
            if is_stdin {
                process_stdin_resume
            } else {
                process_stream_on_once_stub
            },
        ); // resume
        set_field_with_stub(start + 6, lifecycle); // unref
        if is_stdin {
            set_field_with_stub(start + 7, process_stream_on_once_stub); // ref
            set_field_with_stub(start + 8, lifecycle); // destroy
            if is_stdin {
                let se =
                    stdin_native_method(process_stdin_set_encoding as *const u8, "setEncoding", 1);
                js_object_set_field(obj, start + 9, JSValue::from_bits(se.to_bits()));
            } else {
                set_field_with_stub(start + 9, process_stream_set_encoding_stub);
                // setEncoding
            }
            // field 22: Readable.read() returns buffered keyboard input.
            let read = stdin_native_method(process_stdin_read as *const u8, "read", 1);
            js_object_set_field(obj, 22, JSValue::from_bits(read.to_bits()));
            let listeners =
                stdin_native_method(process_stdin_listeners as *const u8, "listeners", 1);
            js_object_set_field(obj, 23, JSValue::from_bits(listeners.to_bits()));
        } else {
            set_field_with_stub(start + 7, lifecycle); // destroy
        }
    }
    obj
}

/// process.stdin -> stream object whose `.write(...)` is a no-op.
#[no_mangle]
pub extern "C" fn js_process_stdin() -> f64 {
    use crate::value::JSValue;
    let obj = STDIN_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stdin_write_noop_stub,
                0.0,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.stdout -> stream object whose `.write(s)` writes `s` to fd 1.
#[no_mangle]
pub extern "C" fn js_process_stdout() -> f64 {
    use crate::value::JSValue;
    let obj = STDOUT_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stdout_write_stub,
                1.0,
                f64::from_bits(crate::value::TAG_TRUE),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// process.stderr -> stream object whose `.write(s)` writes `s` to fd 2.
#[no_mangle]
pub extern "C" fn js_process_stderr() -> f64 {
    use crate::value::JSValue;
    let obj = STDERR_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == 0 {
            *slot = build_stream_object_with_write(
                process_stderr_write_stub,
                2.0,
                f64::from_bits(crate::value::TAG_TRUE),
            ) as usize;
        }
        *slot as *mut crate::object::ObjectHeader
    });
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}
