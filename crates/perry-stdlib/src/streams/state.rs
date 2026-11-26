//! The Web Streams access layer: the one module that knows where a stream
//! family member's state lives. Every other module (streams and fetch) reaches
//! a record through the typed refs and the `with_*` accessors below.
//!
//! The readable family (ReadableStream, its controllers, BYOB request and
//! readers) are ordinary GC objects (design 2026-11-25 §2, P2a): a ref is the
//! object's address, a stream's scalars are its native payload and every JS
//! value it keeps sits in its hidden slot array (`objects.rs`). Lifetime is
//! reachability; nothing registers, roots or scans them. The writable side,
//! transforms and writers are still band ids with records in three
//! process-global registries (P2b). Callers do not change shape:
//!
//! - `with_rs(r, |s| ..)` runs a closure on the record. The closure never
//!   allocates on the JS heap, calls JS or re-enters an accessor (with objects
//!   the closure is a payload borrow that must not live across a GC point;
//!   today a held registry lock would deadlock on re-entry).
//! - Everything a stream used to keep in a side map keyed by its id is a field
//!   of its own record (tee links and flags, transform bookkeeping, parked
//!   BYOB reads, expando properties), so an entry lives and dies with the
//!   record of the id it was keyed by.

use super::byob::ByobPending;
use super::gc::{visit_stream_value_slot, StreamRootVisitor};
use super::objects::{self, ctrl as ctrl_slot, reader as reader_slot, rs as rs_slot};
use super::*;
use perry_runtime::native_class_ids as class_ids;
use perry_runtime::native_payload::{self, OwnerLink};

// ─────────────────────────────────────────────────────────────────────
// Typed refs
// ─────────────────────────────────────────────────────────────────────

macro_rules! stream_ref {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub(crate) struct $name(pub(crate) usize);

        impl $name {
            /// The ref a JS value names (a ref that matches no record when the
            /// value is no stream-family handle).
            #[inline]
            #[allow(dead_code)]
            pub(crate) fn from_value(value: f64) -> Self {
                Self(stream_id(value))
            }

            /// The JS value of this ref.
            #[inline]
            #[allow(dead_code)]
            pub(crate) fn value(self) -> f64 {
                stream_value(self.0)
            }
        }
    };
}

macro_rules! object_ref {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        ///
        /// The ref is the object's address (0 names none). A Rust local of a
        /// ref is a conservative stack word, which pins the object; a ref kept
        /// anywhere on the Rust heap or in a closure capture must be stored as
        /// its traced value ([`Self::value`]) instead.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub(crate) struct $name(pub(crate) usize);

        impl $name {
            /// The ref a JS value names (accessors miss on a value that is no
            /// member of the family).
            #[inline]
            #[allow(dead_code)]
            pub(crate) fn from_value(value: f64) -> Self {
                Self(object_address(value))
            }

            /// The JS value of this ref.
            #[inline]
            #[allow(dead_code)]
            pub(crate) fn value(self) -> f64 {
                stream_value(self.0)
            }
        }
    };
}

object_ref!(
    /// A ReadableStream (an instance or a subclass instance).
    RsRef
);
impl RsRef {
    /// The stream's value bits, for a traced Rust-heap slot.
    #[inline]
    pub(crate) fn bits(self) -> u64 {
        self.value().to_bits()
    }

    /// The ref a traced slot's value bits name.
    #[inline]
    pub(crate) fn from_bits(bits: u64) -> Self {
        Self::from_value(f64::from_bits(bits))
    }

    /// The stream's controller object.
    #[inline]
    pub(crate) fn controller_value(self) -> f64 {
        match object_at(self.0) {
            Some(obj) => objects::slot(obj, rs_slot::CONTROLLER),
            None => f64::from_bits(TAG_UNDEFINED),
        }
    }

    /// The handle a native producer (the fetch engine's sink) keeps on the
    /// stream: its payload cell's `OwnerLink` (0 when `self` is no stream).
    #[inline]
    pub(crate) fn link(self) -> usize {
        with_rs(self, |s| s.link.map(OwnerLink::as_raw))
            .flatten()
            .unwrap_or(0)
    }

    /// The stream a native producer's handle names, while it lives.
    ///
    /// # Safety
    /// `link` came from [`Self::link`] and its cell is alive (the producer
    /// holds a `link_ref`, or its `alive` flag is still set).
    #[inline]
    pub(crate) unsafe fn from_link(link: usize) -> Self {
        if link == 0 {
            return Self(0);
        }
        match native_payload::link_owner(OwnerLink::from_raw(link)) {
            Some(owner) => Self::from_value(owner),
            None => Self(0),
        }
    }
}

stream_ref!(
    /// A WritableStream.
    WsRef
);
stream_ref!(
    /// A TransformStream.
    TsRef
);
object_ref!(
    /// A ReadableStreamDefaultReader or ReadableStreamBYOBReader.
    ReaderRef
);

/// True for a band id (a writable-side registry id) or 0.
#[inline]
fn in_band(id: usize) -> bool {
    id == 0 || (STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id)
}

/// The object value a readable-family ref names; None for 0, a band id or a
/// sentinel (`usize::MAX` tee lock, the body consumer's reader).
#[inline]
pub(super) fn object_at(id: usize) -> Option<f64> {
    if in_band(id) || id < 0x1000 || id >= (1usize << 48) {
        return None;
    }
    Some(perry_runtime::value::js_nanbox_pointer(id as i64))
}

/// The address of a heap object value that is no band handle (0 otherwise).
#[inline]
pub(crate) fn object_address(value: f64) -> usize {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() || perry_runtime::object::stream_handle_id(value).is_some() {
        return 0;
    }
    (value.to_bits() & POINTER_MASK) as usize
}
// ─────────────────────────────────────────────────────────────────────
// Refs held across JS
// ─────────────────────────────────────────────────────────────────────

/// A readable-family ref that a Rust frame still needs after a call that can
/// run JS (an algorithm, a strategy `size()`, a sink, a microtask drain).
/// Generated code reaches precise safepoints inside such a call, where a
/// collection sees only precise roots and may move the object (production
/// runs no conservative stack scan), so the frame roots the object in its
/// `RuntimeHandleScope`, the runtime's mechanism for helper-held values, and
/// reads the ref back with [`Held::id`] after the call. Band ids (the
/// writable side) and sentinels are no heap objects and come back unchanged.
pub(super) struct Held<'s> {
    root: Option<perry_runtime::gc::RuntimeHandle<'s>>,
    raw: usize,
}

impl<'s> Held<'s> {
    #[inline]
    pub(super) fn new(scope: &'s perry_runtime::gc::RuntimeHandleScope, id: usize) -> Self {
        Self {
            root: object_at(id).map(|value| scope.root_nanbox_f64(value)),
            raw: id,
        }
    }

