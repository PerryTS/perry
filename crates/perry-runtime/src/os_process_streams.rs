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

/// Node's `stream.write(chunk[, encoding][, callback])` passes an optional
/// completion callback as the last argument (whichever of the two trailing args
/// is a function). Node invokes it — asynchronously, never synchronously — once
/// the chunk has been handled, with no error argument on success.
///
/// #6672: perry's write stubs used to ignore it entirely, so
/// `await new Promise(r => process.stdout.write(x, r))` never resolved — the
/// promise hung, its awaiter never resumed, and the event loop drained and
/// exited with the continuation (and any `process.exitCode` it would have set)
/// left unrun. That is the pi print-mode exit-code divergence: pi's
/// `flushRawStdout` awaits exactly this callback, so on a request error the
/// natural-exit code stayed 0 where Node exits 1.
///
/// Pick the trailing callback and schedule it on the next tick (matching Node's
/// async completion contract). A missing/non-function arg is a no-op.
fn schedule_write_callback(arg2: f64, arg3: f64) {
    // `write(chunk, cb)` puts the callback at arg2; `write(chunk, encoding, cb)`
    // at arg3. Prefer the later slot, falling back to arg2.
    let cb_ptr = match callable_closure_ptr(arg3) {
        0 => callable_closure_ptr(arg2),
        p => p,
    };
    if cb_ptr != 0 {
        // `js_queue_next_tick` takes the raw closure pointer (drained as
        // `*const ClosureHeader`); no error argument is forwarded, so on the
        // JS side `cb(err)` sees `err === undefined` and reports success.
        crate::builtins::js_queue_next_tick(cb_ptr as i64);
    }
}

/// The closure pointer of `value` if it is a callable function, else 0.
fn callable_closure_ptr(value: f64) -> usize {
    let bits = value.to_bits();
    if crate::value::JSValue::from_bits(bits).is_pointer() {
        let ptr = (bits & crate::value::POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return ptr;
        }
    }
    0
}

/// `write` impl for process.stdout. Writes the value's display bytes to fd 1
/// without appending a newline, matching Node.js semantics, then fires the
/// optional completion callback (see [`schedule_write_callback`]).
extern "C" fn process_stdout_write_stub(
    _closure: *const crate::closure::ClosureHeader,
    chunk: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    use std::io::Write;
    let bytes = jsvalue_to_write_bytes(chunk);
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(&bytes);
    let _ = handle.flush();
    schedule_write_callback(arg2, arg3);
    f64::from_bits(crate::value::TAG_TRUE)
}

/// `write` impl for process.stderr. Same as stdout, targeting fd 2.
extern "C" fn process_stderr_write_stub(
    _closure: *const crate::closure::ClosureHeader,
    chunk: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    use std::io::Write;
    let bytes = jsvalue_to_write_bytes(chunk);
    let stderr = std::io::stderr();
    let mut handle = stderr.lock();
    let _ = handle.write_all(&bytes);
    let _ = handle.flush();
    schedule_write_callback(arg2, arg3);
    f64::from_bits(crate::value::TAG_TRUE)
}

