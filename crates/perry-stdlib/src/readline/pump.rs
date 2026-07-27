//! Drain / pump: keypress decoding plus the per-tick event-loop drain that
//! dispatches queued lines, `'data'` chunks and `'keypress'` events.
//!
//! Split out of `readline.rs` (which had grown past the 2000-line CI cap).
//! `parse_keypress` is `pub(super)` so the unit tests that still live in
//! `readline/mod.rs` keep resolving it.

use super::*;

// ---------------------------------------------------------------------------
// Drain / pump
// ---------------------------------------------------------------------------

/// Build a NaN-boxed object literal `{ name, ctrl, shift, meta, sequence }`
/// suitable for the `'keypress'` event's second argument. The object is
/// rooted across the two string allocations: either one can trigger a
/// moving minor GC that would otherwise leave `obj` pointing at from-space.
fn build_keypress_object(name: &str, ctrl: bool, shift: bool, meta: bool, seq: &str) -> f64 {
    use perry_runtime::object::{js_object_alloc_with_shape, js_object_set_field};
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let packed = b"name\0ctrl\0shift\0meta\0sequence\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF47, 5, packed.as_ptr(), packed.len() as u32);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let name_str = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    js_object_set_field(obj, 0, JSValue::string_ptr(name_str));
    js_object_set_field(obj, 1, JSValue::bool(ctrl));
    js_object_set_field(obj, 2, JSValue::bool(shift));
    js_object_set_field(obj, 3, JSValue::bool(meta));
    let seq_str = js_string_from_bytes(seq.as_ptr(), seq.len() as u32);
    let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    js_object_set_field(obj, 4, JSValue::string_ptr(seq_str));
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Parse one reassembled chunk into a (name, ctrl, shift, meta, sequence)
/// keypress descriptor. Recognises Enter, Backspace, Tab, Escape, Ctrl+
/// letter, and ANSI CSI arrow keys (the 3-byte sequence `\x1b[A`/`B`/`C`/
/// `D`). The raw-mode reader queues one byte per chunk, so multi-byte
/// sequences are reassembled first by [`coalesce_escape_sequences`] in the
/// drain loop — by the time a chunk reaches this parser it is either a
/// complete sequence or a genuine single byte.
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

/// How far along an accumulator (first byte always ESC) is toward a
/// complete ANSI escape sequence.
enum EscState {
    /// Could still be extended — keep accumulating.
    Continue,
    /// A complete CSI/SS3 sequence — emit as one chunk.
    Complete,
    /// Not an escape sequence after all — flush the bytes individually.
    Invalid,
}

/// Longest escape sequence worth accumulating. Keyboard CSI sequences
/// (`\x1b[1;5A` and friends) are far shorter; anything longer is not key
/// input and flushes byte-wise.
const MAX_ESCAPE_LEN: usize = 16;

fn escape_state(acc: &[u8]) -> EscState {
    match acc.len() {
        0 | 1 => EscState::Continue,
        2 => match acc[1] {
            b'[' | b'O' => EscState::Continue,
            _ => EscState::Invalid,
        },
        n if n > MAX_ESCAPE_LEN => EscState::Invalid,
        n => {
            let last = acc[n - 1];
            if acc[1] == b'O' {
                // SS3: exactly one final byte (`\x1bOA`..`\x1bOS`).
                if (0x40..=0x7e).contains(&last) {
                    EscState::Complete
                } else {
                    EscState::Invalid
                }
            } else {
                // CSI: parameter bytes 0x30-0x3F / intermediate 0x20-0x2F,
                // terminated by a final byte 0x40-0x7E.
                match last {
                    0x40..=0x7e => EscState::Complete,
                    0x20..=0x3f => EscState::Continue,
                    _ => EscState::Invalid,
                }
            }
        }
    }
}

/// Reassemble ANSI escape sequences that the raw-mode reader queues as
/// individual 1-byte chunks (`\x1b`, `[`, `A` → one `\x1b[A` chunk) so a
/// single arrow key fires a single `'keypress'`/`'data'` event, matching a
/// terminal's one-write delivery. A sequence still incomplete at the end of
/// a drain batch is carried in [`PENDING_ESCAPE`] and finished by the next
/// tick's bytes; if that next tick brings no new bytes the held bytes flush
/// as-is, so a bare Escape keypress is delivered one tick later (a
/// tick-granularity stand-in for Node's `escapeCodeTimeout`).
fn coalesce_escape_sequences(raw: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut acc: Vec<u8> = PENDING_ESCAPE
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default();
    if raw.is_empty() {
        // No new bytes this tick: a held ESC prefix is a bare Escape (or a
        // torn sequence from a very slow terminal) — deliver it byte-wise
        // instead of holding it forever.
        return acc.into_iter().map(|b| vec![b]).collect();
    }
    let mut out: Vec<Vec<u8>> = Vec::with_capacity(raw.len());
    for chunk in raw {
        if acc.is_empty() {
            if chunk.len() == 1 && chunk[0] == 0x1b {
                acc.push(0x1b);
            } else {
                out.push(chunk);
            }
            continue;
        }
        if chunk.len() != 1 {
            // Multi-byte chunks come from the cooked-mode reader and can't
            // continue a raw-mode escape sequence.
            out.extend(acc.drain(..).map(|b| vec![b]));
            out.push(chunk);
            continue;
        }
        acc.push(chunk[0]);
        match escape_state(&acc) {
            EscState::Continue => {}
            EscState::Complete => out.push(std::mem::take(&mut acc)),
            EscState::Invalid => out.extend(acc.drain(..).map(|b| vec![b])),
        }
    }
    if !acc.is_empty() {
        if let Ok(mut p) = PENDING_ESCAPE.lock() {
            *p = acc;
        }
    }
    out
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
        if let Ok(mut p) = PENDING_ESCAPE.lock() {
            p.clear();
        }
        Vec::new()
    } else if STDIN_PAUSED.load(Ordering::Acquire) {
        // Paused: leave the queue AND any held escape prefix untouched so
        // nothing is delivered (or timed out) until resume.
        Vec::new()
    } else {
        let raw = {
            let mut q = match PENDING_DATA.lock() {
                Ok(g) => g,
                Err(_) => return fired,
            };
            std::mem::take(&mut *q)
        };
        coalesce_escape_sequences(raw)
    };
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
    // 'readable' fires only when this tick actually delivered new bytes —
    // plus once at EOF so a pull-mode consumer gets its final wake-up (its
    // `read()` then returns null). An unconditional per-tick loop here was a
    // JS busy loop: one registered listener meant a callback invocation on
    // every event-loop iteration forever, and the non-zero `fired` return
    // kept the loop hot.
    let readable_eof_due = EOF_REACHED.load(Ordering::Acquire)
        && !READABLE_EOF_NOTIFIED.load(Ordering::Acquire)
        && !STDIN_DESTROYED.load(Ordering::Acquire);
    if !chunks.is_empty() || readable_eof_due {
        let readable_callbacks = READABLE_CALLBACKS
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();
        if !readable_callbacks.is_empty() {
            if chunks.is_empty() {
                READABLE_EOF_NOTIFIED.store(true, Ordering::Release);
            }
            for cb_i64 in &readable_callbacks {
                let closure = *cb_i64 as *const ClosureHeader;
                js_closure_call0(closure);
                fired += 1;
            }
        }
    }

    // 'data' receives the raw bytes as a string; 'keypress' receives
    // (sequence_string, key_object). Listener lists are cloned once per
    // drain (not once per chunk) and each chunk is parsed once, not once
    // per callback.
    let data_callbacks = DATA_CALLBACKS.lock().map(|v| v.clone()).unwrap_or_default();
    let keypress_callbacks = KEYPRESS_CALLBACKS
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    for chunk in chunks {
        for cb_i64 in &data_callbacks {
            let arg = stdin_chunk_value(&chunk);
            let closure = *cb_i64 as *const ClosureHeader;
            js_closure_call1(closure, arg);
            fired += 1;
        }
        if keypress_callbacks.is_empty() {
            continue;
        }
        if let Some((name, ctrl, shift, meta, seq)) = parse_keypress(&chunk) {
            for cb_i64 in &keypress_callbacks {
                // Root the sequence string across build_keypress_object's
                // allocations (a moving minor GC there would leave arg1
                // pointing at from-space).
                let scope = perry_runtime::gc::RuntimeHandleScope::new();
                let seq_str = js_string_from_bytes(seq.as_ptr(), seq.len() as u32);
                let arg1 =
                    scope.root_nanbox_f64(f64::from_bits(JSValue::string_ptr(seq_str).bits()));
                let arg2 = build_keypress_object(&name, ctrl, shift, meta, &seq);
                let closure = *cb_i64 as *const ClosureHeader;
                js_closure_call2(closure, arg1.get_nanbox_f64(), arg2);
                fired += 1;
            }
        }
    }

    // Drain line-mode lines → question (one-shot) or 'line' callback.
    // A paused stdin holds queued lines back, mirroring Node where
    // `rl.pause()` stops 'line' delivery until resume.
    let lines: Vec<String> = if STDIN_PAUSED.load(Ordering::Acquire) {
        Vec::new()
    } else {
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
    // A held escape prefix counts as pending data: the loop must tick once
    // more so the accumulator can flush it as a bare Escape keypress.
    let has_data = PENDING_DATA.lock().map(|q| !q.is_empty()).unwrap_or(false)
        || PENDING_ESCAPE
            .lock()
            .map(|p| !p.is_empty())
            .unwrap_or(false);
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