    /// The ref now: the object's current address.
    #[inline]
    pub(super) fn id(&self) -> usize {
        match &self.root {
            Some(root) => (root.get_nanbox_f64().to_bits() & POINTER_MASK) as usize,
            None => self.raw,
        }
    }
}

stream_ref!(
    /// A WritableStreamDefaultWriter.
    WriterRef
);

/// User properties set on a stream-family value (`stream.allReady = p`).
pub(super) type Expandos = Vec<(String, u64)>;

// ─────────────────────────────────────────────────────────────────────
// Records
// ─────────────────────────────────────────────────────────────────────

/// A ReadableStream's native payload: its scalars. Its JS values live in the
/// stream object's slot array (`objects::rs`), reached through `link`.
pub(super) struct ReadableStreamData {
    /// The stream object's payload link (set at birth): `obj()` reads the
    /// object back from the cell's traced owner edge.
    pub(super) link: Option<OwnerLink>,
    pub(super) native_source: Option<native::NativeSource>,
    pub(super) body_consumer: Option<native::BodyConsumer>,
    pub(super) state: ReadableState,
    /// Per-chunk strategy sizes, parallel to the QUEUE slot (#4915). The size
    /// of a chunk is `strategy.size(chunk)` when a custom strategy is
    /// installed, `chunk.byteLength` on byte streams, else 1 — computed once
    /// at enqueue time per spec. Mutate the queue only through `push_chunk` /
    /// `unshift_chunk` / `pop_chunk` / `clear_chunks` so the running
    /// `queue_total_size` stays in sync.
    pub(super) chunk_sizes: VecDeque<f64>,
    /// Running sum of `chunk_sizes` — what `desiredSize` subtracts from the
    /// highWaterMark (real ByteLengthQueuingStrategy accounting, #4915).
    pub(super) queue_total_size: f64,
    pub(super) high_water_mark: f64,
    pub(super) is_byte_stream: bool,
    /// Internal source used by runtime APIs that hand back byte chunks instead
    /// of receiving a Web Streams controller.
    pub(super) pull_returns_byte_chunk: bool,
    pub(super) pulling: bool,
    pub(super) started: bool,
    /// Per-controller cancel reason captured when `cancel()` is called.
    pub(super) canceled: bool,
    pub(super) disturbed: bool,
    /// Tee source: a pull cycle is scheduled or running.
    pub(super) tee_pulling: bool,
    /// Tee source: the first pull cycle ran.
    pub(super) tee_started: bool,
    /// Readable side of a TransformStream: that transform (a band id).
    pub(super) owner_transform: Option<usize>,
    /// The PENDING_ERROR_AFTER_CHUNKS slot holds an error (which may itself
    /// be `undefined`).
    has_pending_error: bool,
}

/// READER slot sentinels (numbers, never objects): a tee holds the lock, or a
/// body consumer does.
const READER_TEE_LOCK: f64 = -1.0;
const READER_BODY: f64 = -2.0;

#[inline]
fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline]
fn is_undefined(value: f64) -> bool {
    value.to_bits() == TAG_UNDEFINED
}

/// A raw closure pointer (0 = none) as a slot value.
#[inline]
fn closure_value(cb: i64) -> f64 {
    if cb == 0 {
        undefined()
    } else {
        perry_runtime::value::js_nanbox_pointer(cb)
    }
}

/// A slot value back as a raw closure pointer (0 = none).
#[inline]
fn closure_of(value: f64) -> i64 {
    let v = JSValue::from_bits(value.to_bits());
    if v.is_pointer() {
        (value.to_bits() & POINTER_MASK) as i64
    } else {
        0
    }
}

/// A raw promise pointer as a slot value.
#[inline]
fn promise_value(p: *mut Promise) -> f64 {
    if p.is_null() {
        undefined()
    } else {
        perry_runtime::value::js_nanbox_pointer(p as i64)
    }
}

/// A slot value back as a raw promise pointer (null = none).
#[inline]
pub(super) fn promise_of(value: f64) -> *mut Promise {
    closure_of(value) as *mut Promise
}

/// A family ref (or sentinel / 0) as a slot value.
#[inline]
fn ref_value(id: usize) -> f64 {
    match id {
        0 => undefined(),
        usize::MAX => READER_TEE_LOCK,
        id if id == native::BODY_READER => READER_BODY,
        id => stream_value(id),
    }
}

/// A slot value back as a family ref (`None` for undefined).
#[inline]
fn ref_of(value: f64) -> Option<usize> {
    if is_undefined(value) {
        None
    } else if value == READER_TEE_LOCK {
        Some(usize::MAX)
    } else if value == READER_BODY {
        Some(native::BODY_READER)
    } else {
        Some(stream_id(value))
    }
}

impl ReadableStreamData {
    /// A fresh readable payload with every scalar at its initial value; the
    /// algorithms go to [`alloc_rs`].
    pub(super) fn new(high_water_mark: f64, is_byte_stream: bool) -> Self {
        Self {
            link: None,
            native_source: None,
            body_consumer: None,
            state: ReadableState::Readable,
            chunk_sizes: VecDeque::new(),
            queue_total_size: 0.0,
            high_water_mark,
            is_byte_stream,
            pull_returns_byte_chunk: false,
            pulling: false,
            started: false,
            canceled: false,
            disturbed: false,
            tee_pulling: false,
            tee_started: false,
            owner_transform: None,
            has_pending_error: false,
        }
    }

    /// The stream object this payload belongs to: its cell's owner edge (a
    /// borrowed payload is open, on its creator thread).
    #[inline]
    pub(super) fn obj(&self) -> f64 {
        match self.link {
            // SAFETY: `self` is the cell's borrowed payload.
            Some(link) => unsafe { native_payload::borrowed_link_owner(link) },
            None => undefined(),
        }
    }

    #[inline]
    fn slot(&self, i: u32) -> f64 {
        objects::slot(self.obj(), i)
    }

    #[inline]
    fn set_slot(&self, i: u32, value: f64) {
        objects::set_slot(self.obj(), i, value)
    }

    // ── queue ──
    pub(super) fn push_chunk(&mut self, bits: u64, size: f64) {
        objects::queue_push(self.obj(), rs_slot::QUEUE, f64::from_bits(bits));
        self.chunk_sizes.push_back(size);
        self.queue_total_size += size;
    }

    /// Put a chunk back at the queue head (a partially consumed BYOB chunk).
    pub(super) fn unshift_chunk(&mut self, bits: u64, size: f64) {
        objects::queue_unshift(self.obj(), rs_slot::QUEUE, f64::from_bits(bits));
        self.chunk_sizes.push_front(size);
        self.queue_total_size += size;
    }