/// `write` impl for process.stdin. Reading from stdin via `.write` is
/// nonsensical; keep it as a no-op that returns `true`, but still honor the
/// optional completion callback so an awaited `stdin.write(x, cb)` resolves.
extern "C" fn process_stdin_write_noop_stub(
    _closure: *const crate::closure::ClosureHeader,
    _chunk: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    schedule_write_callback(arg2, arg3);
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

/// #3962: set when a TUI tears down stdin via `process.stdin.destroy()` or
/// `.pause()`. `perry-stdlib`'s readline `has_active` consults
/// `stdin_is_detached()` so the runtime stops holding the event loop open for
/// the stdin reader, letting the process quiesce after teardown without an
/// explicit `process.exit()`.
///
/// #9676: this used to cover `.unref()` too, and that was the TUI-input-death
/// bug. `unref()` set this latch, the fd-0 reader below breaks its loop on it
/// and EXITS — and `ref()` was wired to a no-op stub, so nothing ever cleared
/// the latch or restarted the reader. One `unref()`/`ref()` pair (ink performs
/// exactly that pair every time its raw-mode refcount drops to zero and comes
/// back, i.e. whenever the last `useInput` component unmounts and a new one
/// mounts around a tool call) therefore left the process with NO reader on fd 0
/// for the rest of its life: the terminal stayed in raw mode, the loop kept
/// ticking, and not one further keystroke ever reached JS.
static STDIN_DETACHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// #9676: set by `process.stdin.unref()`, cleared by `.ref()`.
///
/// Node's `ref`/`unref` govern ONLY whether the handle keeps the event loop
/// alive — an unref'd stdin still delivers data. So this flag feeds the
/// liveness view (`stdin_is_detached`) but NOT the reader loop, which keeps
/// reading. That separation is what makes the pair symmetric: `ref()` restores
/// the hold, and no keystroke is lost in between.
static STDIN_UNREFED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True once `process.stdin` no longer holds the event loop open — either it
/// was detached (`destroy`/`pause`) or it was `unref()`d. This is the LIVENESS
/// view; the fd-0 reader uses `stdin_reader_should_stop()` instead, which
/// deliberately ignores `unref`.
pub fn stdin_is_detached() -> bool {
    STDIN_DETACHED.load(std::sync::atomic::Ordering::Acquire)
        || STDIN_UNREFED.load(std::sync::atomic::Ordering::Acquire)
}

/// Whether the fd-0 reader thread should stop. `unref()` must NOT stop it
/// (#9676) — only an explicit `destroy()`/`pause()` does.
fn stdin_reader_should_stop() -> bool {
    STDIN_DETACHED.load(std::sync::atomic::Ordering::Acquire)
}

/// `destroy`/`pause` impl for `process.stdin` — releases the stdin reader's
/// hold on the event loop and stops the reader. No-op return (`undefined`).
extern "C" fn process_stdin_detach_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    STDIN_DETACHED.store(true, std::sync::atomic::Ordering::Release);
    // #9676: mirror it into readline's flow state, so `pause()` means the same
    // thing whichever spelling reached it (and so the `resume()` below is a
    // true inverse rather than a partial one).
    if let Some(pause) = stdin_flow_op(&STDIN_FLOW_PAUSE_FN) {
        pause();
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `process.stdin.unref()` — drop the event-loop hold WITHOUT stopping
/// delivery (#9676). Node's contract: an unref'd stdin still emits `'data'`.
extern "C" fn process_stdin_unref_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    STDIN_UNREFED.store(true, std::sync::atomic::Ordering::Release);
    crate::object::js_implicit_this_get()
}

/// `process.stdin.ref()` — restore the event-loop hold (#9676). Was a no-op
/// stub, which is what made `unref()` a one-way latch.
///
/// Deliberately does NOT start the shared reader. `ref` is the inverse of
/// `unref` and nothing more, exactly as in Node: it does not resume a paused
/// stream. `resume()` remains the one call that restarts a stopped reader.
extern "C" fn process_stdin_ref_stub(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    STDIN_UNREFED.store(false, std::sync::atomic::Ordering::Release);
    crate::object::js_implicit_this_get()
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
    STDIN_DETACHED.store(true, std::sync::atomic::Ordering::Release);
    disable_process_stdin_keypress_events();
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
// even though `perry/tui` had its own working reader. The runtime-owned reader
// thread reads fd 0, routes the bytes and wakes the event loop; the loop
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

/// Optional consumer installed by perry-stdlib's readline implementation.
///
/// fd 0 must have exactly one physical reader. Historically this module and
/// `perry-stdlib::readline` each spawned a thread that held
/// `std::io::StdinLock` for its whole lifetime; whichever thread won the lock
/// consumed every byte and the other parked forever. The runtime now owns the
/// sole reader and forwards each read (and EOF) through these callbacks when
/// readline is linked. The callbacks only enqueue Rust-owned bytes/flags; JS
/// dispatch remains on the main-thread pumps.
static STDIN_READER_DATA_FN: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static STDIN_READER_EOF_FN: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Serializes the initial buffered-byte handoff with live reader deliveries.
/// Without it, a read that arrives just after callback registration could be
/// forwarded before bytes that the runtime reader had already buffered.
static STDIN_READER_ROUTE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn stdin_reader_data_consumer() -> Option<extern "C" fn(*const u8, usize)> {
    let ptr = STDIN_READER_DATA_FN.load(std::sync::atomic::Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: `js_register_stdin_reader_consumer` stores this exact ABI.
        Some(unsafe { std::mem::transmute::<*mut (), extern "C" fn(*const u8, usize)>(ptr) })
    }
}

fn stdin_reader_eof_consumer() -> Option<extern "C" fn()> {
    let ptr = STDIN_READER_EOF_FN.load(std::sync::atomic::Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: `js_register_stdin_reader_consumer` stores this exact ABI.
        Some(unsafe { std::mem::transmute::<*mut (), extern "C" fn()>(ptr) })
    }
}

/// Register readline as a subscriber to the runtime-owned fd-0 reader.
///
/// Bytes read before stdlib initialized are handed over synchronously while
/// the route lock is held, preserving their order relative to future reads.
#[no_mangle]
pub extern "C" fn js_register_stdin_reader_consumer(
    on_data: extern "C" fn(*const u8, usize),
    on_eof: extern "C" fn(),
) {
    let _route = STDIN_READER_ROUTE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    STDIN_READER_DATA_FN.store(on_data as *mut (), std::sync::atomic::Ordering::Release);
    STDIN_READER_EOF_FN.store(on_eof as *mut (), std::sync::atomic::Ordering::Release);

    let pending = STDIN_BUFFER
        .lock()
        .map(|mut bytes| std::mem::take(&mut *bytes))
        .unwrap_or_default();
    if !pending.is_empty() {
        on_data(pending.as_ptr(), pending.len());
    }
    if STDIN_EOF_SEEN.load(std::sync::atomic::Ordering::Acquire) {
        on_eof();
    }
}

fn ensure_stdin_reader() {
    use std::sync::atomic::Ordering;
    // A previous reader may have exited (EOF, error, or explicit detach); its
    // drop guard resets `STDIN_READER_STARTED` to false,
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
            // 64 KiB matches Node's pipe read size and the former readline
            // reader's #9489 chunking contract. With this module now owning
            // the sole fd-0 read, a smaller buffer would regress a 1 MiB pipe
            // from ~16 `data` events to hundreds.
            let mut buf = [0u8; 65536];
            loop {
                // #9676: `stdin_reader_should_stop`, NOT `stdin_is_detached` —
                // an `unref()`d stdin still delivers data in Node, and reading
                // the liveness view here is what killed the reader for good.
                if stdin_reader_should_stop() {
                    break;
                }
                match handle.read(&mut buf) {
                    Ok(0) => {
                        // EOF: record it so the main-thread pump can fire JS
                        // `'end'`/`'close'` listeners after the buffer drains,
                        // and wake the loop so a final pump runs even when no
                        // more bytes arrive (e.g. `< /dev/null`).
                        let _route = STDIN_READER_ROUTE
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        STDIN_EOF_SEEN.store(true, std::sync::atomic::Ordering::Release);
                        if let Some(on_eof) = stdin_reader_eof_consumer() {
                            on_eof();
                        }
                        crate::event_pump::js_notify_main_thread();
                        break;
                    }
                    Ok(n) => {
                        let _route = STDIN_READER_ROUTE
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        if let Some(on_data) = stdin_reader_data_consumer() {
                            on_data(buf.as_ptr(), n);
                        } else if let Ok(mut q) = STDIN_BUFFER.lock() {
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

/// Start (or restart) the single runtime-owned fd-0 reader.
///
/// Readline calls this after installing its consumer callbacks. Keeping the
/// compare/exchange in this module makes it impossible for the two surfaces to
/// race separate `StdinLock`s again.
pub fn ensure_process_stdin_reader() {
    ensure_stdin_reader();
}

/// Append bytes to the buffer that `process.stdin.read()` drains.
///
/// `process.stdin.on(...)` / `.setRawMode(...)` / `.pause()` / `.resume()` do NOT
/// dispatch on this object — codegen lowers them to direct extern calls into
/// `perry-stdlib`'s readline. `read()` has no such route, so it stays a method
/// here and drains `STDIN_BUFFER`. The shared reader's stdlib consumer deposits
/// non-flowing bytes here so `on("readable")` and `read()` use the same buffer.
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
static STDIN_REMOVE_ALL_FN: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

/// Encoding set via `process.stdin.setEncoding(enc)`. `None` — Node's default —
/// means `data` chunks arrive as **Buffers**, not strings.
static STDIN_ENCODING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// True once `setEncoding` has been called; readline's pump consults this too, so
/// both stdin delivery paths agree on Buffer-vs-string.
pub fn stdin_has_encoding() -> bool {
    STDIN_ENCODING.lock().map(|e| e.is_some()).unwrap_or(false)
}

/// Incremental UTF-8 decode state for `process.stdin`, live once
/// `setEncoding` has been called (#9490).
///
/// `process.stdin` is a process-global stream, so one decoder serves both
/// delivery paths (the runtime's own pump and perry-stdlib's readline pump)
/// and `read()`. Only one of those is ever active for a given program, and
/// sharing the state is what lets a code point split across a chunk boundary
/// survive whichever path picks the halves up.
static STDIN_DECODER: std::sync::Mutex<crate::utf8_stream_decoder::Utf8StreamDecoder> =
    std::sync::Mutex::new(crate::utf8_stream_decoder::Utf8StreamDecoder::new());

/// Decode raw stdin bytes under the active `setEncoding`, holding back a
/// trailing incomplete sequence for the next chunk. Returns `None` when the
/// whole chunk was absorbed into the held partial — Node emits no `'data'`
/// event for that.
fn stdin_decode_encoded(chunk: &[u8]) -> Option<String> {
    let decoded = match STDIN_DECODER.lock() {
        Ok(mut d) => d.write(chunk),
        // A poisoned decoder must not silently drop input; fall back to the
        // one-shot lossy decode, which is correct except across a boundary.
        Err(_) => String::from_utf8_lossy(chunk).into_owned(),
    };
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn string_jsvalue(s: &str) -> f64 {
    let sh = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(sh).bits())
}

/// End-of-stream flush: a held incomplete sequence becomes one U+FFFD,
/// delivered as its own final `'data'` chunk before `'end'` (Node's
/// `StringDecoder.prototype.end`, and how Node's Readable emits it).
pub fn stdin_encoding_flush_jsvalue() -> Option<f64> {
    if !stdin_has_encoding() {
        return None;
    }
    let flushed = STDIN_DECODER.lock().ok()?.end(None);
    if flushed.is_empty() {
        return None;
    }
    Some(string_jsvalue(&flushed))
}

/// A `data` chunk as Node delivers it: a Buffer by default, a decoded string
/// once an encoding is set. `None` means "emit no event for this chunk" —
/// the bytes were absorbed into the decoder's held partial.
pub fn stdin_chunk_jsvalue_opt(chunk: &[u8]) -> Option<f64> {
    if stdin_has_encoding() {
        return stdin_decode_encoded(chunk).map(|s| string_jsvalue(&s));
    }
    Some(stdin_chunk_jsvalue(chunk))
}

/// A `data` chunk as Node delivers it: a Buffer by default, a string once an
/// encoding is set.
///
/// #9490: the encoded arm used to hand the raw bytes to
/// `js_string_from_bytes`, which validates nothing — it memcpy's them into
/// the string payload and counts UTF-16 units with a WTF-8-shaped walk. Bytes
/// 0..255 therefore came out as 158 code units with zero U+FFFD, high bytes
/// passed through raw, where Node yields 256 units and 128 replacements.
pub fn stdin_chunk_jsvalue(chunk: &[u8]) -> f64 {
    if stdin_has_encoding() {
        let decoded = stdin_decode_encoded(chunk).unwrap_or_default();
        return string_jsvalue(&decoded);
    }
    let buf = crate::buffer::buffer_alloc(chunk.len() as u32);
    unsafe {
        // #9399: `buffer_alloc` only reserves CAPACITY — it leaves `length` at
        // 0, and every other caller sets the length itself after filling the
        // payload. This one never did, so a `data` chunk delivered as a Buffer
        // (Node's default, i.e. whenever `setEncoding` has NOT been called)
        // arrived with `.length === 0`: the bytes were copied into the payload
        // but no consumer could see them. `chunk.toString()` was `""`,
        // `Buffer.concat([acc, chunk])` appended nothing, and
        // `JSON.stringify(chunk)` reported `{"type":"Buffer","data":[]}`.
        //
        // That is why claude-code's MCP stdio server answered nothing: its
        // transport does `readBuffer.append(chunk)` on raw (unencoded) chunks,
        // so the newline-delimited JSON-RPC framer never saw a single byte and
        // `readMessage()` returned null forever. The `setEncoding("utf8")`
        // branch above was unaffected, which is why the string path looked fine.
        (*buf).length = chunk.len() as u32;
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
    remove_all: extern "C" fn(*const u8, usize, i32),
) {
    STDIN_ON_FN.store(on as *mut (), std::sync::atomic::Ordering::Release);
    STDIN_OFF_FN.store(off as *mut (), std::sync::atomic::Ordering::Release);
    STDIN_REMOVE_ALL_FN.store(remove_all as *mut (), std::sync::atomic::Ordering::Release);
}

/// #9676: perry-stdlib's readline `pause`/`resume`, so the stdin OBJECT's
/// `pause()`/`resume()` reach the same flow state as codegen's literal
/// `process.stdin.pause()` / `.resume()`.
///
/// Without this bridge the two spellings latched DIFFERENT flags. `rl.close()`
/// and a literal `process.stdin.pause()` both set readline's `STDIN_PAUSED`,
/// whose pump branch returns without draining `PENDING_DATA` — while the shared
/// fd-0 reader keeps reading and notifying the main thread. Recovering
/// with an ALIASED `stdin.resume()` (`const s = process.stdin; s.resume()`, and
/// every TUI that holds stdin in a variable) landed on the runtime object stub,
/// which cleared only the runtime's own flags and left `STDIN_PAUSED` set for
/// the life of the process. The result is exactly the reported wedge: bytes are
/// consumed off the terminal, the process wakes and burns CPU on every
/// keystroke, and nothing is ever dispatched to JS.
static STDIN_FLOW_PAUSE_FN: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static STDIN_FLOW_RESUME_FN: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub extern "C" fn js_register_stdin_flow_ops(pause: extern "C" fn(), resume: extern "C" fn()) {
    STDIN_FLOW_PAUSE_FN.store(pause as *mut (), std::sync::atomic::Ordering::Release);
    STDIN_FLOW_RESUME_FN.store(resume as *mut (), std::sync::atomic::Ordering::Release);
}

fn stdin_flow_op(slot: &std::sync::atomic::AtomicPtr<()>) -> Option<extern "C" fn()> {
    let p = slot.load(std::sync::atomic::Ordering::Acquire);
    if p.is_null() {
        return None;
    }
    // SAFETY: `js_register_stdin_flow_ops` only ever stores this exact ABI.
    Some(unsafe { std::mem::transmute::<*mut (), extern "C" fn()>(p) })
}

/// True when readline owns the stdin listener registry (it always does once
/// perry-stdlib is linked).
fn stdin_ops_provider() -> Option<(
    extern "C" fn(*const u8, usize, i64, i32),
    extern "C" fn(*const u8, usize, i64),
    extern "C" fn(*const u8, usize, i32),
)> {
    let on = STDIN_ON_FN.load(std::sync::atomic::Ordering::Acquire);
    let off = STDIN_OFF_FN.load(std::sync::atomic::Ordering::Acquire);
    let remove_all = STDIN_REMOVE_ALL_FN.load(std::sync::atomic::Ordering::Acquire);
    if on.is_null() || off.is_null() || remove_all.is_null() {
        return None;
    }
    unsafe {
        Some((
            std::mem::transmute::<*mut (), extern "C" fn(*const u8, usize, i64, i32)>(on),
            std::mem::transmute::<*mut (), extern "C" fn(*const u8, usize, i64)>(off),
            std::mem::transmute::<*mut (), extern "C" fn(*const u8, usize, i32)>(remove_all),
        ))
    }
}

/// Move listeners registered on the stdin object before readline initialized
/// into readline's canonical registry. This is called only after stdlib has
/// finished installing the provider, and before the reader-buffer handoff, so
/// no already-read byte can overtake an earlier listener registration.
#[no_mangle]
pub extern "C" fn js_migrate_stdin_listeners_to_provider() {
    let Some((on, _, _)) = stdin_ops_provider() else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let take = |list: &std::sync::Mutex<Vec<i64>>| {
        list.lock()
            .map(|mut listeners| std::mem::take(&mut *listeners))
            .unwrap_or_default()
    };
    for (name, listeners, once) in [
        (b"data".as_slice(), take(&STDIN_DATA_LISTENERS), 0),
        (b"data".as_slice(), take(&STDIN_DATA_ONCE), 1),
        (b"readable".as_slice(), take(&STDIN_READABLE_LISTENERS), 0),
        (b"readable".as_slice(), take(&STDIN_READABLE_ONCE), 1),
        (b"end".as_slice(), take(&STDIN_END_LISTENERS), 0),
        (b"end".as_slice(), take(&STDIN_END_ONCE), 1),
    ] {
        let callbacks: Vec<_> = listeners
            .into_iter()
            .map(|callback| {
                scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader)
            })
            .collect();
        for callback in callbacks {
            on(
                name.as_ptr(),
                name.len(),
                callback.get_raw_const_ptr::<crate::closure::ClosureHeader>() as i64,
                once,
            );
        }
    }
}

/// Whether `readline.emitKeypressEvents(process.stdin)` has installed its
/// data-to-keypress parser. Keypress listeners alone do not make Node emit;
/// the readline helper is the plumbing that activates them.
static STDIN_KEYPRESS_EVENTS_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn stdin_keypress_events_enabled() -> bool {
    STDIN_KEYPRESS_EVENTS_ENABLED.load(std::sync::atomic::Ordering::Acquire)
}

pub fn disable_process_stdin_keypress_events() {
    STDIN_KEYPRESS_EVENTS_ENABLED.store(false, std::sync::atomic::Ordering::Release);
}

/// True when a raw object handle is the process-global stdin singleton.
pub fn is_process_stdin_handle(handle: i64) -> bool {
    STDIN_STREAM_SINGLETON.with(|slot| *slot.borrow() == handle as usize)
}

/// Install the hidden `data` listener used by
/// `readline.emitKeypressEvents(process.stdin)` into the same registry as
/// ordinary stdin data listeners. That makes cooked/piped input flow through
/// the keypress parser too, and lets `removeAllListeners("data")` remove the
/// plumbing exactly as Node does.
pub fn enable_process_stdin_keypress_events(callback: i64) {
    if STDIN_KEYPRESS_EVENTS_ENABLED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    if let Some((on, _, _)) = stdin_ops_provider() {
        on(b"data".as_ptr(), 4, callback, 0);
    } else if let Ok(mut listeners) = STDIN_DATA_LISTENERS.lock() {
        listeners.push(callback);
    }
    ensure_stdin_reader();
}

/// `process.stdin.addListener(event, cb)` / `.on(...)` reached as an object method.
extern "C" fn process_stdin_add_listener(
    closure: *const crate::closure::ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    if let Some((on, _, _)) = stdin_ops_provider() {
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
    if let Some((on, _, _)) = stdin_ops_provider() {
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
    let cb = stdin_callback_ptr(callback);
    if cb == 0 {
        return stdin_this_value();
    }
    let name = stdin_event_name(event).unwrap_or_default();
    if let Some((_, off, _)) = stdin_ops_provider() {
        off(name.as_ptr(), name.len(), cb);
    }
    // #9399: ALSO drop it from the runtime-local lists. `on`/`once` on the
    // stdin object file listeners here (see `process_stdin_on`), while `off`
    // only ever reached readline's registry — so a listener added and then
    // removed through the object stayed in this list forever. That was merely
    // wasteful until `stdin_listeners_keep_loop_alive` started reporting these
    // lists to the event loop; from then on a program that removes its stdin
    // listener to let itself exit would have been pinned open until EOF, which
    // never comes on a TTY. Remove from both registries so `off` means off.
    for list in [
        &STDIN_DATA_LISTENERS,
        &STDIN_DATA_ONCE,
        &STDIN_READABLE_LISTENERS,
        &STDIN_READABLE_ONCE,
        &STDIN_END_LISTENERS,
        &STDIN_END_ONCE,
    ] {
        if let Ok(mut l) = list.lock() {
            l.retain(|entry| *entry != cb);
        }
    }
    // A `keypress` listener registered before readline's provider existed is
    // kept in node:stream's generic emitter registry (the hidden parser emits
    // there). Removing through the stdin object must reach it too.
    if name == "keypress" {
        let scope = crate::gc::RuntimeHandleScope::new();
        let stream = scope.root_nanbox_f64(stdin_this_value());
        let event = scope.root_nanbox_f64(event);
        let callback = scope.root_nanbox_f64(callback);
        let stream_raw = crate::value::js_nanbox_get_pointer(stream.get_nanbox_f64()) as i64;
        if stream_raw != 0 {
            let _ = crate::node_stream::js_node_stream_method_remove_listener(
                stream_raw,
                event.get_nanbox_f64(),
                callback.get_nanbox_f64(),
            );
        }
    }
    stdin_this_value()
}

fn clear_stdin_listener_list(list: &std::sync::Mutex<Vec<i64>>) {
    if let Ok(mut listeners) = list.lock() {
        listeners.clear();
    }
}

/// `process.stdin.removeAllListeners([event])` across both stdin registries.
///
/// The stdin object has its own native listener surface, while
/// `emitKeypressEvents` uses node:stream's hidden EventEmitter storage for its
/// generated `keypress` emission. Both must be cleared or callbacks survive a
/// call that Node treats as authoritative.
extern "C" fn process_stdin_remove_all_listeners(
    _closure: *const crate::closure::ClosureHeader,
    event: f64,
) -> f64 {
    let name = stdin_event_name(event);
    if let Some((_, _, remove_all)) = stdin_ops_provider() {
        match name.as_deref() {
            Some(name) => remove_all(name.as_ptr(), name.len(), 1),
            None => remove_all(std::ptr::null(), 0, 0),
        }
    }

    match name.as_deref() {
        Some("data") => {
            clear_stdin_listener_list(&STDIN_DATA_LISTENERS);
            clear_stdin_listener_list(&STDIN_DATA_ONCE);
            disable_process_stdin_keypress_events();
        }
        Some("readable") => {
            clear_stdin_listener_list(&STDIN_READABLE_LISTENERS);
            clear_stdin_listener_list(&STDIN_READABLE_ONCE);
        }
        Some("end") | Some("close") => {
            clear_stdin_listener_list(&STDIN_END_LISTENERS);
            clear_stdin_listener_list(&STDIN_END_ONCE);
        }
        Some(_) => {}
        None => {
            for list in [
                &STDIN_DATA_LISTENERS,
                &STDIN_DATA_ONCE,
                &STDIN_READABLE_LISTENERS,
                &STDIN_READABLE_ONCE,
                &STDIN_END_LISTENERS,
                &STDIN_END_ONCE,
            ] {
                clear_stdin_listener_list(list);
            }
            disable_process_stdin_keypress_events();
        }
    }

    // A pre-provider keypress listener lives in node:stream's generic emitter
    // registry, so clear that registry too. Root both values across the array
    // allocations performed by remove-all.
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stdin_this_value());
    let event = scope.root_nanbox_f64(event);
    let stream_value = stream.get_nanbox_f64();
    let stream_raw = crate::value::js_nanbox_get_pointer(stream_value) as i64;
    if stream_raw != 0 {
        let _ = crate::node_stream::js_node_stream_method_remove_all_listeners(
            stream_raw,
            event.get_nanbox_f64(),
        );
    }
    stream.get_nanbox_f64()
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stdin_this_value());
    let event = scope.root_nanbox_f64(event);
    let stream_raw = crate::value::js_nanbox_get_pointer(stream.get_nanbox_f64()) as i64;
    if stream_raw != 0 {
        let listeners =
            crate::node_stream::js_node_stream_method_listeners(stream_raw, event.get_nanbox_f64());
        return f64::from_bits(
            crate::value::JSValue::array_ptr(listeners as *mut crate::array::ArrayHeader).bits(),
        );
    }
    let arr = crate::array::js_array_alloc(0);
    f64::from_bits(crate::value::JSValue::array_ptr(arr).bits())
}

/// Whether a `process.stdin` listener registered on the stdin OBJECT still
/// needs the event loop to keep turning.
///
/// #9399. `process.stdin.on(...)` reached as an object method — an alias
/// (`const s = process.stdin`), a parameter, or a field, which is what
/// claude-code's MCP stdio transport does with `this._stdin.on("data",
/// this._ondata)` — files the callback in the runtime-local lists below and
/// starts [`ensure_stdin_reader`]. Those lists were invisible to every
/// has-active check the generated event loop consults, so the loop found no
/// work and the process exited 0 *with the pipe still open and the bytes
/// unread*: `claude mcp serve` died ~1.7 s after start and never answered a
/// request. (The literal `process.stdin.on(...)` spelling was unaffected —
/// codegen lowers that one straight to a readline extern, whose registry
/// `js_readline_has_active` does report.)
///
/// The liveness window matches Node's: a `data`/`readable`/`end` listener holds
/// the process open until stdin reaches EOF and the buffered bytes have been
/// delivered, then — once `'end'`/`'close'` have fired, or when nobody is
/// listening for them — lets it exit. `pause()`/`unref()`/`destroy()` release
/// the hold immediately, via the same `stdin_is_detached` latch readline uses.
/// Test-only: seed or clear the runtime-local `'data'` listener registry.
///
/// #9416's unit test drives `js_stdlib_has_active_handles` — the symbol the
/// generated event loop calls — through the same registry a real
/// `const s = process.stdin; s.on("data", …)` fills, without spawning an fd-0
/// reader that would fight the test harness for the terminal.
#[cfg(test)]
pub(crate) fn test_set_stdin_data_listener(cb: Option<i64>) {
    use std::sync::atomic::Ordering;
    if let Ok(mut l) = STDIN_DATA_LISTENERS.lock() {
        l.clear();
        if let Some(cb) = cb {
            l.push(cb);
        }
    }
    STDIN_EOF_SEEN.store(false, Ordering::Release);
    STDIN_END_FIRED.store(false, Ordering::Release);
}

pub fn stdin_listeners_keep_loop_alive() -> bool {
    if stdin_is_detached() {
        return false;
    }
    let non_empty =
        |l: &std::sync::Mutex<Vec<i64>>| l.lock().map(|v| !v.is_empty()).unwrap_or(false);
    let has_end_listener = non_empty(&STDIN_END_LISTENERS) || non_empty(&STDIN_END_ONCE);
    let has_any_listener = has_end_listener
        || non_empty(&STDIN_DATA_LISTENERS)
        || non_empty(&STDIN_DATA_ONCE)
        || non_empty(&STDIN_READABLE_LISTENERS)
        || non_empty(&STDIN_READABLE_ONCE);
    if !has_any_listener {
        return false;
    }
    // Bytes already read but not yet handed to JS: the pump must run again.
    if STDIN_BUFFER.lock().map(|b| !b.is_empty()).unwrap_or(false) {
        return true;
    }
    use std::sync::atomic::Ordering;
    if !STDIN_EOF_SEEN.load(Ordering::Acquire) {
        // stdin is still open — more bytes may arrive, exactly as in Node.
        return true;
    }
    // EOF reached and drained: the only work left is the terminal
    // `'end'`/`'close'` dispatch, and only if somebody asked for it.
    has_end_listener && !STDIN_END_FIRED.load(Ordering::Acquire)
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
            Some("keypress") => {
                let stream_raw = crate::value::js_nanbox_get_pointer(stdin_this_value()) as i64;
                if stream_raw != 0 {
                    let _ =
                        crate::node_stream::js_node_stream_method_on(stream_raw, event, callback);
                }
            }
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
            Some("keypress") => {
                let stream_raw = crate::value::js_nanbox_get_pointer(stdin_this_value()) as i64;
                if stream_raw != 0 {
                    let _ =
                        crate::node_stream::js_node_stream_method_once(stream_raw, event, callback);
                }
            }
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
        // EOF with an incomplete sequence still held: Node's last `read()`
        // yields the decoder's flush rather than `null` (#9490).
        if STDIN_EOF_SEEN.load(std::sync::atomic::Ordering::Acquire) {
            if let Some(flushed) = stdin_encoding_flush_jsvalue() {
                return flushed;
            }
        }
        return f64::from_bits(crate::value::TAG_NULL);
    }
    // #9490: pull mode ("readable" + read()) shares the stream decoder, so a
    // code point split across two reads is reassembled instead of becoming
    // two replacement characters.
    let s = if stdin_has_encoding() {
        stdin_decode_encoded(&bytes).unwrap_or_default()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let sh = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    // Nanbox as a STRING value (not a generic object pointer) so JS sees a
    // real string from `read()` — `typeof` / `+` / `!== null` all rely on this.
    f64::from_bits(crate::value::STRING_TAG | (sh as u64 & crate::value::POINTER_MASK))
}

/// `process.stdin.resume()` — flowing mode. Clears any prior detach (from
/// `pause`/`destroy`) and any prior `unref()`, and (re)starts the reader, so a
/// paused stdin can resume.
extern "C" fn process_stdin_resume(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    STDIN_DETACHED.store(false, std::sync::atomic::Ordering::Release);
    STDIN_UNREFED.store(false, std::sync::atomic::Ordering::Release);
    // #9676: clear readline's `STDIN_PAUSED` too. `rl.close()` and a literal
    // `process.stdin.pause()` set it, its pump branch stops draining
    // `PENDING_DATA`, and before this bridge only the LITERAL
    // `process.stdin.resume()` could clear it — an aliased `s.resume()` left
    // stdin permanently deaf while the reader kept consuming bytes.
    if let Some(resume) = stdin_flow_op(&STDIN_FLOW_RESUME_FN) {
        resume();
    }
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
        // #9490: decode ONCE per chunk. The UTF-8 decoder carries state
        // across chunks, so decoding per listener would push the same bytes
        // through it N times and give the second listener a continuation of
        // the first one's leftovers. `None` = the chunk was absorbed whole
        // into a held partial, for which Node fires no `'data'` event.
        let Some(arg) = stdin_chunk_jsvalue_opt(&bytes) else {
            return;
        };
        // The value is built ONCE now, so it must survive every listener call:
        // a GC inside listener N can move the string listener N+1 still has to
        // receive, and a bare `f64` local would then be stale. Root it in a
        // scope that outlives the loop and re-read the handle each iteration.
        // (Before #9490 this was safe by accident — the arg was rebuilt inside
        // the loop, which a stateful decoder can no longer do.)
        let arg_scope = crate::gc::RuntimeHandleScope::new();
        let arg_handle = arg_scope.root_nanbox_f64(arg);
        for cb in data_listeners {
            let scope = crate::gc::RuntimeHandleScope::new();
            let cb_handle = scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
            // Node calls stream listeners with `this === stream`. Re-read the
            // singleton per listener and root the displaced receiver: the previous
            // listener was user code, so either may have moved (#9445).
            let this = stdin_this_value();
            let prev_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(this));
            cb_handle.with_const_ptr::<crate::closure::ClosureHeader, _>(|closure| {
                crate::closure::js_closure_call1(closure, arg_handle.get_nanbox_f64());
            });
            crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
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
    for cb in readable_listeners {
        let scope = crate::gc::RuntimeHandleScope::new();
        let cb_handle = scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
        // Per-listener re-read + rooted save/restore (#9445), as for `data`.
        let this = stdin_this_value();
        let prev_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(this));
        cb_handle.with_const_ptr::<crate::closure::ClosureHeader, _>(|closure| {
            crate::closure::js_closure_call0(closure);
        });
        crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
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
    // #9490: a sequence left incomplete at EOF is flushed as one U+FFFD, in
    // its own final `'data'` event, BEFORE `'end'`.
    //
    // Only when a `data` listener exists to receive it: in pull mode the
    // flush belongs to the consumer's last `read()`, and taking it here would
    // consume the decoder state and drop the replacement character.
    let has_data_listener = STDIN_DATA_LISTENERS
        .lock()
        .map(|l| !l.is_empty())
        .unwrap_or(false);
    if has_data_listener {
        if let Some(flushed) = stdin_encoding_flush_jsvalue() {
            let data_listeners: Vec<i64> = STDIN_DATA_LISTENERS
                .lock()
                .map(|l| l.clone())
                .unwrap_or_default();
            let flush_scope = crate::gc::RuntimeHandleScope::new();
            let flush_handle = flush_scope.root_nanbox_f64(flushed);
            for cb in data_listeners {
                let scope = crate::gc::RuntimeHandleScope::new();
                let cb_handle =
                    scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
                // Per-listener re-read + rooted save/restore (#9445), as for `data`.
                let this = stdin_this_value();
                let prev_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(this));
                cb_handle.with_const_ptr::<crate::closure::ClosureHeader, _>(|closure| {
                    crate::closure::js_closure_call1(closure, flush_handle.get_nanbox_f64());
                });
                crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
            }
        }
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
    for cb in end_listeners {
        let scope = crate::gc::RuntimeHandleScope::new();
        let cb_handle = scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
        // Per-listener re-read + rooted save/restore (#9445), as for `data`.
        let this = stdin_this_value();
        let prev_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(this));
        cb_handle.with_const_ptr::<crate::closure::ClosureHeader, _>(|closure| {
            crate::closure::js_closure_call0(closure);
        });
        crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
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
    write_stub: extern "C" fn(*const crate::closure::ClosureHeader, f64, f64, f64) -> f64,
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
    // `write` takes up to three positional args — `write(chunk[, encoding][,
    // callback])`. Register its arity so dispatch pads/truncates to exactly the
    // three the stub declares (Direct dispatch would otherwise size the call to
    // the call site, dropping the trailing callback — #6672).
    crate::closure::js_register_closure_arity(write_stub as *const u8, 3);
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
    // #3962: install the appended listener-removal + lifecycle methods. stdin
    // replaces the stream stubs below with its real listener/flow operations;
    // stdout and stderr retain the stubs.
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
        if is_stdin {
            let remove_all = stdin_native_method(
                process_stdin_remove_all_listeners as *const u8,
                "removeAllListeners",
                1,
            );
            js_object_set_field(obj, start + 3, JSValue::from_bits(remove_all.to_bits()));
        } else {
            set_field_with_stub(start + 3, process_stream_on_once_stub);
        }
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
           // #9676: on stdin, `unref`/`ref` are a SYMMETRIC pair that only moves
           // the event-loop hold; on stdout/stderr `unref` stays the shared no-op.
        set_field_with_stub(
            start + 6,
            if is_stdin {
                process_stdin_unref_stub
            } else {
                process_stream_on_once_stub
            },
        ); // unref
        if is_stdin {
            set_field_with_stub(start + 7, process_stdin_ref_stub); // ref
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
    let (obj, fresh) = STDIN_STREAM_SINGLETON.with(|slot| {
        let mut slot = slot.borrow_mut();
        let fresh = *slot == 0;
        if fresh {
            *slot = build_stream_object_with_write(
                process_stdin_write_noop_stub,
                0.0,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            ) as usize;
        }
        (*slot as *mut crate::object::ObjectHeader, fresh)
    });
    let value = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    if !fresh {
        return value;
    }
    // #9400: `for await (const chunk of process.stdin)` — Node's stdin is a
    // Readable and therefore async-iterable. The install has to happen after
    // the singleton slot is populated, because the iterator closure captures
    // this very value, and only once, or each call would re-install it.
    crate::node_stream::async_iterator::install_method_listener_readable_async_iterator_symbol(
        value,
    );
    // The install allocates (two hidden fields, a closure, a symbol property),
    // so a collection can relocate the just-born stdin object underneath it.
    // `STDIN_STREAM_SINGLETON` is a MUTABLE root (see
    // `scan_process_stream_singleton_roots_mut`), so the slot is rewritten to
    // the new address — re-read it rather than handing JS the pre-install
    // pointer, which after a move is a dangling one.
    STDIN_STREAM_SINGLETON.with(|slot| {
        let obj = *slot.borrow() as *mut crate::object::ObjectHeader;
        f64::from_bits(JSValue::pointer(obj as *const u8).bits())
    })
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
