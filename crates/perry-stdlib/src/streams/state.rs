//! The Web Streams access layer: the one module that knows where a stream
//! family member's state lives. Every other module (streams and fetch) reaches
//! a record through the typed refs and the `with_*` accessors below.
//!
//! Today a ref is a band id and the records sit in five process-global
//! registries. The GC-objects plan (design 2026-11-25) swaps what is under
//! these functions: a ref becomes the heap object, a record's scalars its
//! native payload and its JS-valued fields (the ones `trace` visits) its
//! hidden JS-state slots. Callers do not change shape:
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
use super::*;

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

stream_ref!(
    /// A ReadableStream (its controller and BYOB request are derived values).
    RsRef
);
impl RsRef {
    /// The value of this stream's controller.
    #[inline]
    pub(crate) fn controller_value(self) -> f64 {
        controller_value(self.0)
    }

    /// The handle a native producer (the fetch engine's sink) keeps on the
    /// stream. Today the id; GC-objects plan: the stream's `OwnerLink`.
    #[inline]
    pub(crate) fn link(self) -> usize {
        self.0
    }

    /// The stream a native producer's handle names.
    #[inline]
    pub(crate) fn from_link(link: usize) -> Self {
        Self(link)
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
stream_ref!(
    /// A ReadableStreamDefaultReader or ReadableStreamBYOBReader.
    ReaderRef
);
stream_ref!(
    /// A WritableStreamDefaultWriter.
    WriterRef
);

/// User properties set on a stream-family value (`stream.allReady = p`).
pub(super) type Expandos = Vec<(String, u64)>;

// ─────────────────────────────────────────────────────────────────────
// Records
// ─────────────────────────────────────────────────────────────────────

pub(super) struct ReadableStreamData {
    pub(super) props: perry_ffi::NativePropertyBag,
    pub(super) native_source: Option<native::NativeSource>,
    pub(super) tee_cancel_promise: Option<*mut Promise>,
    pub(super) body_consumer: Option<native::BodyConsumer>,
    pub(super) state: ReadableState,
    /// Queued chunks as NaN-boxed pointers (typically Uint8Array via POINTER_TAG).
    pub(super) chunks: VecDeque<u64>,
    /// Per-chunk strategy sizes, parallel to `chunks` (#4915). The size of a
    /// chunk is `strategy.size(chunk)` when a custom strategy is installed,
    /// `chunk.byteLength` on byte streams, else 1 — computed once at enqueue
    /// time per spec. Mutate the queue only through `push_chunk` /
    /// `pop_chunk` / `clear_chunks` / `drain_chunks` so the running
    /// `queue_total_size` stays in sync.
    pub(super) chunk_sizes: VecDeque<f64>,
    /// Running sum of `chunk_sizes` — what `desiredSize` subtracts from the
    /// highWaterMark (real ByteLengthQueuingStrategy accounting, #4915).
    pub(super) queue_total_size: f64,
    /// FIFO of read() promises waiting for a chunk.
    pub(super) pending_reads: VecDeque<*mut Promise>,
    pub(super) start_cb: i64,
    pub(super) pull_cb: i64,
    pub(super) cancel_cb: i64,
    pub(super) strategy_size_cb: i64,
    pub(super) high_water_mark: f64,
    pub(super) is_byte_stream: bool,
    /// Internal source used by runtime APIs that hand back byte chunks instead
    /// of receiving a Web Streams controller.
    pub(super) pull_returns_byte_chunk: bool,
    pub(super) pulling: bool,
    pub(super) started: bool,
    pub(super) reader_handle: Option<usize>,
    pub(super) error_value: u64,
    pub(super) pending_error_after_chunks: Option<u64>,
    /// Per-controller cancel reason captured when `cancel()` is called.
    pub(super) canceled: bool,
    pub(super) disturbed: bool,
    /// Tee source: its two branches (a slot is 0 once that branch is gone).
    pub(super) tee_branches: Option<(usize, usize)>,
    /// Tee branch: the source it reads from.
    pub(super) tee_source: Option<usize>,
    /// Tee source: a pull cycle is scheduled or running.
    pub(super) tee_pulling: bool,
    /// Tee source: the first pull cycle ran.
    pub(super) tee_started: bool,
    /// Readable side of a TransformStream: that transform.
    pub(super) owner_transform: Option<usize>,
    /// Readable side of a TransformStream: write promises parked on
    /// backpressure, released when the consumer drains the readable.
    pub(super) transform_parked_writes: Vec<*mut Promise>,
    /// Readable side of a TransformStream: write jobs deferred while
    /// backpressured.
    pub(super) transform_backpressured_jobs: Vec<*mut ClosureHeader>,
    /// Outstanding BYOB `read(view)` requests.
    pub(super) byob_pending: VecDeque<ByobPending>,
    pub(super) expando: Expandos,
    pub(super) controller_expando: Expandos,
    pub(super) byob_request_expando: Expandos,
}

impl ReadableStreamData {
    /// A fresh readable record with everything but the controller's inputs
    /// at its initial value.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        start_cb: i64,
        pull_cb: i64,
        cancel_cb: i64,
        strategy_size_cb: i64,
        high_water_mark: f64,
        is_byte_stream: bool,
    ) -> Self {
        Self {
            props: Default::default(),
            native_source: None,
            tee_cancel_promise: None,
            body_consumer: None,
            state: ReadableState::Readable,
            chunks: VecDeque::new(),
            chunk_sizes: VecDeque::new(),
            queue_total_size: 0.0,
            pending_reads: VecDeque::new(),
            start_cb,
            pull_cb,
            cancel_cb,
            strategy_size_cb,
            high_water_mark,
            is_byte_stream,
            pull_returns_byte_chunk: false,
            pulling: false,
            started: false,
            reader_handle: None,
            error_value: 0,
            pending_error_after_chunks: None,
            canceled: false,
            disturbed: false,
            tee_branches: None,
            tee_source: None,
            tee_pulling: false,
            tee_started: false,
            owner_transform: None,
            transform_parked_writes: Vec::new(),
            transform_backpressured_jobs: Vec::new(),
            byob_pending: VecDeque::new(),
            expando: Vec::new(),
            controller_expando: Vec::new(),
            byob_request_expando: Vec::new(),
        }
    }

    pub(super) fn push_chunk(&mut self, bits: u64, size: f64) {
        self.chunks.push_back(bits);
        self.chunk_sizes.push_back(size);
        self.queue_total_size += size;
    }

    pub(super) fn pop_chunk(&mut self) -> Option<u64> {
        let bits = self.chunks.pop_front()?;
        let size = self.chunk_sizes.pop_front().unwrap_or(1.0);
        self.queue_total_size = (self.queue_total_size - size).max(0.0);
        Some(bits)
    }

    pub(super) fn clear_chunks(&mut self) {
        self.chunks.clear();
        self.chunk_sizes.clear();
        self.queue_total_size = 0.0;
    }

    /// ReadableStreamDefaultControllerClearAlgorithms /
    /// ReadableByteStreamControllerClearAlgorithms: drop the underlying
    /// source's algorithms and the strategy size so the source (and whatever
    /// its closures capture) can be collected while the stream itself stays
    /// referenced. Runs when the stream closes after its queue drained, when
    /// it errors, and after the cancel algorithm ran; no path calls them after.
    pub(super) fn clear_algorithms(&mut self) {
        self.start_cb = 0;
        self.pull_cb = 0;
        self.cancel_cb = 0;
        self.strategy_size_cb = 0;
    }

    /// The record's JS-valued fields (GC-objects plan: its hidden JS state).
    pub(super) fn trace<V: StreamRootVisitor>(&mut self, visitor: &mut V) {
        if let Some(slot) = self.props.slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
        if let Some(promise) = self.tee_cancel_promise.as_mut() {
            visitor.visit_raw_mut_ptr_slot(promise);
        }
        if let Some(source) = self.native_source.as_mut() {
            if let Some(promise) = source.promise.as_mut() {
                visitor.visit_raw_mut_ptr_slot(promise);
            }
        }
        if let Some(consumer) = self.body_consumer.as_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut consumer.promise);
        }
        visitor.visit_i64_slot(&mut self.start_cb);
        visitor.visit_i64_slot(&mut self.pull_cb);
        visitor.visit_i64_slot(&mut self.cancel_cb);
        visitor.visit_i64_slot(&mut self.strategy_size_cb);
        for chunk in self.chunks.iter_mut() {
            visit_stream_value_slot(visitor, chunk);
        }
        for promise in self.pending_reads.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(promise);
        }
        if self.state == ReadableState::Errored {
            visit_stream_value_slot(visitor, &mut self.error_value);
        }
        if let Some(error) = &mut self.pending_error_after_chunks {
            visit_stream_value_slot(visitor, error);
        }
        for promise in self.transform_parked_writes.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(promise);
        }
        for job in self.transform_backpressured_jobs.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(job);
        }
        for pending in self.byob_pending.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut pending.promise);
            visit_stream_value_slot(visitor, &mut pending.view_bits);
        }
        trace_expandos(visitor, &mut self.expando);
        trace_expandos(visitor, &mut self.controller_expando);
        trace_expandos(visitor, &mut self.byob_request_expando);
    }

    /// #11471: does the record hold an address of an exited thread's heap
    /// (expandos excepted: those drop alone, see `release_freed_expandos`)?
    fn holds_freed(&self, freed: &FreedRanges) -> bool {
        let promise = |p: *mut Promise| freed.contains(p as usize);
        [
            self.start_cb,
            self.pull_cb,
            self.cancel_cb,
            self.strategy_size_cb,
        ]
        .iter()
        .any(|&cb| freed.holds_i64(cb))
            || self.chunks.iter().any(|&bits| freed.holds_bits(bits))
            || self.pending_reads.iter().any(|&p| promise(p))
            || freed.holds_bits(self.error_value)
            || self
                .pending_error_after_chunks
                .is_some_and(|bits| freed.holds_bits(bits))
            || self.transform_parked_writes.iter().any(|&p| promise(p))
            || self
                .transform_backpressured_jobs
                .iter()
                .any(|&job| freed.contains(job as usize))
            || self
                .byob_pending
                .iter()
                .any(|p| promise(p.promise) || freed.holds_bits(p.view_bits))
    }

    fn release_freed_expandos(&mut self, freed: &FreedRanges) {
        release_freed(&mut self.expando, freed);
        release_freed(&mut self.controller_expando, freed);
        release_freed(&mut self.byob_request_expando, freed);
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
    pub(super) readable_handle: usize,
    pub(super) writable_handle: usize,
    pub(super) transform_cb: i64,
    pub(super) flush_cb: i64,
    pub(super) native: Option<NativeTransformKind>,
    pub(super) backpressure: bool,
    pub(super) expando: Expandos,
}