    pub(super) fn pop_chunk(&mut self) -> Option<u64> {
        let size = self.chunk_sizes.pop_front()?;
        let bits = objects::queue_shift(self.obj(), rs_slot::QUEUE)?.to_bits();
        self.queue_total_size = (self.queue_total_size - size).max(0.0);
        Some(bits)
    }

    pub(super) fn clear_chunks(&mut self) {
        self.set_slot(rs_slot::QUEUE, undefined());
        self.chunk_sizes.clear();
        self.queue_total_size = 0.0;
    }

    /// Number of queued chunks.
    #[cfg(test)]
    #[inline]
    pub(super) fn chunk_count(&self) -> usize {
        self.chunk_sizes.len()
    }

    #[inline]
    pub(super) fn chunks_empty(&self) -> bool {
        self.chunk_sizes.is_empty()
    }

    // ── read requests ──
    pub(super) fn push_read_request(&mut self, promise: *mut Promise) {
        objects::queue_push(self.obj(), rs_slot::READ_REQUESTS, promise_value(promise));
    }

    pub(super) fn shift_read_request(&mut self) -> Option<*mut Promise> {
        objects::queue_shift(self.obj(), rs_slot::READ_REQUESTS).map(promise_of)
    }

    pub(super) fn read_request_count(&self) -> u32 {
        objects::queue_len(self.obj(), rs_slot::READ_REQUESTS)
    }

    /// Detach every pending read request: a JS array value (walk it with
    /// `objects::array_len` / `array_get` + [`promise_of`]; keep it in a
    /// local, never on the Rust heap).
    pub(super) fn take_read_requests(&mut self) -> f64 {
        objects::queue_take(self.obj(), rs_slot::READ_REQUESTS)
    }

    // ── algorithms ──
    pub(super) fn start_cb(&self) -> i64 {
        closure_of(self.slot(rs_slot::START))
    }
    pub(super) fn pull_cb(&self) -> i64 {
        closure_of(self.slot(rs_slot::PULL))
    }
    pub(super) fn cancel_cb(&self) -> i64 {
        closure_of(self.slot(rs_slot::CANCEL))
    }
    pub(super) fn strategy_size_cb(&self) -> i64 {
        closure_of(self.slot(rs_slot::SIZE))
    }
    /// ReadableStreamDefaultControllerClearAlgorithms /
    /// ReadableByteStreamControllerClearAlgorithms: drop the underlying
    /// source's algorithms and the strategy size so the source (and whatever
    /// its closures capture) can be collected while the stream itself stays
    /// referenced. Runs when the stream closes after its queue drained, when
    /// it errors, and after the cancel algorithm ran; no path calls them after.
    pub(super) fn clear_algorithms(&mut self) {
        for i in [
            rs_slot::START,
            rs_slot::PULL,
            rs_slot::CANCEL,
            rs_slot::SIZE,
        ] {
            self.set_slot(i, undefined());
        }
    }

    // ── links ──
    /// The reader holding the lock (a reader ref, `usize::MAX` for a tee,
    /// `native::BODY_READER` for a body consumer).
    pub(super) fn reader_handle(&self) -> Option<usize> {
        ref_of(self.slot(rs_slot::READER))
    }
    pub(super) fn set_reader_handle(&self, reader: Option<usize>) {
        self.set_slot(rs_slot::READER, ref_value(reader.unwrap_or(0)))
    }

    /// The stored error (bits; `undefined` before the stream errors).
    pub(super) fn error_value(&self) -> u64 {
        self.slot(rs_slot::STORED_ERROR).to_bits()
    }
    pub(super) fn set_error_value(&self, bits: u64) {
        self.set_slot(rs_slot::STORED_ERROR, f64::from_bits(bits))
    }

    /// An iterable source's error, held until the queued chunks were read.
    pub(super) fn pending_error_after_chunks(&self) -> Option<u64> {
        self.has_pending_error
            .then(|| self.slot(rs_slot::PENDING_ERROR_AFTER_CHUNKS).to_bits())
    }
    pub(super) fn set_pending_error_after_chunks(&mut self, bits: Option<u64>) {
        self.has_pending_error = bits.is_some();
        self.set_slot(
            rs_slot::PENDING_ERROR_AFTER_CHUNKS,
            f64::from_bits(bits.unwrap_or(TAG_UNDEFINED)),
        );
    }
    pub(super) fn take_pending_error_after_chunks(&mut self) -> Option<u64> {
        let error = self.pending_error_after_chunks();
        if error.is_some() {
            self.set_pending_error_after_chunks(None);
        }
        error
    }

    /// Tee source: its two branches (a ref is 0 once that branch is gone).
    pub(super) fn tee_branches(&self) -> Option<(usize, usize)> {
        let a = ref_of(self.slot(rs_slot::TEE_BRANCH1));
        let b = ref_of(self.slot(rs_slot::TEE_BRANCH2));
        match (a, b) {
            (None, None) => None,
            (a, b) => Some((a.unwrap_or(0), b.unwrap_or(0))),
        }
    }
    pub(super) fn set_tee_branches(&self, branches: Option<(usize, usize)>) {
        let (a, b) = branches.unwrap_or((0, 0));
        self.set_slot(rs_slot::TEE_BRANCH1, ref_value(a));
        self.set_slot(rs_slot::TEE_BRANCH2, ref_value(b));
    }

    /// Tee branch: the source it reads from.
    pub(super) fn tee_source(&self) -> Option<usize> {
        ref_of(self.slot(rs_slot::TEE_SOURCE))
    }
    pub(super) fn set_tee_source(&self, source: Option<usize>) {
        self.set_slot(rs_slot::TEE_SOURCE, ref_value(source.unwrap_or(0)))
    }

    /// Tee source: the promise its cancel() returns while both branches
    /// cancel.
    pub(super) fn tee_cancel_promise(&self) -> Option<*mut Promise> {
        let p = promise_of(self.slot(rs_slot::TEE_CANCEL_PROMISE));
        (!p.is_null()).then_some(p)
    }
    pub(super) fn set_tee_cancel_promise(&self, promise: Option<*mut Promise>) {
        self.set_slot(
            rs_slot::TEE_CANCEL_PROMISE,
            promise_value(promise.unwrap_or(std::ptr::null_mut())),
        )
    }

