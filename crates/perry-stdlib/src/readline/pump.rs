//! Drain / pump: keypress decoding plus the per-tick event-loop drain that
//! dispatches queued lines, `'data'` chunks and `'keypress'` events.
//!
//! Split out of `readline.rs` (which had grown past the 2000-line CI cap).
//! Contents are unchanged; `parse_keypress` was widened to `pub(super)` so
//! the unit tests that still live in `readline/mod.rs` keep resolving it.

use super::*;

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
pub(super) fn parse_keypress(chunk: &[u8]) -> Option<(String, bool, bool, bool, String)> {
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
    // Paused ("pull") mode: hand the bytes to the buffer that `process.stdin.read()`
    // drains, then notify the `readable` listeners (which take no argument and pull
    // the data themselves). `read()` is the one stdin method codegen does NOT lower
    // to a direct readline extern — it stays a method on the runtime's stdin object
    // and reads that buffer — so the bytes have to be deposited there or the two
    // halves of `on("readable") + read()` would never meet.
    // Buffer the bytes wherever `process.stdin.read()` can still reach them
    // whenever stdin is NOT in flowing mode (i.e. no `data` listener is consuming
    // them). That covers two cases:
    //
    //   * paused/pull mode — an `on("readable")` listener plus `read()`.
    //   * NO listener at all — which is not the same as "nobody wants these
    //     bytes". A TUI can deliberately strip its `readable` listener to read a
    //     terminal query response directly with `read()` (suspend/resume around a
    //     capability probe). Discarding the bytes there hangs it forever: the
    //     response never arrives, stdin is never resumed, and the keyboard stays
    //     dead for the rest of the session.
    //
    // `read()` is the one stdin method codegen does NOT lower to a readline extern
    // — it stays a method on the runtime's stdin object and drains that buffer — so
    // the bytes have to be deposited there or the two halves never meet.
    let data_flowing = DATA_CALLBACKS
        .lock()
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if !data_flowing && !chunks.is_empty() {
        for chunk in &chunks {
            perry_runtime::os::stdin_push_bytes(chunk);
        }
    }
    let readable_callbacks = READABLE_CALLBACKS
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    for cb_i64 in &readable_callbacks {
        let closure = *cb_i64 as *const ClosureHeader;
        js_closure_call0(closure);
        fired += 1;
    }

    for chunk in chunks {
        // 'data' callback receives the raw bytes as a string.
        let data_callbacks = DATA_CALLBACKS.lock().map(|v| v.clone()).unwrap_or_default();
        for cb_i64 in data_callbacks {
            let arg = stdin_chunk_value(&chunk);
            let closure = cb_i64 as *const ClosureHeader;
            js_closure_call1(closure, arg);
            fired += 1;
        }
        // 'keypress' callback receives (sequence_string, key_object).
        let keypress_callbacks = KEYPRESS_CALLBACKS
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();
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
    let has_stdin_callbacks = DATA_CALLBACKS
        .lock()
        .map(|v| !v.is_empty())
        .unwrap_or(false)
        || KEYPRESS_CALLBACKS
            .lock()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
        || READABLE_CALLBACKS
            .lock()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
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
        && (((RAW_MODE.load(Ordering::Acquire) || STDIN_DATA_FLOWING.load(Ordering::Acquire))
            && has_stdin_callbacks)
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
