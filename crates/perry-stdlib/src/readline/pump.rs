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

fn arm_escape_timeout() {
    if let Ok(mut deadline) = PENDING_ESCAPE_DEADLINE.lock() {
        *deadline = Some(Instant::now() + ESCAPE_CODE_TIMEOUT);
    }
}

fn cancel_escape_timeout() {
    if let Ok(mut deadline) = PENDING_ESCAPE_DEADLINE.lock() {
        *deadline = None;
    }
}

fn escape_timeout_expired() -> bool {
    let Ok(mut deadline) = PENDING_ESCAPE_DEADLINE.lock() else {
        return false;
    };
    if deadline.is_some_and(|at| at <= Instant::now()) {
        *deadline = None;
        true
    } else {
        false
    }
}

/// Deadline provider registered with perry-runtime's event pump. Returning the
/// ceiling avoids truncating a sub-millisecond remainder to zero and spinning
/// before the timeout is actually due.
pub(crate) extern "C" fn js_readline_next_wake_ms() -> f64 {
    if STDIN_DESTROYED.load(Ordering::Acquire) || STDIN_PAUSED.load(Ordering::Acquire) {
        return -1.0;
    }
    let Ok(deadline) = PENDING_ESCAPE_DEADLINE.lock() else {
        return -1.0;
    };
    let Some(deadline) = *deadline else {
        return -1.0;
    };
    let now = Instant::now();
    if deadline <= now {
        0.0
    } else {
        deadline.duration_since(now).as_millis().saturating_add(1) as f64
    }
}

/// Reassemble ANSI escape sequences that the raw-mode reader queues as
/// individual 1-byte chunks (`\x1b`, `[`, `A` → one `\x1b[A` chunk) so a
/// single arrow key fires a single `'keypress'`/`'data'` event, matching a
/// terminal's one-write delivery. A sequence still incomplete at the end of
/// a drain batch is carried in [`PENDING_ESCAPE`] and arms a one-shot
/// `escapeCodeTimeout`. Bytes that complete the sequence cancel that deadline;
/// expiry flushes the held bytes as-is. Event-loop ticks from unrelated work do
/// not advance this state (#9593).
fn coalesce_escape_sequences(raw: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut acc: Vec<u8> = PENDING_ESCAPE
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default();
    if raw.is_empty() {
        if acc.is_empty() {
            cancel_escape_timeout();
            return Vec::new();
        }
        if escape_timeout_expired() {
            return acc.into_iter().map(|b| vec![b]).collect();
        }
        // A timer, I/O source, or stale notify can produce arbitrarily many
        // pump turns before the deadline. Put the prefix back unchanged.
        if let Ok(mut p) = PENDING_ESCAPE.lock() {
            *p = acc;
        }
        return Vec::new();
    }
    // Fresh bytes either complete/invalidate the held prefix or replace it
    // with a new incomplete one below. In every case the old one-shot is done.
    if !acc.is_empty() {
        cancel_escape_timeout();
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
            arm_escape_timeout();
        }
    }
    out
}

/// Classify reader blocks after synchronous JS for this event-loop turn has
/// settled the stream mode. This keeps physical I/O single-owned without
/// exposing producer-thread scheduling as observable listener behavior.
fn route_pending_stdin_input() {
    if STDIN_DESTROYED.load(Ordering::Acquire) {
        if let Ok(mut pending) = PENDING_INPUT.lock() {
            pending.clear();
        }
        if let Ok(mut line) = PENDING_LINE_BYTES.lock() {
            line.clear();
        }
        return;
    }

    let blocks = PENDING_INPUT
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default();
    let mut line_buf = PENDING_LINE_BYTES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut raw_chunks: Vec<Vec<u8>> = Vec::new();

    for block in blocks {
        for byte in block {
            if RAW_MODE.load(Ordering::Acquire) {
                // Preserve byte-sized raw chunks so escape sequences can be
                // reassembled across reads by `coalesce_escape_sequences`.
                raw_chunks.push(vec![byte]);
            } else if STDIN_DATA_FLOWING.load(Ordering::Acquire)
                || STDIN_PULL_MODE.load(Ordering::Acquire)
            {
                line_buf.push(byte);
            } else if byte == b'\n' {
                if line_buf.last() == Some(&b'\r') {
                    line_buf.pop();
                }
                let line = String::from_utf8_lossy(&line_buf).into_owned();
                line_buf.clear();
                if let Ok(mut queue) = PENDING_LINES.lock() {
                    queue.push(line);
                }
            } else {
                line_buf.push(byte);
            }
        }

        if !raw_chunks.is_empty() {
            if let Ok(mut queue) = PENDING_DATA.lock() {
                queue.append(&mut raw_chunks);
            }
        }
        // One cooked `'data'` chunk per physical read, preserving #9489's Node
        // chunking while leaving line splitting to the readline consumer.
        if !line_buf.is_empty()
            && !RAW_MODE.load(Ordering::Acquire)
            && (STDIN_DATA_FLOWING.load(Ordering::Acquire)
                || STDIN_PULL_MODE.load(Ordering::Acquire))
        {
            if let Ok(mut queue) = PENDING_DATA.lock() {
                queue.push(std::mem::take(&mut *line_buf));
            }
            *line_buf = Vec::with_capacity(65536);
        }
    }

    // EOF is published after the reader has enqueued its final block. The
    // producer may publish it while this pump is still classifying an earlier
    // snapshot, so flush only when no later block remains in PENDING_INPUT.
    if stdin_eof_input_drained() {
        if !line_buf.is_empty() {
            if (STDIN_DATA_FLOWING.load(Ordering::Acquire)
                || STDIN_PULL_MODE.load(Ordering::Acquire))
                && !RAW_MODE.load(Ordering::Acquire)
            {
                if let Ok(mut queue) = PENDING_DATA.lock() {
                    queue.push(std::mem::take(&mut *line_buf));
                }
            } else if !RAW_MODE.load(Ordering::Acquire) {
                if line_buf.last() == Some(&b'\r') {
                    line_buf.pop();
                }
                let line = String::from_utf8_lossy(&line_buf).into_owned();
                line_buf.clear();
                if let Ok(mut queue) = PENDING_LINES.lock() {
                    queue.push(line);
                }
            }
        }
    }
}