    // ── transform bookkeeping (readable side of a TransformStream) ──
    /// Park a write promise on backpressure.
    pub(super) fn park_write(&mut self, promise: *mut Promise) {
        objects::queue_push(self.obj(), rs_slot::PARKED_WRITES, promise_value(promise));
    }
    /// Detach the parked write promises (a JS array value; see
    /// [`Self::take_read_requests`]).
    pub(super) fn take_parked_writes(&mut self) -> f64 {
        objects::queue_take(self.obj(), rs_slot::PARKED_WRITES)
    }
    #[cfg(test)]
    pub(super) fn parked_write_count(&self) -> u32 {
        objects::queue_len(self.obj(), rs_slot::PARKED_WRITES)
    }
    /// Defer a write job while backpressured.
    pub(super) fn defer_job(&mut self, job: *mut ClosureHeader) {
        objects::queue_push(
            self.obj(),
            rs_slot::BACKPRESSURED_JOBS,
            perry_runtime::value::js_nanbox_pointer(job as i64),
        );
    }
    /// Detach the deferred write jobs (a JS array value of closures).
    pub(super) fn take_deferred_jobs(&mut self) -> f64 {
        objects::queue_take(self.obj(), rs_slot::BACKPRESSURED_JOBS)
    }
    #[cfg(test)]
    pub(super) fn deferred_job_count(&self) -> u32 {
        objects::queue_len(self.obj(), rs_slot::BACKPRESSURED_JOBS)
    }

    // ── BYOB pending reads: flat [promise, view, promise, view, ..] ──
    pub(super) fn byob_pending_is_empty(&self) -> bool {
        objects::queue_len(self.obj(), rs_slot::BYOB_PENDING) == 0
    }
    pub(super) fn byob_push_back(&mut self, pending: ByobPending) {
        let obj = self.obj();
        objects::queue_push(obj, rs_slot::BYOB_PENDING, promise_value(pending.promise));
        objects::queue_push(
            obj,
            rs_slot::BYOB_PENDING,
            f64::from_bits(pending.view_bits),
        );
    }
    pub(super) fn byob_push_front(&mut self, pending: ByobPending) {
        let obj = self.obj();
        objects::queue_unshift(
            obj,
            rs_slot::BYOB_PENDING,
            f64::from_bits(pending.view_bits),
        );
        objects::queue_unshift(obj, rs_slot::BYOB_PENDING, promise_value(pending.promise));
    }
    /// Take the head pull-into; the current BYOB request object (if any)
    /// answered for it, so it is dropped.
    pub(super) fn byob_pop_front(&mut self) -> Option<ByobPending> {
        let obj = self.obj();
        objects::set_slot(obj, rs_slot::BYOB_REQUEST, undefined());
        let promise = promise_of(objects::queue_shift(obj, rs_slot::BYOB_PENDING)?);
        let view_bits = objects::queue_shift(obj, rs_slot::BYOB_PENDING)
            .unwrap_or_else(undefined)
            .to_bits();
        Some(ByobPending { promise, view_bits })
    }
    pub(super) fn byob_front_view(&self) -> Option<u64> {
        let obj = self.obj();
        (objects::queue_len(obj, rs_slot::BYOB_PENDING) >= 2)
            .then(|| objects::queue_get(obj, rs_slot::BYOB_PENDING, 1).to_bits())
    }
    /// Detach every pending BYOB read: a flat JS array value of
    /// (promise, view) pairs.
    pub(super) fn take_byob_pending(&mut self) -> f64 {
        self.set_slot(rs_slot::BYOB_REQUEST, undefined());
        objects::queue_take(self.obj(), rs_slot::BYOB_PENDING)
    }

    // ── native producer / body consumer promises ──
    pub(super) fn native_promise(&self) -> Option<*mut Promise> {
        let p = promise_of(self.slot(rs_slot::NATIVE_PROMISE));
        (!p.is_null()).then_some(p)
    }
    pub(super) fn set_native_promise(&self, promise: Option<*mut Promise>) {
        self.set_slot(
            rs_slot::NATIVE_PROMISE,
            promise_value(promise.unwrap_or(std::ptr::null_mut())),
        )
    }
    pub(super) fn body_consumer_promise(&self) -> *mut Promise {
        promise_of(self.slot(rs_slot::BODY_CONSUMER_PROMISE))
    }
    pub(super) fn set_body_consumer_promise(&self, promise: *mut Promise) {
        self.set_slot(rs_slot::BODY_CONSUMER_PROMISE, promise_value(promise))
    }
}

impl Drop for ReadableStreamData {
    /// The stream object died (sweep or thread teardown): a native producer
    /// still feeding it learns so through its token and cancels. No
    /// allocation, no JS, no thread-locals.
    fn drop(&mut self) {
        if let Some(source) = self.native_source.as_ref() {
            source.token.stream_died();
        }
    }
}

#[allow(dead_code)]
pub(super) struct WritableStreamData {
    pub(super) props: perry_ffi::NativePropertyBag,
    pub(super) state: WritableState,
    pub(super) write_cb: i64,
    pub(super) close_cb: i64,
    pub(super) abort_cb: i64,
    /// Custom `strategy.size(chunk)` callback (#4915). 0 when absent; each
    /// chunk then counts as 1 toward `desiredSize`.
    pub(super) strategy_size_cb: i64,
    /// Backlog of writes while the sink's previous `write()` Promise is
    /// pending: `(chunk_bits, write_promise, strategy_size)`.
    pub(super) write_queue: VecDeque<(u64, *mut Promise, f64)>,
    /// Strategy size of the chunk currently in flight (0.0 when idle).
    pub(super) in_flight_size: f64,
    pub(super) in_flight: bool,
    pub(super) high_water_mark: f64,
    pub(super) writer_handle: Option<usize>,
    pub(super) error_value: u64,
    /// Resolved when the stream becomes ready for more writes (i.e. queue drains).
    pub(super) ready_promise: *mut Promise,
    /// Resolved when the stream finishes / rejects on error.
    pub(super) closed_promise: *mut Promise,
    /// Pending `close()` request, if close is waiting for queued writes.
    pub(super) close_request_promise: *mut Promise,
    pub(super) close_started: bool,
    /// Writable side of a TransformStream: that transform.
    pub(super) owner_transform: Option<usize>,
    /// #6607: write jobs queued by `transform_write` whose transformer has not
    /// finished delivering yet (async transformers count until their returned
    /// promise settles).
    pub(super) transform_pending_writes: usize,
    /// #6607: the close-request promise deferred until those jobs drain.
    pub(super) transform_pending_close: Option<*mut Promise>,
    pub(super) expando: Expandos,
}

impl WritableStreamData {
    pub(super) fn new(
        write_cb: i64,
        close_cb: i64,
        abort_cb: i64,
        strategy_size_cb: i64,
        high_water_mark: f64,
        ready_promise: *mut Promise,
        closed_promise: *mut Promise,
    ) -> Self {
        Self {
            props: Default::default(),
            state: WritableState::Writable,
            write_cb,
            close_cb,
            abort_cb,
            strategy_size_cb,
            write_queue: VecDeque::new(),
            in_flight_size: 0.0,
            in_flight: false,
            high_water_mark,
            writer_handle: None,
            error_value: 0,
            ready_promise,
            closed_promise,
            close_request_promise: std::ptr::null_mut(),
            close_started: false,
            owner_transform: None,
            transform_pending_writes: 0,
            transform_pending_close: None,
            expando: Vec::new(),
        }
    }

