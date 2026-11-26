//! Native producers share the ordinary stream queue, readers, tee and GC scanner.
//! Their pull/cancel hooks contain numeric exchange keys, never heap pointers.
use super::*;
use perry_runtime::native_payload::OwnerLink;
use std::cell::Cell;
use std::rc::Rc;

/// A native producer feeding the stream. Its pending response promise is the
/// stream's NATIVE_PROMISE slot. `token` is shared with the producer, whose
/// sink context points at it.
pub(super) struct NativeSource {
    pull: fn(usize),
    cancel: fn(usize),
    pub(super) token: Rc<SourceToken>,
}

/// What a native producer's sink context names: plain Rust data shared by the
/// stream payload and the producer (one `Rc` count each), so the producer can
/// always ask whether its stream still lives.
///
/// While the producer may deliver (from submit until it pauses at zero
/// credit) the stream object is pinned through its payload link, so a body
/// nobody has read yet stays alive. Paused, it is unpinned: a body the
/// program dropped is collected, `Drop for ReadableStreamData` clears
/// `alive` and counts the death on the producer's `dropped` cell, and the
/// producer aborts the exchange the next time it looks at that count
/// (`Sink::alive` names which one). A pull re-pins before it resumes the
/// producer.
pub(crate) struct SourceToken {
    link: Cell<usize>,
    pub(super) alive: Cell<bool>,
    pinned: Cell<bool>,
    /// The producer's count of consumers that died while it was paused on
    /// them (one cell per producer engine), so the producer looks for a dead
    /// consumer only after one died.
    dropped: Option<Rc<Cell<u32>>>,
}

impl SourceToken {
    /// The stream died (`Drop for ReadableStreamData`): no allocation, no JS,
    /// no thread-locals.
    pub(super) fn stream_died(&self) {
        self.alive.set(false);
        if let Some(dropped) = self.dropped.as_ref() {
            dropped.set(dropped.get().wrapping_add(1));
        }
    }
}

impl SourceToken {
    fn pin(&self) {
        if self.alive.get() && !self.pinned.get() && self.link.get() != 0 {
            // SAFETY: alive => the stream's payload cell lives; creator thread.
            unsafe {
                perry_runtime::native_payload::link_ref(OwnerLink::from_raw(self.link.get()))
            };
            self.pinned.set(true);
        }
    }

    fn unpin(&self) {
        if self.alive.get() && self.pinned.get() {
            // SAFETY: as in `pin`; matches its link_ref.
            unsafe {
                perry_runtime::native_payload::link_unref(OwnerLink::from_raw(self.link.get()))
            };
            self.pinned.set(false);
        }
    }
}

#[inline]
fn ctx_of(token: &Rc<SourceToken>) -> usize {
    Rc::as_ptr(token) as usize
}

/// The token a sink context names.
///
/// # Safety
/// `ctx` came from [`alloc_native_readable`] and [`release_native`] has not
/// run for it.
#[inline]
unsafe fn token<'a>(ctx: usize) -> &'a SourceToken {
    &*(ctx as *const SourceToken)
}

/// The stream a sink context feeds, while it lives.
pub(crate) fn native_stream(ctx: usize) -> Option<RsRef> {
    // SAFETY: the producer holds the context until `release_native`.
    let token = unsafe { token(ctx) };
    if !token.alive.get() {
        return None;
    }
    // SAFETY: alive => the payload cell lives.
    let stream = unsafe { RsRef::from_link(token.link.get()) };
    (stream.0 != 0).then_some(stream)
}

/// `Sink::alive` of a native body producer.
pub(crate) fn native_alive(ctx: usize) -> bool {
    // SAFETY: as in `native_stream`.
    unsafe { token(ctx) }.alive.get()
}

/// The producer is done with its context (after its final callback, or when
/// the engine refused the request): drop its pin and its token count.
pub(crate) fn release_native(ctx: usize) {
    // SAFETY: as in `native_stream`; this is the producer's last use.
    let token = unsafe { Rc::from_raw(ctx as *const SourceToken) };
    token.unpin();
}

/// A whole-body consumer (`text()`, `arrayBuffer()`, ..). Its promise is the
/// stream's BODY_CONSUMER_PROMISE slot.
pub(super) struct BodyConsumer {
    bytes: Vec<u8>,
    metadata: String,
    finish: fn(Vec<u8>, String) -> Result<f64, f64>,
}

pub(super) const BODY_READER: usize = usize::MAX - 1;
pub(crate) const NATIVE_BODY_WINDOW: usize = 64 * 1024;

/// A byte stream fed by a native producer, pinned until the producer
/// pauses; returns the stream and the producer's sink context (release it
/// with [`release_native`]).
pub(crate) fn alloc_native_readable(
    promise: *mut Promise,
    pull: fn(usize),
    cancel: fn(usize),
    dropped: Option<Rc<Cell<u32>>>,
) -> (RsRef, usize) {
    ensure_gc_registered();
    let stream = RsRef(alloc_readable_with_type(
        0,
        0,
        0,
        NATIVE_BODY_WINDOW as f64,
        true,
    ));
    let token = Rc::new(SourceToken {
        link: Cell::new(stream.link()),
        alive: Cell::new(true),
        pinned: Cell::new(false),
        dropped,
    });
    token.pin();
    let ctx = Rc::into_raw(token.clone()) as usize;
    with_rs(stream, |s| {
        s.started = true;
        s.set_native_promise(Some(promise));
        s.native_source = Some(NativeSource {
            pull,
            cancel,
            token,
        });
    })
    .unwrap();
    (stream, ctx)
}

