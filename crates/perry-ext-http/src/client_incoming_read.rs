//! Readable surface of the client `IncomingMessage` (`res` in `http.get`
//! callbacks): paused-mode `read()` / `'readable'`, real `pause()`/`resume()`,
//! `destroy()`, `removeListener`, and the `readable`/`readableEnded`/`closed`/
//! `destroyed` state.
//!
//! Body bytes live in `IncomingMessageHandle::body` until a consumer takes
//! them. A `'readable'` listener switches the message to paused mode (chunks
//! stay buffered and `'readable'` fires per arrival and once at EOF); `'data'`
//! listeners, pipes and `resume()` flow chunks as they arrive; `'end'` follows
//! the last byte leaving the buffer. Edges Node emits on a later tick (a late
//! listener, `resume()`, a draining `read()`) ride one queued
//! [`PendingHttpEvent::IncomingDrain`].

use super::*;

/// Per-message Readable state, kept next to the body buffer.
#[derive(Default)]
pub struct ReadState {
    /// A `'readable'` listener is attached: chunks stay buffered for `read()`.
    pub readable_listening: bool,
    /// `pause()` was called: `'data'`/pipe delivery is held back.
    pub paused: bool,
    /// `resume()` was called: chunks flow (and are discarded without a consumer).
    pub resumed: bool,
    /// `read()` was called, so draining the buffer at EOF ends the stream.
    pub read_called: bool,
    /// An [`PendingHttpEvent::IncomingDrain`] is already queued.
    pub drain_queued: bool,
    /// The whole body has been received.
    pub eof_seen: bool,
    /// `'readable'` already fired for the current buffer contents.
    pub signaled: bool,
    /// `'end'` was emitted (`readableEnded`).
    pub ended: bool,
    pub closed: bool,
    pub destroyed: bool,
    /// `destroy()` ran and its `'error'`/`'close'` edges are still queued.
    pub close_pending: bool,
    /// NaN-boxed error handed to `destroy(err)`; 0 when there is none.
    pub pending_error: u64,
    /// `(wrapper, original)` for `once()` listeners so `removeListener(original)`
    /// finds the wrapper that was registered.
    pub once_targets: Vec<(i64, i64)>,
}

fn null_value() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

fn self_value(handle: Handle) -> f64 {
    f64::from_bits(perry_ffi::JsValue::from_object_ptr(handle as usize as *mut u8).bits())
}

fn has_listener(im: &IncomingMessageHandle, event: &str) -> bool {
    im.listeners.get(event).is_some_and(|l| !l.is_empty())
}

fn has_consumer(im: &IncomingMessageHandle) -> bool {
    has_listener(im, "data") || !im.pipes.is_empty()
}

/// Chunks flow to `'data'` listeners and pipes (or are discarded after
/// `resume()`) unless paused mode or `pause()` holds them back.
fn flows(im: &IncomingMessageHandle) -> bool {
    !im.read.readable_listening && !im.read.paused && (im.read.resumed || has_consumer(im))
}

fn queue_drain(incoming: Handle) {
    let Some((owner, request_handle)) =
        with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
            if im.read.drain_queued {
                None
            } else {
                im.read.drain_queued = true;
                Some((im.owner_agent, im.request_handle))
            }
        })
        .flatten()
    else {
        return;
    };
    if let Ok(mut queue) = HTTP_PENDING_EVENTS.lock() {
        queue.push((
            owner,
            PendingHttpEvent::IncomingDrain {
                request_handle,
                incoming,
            },
        ));
    }
    notify_main_thread();
}

/// A listener was registered on a client `IncomingMessage`.
pub(crate) fn on_listener_added(incoming: Handle, event: &str) {
    let queue = with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| match event {
        "readable" => {
            im.read.readable_listening = true;
            (!im.body.is_empty() && !im.read.signaled) || (im.read.eof_seen && !im.read.ended)
        }
        "data" => {
            !im.read.readable_listening
                && !im.read.paused
                && ((!im.body.is_empty()) || (im.read.eof_seen && !im.read.ended))
        }
        _ => false,
    })
    .unwrap_or(false);
    if queue {
        queue_drain(incoming);
    }
}