    /// WritableStreamDefaultControllerClearAlgorithms: drop the underlying
    /// sink's algorithms and the strategy size. Runs right after the close
    /// algorithm or the abort algorithm was called; no path calls them after.
    pub(super) fn clear_algorithms(&mut self) {
        self.write_cb = 0;
        self.close_cb = 0;
        self.abort_cb = 0;
        self.strategy_size_cb = 0;
    }

    pub(super) fn trace<V: StreamRootVisitor>(&mut self, visitor: &mut V) {
        if let Some(slot) = self.props.slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
        visitor.visit_i64_slot(&mut self.write_cb);
        visitor.visit_i64_slot(&mut self.close_cb);
        visitor.visit_i64_slot(&mut self.abort_cb);
        visitor.visit_i64_slot(&mut self.strategy_size_cb);
        for (chunk, promise, _size) in self.write_queue.iter_mut() {
            visit_stream_value_slot(visitor, chunk);
            visitor.visit_raw_mut_ptr_slot(promise);
        }
        visitor.visit_raw_mut_ptr_slot(&mut self.ready_promise);
        visitor.visit_raw_mut_ptr_slot(&mut self.closed_promise);
        visitor.visit_raw_mut_ptr_slot(&mut self.close_request_promise);
        if self.state == WritableState::Errored {
            visit_stream_value_slot(visitor, &mut self.error_value);
        }
        if let Some(promise) = self.transform_pending_close.as_mut() {
            visitor.visit_raw_mut_ptr_slot(promise);
        }
        trace_expandos(visitor, &mut self.expando);
    }

    fn holds_freed(&self, freed: &FreedRanges) -> bool {
        let promise = |p: *mut Promise| freed.contains(p as usize);
        [
            self.write_cb,
            self.close_cb,
            self.abort_cb,
            self.strategy_size_cb,
        ]
        .iter()
        .any(|&cb| freed.holds_i64(cb))
            || self
                .write_queue
                .iter()
                .any(|&(bits, p, _)| freed.holds_bits(bits) || promise(p))
            || freed.holds_bits(self.error_value)
            || promise(self.ready_promise)
            || promise(self.closed_promise)
            || promise(self.close_request_promise)
            || self.transform_pending_close.is_some_and(promise)
    }
}

pub(super) struct TransformStreamData {
    pub(super) props: perry_ffi::NativePropertyBag,
    /// The readable side: the stream object's value bits (traced below).
    pub(super) readable: u64,
    pub(super) writable_handle: usize,
    pub(super) transform_cb: i64,
    pub(super) flush_cb: i64,
    pub(super) native: Option<NativeTransformKind>,
    pub(super) backpressure: bool,
    pub(super) expando: Expandos,
    /// User properties set on its TransformStreamDefaultController.
    pub(super) controller_expando: Expandos,
}

impl TransformStreamData {
    /// The readable side's ref.
    #[inline]
    pub(super) fn readable_handle(&self) -> usize {
        stream_id(f64::from_bits(self.readable))
    }

    pub(super) fn trace<V: StreamRootVisitor>(&mut self, visitor: &mut V) {
        if let Some(slot) = self.props.slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
        visitor.visit_nanbox_u64_slot(&mut self.readable);
        visitor.visit_i64_slot(&mut self.transform_cb);
        visitor.visit_i64_slot(&mut self.flush_cb);
        trace_expandos(visitor, &mut self.expando);
        trace_expandos(visitor, &mut self.controller_expando);
    }
}

/// A reader's state as read from its slot array (`objects::reader`); the
/// reader object (its class id) is the truth, this is a stack copy.
pub(super) struct ReaderData {
    pub(super) stream_handle: usize,
    pub(super) locked: bool,
    pub(super) closed_promise: *mut Promise,
    /// True for readers minted by `getReader({ mode: "byob" })` /
    /// `new ReadableStreamBYOBReader(stream)` (#4915): the BYOB reader class
    /// id. BYOB readers route `read(view)` through
    /// `byob::js_reader_read_with_view`.
    pub(super) is_byob: bool,
}

impl ReaderData {
    pub(super) fn new(stream_handle: usize, closed_promise: *mut Promise, is_byob: bool) -> Self {
        Self {
            stream_handle,
            locked: true,
            closed_promise,
            is_byob,
        }
    }
}

pub(super) struct WriterData {
    pub(super) props: perry_ffi::NativePropertyBag,
    pub(super) stream_handle: usize,
    pub(super) locked: bool,
    pub(super) closed_promise: *mut Promise,
    pub(super) ready_promise: *mut Promise,
    pub(super) expando: Expandos,
}

impl WriterData {
    pub(super) fn new(
        stream_handle: usize,
        closed_promise: *mut Promise,
        ready_promise: *mut Promise,
    ) -> Self {
        Self {
            props: Default::default(),
            stream_handle,
            locked: true,
            closed_promise,
            ready_promise,
            expando: Vec::new(),
        }
    }

    pub(super) fn trace<V: StreamRootVisitor>(&mut self, visitor: &mut V) {
        if let Some(slot) = self.props.slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
        visitor.visit_raw_mut_ptr_slot(&mut self.closed_promise);
        visitor.visit_raw_mut_ptr_slot(&mut self.ready_promise);
        trace_expandos(visitor, &mut self.expando);
    }
}

unsafe impl Send for WritableStreamData {}
unsafe impl Send for TransformStreamData {}
unsafe impl Send for WriterData {}

fn trace_expandos<V: StreamRootVisitor>(visitor: &mut V, entries: &mut Expandos) {
    for (_, bits) in entries.iter_mut() {
        visit_stream_value_slot(visitor, bits);
    }
}

// ─────────────────────────────────────────────────────────────────────
// Registries (private to this module)
// ─────────────────────────────────────────────────────────────────────

/// A record registry keyed by stream-family id (spelled out at each static so
/// the registry audits see the id-keyed map).
type Registry<T> = std::sync::LazyLock<Mutex<HashMap<usize, T>>>;

static WRITABLE_STREAMS: std::sync::LazyLock<Mutex<HashMap<usize, WritableStreamData>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static TRANSFORM_STREAMS: std::sync::LazyLock<Mutex<HashMap<usize, TransformStreamData>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static WRITERS: std::sync::LazyLock<Mutex<HashMap<usize, WriterData>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// One record, borrowed: derefs to the record for as long as it lives
/// (today it holds its registry's lock, so it must not outlive the statement
/// that uses it and must not span a JS call; GC-objects plan: a payload
/// borrow with the same rule).
pub(super) struct Rec<T: 'static> {
    _guard: std::sync::MutexGuard<'static, HashMap<usize, T>>,
    record: *mut T,
}

