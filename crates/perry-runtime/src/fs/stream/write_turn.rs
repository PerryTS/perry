//! The `WriteStream` turn state machine — one parked callback-timer turn per
//! stream, deciding open/drain/close from the current state each time it
//! runs (#9493).
//!
//! Split out of `stream.rs` only to keep that file under the repository's
//! 2000-line cap (`scripts/check_file_size.sh`); same subject as before, just
//! its own file (mirrors how `utf8_stream.rs`/`stream_errors.rs`/etc. were
//! already split out of this same file).

use super::*;

/// #9493: park one `WriteStream` turn on the callback-timer queue. At most one
/// is pending per stream; a turn re-schedules while work remains.
///
/// This is the mechanism `fs::deferred` uses for `fs.writeFile`: the timer
/// queue roots the closure; a pending refed callback timer is a live event
/// source, so a program that ends by draining its loop still lands every byte
/// and `'finish'` still fires; and `process.exit()` terminates through
/// `libc::_exit` without ticking the queue, so an exit in the same tick
/// abandons the parked open and writes the way Node abandons its not-yet-run
/// thread-pool requests.
pub(super) fn schedule_write_stream_turn(id: usize) {
    let should_schedule = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return false;
        };
        if state.turn_pending || state.closed {
            return false;
        }
        state.turn_pending = true;
        true
    });
    if should_schedule {
        let closure = js_closure_alloc(
            crate::fn_info!(write_stream_turn_impl, 0; with_declared(0)),
            1,
        );
        js_closure_set_capture_ptr(closure, 0, id as i64);
        let _ = crate::timer::js_set_timeout_callback(closure as i64, 0.0);
    }
}

pub(super) extern "C" fn write_stream_turn_impl(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let id = stream_id_of(closure);
    STREAM_REGISTRY.with(|registry| {
        if let Some(state) = registry.borrow_mut().get_mut(&id) {
            state.turn_pending = false;
        }
    });
    run_write_stream_turn(id);
    undefined_value()
}

/// What a `WriteStream` turn does, decided from the state when it runs. One
/// step per turn, each the analogue of one Node thread-pool request: the open
/// (`_construct` → `fs.open`), then the queued writes ending in `'finish'`,
/// then the close. A step schedules the next when more work remains, so a
/// microtask queued by an `'open'` or `'finish'` listener runs before the
/// first write callback / before `'close'`, as it does in Node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteStreamStep {
    Open,
    Drain,
    Close,
    Idle,
}

fn write_stream_step(id: usize) -> WriteStreamStep {
    STREAM_REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let Some(state) = registry.get(&id) else {
            return WriteStreamStep::Idle;
        };
        if state.kind != StreamKind::Write || state.closed {
            return WriteStreamStep::Idle;
        }
        if state.destroyed {
            return WriteStreamStep::Close;
        }
        if !state.opened && state.error_msg.is_none() && matches!(state.owner, FdOwner::Path) {
            return WriteStreamStep::Open;
        }
        if !state.pending_writes.is_empty() || (state.ended && !state.finished) {
            return WriteStreamStep::Drain;
        }
        if state.finished && state.auto_close {
            return WriteStreamStep::Close;
        }
        WriteStreamStep::Idle
    })
}

fn run_write_stream_turn(id: usize) {
    match write_stream_step(id) {
        WriteStreamStep::Open => write_stream_open_step(id),
        WriteStreamStep::Drain => write_stream_drain_step(id),
        WriteStreamStep::Close => write_stream_close_step(id),
        WriteStreamStep::Idle => {}
    }
}

pub(super) fn schedule_next_write_stream_step(id: usize) {
    if write_stream_step(id) != WriteStreamStep::Idle {
        schedule_write_stream_turn(id);
    }
}

/// The deferred `open(2)`: Node's `_construct` runs `fs.open` on the pool and
/// then emits `'open'` and `'ready'`. The queued writes are performed on a
/// LATER turn — their `fs.write` requests are only dispatched once the open
/// callback has returned.
fn write_stream_open_step(id: usize) {
    let (path, flags) = STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| (state.path.clone(), state.flags.clone()))
            .unwrap_or_default()
    });
    match fs_open_path_str_result(&path, &flags) {
        Ok(fd) => {
            STREAM_REGISTRY.with(|registry| {
                if let Some(state) = registry.borrow_mut().get_mut(&id) {
                    state.fd = Some(fd);
                    state.opened = true;
                    if matches!(state.flags.as_str(), "a" | "a+" | "ax" | "ax+") {
                        state.position = end_position_for_fd(fd);
                    }
                    update_common_props(state);
                }
            });
            emit_event1(id, "open", fd as f64);
            emit_event0(id, "ready");
            schedule_next_write_stream_step(id);
        }
        Err(err) => {
            let message = err.to_string();
            let error_value = unsafe { build_fs_error_value(&err, "open", &path) };
            write_stream_fail(id, error_value, message);
        }
    }
}