/// `res.pipe(dest)` registered a destination.
pub(crate) fn on_pipe_added(incoming: Handle) {
    on_listener_added(incoming, "data");
}

pub(crate) fn pause(handle: Handle) -> f64 {
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |im| {
        im.read.paused = true;
    });
    self_value(handle)
}

pub(crate) fn resume(handle: Handle) -> f64 {
    let known = with_handle_mut::<IncomingMessageHandle, _, _>(handle, |im| {
        im.read.paused = false;
        im.read.resumed = true;
    })
    .is_some();
    if known {
        queue_drain(handle);
    }
    self_value(handle)
}

/// A streamed chunk arrived. Buffers it and returns `None` when the message is
/// paused or has no consumer; otherwise returns everything to deliver now
/// (earlier buffered bytes first, so order is kept).
pub(crate) unsafe fn accept_chunk(incoming: Handle, chunk: &[u8]) -> Option<Vec<u8>> {
    let (deliver, fire_readable) = with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
        if flows(im) {
            let mut bytes = std::mem::take(&mut im.body);
            bytes.extend_from_slice(chunk);
            (Some(bytes), false)
        } else {
            im.body.extend_from_slice(chunk);
            im.read.signaled = false;
            (None, im.read.readable_listening)
        }
    })
    .unwrap_or((None, false));
    if fire_readable {
        signal_readable(incoming);
    }
    deliver
}

unsafe fn signal_readable(incoming: Handle) {
    with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| im.read.signaled = true);
    crate::client_events::fire_incoming_event(incoming, "readable", None);
}

/// Deliver `bytes` to `'data'` listeners and pipes. `end` flushes the decoder.
pub(crate) unsafe fn deliver_to_consumers(incoming: Handle, bytes: &[u8], end: bool) {
    let Some((data_listeners, encoding, pipes)) = get_handle_mut::<IncomingMessageHandle>(incoming)
        .map(|r| {
            (
                r.listeners.get("data").cloned().unwrap_or_default(),
                r.encoding.clone(),
                r.pipes.clone(),
            )
        })
    else {
        return;
    };
    if data_listeners.is_empty() && pipes.is_empty() {
        return;
    }
    let scope = perry_ffi::TransientRootScope::enter();
    let data_listeners = scope.root_addrs(&data_listeners);
    let pipes = pipes
        .iter()
        .map(|bits| scope.root_nanbox(f64::from_bits(*bits)))
        .collect::<Vec<_>>();
    let arg = get_handle_mut::<IncomingMessageHandle>(incoming).and_then(|response| {
        crate::client_surface::streaming_body_chunk_value(
            bytes,
            encoding.as_deref(),
            &mut response.decoder_pending,
            end,
        )
    });
    let Some(arg) = arg else {
        return;
    };
    let arg = scope.root_nanbox(arg);
    for cb in data_listeners {
        if cb.get() != 0 {
            let closure = JsClosure::from_raw(cb.get() as *const RawClosureHeader);
            let _ = closure.call1(perry_ffi::JsThis::UNDEFINED, arg.get());
        }
    }
    for dest in pipes {
        crate::client_events::forward_pipe_write_value(dest.get(), arg.get());
    }
}