impl<T> std::ops::Deref for Rec<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        // SAFETY: `record` points into the map `_guard` locks; nothing else
        // can reach that map while the guard lives.
        unsafe { &*self.record }
    }
}

impl<T> std::ops::DerefMut for Rec<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in `deref`; `&mut self` makes the borrow unique.
        unsafe { &mut *self.record }
    }
}

#[inline]
fn rec<T>(table: &'static Registry<T>, id: usize) -> Option<Rec<T>> {
    let mut guard = table.lock().unwrap();
    let record = guard.get_mut(&id)? as *mut T;
    Some(Rec {
        _guard: guard,
        record,
    })
}

macro_rules! accessors {
    ($table:ident, $ref:ident, $data:ty, $with:ident, $rec:ident, $exists:ident, $insert:ident) => {
        /// Borrow the record `r` names; None when there is none.
        #[inline]
        pub(super) fn $rec(r: $ref) -> Option<Rec<$data>> {
            rec(&$table, r.0)
        }

        /// Run `f` on the record `r` names; None when there is none.
        #[inline]
        #[allow(dead_code)]
        pub(super) fn $with<R>(r: $ref, f: impl FnOnce(&mut $data) -> R) -> Option<R> {
            $table.lock().unwrap().get_mut(&r.0).map(f)
        }

        /// True while `r` names a record.
        #[inline]
        #[allow(dead_code)]
        pub(super) fn $exists(r: $ref) -> bool {
            $table.lock().unwrap().contains_key(&r.0)
        }

        /// Install the record of a freshly minted id.
        pub(super) fn $insert(r: $ref, data: $data) {
            ensure_streams_thread_exit_hook();
            $table.lock().unwrap().insert(r.0, data);
        }
    };
}

accessors!(
    WRITABLE_STREAMS,
    WsRef,
    WritableStreamData,
    with_ws,
    ws_rec,
    ws_exists,
    insert_ws
);
accessors!(
    TRANSFORM_STREAMS,
    TsRef,
    TransformStreamData,
    with_ts,
    ts_rec,
    ts_exists,
    insert_ts
);
accessors!(
    WRITERS,
    WriterRef,
    WriterData,
    with_writer,
    writer_rec,
    writer_exists,
    insert_writer
);

// ─────────────────────────────────────────────────────────────────────
// Readable family accessors (GC objects)
// ─────────────────────────────────────────────────────────────────────