pub(crate) fn take_native_promise(stream: RsRef) -> Option<*mut Promise> {
    let rec = rs_rec(stream)?;
    rec.native_source.as_ref()?;
    let promise = rec.native_promise();
    if promise.is_some() {
        rec.set_native_promise(None);
    }
    promise
}

/// How many more bytes the stream behind sink context `ctx` takes. At zero
/// the producer pauses, so the stream is unpinned (see [`SourceToken`]).
pub(crate) fn native_capacity(ctx: usize) -> usize {
    let Some(stream) = native_stream(ctx) else {
        return 0;
    };
    let credit = rs_rec(stream).as_deref().map_or(0, |s| {
        if s.state != ReadableState::Readable {
            return 0;
        }
        NATIVE_BODY_WINDOW.saturating_sub(s.queue_total_size as usize)
    });
    if credit == 0 {
        // SAFETY: as in `native_stream`.
        unsafe { token(ctx) }.unpin();
    }
    credit
}

pub(super) fn pull_native(id: usize) -> bool {
    let source = rs_rec(RsRef(id))
        .as_deref()
        .and_then(|s| s.native_source.as_ref())
        .map(|s| (s.pull, s.token.clone()));
    if let Some((pull, token)) = source {
        token.pin();
        pull(ctx_of(&token));
        true
    } else {
        false
    }
}

pub(super) fn cancel_native(id: usize) {
    let source = rs_rec(RsRef(id))
        .as_deref_mut()
        .and_then(|s| s.native_source.take());
    if let Some(source) = source {
        (source.cancel)(ctx_of(&source.token));
    }
}

pub(crate) unsafe fn native_enqueue(stream: RsRef, bytes: &[u8]) {
    let id = stream.0;
    let usable = rs_rec(RsRef(id))
        .as_deref()
        .is_some_and(|s| s.state == ReadableState::Readable);
    if !usable || bytes.is_empty() {
        return;
    }
    if collect_bytes(id, bytes) {
        return;
    }
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let chunk = scope.root_nanbox_u64(alloc_uint8array_from_bytes(bytes));
    js_readable_stream_controller_enqueue(
        controller_value(id),
        f64::from_bits(chunk.get_nanbox_u64()),
    );
}

pub(super) fn collect_bytes(id: usize, bytes: &[u8]) -> bool {
    if let Some(c) = rs_rec(RsRef(id))
        .as_deref_mut()
        .and_then(|s| s.body_consumer.as_mut())
    {
        c.bytes.extend_from_slice(bytes);
        true
    } else {
        false
    }
}

pub(super) unsafe fn collect_chunk(id: usize, bits: u64) -> bool {
    if !rs_rec(RsRef(id))
        .as_deref()
        .is_some_and(|s| s.body_consumer.is_some())
    {
        return false;
    }
    if let Some(bytes) = subclass::body_chunk_bytes(bits) {
        collect_bytes(id, &bytes);
    }
    true
}

/// Body methods consume asynchronously. Only these explicit whole-body
/// requests accumulate bytes; reader and iterator consumers keep the window.
pub(crate) unsafe fn collect_body(
    stream: RsRef,
    promise: *mut Promise,
    metadata: String,
    finish: fn(Vec<u8>, String) -> Result<f64, f64>,
) {
    let id = stream.0;
    {
        if let Some(s) = rs_rec(RsRef(id)).as_deref_mut() {
            s.disturbed = true;
            s.set_reader_handle(Some(BODY_READER));
            s.set_body_consumer_promise(promise);
            s.body_consumer = Some(BodyConsumer {
                metadata,
                finish,
                bytes: Vec::new(),
            });
        }
    }
    advance_body(id);
}

pub(super) unsafe fn advance_body(id: usize) {
    loop {
        let chunk = rs_rec(RsRef(id)).as_deref_mut().and_then(|s| {
            if s.body_consumer.is_some() {
                s.pop_chunk()
            } else {
                None
            }
        });
        let Some(chunk) = chunk else {
            break;
        };
        collect_chunk(id, chunk);
    }
    let terminal = {
        rs_rec(RsRef(id)).as_deref_mut().and_then(|s| {
            if s.state != ReadableState::Readable && s.chunks_empty() {
                let promise = s.body_consumer_promise();
                s.set_body_consumer_promise(std::ptr::null_mut());
                s.body_consumer
                    .take()
                    .map(|c| (c, promise, s.state, s.error_value()))
            } else {
                None
            }
        })
    };
    if let Some((c, promise, state, error)) = terminal {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise = scope.root_raw_mut_ptr(promise);
        if state == ReadableState::Errored {
            js_promise_reject(promise.get_raw_mut_ptr(), f64::from_bits(error));
        } else {
            match (c.finish)(c.bytes, c.metadata) {
                Ok(value) => js_promise_resolve(promise.get_raw_mut_ptr(), value),
                Err(error) => js_promise_reject(promise.get_raw_mut_ptr(), error),
            }
        }
        // The consumer just emptied a stream that closed with its body still
        // queued (a small body that arrived before `text()`), so the close
        // could not retire it; nothing else reaches this stream again (#12862).
        // A closed stream whose queue just drained: ClearAlgorithms (error
        // already cleared them in error_pending).
        readable_clear_algorithms(id);
    } else if rs_rec(RsRef(id))
        .as_deref()
        .is_some_and(|s| s.body_consumer.is_some())
    {
        maybe_pull_force(id);
    }
}