impl TransformStreamData {
    pub(super) fn trace<V: StreamRootVisitor>(&mut self, visitor: &mut V) {
        if let Some(slot) = self.props.slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
        visitor.visit_i64_slot(&mut self.transform_cb);
        visitor.visit_i64_slot(&mut self.flush_cb);
        trace_expandos(visitor, &mut self.expando);
    }
}

pub(super) struct ReaderData {
    pub(super) props: perry_ffi::NativePropertyBag,
    pub(super) stream_handle: usize,
    pub(super) locked: bool,
    pub(super) closed_promise: *mut Promise,
    /// True for readers minted by `getReader({ mode: "byob" })` /
    /// `new ReadableStreamBYOBReader(stream)` (#4915). BYOB readers route
    /// `read(view)` through `byob::js_reader_read_with_view`.
    pub(super) is_byob: bool,
    pub(super) expando: Expandos,
}

impl ReaderData {
    pub(super) fn new(stream_handle: usize, closed_promise: *mut Promise, is_byob: bool) -> Self {
        Self {
            props: Default::default(),
            stream_handle,
            locked: true,
            closed_promise,
            is_byob,
            expando: Vec::new(),
        }
    }

    pub(super) fn trace<V: StreamRootVisitor>(&mut self, visitor: &mut V) {
        if let Some(slot) = self.props.slot() {
            visitor.visit_nanbox_u64_slot(slot);
        }
        visitor.visit_raw_mut_ptr_slot(&mut self.closed_promise);
        trace_expandos(visitor, &mut self.expando);
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

unsafe impl Send for ReadableStreamData {}
unsafe impl Send for WritableStreamData {}
unsafe impl Send for TransformStreamData {}
unsafe impl Send for ReaderData {}
unsafe impl Send for WriterData {}

fn trace_expandos<V: StreamRootVisitor>(visitor: &mut V, entries: &mut Expandos) {
    for (_, bits) in entries.iter_mut() {
        visit_stream_value_slot(visitor, bits);
    }
}

// ─────────────────────────────────────────────────────────────────────
// Registries (private to this module)
// ─────────────────────────────────────────────────────────────────────

type Registry<T> = std::sync::LazyLock<Mutex<HashMap<usize, T>>>;

static READABLE_STREAMS: Registry<ReadableStreamData> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static WRITABLE_STREAMS: Registry<WritableStreamData> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static TRANSFORM_STREAMS: Registry<TransformStreamData> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static READERS: Registry<ReaderData> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static WRITERS: Registry<WriterData> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

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
    READABLE_STREAMS,
    RsRef,
    ReadableStreamData,
    with_rs,
    rs_rec,
    rs_exists,
    insert_rs
);
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
    READERS,
    ReaderRef,
    ReaderData,
    with_reader,
    reader_rec,
    reader_exists,
    insert_reader
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

/// Run `f` on the expando list of any stream-family value id: a stream,
/// reader, writer or transform, or a readable's controller / BYOB request
/// (those keep theirs on the readable's record).
pub(super) fn with_expandos<R>(id: usize, f: impl FnOnce(&mut Expandos) -> R) -> Option<R> {
    let requested = byob_request_id_stream(id);
    if requested != 0 {
        return with_rs(RsRef(requested), |s| f(&mut s.byob_request_expando));
    }
    let controlled = controller_id_stream(id);
    if controlled != 0 {
        return with_rs(RsRef(controlled), |s| f(&mut s.controller_expando));
    }
    if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&id) {
        return Some(f(&mut s.expando));
    }
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&id) {
        return Some(f(&mut s.expando));
    }
    if let Some(t) = TRANSFORM_STREAMS.lock().unwrap().get_mut(&id) {
        return Some(f(&mut t.expando));
    }
    if let Some(r) = READERS.lock().unwrap().get_mut(&id) {
        return Some(f(&mut r.expando));
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
    if let Ok(mut map) = READABLE_STREAMS.lock() {
        map.values_mut().for_each(|s| s.trace(visitor));
    }
    if let Ok(mut map) = WRITABLE_STREAMS.lock() {
        map.values_mut().for_each(|s| s.trace(visitor));
    }
    if let Ok(mut map) = TRANSFORM_STREAMS.lock() {
        map.values_mut().for_each(|t| t.trace(visitor));
    }
    if let Ok(mut map) = READERS.lock() {
        map.values_mut().for_each(|r| r.trace(visitor));
    }
    if let Ok(mut map) = WRITERS.lock() {
        map.values_mut().for_each(|w| w.trace(visitor));
    }
}