/// A borrowed ReadableStream payload. Like a registry record it must not
/// outlive the statement that uses it and must not span a JS call (the
/// payload is a `Box` that stays put, but a re-entrant accessor would alias
/// it); slot accessors on it may allocate.
pub(super) struct RsRec(&'static mut ReadableStreamData);

impl std::ops::Deref for RsRec {
    type Target = ReadableStreamData;
    #[inline]
    fn deref(&self) -> &ReadableStreamData {
        self.0
    }
}

impl std::ops::DerefMut for RsRec {
    #[inline]
    fn deref_mut(&mut self) -> &mut ReadableStreamData {
        self.0
    }
}

/// Borrow the payload of the stream `r` names; None when `r` is no stream.
#[inline]
pub(super) fn rs_rec(r: RsRef) -> Option<RsRec> {
    let obj = object_at(r.0)?;
    // SAFETY: the family's payload type; the borrow rule is RsRec's.
    unsafe { objects::rs_payload::<ReadableStreamData>(obj) }.map(RsRec)
}

/// Run `f` on the payload of the stream `r` names; None when there is none.
#[inline]
pub(super) fn with_rs<R>(r: RsRef, f: impl FnOnce(&mut ReadableStreamData) -> R) -> Option<R> {
    rs_rec(r).map(|mut s| f(&mut s))
}

/// True while `r` names a ReadableStream.
#[inline]
pub(super) fn rs_exists(r: RsRef) -> bool {
    rs_rec(r).is_some()
}

/// Which controller a new stream gets: a byte stream a
/// ReadableByteStreamController, every other a default controller object. A
/// TransformStream's readable side hands out its transform's
/// TransformStreamDefaultController instead, which stays a band value of the
/// transform family until that family converts (P2b): `transform id |
/// STREAM_CONTROLLER_BIT`.
fn controller_kind(data: &ReadableStreamData) -> (u32, &'static str) {
    if data.is_byte_stream {
        (
            class_ids::WEB_RS_BYTE_CONTROLLER,
            "ReadableByteStreamController",
        )
    } else {
        (
            class_ids::WEB_RS_DEFAULT_CONTROLLER,
            "ReadableStreamDefaultController",
        )
    }
}

/// Finish a stream object's birth: its payload link, its controller and its
/// algorithms (`[start, pull, cancel, size]` raw closure pointers, 0 = none).
fn born_rs(value: f64, controller: (u32, &'static str), algorithms: [i64; 4]) -> RsRef {
    let r = RsRef::from_value(value);
    let link = native_payload::owner_link(value, &objects::RS_FAMILY).ok();
    let Some(owner_transform) = with_rs(r, |s| {
        s.link = link;
        s.owner_transform
    }) else {
        return RsRef(0);
    };
    let ctrl = match owner_transform {
        Some(transform) => stream_value(transform | STREAM_CONTROLLER_BIT),
        None => {
            let (class_id, interface) = controller;
            let ctrl = objects::alloc_slots_object(class_id, interface, ctrl_slot::LEN);
            objects::set_slot(ctrl, ctrl_slot::STREAM, value);
            ctrl
        }
    };
    objects::set_slot(value, rs_slot::CONTROLLER, ctrl);
    for (i, cb) in [
        rs_slot::START,
        rs_slot::PULL,
        rs_slot::CANCEL,
        rs_slot::SIZE,
    ]
    .into_iter()
    .zip(algorithms)
    {
        objects::set_slot(value, i, closure_value(cb));
    }
    r
}

/// A new ReadableStream object owning `data`.
pub(super) fn alloc_rs(data: ReadableStreamData, algorithms: [i64; 4]) -> RsRef {
    let controller = controller_kind(&data);
    let value = objects::alloc_rs_object(data, 0);
    born_rs(value, controller, algorithms)
}

/// Make `this` (a subclass instance) a ReadableStream owning `data`; RsRef(0)
/// when `this` cannot take it.
pub(super) fn attach_rs_to(this: f64, data: ReadableStreamData, algorithms: [i64; 4]) -> RsRef {
    let controller = controller_kind(&data);
    if !objects::attach_rs(this, data, 0) {
        return RsRef(0);
    }
    born_rs(this, controller, algorithms)
}

/// The transform whose TransformStreamDefaultController band id is `id`
/// (0 for any other id).
#[inline]
pub(crate) fn transform_controller_transform(id: usize) -> usize {
    if (STREAM_HANDLE_ID_START + STREAM_CONTROLLER_BIT..STREAM_HANDLE_ID_END).contains(&id) {
        id & !STREAM_CONTROLLER_BIT
    } else {
        0
    }
}

/// The stream a controller controls (0 for any other value): a controller
/// object's stream, or a TransformStreamDefaultController's readable side.
pub(crate) fn controller_stream(value: f64) -> usize {
    if let Some(id) = perry_runtime::object::stream_handle_id(value) {
        let transform = transform_controller_transform(id);
        if transform == 0 {
            return 0;
        }
        return with_ts(TsRef(transform), |t| t.readable_handle()).unwrap_or(0);
    }
    match objects::class_id_of(value) {
        class_ids::WEB_RS_DEFAULT_CONTROLLER | class_ids::WEB_RS_BYTE_CONTROLLER => {
            stream_id(objects::slot(value, ctrl_slot::STREAM))
        }
        _ => 0,
    }
}

/// The byte stream a BYOB request object belongs to (0 for any other value).
pub(crate) fn byob_request_stream(value: f64) -> usize {
    if objects::class_id_of(value) != class_ids::WEB_RS_BYOB_REQUEST {
        return 0;
    }
    controller_stream(objects::slot(value, objects::req::CONTROLLER))
}

/// A borrowed reader state: a stack copy of the slots; `locked` is written
/// back when the borrow ends.
pub(super) struct ReaderRec {
    obj: f64,
    data: ReaderData,
    locked_was: bool,
}

impl std::ops::Deref for ReaderRec {
    type Target = ReaderData;
    #[inline]
    fn deref(&self) -> &ReaderData {
        &self.data
    }
}

impl std::ops::DerefMut for ReaderRec {
    #[inline]
    fn deref_mut(&mut self) -> &mut ReaderData {
        &mut self.data
    }
}

impl Drop for ReaderRec {
    fn drop(&mut self) {
        if self.data.locked != self.locked_was {
            objects::set_slot(
                self.obj,
                reader_slot::LOCKED,
                f64::from_bits(if self.data.locked {
                    TAG_TRUE
                } else {
                    TAG_FALSE
                }),
            );
        }
    }
}

/// The reader object `r` names, if it is one.
#[inline]
fn reader_object(r: ReaderRef) -> Option<(f64, bool)> {
    let obj = object_at(r.0)?;
    match objects::class_id_of(obj) {
        class_ids::WEB_RS_DEFAULT_READER => Some((obj, false)),
        class_ids::WEB_RS_BYOB_READER => Some((obj, true)),
        _ => None,
    }
}

/// Borrow the state of the reader `r` names; None when `r` is no reader.
pub(super) fn reader_rec(r: ReaderRef) -> Option<ReaderRec> {
    let (obj, is_byob) = reader_object(r)?;
    let locked = objects::slot(obj, reader_slot::LOCKED).to_bits() == TAG_TRUE;
    Some(ReaderRec {
        obj,
        data: ReaderData {
            stream_handle: stream_id(objects::slot(obj, reader_slot::STREAM)),
            locked,
            closed_promise: promise_of(objects::slot(obj, reader_slot::CLOSED_PROMISE)),
            is_byob,
        },
        locked_was: locked,
    })
}

/// Run `f` on the state of the reader `r` names.
pub(super) fn with_reader<R>(r: ReaderRef, f: impl FnOnce(&mut ReaderData) -> R) -> Option<R> {
    reader_rec(r).map(|mut rd| f(&mut rd))
}

/// True while `r` names a reader.
#[allow(dead_code)]
pub(super) fn reader_exists(r: ReaderRef) -> bool {
    reader_object(r).is_some()
}

/// A new reader object (default or BYOB by `data.is_byob`).
pub(super) fn alloc_reader(data: ReaderData) -> ReaderRef {
    let (class_id, interface) = if data.is_byob {
        (class_ids::WEB_RS_BYOB_READER, "ReadableStreamBYOBReader")
    } else {
        (
            class_ids::WEB_RS_DEFAULT_READER,
            "ReadableStreamDefaultReader",
        )
    };
    let obj = objects::alloc_slots_object(class_id, interface, reader_slot::LEN);
    objects::set_slot(obj, reader_slot::STREAM, stream_value(data.stream_handle));
    objects::set_slot(
        obj,
        reader_slot::CLOSED_PROMISE,
        promise_value(data.closed_promise),
    );
    objects::set_slot(
        obj,
        reader_slot::LOCKED,
        f64::from_bits(if data.locked { TAG_TRUE } else { TAG_FALSE }),
    );
    ReaderRef::from_value(obj)
}

/// Remove a pipe writer's record at release (pipeTo's writer is never
/// handed to JS).
pub(super) fn remove_writer(w: WriterRef) -> Option<WriterData> {
    WRITERS.lock().unwrap().remove(&w.0)
}

/// Run `f` on the expando list of a band-id value: a writable stream, writer
/// or transform (readable-family objects keep ordinary own properties).
pub(super) fn with_expandos<R>(id: usize, f: impl FnOnce(&mut Expandos) -> R) -> Option<R> {
    let transform = transform_controller_transform(id);
    if transform != 0 {
        return with_ts(TsRef(transform), |t| f(&mut t.controller_expando));
    }
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&id) {
        return Some(f(&mut s.expando));
    }
    if let Some(t) = TRANSFORM_STREAMS.lock().unwrap().get_mut(&id) {
        return Some(f(&mut t.expando));
    }
    if let Some(w) = WRITERS.lock().unwrap().get_mut(&id) {
        return Some(f(&mut w.expando));
    }
    None
}

/// The TransformStream whose readable side is `r`, while it exists.
pub(super) fn transform_of_readable(r: RsRef) -> Option<TsRef> {
    let t = with_rs(r, |s| s.owner_transform).flatten()?;
    ts_exists(TsRef(t)).then_some(TsRef(t))
}

/// The TransformStream whose writable side is `w`.
pub(super) fn transform_of_writable(w: WsRef) -> Option<TsRef> {
    with_ws(w, |s| s.owner_transform).flatten().map(TsRef)
}

// ─────────────────────────────────────────────────────────────────────
// GC trace, eviction, thread exit
// ─────────────────────────────────────────────────────────────────────

/// Visit every record's JS-valued fields (the `stdlib:streams` root scanner).
pub(super) fn trace_all<V: StreamRootVisitor>(visitor: &mut V) {
    if let Ok(mut map) = WRITABLE_STREAMS.lock() {
        map.values_mut().for_each(|s| s.trace(visitor));
    }
    if let Ok(mut map) = TRANSFORM_STREAMS.lock() {
        map.values_mut().for_each(|t| t.trace(visitor));
    }
    if let Ok(mut map) = WRITERS.lock() {
        map.values_mut().for_each(|w| w.trace(visitor));
    }
}

/// Remove the records of `batch` (quarantine eviction or thread exit).
pub(super) fn evict(batch: &[usize]) {
    use std::sync::PoisonError;
    let mut writables = WRITABLE_STREAMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut transforms = TRANSFORM_STREAMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut writers = WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
    for id in batch {
        writables.remove(id);
        transforms.remove(id);
        writers.remove(id);
    }
}

pub(super) use perry_runtime::arena::thread_exit::FreedRanges;

fn release_freed(entries: &mut Expandos, freed: &FreedRanges) {
    entries.retain(|(_, bits)| !freed.holds_bits(*bits));
}

/// #11471: register [`release_web_streams_in_freed_ranges`] before the first
/// record insert.
fn ensure_streams_thread_exit_hook() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        perry_runtime::arena::thread_exit::register_thread_exit_range_hook(
            release_web_streams_in_freed_ranges,
        )
    });
}