/// Whether physical EOF has been observed and every preceding reader block
/// has left the handoff queue. The reader enqueues a block before publishing
/// EOF, so the acquire load followed by the queue lock closes the race where a
/// pump took an earlier snapshot just before the final blocks arrived.
fn stdin_eof_input_drained() -> bool {
    EOF_REACHED.load(Ordering::Acquire)
        && PENDING_INPUT
            .lock()
            .map(|pending| pending.is_empty())
            .unwrap_or(false)
}

/// Drain pending lines and byte chunks, dispatching to registered
/// callbacks. Called from the async-bridge tick on every event-loop
/// iteration. Returns the number of callbacks fired.
#[no_mangle]
pub extern "C" fn js_readline_process_pending() -> i32 {
    let mut fired: i32 = 0;
    route_pending_stdin_input();

    // Drain raw- or cooked-mode byte chunks → 'data' / 'keypress' callbacks.
    let chunks: Vec<Vec<u8>> = if STDIN_DESTROYED.load(Ordering::Acquire) {
        if let Ok(mut q) = PENDING_DATA.lock() {
            q.clear();
        }
        if let Ok(mut p) = PENDING_ESCAPE.lock() {
            p.clear();
        }
        cancel_escape_timeout();
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
    let readable_eof_due = stdin_eof_input_drained()
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
            // A callback may allocate and move every later closure in this
            // cloned list. The registry scanner rewrites the original list,
            // not this snapshot, so keep the snapshot in mutable handles and
            // reload each pointer immediately before dispatch.
            let callback_scope = perry_runtime::gc::RuntimeHandleScope::new();
            let callback_handles: Vec<_> = readable_callbacks
                .iter()
                .map(|cb| callback_scope.root_raw_const_ptr(*cb as *const ClosureHeader))
                .collect();
            for callback in callback_handles {
                let closure = callback.get_raw_const_ptr::<ClosureHeader>();
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
    let keypress_callbacks = if cfg!(test) || perry_runtime::os::stdin_keypress_events_enabled() {
        KEYPRESS_CALLBACKS
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    // `stdin_chunk_value`, the sequence string and key-object construction
    // all allocate. Root these cloned callback snapshots because the mutable
    // registry scanner can only rewrite the original listener lists.
    let callback_scope = perry_runtime::gc::RuntimeHandleScope::new();
    let data_callback_handles: Vec<_> = data_callbacks
        .iter()
        .map(|cb| callback_scope.root_raw_const_ptr(*cb as *const ClosureHeader))
        .collect();
    let keypress_callback_handles: Vec<_> = keypress_callbacks
        .iter()
        .map(|cb| callback_scope.root_raw_const_ptr(*cb as *const ClosureHeader))
        .collect();
    for chunk in chunks {
        // #9490: decode ONCE per chunk, above the listener loop. The UTF-8
        // decoder is stateful; decoding per listener fed the same bytes
        // through it once per registered callback. `None` = the chunk was
        // absorbed whole into a held partial (a code point split across this
        // read boundary), for which Node emits no `'data'` event.
        let data_arg = if data_callback_handles.is_empty() {
            None
        } else {
            stdin_chunk_value(&chunk)
        };
        if let Some(arg) = data_arg {
            // Root the decoded value: a GC inside one listener can move the
            // string the remaining listeners still have to receive.
            let arg_scope = perry_runtime::gc::RuntimeHandleScope::new();
            let arg_handle = arg_scope.root_nanbox_f64(arg);
            for callback in &data_callback_handles {
                let closure = callback.get_raw_const_ptr::<ClosureHeader>();
                js_closure_call1(closure, arg_handle.get_nanbox_f64());
                fired += 1;
            }
        }
        for key_chunk in perry_runtime::readline_helpers::split_keypress_chunks(&chunk) {
            let Some((name, ctrl, shift, meta, seq)) = parse_keypress(&key_chunk) else {
                continue;
            };
            for callback in &keypress_callback_handles {
                // Root the sequence string across build_keypress_object's
                // allocations (a moving minor GC there would leave arg1
                // pointing at from-space).
                let scope = perry_runtime::gc::RuntimeHandleScope::new();
                let seq_str = js_string_from_bytes(seq.as_ptr(), seq.len() as u32);
                let arg1 =
                    scope.root_nanbox_f64(f64::from_bits(JSValue::string_ptr(seq_str).bits()));
                let arg2 = build_keypress_object(&name, ctrl, shift, meta, &seq);
                let closure = callback.get_raw_const_ptr::<ClosureHeader>();
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

    // Physical EOF closes an active readline interface and independently
    // emits process.stdin's end/close events. `rl.close()` may already have
    // fired the first event without stdin actually ending, so the two
    // one-shot states must not be conflated.
    if stdin_eof_input_drained() {
        let readline_close_already = CLOSE_FIRED.with(|f| {
            let was = *f.borrow();
            *f.borrow_mut() = true;
            was
        });
        let stdin_end_already = STDIN_END_FIRED.swap(true, Ordering::AcqRel);
        if !stdin_end_already {
            // #9490: flush the stream decoder first — a sequence left
            // incomplete at EOF is one final `'data'` chunk of U+FFFD, ahead
            // of `'end'`/`'close'`.
            // Only when a `data` listener exists to receive it: in pull mode
            // the flush belongs to the consumer's last `read()`, and taking
            // it here would consume the state and drop the replacement.
            let flush_targets = DATA_CALLBACKS.lock().map(|v| v.clone()).unwrap_or_default();
            if let Some(flushed) = if flush_targets.is_empty() {
                None
            } else {
                perry_runtime::os::stdin_encoding_flush_jsvalue()
            } {
                let flush_scope = perry_runtime::gc::RuntimeHandleScope::new();
                let flush_handles: Vec<_> = flush_targets
                    .iter()
                    .map(|cb| flush_scope.root_raw_const_ptr(*cb as *const ClosureHeader))
                    .collect();
                // Root the flushed string too, and re-read it per call: a GC
                // inside one listener can move it out from under the next.
                let arg_handle = flush_scope.root_nanbox_f64(flushed);
                for callback in &flush_handles {
                    let closure = callback.get_raw_const_ptr::<ClosureHeader>();
                    js_closure_call1(closure, arg_handle.get_nanbox_f64());
                    fired += 1;
                }
            }
        }
        if !readline_close_already {
            let cb = CLOSE_CALLBACK.with(|c| c.borrow_mut().take());
            if let Some(cb_i64) = cb {
                let closure = cb_i64 as *const ClosureHeader;
                js_closure_call0(closure);
                fired += 1;
            }
        }
        if !stdin_end_already {
            // Every `process.stdin.on("end" | "close", …)` listener, in
            // registration order. Node fires all of them; the previous
            // single-slot storage kept only the last one registered.
            let end_cbs: Vec<i64> = STDIN_END_CALLBACKS
                .lock()
                .map(|mut v| std::mem::take(&mut *v))
                .unwrap_or_default();
            for cb_i64 in end_cbs {
                js_closure_call0(cb_i64 as *const ClosureHeader);
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
    let has_input = PENDING_INPUT.lock().map(|q| !q.is_empty()).unwrap_or(false);
    // A held escape prefix counts as pending data: the loop must stay alive
    // until its explicit escapeCodeTimeout deadline can flush it.
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
    let has_readline_close_cb =
        !CLOSE_FIRED.with(|f| *f.borrow()) && CLOSE_CALLBACK.with(|c| c.borrow().is_some());
    let has_stdin_end_cb = !STDIN_END_FIRED.load(Ordering::Acquire)
        && STDIN_END_CALLBACKS
            .lock()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
    let has_close_cb = has_readline_close_cb || has_stdin_end_cb;
    let has_dispatchable_lines = has_lines && !paused;
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
        && ((has_input && !paused)
            || has_dispatchable_lines
            || has_dispatchable_data
            || has_close_cb
            || reader_keeps_alive)
    {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::parse_keypress;

    #[test]
    fn eof_waits_for_every_reader_handoff_block() {
        let _guard = super::super::test_support::reset();
        super::PENDING_INPUT
            .lock()
            .unwrap()
            .push(b"still pending".to_vec());
        super::EOF_REACHED.store(true, std::sync::atomic::Ordering::Release);

        assert!(!super::stdin_eof_input_drained());
        super::PENDING_INPUT.lock().unwrap().clear();
        assert!(super::stdin_eof_input_drained());

        super::EOF_REACHED.store(false, std::sync::atomic::Ordering::Release);
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
}