/// Emit `'end'`, end pipe destinations, then auto-destroy with `'close'`.
unsafe fn emit_end(incoming: Handle) {
    let first = with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
        if im.read.ended || im.read.destroyed {
            false
        } else {
            im.read.ended = true;
            true
        }
    })
    .unwrap_or(false);
    if !first {
        return;
    }
    crate::client_events::fire_incoming_event(incoming, "end", None);
    let pipes = get_handle_mut::<IncomingMessageHandle>(incoming)
        .map(|r| r.pipes.clone())
        .unwrap_or_default();
    let scope = perry_ffi::TransientRootScope::enter();
    let pipes = pipes
        .iter()
        .map(|bits| scope.root_nanbox(f64::from_bits(*bits)))
        .collect::<Vec<_>>();
    for dest in pipes {
        crate::client_events::forward_pipe_end(dest.get().to_bits());
    }
    with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
        im.read.destroyed = true;
        im.read.closed = true;
    });
    crate::client_events::fire_incoming_event(incoming, "close", None);
}

/// Advance the message: fire pending `'readable'`, flow buffered bytes to
/// consumers, and emit `'end'` once the buffer is drained at EOF.
unsafe fn advance(incoming: Handle) {
    let Some((readable, flowing, body)) =
        with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
            if im.read.destroyed || im.read.ended {
                return None;
            }
            if im.read.readable_listening {
                let fire = !im.body.is_empty() && !im.read.signaled;
                return Some((fire, false, Vec::new()));
            }
            if flows(im) {
                return Some((false, true, std::mem::take(&mut im.body)));
            }
            Some((false, false, Vec::new()))
        })
        .flatten()
    else {
        return;
    };
    if readable {
        signal_readable(incoming);
    }
    let eof = with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| im.read.eof_seen)
        .unwrap_or(false);
    if flowing {
        deliver_to_consumers(incoming, &body, eof);
    }
    let finish = with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
        im.read.eof_seen
            && im.body.is_empty()
            && !im.read.ended
            && !im.read.destroyed
            && (im.read.readable_listening
                || im.read.read_called
                || (!im.read.paused && (im.read.resumed || has_consumer(im))))
    })
    .unwrap_or(false);
    if finish {
        emit_end(incoming);
    }
}

/// The whole body has been received (streaming end edge, or the buffered
/// single-event response after the user callbacks ran).
pub(crate) unsafe fn on_body_complete(incoming: Handle) {
    let (readable, end_now) = with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
        im.read.eof_seen = true;
        im.complete = true;
        // EOF is itself a `'readable'` edge, even with an empty buffer.
        im.read.signaled = false;
        // `'end'` listeners with no consumer still fire.
        let idle_end =
            !im.read.readable_listening && !im.read.paused && !flows(im) && has_listener(im, "end");
        (im.read.readable_listening && !im.read.destroyed, idle_end)
    })
    .unwrap_or((false, false));
    if readable {
        signal_readable(incoming);
        // Node's `read(0)` after the EOF `'readable'` lets an unread, empty
        // stream end.
        queue_drain(incoming);
    } else if end_now {
        emit_end(incoming);
    } else {
        advance(incoming);
    }
}

pub(crate) unsafe fn handle_drain(incoming: Handle) {
    let close = with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
        im.read.drain_queued = false;
        std::mem::take(&mut im.read.close_pending)
            .then(|| std::mem::take(&mut im.read.pending_error))
    })
    .flatten();
    let Some(error) = close else {
        advance(incoming);
        return;
    };
    if error != 0 {
        crate::client_events::fire_incoming_event(incoming, "error", Some(f64::from_bits(error)));
    }
    with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| im.read.closed = true);
    crate::client_events::fire_incoming_event(incoming, "close", None);
}