/// #11471: an exiting thread's Web Streams records outlive its heap. The
/// registries are process-global (ids from the shared `idalloc` band) and
/// the `stdlib:streams` root scanner of every surviving thread visits every
/// record, so a record holding the dead thread's callbacks, queued chunks,
/// promises or error values would have those threads mark and rewrite freed
/// (or reused) memory, and a later call on the id would run a dangling
/// closure.
///
/// A stream id is DEAD when its record holds an address in `freed`. Deadness
/// then spreads along the links between records — stream <-> reader /
/// writer, transform <-> its readable and writable sides — so no surviving
/// record names an evicted id (which `idalloc` could otherwise hand to an
/// unrelated stream). Every dead id then gets the same removal as a
/// quarantine eviction. Expando values in `freed` drop alone (the record
/// stays). The ids themselves are not recycled (a bounded band leak per dead
/// stream; `idalloc` is left alone so this hook never takes its lock).
///
/// Runs in the exiting thread's TLS destructor: registry locks one at a
/// time, poison-tolerant, no allocation on the JS heap, no JS calls.
fn release_web_streams_in_freed_ranges(freed: &FreedRanges) {
    use std::collections::HashSet;
    use std::sync::PoisonError;
    let promise = |p: *mut Promise| freed.contains(p as usize);
    let mut dead: HashSet<usize> = HashSet::new();
    let mut known: HashSet<usize> = HashSet::new();
    let mut links: Vec<(usize, usize)> = Vec::new();
    {
        let mut g = WRITABLE_STREAMS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for (&id, s) in g.iter_mut() {
            known.insert(id);
            release_freed(&mut s.expando, freed);
            if let Some(writer) = s.writer_handle {
                links.push((id, writer));
            }
            if let Some(transform) = s.owner_transform {
                links.push((id, transform));
            }
            if s.holds_freed(freed) {
                dead.insert(id);
            }
        }
    }
    {
        let mut g = TRANSFORM_STREAMS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for (&id, t) in g.iter_mut() {
            known.insert(id);
            release_freed(&mut t.expando, freed);
            release_freed(&mut t.controller_expando, freed);
            links.push((id, t.writable_handle));
            if freed.holds_i64(t.transform_cb)
                || freed.holds_i64(t.flush_cb)
                || freed.holds_bits(t.readable)
            {
                dead.insert(id);
            }
        }
    }
    {
        let mut g = WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
        for (&id, w) in g.iter_mut() {
            known.insert(id);
            release_freed(&mut w.expando, freed);
            links.push((id, w.stream_handle));
            if promise(w.closed_promise) || promise(w.ready_promise) {
                dead.insert(id);
            }
        }
    }
    if dead.is_empty() {
        return;
    }

    let mut adjacent: HashMap<usize, Vec<usize>> = HashMap::new();
    for (a, b) in links {
        if known.contains(&a) && known.contains(&b) {
            adjacent.entry(a).or_default().push(b);
            adjacent.entry(b).or_default().push(a);
        }
    }
    let mut work: Vec<usize> = dead.iter().copied().collect();
    while let Some(id) = work.pop() {
        if let Some(next) = adjacent.get(&id) {
            for &n in next {
                if dead.insert(n) {
                    work.push(n);
                }
            }
        }
    }
    let batch: Vec<usize> = dead.into_iter().collect();
    evict(&batch);
}

/// #11471 test probe: which registries hold `id`, as a bitmask (writable 2,
/// transform 4, writer 16; the readable family is GC objects, no registry).
#[cfg(test)]
pub(crate) fn stream_registry_presence_for_test(id: usize) -> u32 {
    let mut mask = 0;
    if rs_exists(RsRef(id)) {
        mask |= 1;
    }
    if ws_exists(WsRef(id)) {
        mask |= 2;
    }
    if ts_exists(TsRef(id)) {
        mask |= 4;
    }
    if reader_exists(ReaderRef(id)) {
        mask |= 8;
    }
    if writer_exists(WriterRef(id)) {
        mask |= 16;
    }
    mask
}

/// #11471 test probe: does `id`'s record hold transform bookkeeping (the
/// former transform side tables)?
#[cfg(test)]
pub(crate) fn transform_side_tables_hold_for_test(id: usize) -> bool {
    with_rs(RsRef(id), |s| {
        s.owner_transform.is_some() || s.parked_write_count() != 0 || s.deferred_job_count() != 0
    })
    .unwrap_or(false)
        || with_ws(WsRef(id), |s| {
            s.owner_transform.is_some()
                || s.transform_pending_writes != 0
                || s.transform_pending_close.is_some()
        })
        .unwrap_or(false)
        || ts_exists(TsRef(id))
}

/// Run `f` on the descriptor bag of the record `id` names (stream, reader,
/// writer or transform).
pub(super) fn with_props<R>(
    id: usize,
    f: impl FnOnce(&mut perry_ffi::NativePropertyBag) -> R,
) -> Option<R> {
    macro_rules! try_table {
        ($table:expr) => {
            if let Some(record) = $table.lock().ok()?.get_mut(&id) {
                return Some(f(&mut record.props));
            }
        };
    }
    try_table!(WRITABLE_STREAMS);
    try_table!(TRANSFORM_STREAMS);
    try_table!(WRITERS);
    None
}

/// Drop descriptor-bag values that lie in an exited thread's heap.
pub(super) fn clear_props_in_freed_ranges(freed: &FreedRanges) {
    macro_rules! clear {
        ($table:expr) => {
            if let Ok(mut table) = $table.lock() {
                for record in table.values_mut() {
                    record.props.clear_if(|address| freed.contains(address));
                }
            }
        };
    }
    clear!(WRITABLE_STREAMS);
    clear!(TRANSFORM_STREAMS);
    clear!(WRITERS);
}

/// Test cleanup: drop the records of `ids` without retiring them.
#[cfg(test)]
pub(super) fn test_remove(ids: &[usize]) {
    evict(ids);
}