/// The error cascade Node runs when the open fails: every pending write
/// callback and the `end()` callback receive the error, then `'error'` fires,
/// then the stream is destroyed and `'close'` follows on a later turn.
fn write_stream_fail(id: usize, error_value: f64, message: String) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error_handle = scope.root_nanbox_f64(error_value);
    // The value goes into the state first — the registry is a GC root and the
    // callbacks below allocate.
    let (writes, end_callback) = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (Vec::new(), undefined_value());
        };
        state.errored = true;
        state.error_msg = Some(message);
        state.error_value = error_handle.get_nanbox_f64();
        state.destroyed = true;
        state.writable_length = 0;
        state.writable_need_drain = false;
        let writes = std::mem::take(&mut state.pending_writes);
        let end_callback = std::mem::replace(&mut state.end_callback, undefined_value());
        update_common_props(state);
        (writes, end_callback)
    });
    let callbacks: Vec<_> = writes
        .iter()
        .map(|write| scope.root_nanbox_f64(write.callback))
        .collect();
    let end_handle = scope.root_nanbox_f64(end_callback);
    for callback in &callbacks {
        call_stream_callback1(callback.get_nanbox_f64(), error_handle.get_nanbox_f64());
    }
    call_stream_callback1(end_handle.get_nanbox_f64(), error_handle.get_nanbox_f64());
    emit_event1(id, "error", error_handle.get_nanbox_f64());
    schedule_next_write_stream_step(id);
}

/// The queued writes, in order — Node batches them into one `writev` — then
/// Node's `afterWrite` order: `'drain'` (when a `write()` returned `false`
/// and the stream is not ending) BEFORE the completed writes' callbacks. Once
/// `end()` has been called and nothing is left: the `end()` callback and
/// `'finish'`; with `autoClose`, the close lands on the next turn.
fn write_stream_drain_step(id: usize) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut completed = Vec::new();
    loop {
        let next = STREAM_REGISTRY.with(|registry| {
            let mut registry = registry.borrow_mut();
            let state = registry.get_mut(&id)?;
            if state.destroyed || state.pending_writes.is_empty() {
                return None;
            }
            Some(state.pending_writes.remove(0))
        });
        let Some(write) = next else {
            break;
        };
        let callback = scope.root_nanbox_f64(write.callback);
        match write_to_stream_fd(id, &write.bytes) {
            Ok(()) => completed.push(callback),
            Err(message) => {
                // The writes that did land complete normally; the failing one
                // gets the error, then the rest of the queue does via the
                // error cascade.
                for done in &completed {
                    call_stream_callback0(done.get_nanbox_f64());
                }
                let error_value = scope.root_nanbox_f64(make_error_value(&message));
                call_stream_callback1(callback.get_nanbox_f64(), error_value.get_nanbox_f64());
                write_stream_fail(id, error_value.get_nanbox_f64(), message);
                return;
            }
        }
    }
    let (emit_drain, finish) = STREAM_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let Some(state) = registry.get_mut(&id) else {
            return (false, false);
        };
        if state.destroyed {
            return (false, false);
        }
        state.writable_length = 0;
        let emit_drain = state.writable_need_drain && !state.ended;
        state.writable_need_drain = false;
        let mut finish = false;
        if state.ended && !state.finished {
            if state.error_msg.is_none() {
                state.finished = true;
                finish = true;
            } else {
                state.destroyed = true;
            }
        }
        update_common_props(state);
        (emit_drain, finish)
    });
    if emit_drain {
        emit_event0(id, "drain");
    }
    for callback in &completed {
        call_stream_callback0(callback.get_nanbox_f64());
    }
    if finish {
        let end_callback = STREAM_REGISTRY.with(|registry| {
            registry
                .borrow_mut()
                .get_mut(&id)
                .map(|state| std::mem::replace(&mut state.end_callback, undefined_value()))
                .unwrap_or_else(undefined_value)
        });
        let scope = crate::gc::RuntimeHandleScope::new();
        let end_callback = scope.root_nanbox_f64(end_callback);
        call_stream_callback0(end_callback.get_nanbox_f64());
        emit_event0(id, "finish");
    }
    schedule_next_write_stream_step(id);
}

fn write_stream_close_step(id: usize) {
    let force = STREAM_REGISTRY.with(|registry| {
        registry
            .borrow()
            .get(&id)
            .map(|state| state.destroyed)
            .unwrap_or(false)
    });
    maybe_close_stream(id, force);
}