/// Remove the records of `batch` (quarantine eviction or thread exit). The
/// tee links of a surviving partner that named an evicted id are cleared:
/// an evicted branch slot is zeroed (0 never matches a record) so the live
/// sibling keeps receiving, and a source drops its link once both slots are
/// gone; a branch of an evicted source forgets its source. Without that, a
/// reused id would receive an unrelated tee's chunks.
pub(super) fn evict(batch: &[usize]) {
    use std::sync::PoisonError;
    let mut tee_links: Vec<(usize, Option<(usize, usize)>, Option<usize>)> = Vec::new();
    {
        let mut g = READABLE_STREAMS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for id in batch {
            if let Some(s) = g.remove(id) {
                if s.tee_branches.is_some() || s.tee_source.is_some() {
                    tee_links.push((*id, s.tee_branches, s.tee_source));
                }
            }
        }
        for (id, branches, source) in tee_links {
            if let Some((a, b)) = branches {
                for branch in [a, b] {
                    if let Some(s) = g.get_mut(&branch) {
                        if s.tee_source == Some(id) {
                            s.tee_source = None;
                        }
                    }
                }
            }
            if let Some(source) = source {
                if let Some(s) = g.get_mut(&source) {
                    if let Some((a, b)) = s.tee_branches.as_mut() {
                        if *a == id {
                            *a = 0;
                        }
                        if *b == id {
                            *b = 0;
                        }
                        if *a == 0 && *b == 0 {
                            s.tee_branches = None;
                        }
                    }
                }
            }
        }
    }
    let mut writables = WRITABLE_STREAMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut transforms = TRANSFORM_STREAMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut readers = READERS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut writers = WRITERS.lock().unwrap_or_else(PoisonError::into_inner);
    for id in batch {
        writables.remove(id);
        transforms.remove(id);
        readers.remove(id);
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
        let mut g = READABLE_STREAMS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for (&id, s) in g.iter_mut() {
            known.insert(id);
            s.release_freed_expandos(freed);
            if let Some(reader) = s.reader_handle {
                links.push((id, reader));
            }
            if s.holds_freed(freed) {
                dead.insert(id);
            }
        }
    }
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
            links.push((id, t.readable_handle));
            links.push((id, t.writable_handle));
            if freed.holds_i64(t.transform_cb) || freed.holds_i64(t.flush_cb) {
                dead.insert(id);
            }
        }
    }
    {
        let mut g = READERS.lock().unwrap_or_else(PoisonError::into_inner);
        for (&id, r) in g.iter_mut() {
            known.insert(id);
            release_freed(&mut r.expando, freed);
            links.push((id, r.stream_handle));
            if promise(r.closed_promise) {
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

/// #11471 test probe: which of the five registries hold `id`, as a bitmask
/// (readable 1, writable 2, transform 4, reader 8, writer 16).
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
        s.owner_transform.is_some()
            || !s.transform_parked_writes.is_empty()
            || !s.transform_backpressured_jobs.is_empty()
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
    try_table!(READABLE_STREAMS);
    try_table!(WRITABLE_STREAMS);
    try_table!(TRANSFORM_STREAMS);
    try_table!(READERS);
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
    clear!(READABLE_STREAMS);
    clear!(WRITABLE_STREAMS);
    clear!(TRANSFORM_STREAMS);
    clear!(READERS);
    clear!(WRITERS);
}

/// Test reset: drop every readable record.
#[cfg(test)]
pub(super) fn test_clear_readables() {
    READABLE_STREAMS.lock().unwrap().clear();
}

/// Test cleanup: drop the records of `ids` without retiring them.
#[cfg(test)]
pub(super) fn test_remove(ids: &[usize]) {
    evict(ids);
}