/// `res.read([size])`: a Buffer (a string after `setEncoding`), or `null` when
/// nothing is available yet.
pub(crate) fn read(handle: Handle, size: Option<usize>) -> f64 {
    let outcome = with_handle_mut::<IncomingMessageHandle, _, _>(handle, |im| {
        im.read.read_called = true;
        if im.read.destroyed {
            return None;
        }
        let available = im.body.len();
        let n = match size {
            None => available,
            Some(n) if n <= available => n,
            Some(_) if im.read.eof_seen => available,
            Some(_) => 0,
        };
        let chunk: Vec<u8> = im.body.drain(..n).collect();
        let drained = im.body.is_empty();
        let finished = drained && im.read.eof_seen && !im.read.ended;
        Some((
            chunk,
            im.encoding.clone(),
            im.read.eof_seen && drained,
            finished,
        ))
    })
    .flatten();
    let Some((chunk, encoding, flush, finished)) = outcome else {
        return null_value();
    };
    if finished {
        queue_drain(handle);
    }
    if chunk.is_empty() {
        return null_value();
    }
    match encoding {
        None => crate::client_surface::body_chunk_value(&chunk, None),
        Some(encoding) => with_handle_mut::<IncomingMessageHandle, _, _>(handle, |im| {
            crate::client_surface::streaming_body_chunk_value(
                &chunk,
                Some(&encoding),
                &mut im.decoder_pending,
                flush,
            )
        })
        .flatten()
        .unwrap_or_else(null_value),
    }
}

/// The transport failed under the message: it is destroyed and closed.
pub(crate) fn mark_destroyed(incoming: Handle) {
    with_handle_mut::<IncomingMessageHandle, _, _>(incoming, |im| {
        im.read.destroyed = true;
        im.read.closed = true;
        im.body.clear();
    });
}

/// `res.destroy([err])`: drop the buffered body, stop the exchange when the
/// body is still streaming, then queue `'error'` (with `err`) and `'close'`.
pub(crate) unsafe fn destroy(handle: Handle, error: Option<f64>) -> f64 {
    let Some((request_handle, complete)) =
        with_handle_mut::<IncomingMessageHandle, _, _>(handle, |im| {
            if im.read.destroyed {
                return None;
            }
            im.read.destroyed = true;
            im.read.close_pending = true;
            im.read.pending_error = error.map_or(0, f64::to_bits);
            im.body.clear();
            Some((im.request_handle, im.complete))
        })
        .flatten()
    else {
        return self_value(handle);
    };
    if !complete && request_handle != 0 {
        // The response already exists, so the request must not report a hang up.
        with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| req.completed = true);
        crate::client_request_surface::js_http_client_request_destroy(
            request_handle,
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    queue_drain(handle);
    self_value(handle)
}

/// `res.removeListener(event, cb)` / `res.off(event, cb)`.
pub(crate) fn remove_listener(handle: Handle, event: &str, callback: i64) -> f64 {
    let removed_readable = with_handle_mut::<IncomingMessageHandle, _, _>(handle, |im| {
        let targets = &im.read.once_targets;
        let Some(listeners) = im.listeners.get_mut(event) else {
            return false;
        };
        let Some(position) = listeners.iter().rposition(|entry| {
            *entry == callback
                || targets
                    .iter()
                    .any(|(wrapper, original)| wrapper == entry && *original == callback)
        }) else {
            return false;
        };
        listeners.remove(position);
        let listening = !listeners.is_empty();
        let live: Vec<i64> = im.listeners.values().flatten().copied().collect();
        im.read
            .once_targets
            .retain(|(wrapper, _)| live.contains(wrapper));
        event == "readable" && !listening
    })
    .unwrap_or(false);
    if removed_readable {
        with_handle_mut::<IncomingMessageHandle, _, _>(handle, |im| {
            im.read.readable_listening = false;
        });
        queue_drain(handle);
    }
    self_value(handle)
}

/// `res.readable`, `res.readableEnded`, `res.closed`, `res.destroyed`.
pub(crate) fn state_property(handle: Handle, name: &str) -> f64 {
    let value = with_handle_mut::<IncomingMessageHandle, _, _>(handle, |im| match name {
        "readable" => !im.read.ended && !im.read.destroyed,
        "readableEnded" => im.read.ended,
        "closed" => im.read.closed,
        _ => im.read.destroyed,
    })
    .unwrap_or(false);
    bool_value(value)
}
