//! #5989 `readable.tee()` — Web Streams `ReadableStreamTee`. Split out of
//! `streams.rs` to keep that file under the file-size gate.
//!
//! A tee'd source stream stays LIVE and is driven lazily by reads on its two
//! branches: a branch read with an empty queue pulls the source (`maybe_pull`
//! routes tee-branch pulls to the source), and each chunk the source enqueues
//! fans out to BOTH branches (see the hooks in `controller_enqueue` /
//! `controller_close` / `controller_error`).

use super::{
    build_iter_result, byob, close_pending, error_pending, idalloc, js_promise_resolve,
    next_stream_id, throw_type_error, ReadableState, ReadableStreamData, READABLE_STREAMS,
};
use perry_runtime::array::{js_array_alloc, js_array_push};
use perry_runtime::closure::ClosureHeader;
use perry_runtime::value::JSValue;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

lazy_static::lazy_static! {
    /// Source id -> its two branch ids (so the source's enqueue/close/error
    /// fan out to both branches).
    static ref TEE_SOURCE_BRANCHES: Mutex<HashMap<usize, (usize, usize)>> = Mutex::new(HashMap::new());
    /// Branch id -> its source (so a branch read pulls the source).
    static ref TEE_BRANCH_SOURCE: Mutex<HashMap<usize, usize>> = Mutex::new(HashMap::new());
    /// Tee sources with a pull job already queued (one job per source at a time).
    static ref TEE_PULLING: Mutex<HashSet<usize>> = Mutex::new(HashSet::new());
    /// Tee sources whose pull pipeline has delivered at least once. The
    /// COLD-START demand pull pays Node's full two-hop pipeline latency
    /// (`sourceReader.read()` resolution + `.then(fanout)` reaction); once
    /// the pipeline is warm, mid-stream re-parks resolve on the calibrated
    /// one-hop cadence (Node's pipelined stages overlap, so only the first
    /// delivery exposes the latency).
    static ref TEE_STARTED: Mutex<HashSet<usize>> = Mutex::new(HashSet::new());
}

/// Sentinel `reader_handle` stamped on a tee'd source so it can't be read
/// directly (spec: `tee()` acquires a reader on the source). It is never a real
/// entry in `READERS`, so reader lookups against it resolve to `None`.
const TEE_LOCK_SENTINEL: usize = usize::MAX;

pub(super) fn tee_branches_of(source: usize) -> Option<(usize, usize)> {
    TEE_SOURCE_BRANCHES.lock().unwrap().get(&source).copied()
}

pub(super) fn tee_source_of(branch: usize) -> Option<usize> {
    TEE_BRANCH_SOURCE.lock().unwrap().get(&branch).copied()
}

/// Deliver one chunk to a single tee branch: hand it to a parked read if one is
/// waiting, else queue it. Mirrors the default-reader path of
/// `js_readable_stream_controller_enqueue`.
pub(super) unsafe fn tee_deliver(branch: usize, chunk_bits: u64, is_byte: bool) {
    // A parked BYOB read takes the bytes directly (mirrors the
    // default-reader-vs-BYOB order in `js_readable_stream_controller_enqueue`).
    if is_byte && byob::service_pending_with_chunk(branch, chunk_bits) {
        return;
    }
    let popped = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&branch) {
            Some(s) if s.state == ReadableState::Readable => {
                if let Some(p) = s.pending_reads.pop_front() {
                    Some(p)
                } else {
                    let size = if is_byte {
                        byob::chunk_byte_length(chunk_bits)
                    } else {
                        1.0
                    };
                    s.push_chunk(chunk_bits, size);
                    None
                }
            }
            _ => None,
        }
    };
    if let Some(p) = popped {
        let result = build_iter_result(chunk_bits, false);
        js_promise_resolve(p, f64::from_bits(result));
    }
}

/// A branch has live read demand: a parked default read OR a parked BYOB read.
unsafe fn tee_branch_demand(a: usize, b: usize) -> bool {
    {
        let g = READABLE_STREAMS.lock().unwrap();
        if [a, b].iter().any(|id| {
            g.get(id)
                .map(|s| !s.pending_reads.is_empty())
                .unwrap_or(false)
        }) {
            return true;
        }
    }
    byob::has_pending(a) || byob::has_pending(b)
}

/// The tee'd source closed — close both branches (each drains its queue first,
/// per `js_reader_read`'s close-after-last-chunk handling) and drop the links.
pub(super) unsafe fn tee_close_branches(source: usize) {
    let Some((a, b)) = tee_branches_of(source) else {
        return;
    };
    for branch in [a, b] {
        let queue_empty = {
            let mut g = READABLE_STREAMS.lock().unwrap();
            match g.get_mut(&branch) {
                Some(s) => {
                    if s.state == ReadableState::Readable {
                        s.state = ReadableState::Closed;
                    }
                    s.chunks.is_empty()
                }
                None => true,
            }
        };
        if queue_empty {
            close_pending(branch);
        }
    }
    tee_unlink(source, a, b);
}

/// The tee'd source errored — error both branches and drop the links.
pub(super) unsafe fn tee_error_branches(source: usize, reason_bits: u64) {
    let Some((a, b)) = tee_branches_of(source) else {
        return;
    };
    // #6602: stamp the source itself terminal. It used to stay `Readable`
    // forever, leaking its registry entry, queued chunks and id once the tee
    // links were gone; nothing can drive it after the unlink below.
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&source) {
            if s.state == ReadableState::Readable {
                s.state = ReadableState::Errored;
                s.error_value = reason_bits;
            }
            s.clear_chunks();
        }
    }
    for branch in [a, b] {
        {
            let mut g = READABLE_STREAMS.lock().unwrap();
            if let Some(s) = g.get_mut(&branch) {
                if s.state == ReadableState::Readable {
                    s.state = ReadableState::Errored;
                    s.error_value = reason_bits;
                }
            }
        }
        error_pending(branch, reason_bits);
    }
    tee_unlink(source, a, b);
}

fn tee_unlink(source: usize, a: usize, b: usize) {
    TEE_SOURCE_BRANCHES.lock().unwrap().remove(&source);
    {
        let mut bs = TEE_BRANCH_SOURCE.lock().unwrap();
        bs.remove(&a);
        bs.remove(&b);
    }
    TEE_STARTED.lock().unwrap().remove(&source);
    // #6602: the unlinked source is terminal (close flow: Closed and drained
    // by pull discovery; error flow: stamped Errored above) — retire its id.
    // Its `TEE_LOCK_SENTINEL` reader_handle matches no READERS entry.
    idalloc::retire_readable_terminal(source);
}

/// #6602: eviction hook — scrub BOTH directions of every tee relationship an
/// evicted id participates in. A one-directional key removal would leave a
/// `TEE_SOURCE_BRANCHES` tuple naming an evicted branch (a cancelled branch
/// keeps its links), and once that id is reused the source's fan-out would
/// deliver chunks into an unrelated new stream. An evicted branch slot is
/// zeroed (0 never matches a registry entry) so the live sibling keeps
/// receiving; the entry drops when both slots are dead.
pub(super) fn evict_ids(batch: &[usize]) {
    let dead: HashSet<usize> = batch.iter().copied().collect();
    {
        let mut g = TEE_SOURCE_BRANCHES.lock().unwrap();
        g.retain(|source, (a, b)| {
            if dead.contains(source) {
                return false;
            }
            if dead.contains(a) {
                *a = 0;
            }
            if dead.contains(b) {
                *b = 0;
            }
            *a != 0 || *b != 0
        });
    }
    {
        let mut g = TEE_BRANCH_SOURCE.lock().unwrap();
        g.retain(|branch, source| !dead.contains(branch) && !dead.contains(source));
    }
    {
        let mut g = TEE_PULLING.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
    {
        let mut g = TEE_STARTED.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
}

/// Enqueue on a tee'd SOURCE: the chunk stays in the source queue (spec size
/// accounting included) and branches receive it only through the demand-driven
/// pull cycle. The old eager fan-out pre-filled both branch queues at
/// producer-flush time, so consumer reads resolved at attach with zero pull
/// ticks — reordering promise chains racing the stream (Next.js cold-start
/// head reorder). Returns false when the stream is not a tee source.
pub(super) unsafe fn tee_source_enqueue(
    id: usize,
    chunk: f64,
    chunk_bits: u64,
    is_byte_stream: bool,
) -> bool {
    if tee_branches_of(id).is_none() {
        return false;
    }
    // Per spec the strategy's size(chunk) runs once at enqueue time (same
    // accounting as the non-tee enqueue path).
    let size_cb = {
        let g = READABLE_STREAMS.lock().unwrap();
        g.get(&id).map(|s| s.strategy_size_cb).unwrap_or(0)
    };
    let size = if size_cb != 0 {
        let size = super::readable_strategy_size_to_number(
            perry_runtime::closure::js_closure_call1(size_cb as *const ClosureHeader, chunk),
        );
        if size.is_nan() || size < 0.0 || size.is_infinite() {
            super::throw_invalid_readable_strategy_size(id, size);
        }
        size
    } else if is_byte_stream {
        byob::chunk_byte_length(chunk_bits)
    } else {
        1.0
    };
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        if let Some(s) = g.get_mut(&id) {
            if s.state == ReadableState::Readable {
                s.push_chunk(chunk_bits, size);
            }
        }
    }
    tee_schedule_pull(id);
    true
}

/// Schedule ONE tee pull cycle as a microtask job. Spec/Node tick parity
/// (streamhops.js): every chunk — including chunks already buffered in the
/// source at `tee()` time — travels `sourceReader.read().then(fanout)`, a real
/// promise-reaction cycle, before a branch read resolves. The old shape
/// (branch queues pre-filled at tee() time / synchronous source pops) resolved
/// branch reads a tick early and delivered `done` immediately after the last
/// chunk, which reordered promise chains racing the stream (Next.js cold-start
/// head reorder: the flight read loop overtook the module-dep unblock chain).
pub(super) unsafe fn tee_schedule_pull(source: usize) {
    if !TEE_PULLING.lock().unwrap().insert(source) {
        return;
    }
    let f = tee_pull_microtask as *const u8;
    perry_runtime::closure::js_register_closure_arity(f, 0);
    let job = perry_runtime::closure::js_closure_alloc(f, 1);
    perry_runtime::closure::js_closure_set_capture_ptr(job, 0, source as i64);
    perry_runtime::builtins::js_queue_microtask(job as i64);
}

/// Demand-initiated pull entry — a branch read parked against an empty branch
/// queue (`maybe_pull` routing). On a COLD pipeline Node pays TWO microtask
/// hops before that read resolves: the `sourceReader.read()` promise
/// resolution plus the `.then(fanout)` reaction (streamsuite first-delivery
/// cadence: node's first chunk lands after t2, Perry's landed after t1 — the
/// one-hop-short residual behind the Next.js RSC Flight row-reorder). Once
/// the pipeline has delivered, Node's stages overlap and a mid-stream re-park
/// resolves on the one-hop cadence — so only the cold-start entry pays the
/// extra hop. CHAINED cycles and producer-side arrivals keep their existing
/// calibrated cadence throughout.
pub(super) unsafe fn tee_schedule_pull_demand(source: usize) {
    if TEE_STARTED.lock().unwrap().contains(&source) {
        tee_schedule_pull(source);
        return;
    }
    if !TEE_PULLING.lock().unwrap().insert(source) {
        return;
    }
    let f = tee_demand_hop as *const u8;
    perry_runtime::closure::js_register_closure_arity(f, 0);
    let job = perry_runtime::closure::js_closure_alloc(f, 1);
    perry_runtime::closure::js_closure_set_capture_ptr(job, 0, source as i64);
    perry_runtime::builtins::js_queue_microtask(job as i64);
}

/// The extra demand-entry hop: hand off to the real pull job one microtask
/// later. `TEE_PULLING` stays held across the hop (single-threaded microtask
/// dispatch — the remove+insert below has no interleaving window), so
/// coalescing against enqueue/close reroutes keeps working.
extern "C" fn tee_demand_hop(closure: *const ClosureHeader) -> f64 {
    unsafe {
        let source = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as usize;
        TEE_PULLING.lock().unwrap().remove(&source);
        tee_schedule_pull(source);
    }
    f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
}

/// The extra tick a byte-stream tee's CHAINED pull pays before the next
/// cycle (see the chain decision in `tee_pull_microtask`).
extern "C" fn tee_byte_chain_hop(closure: *const ClosureHeader) -> f64 {
    unsafe {
        let source = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as usize;
        tee_schedule_pull(source);
    }
    f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
}

extern "C" fn tee_close_tick(closure: *const ClosureHeader) -> f64 {
    unsafe {
        let source = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as usize;
        tee_close_branches(source);
    }
    f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
}

extern "C" fn tee_pull_microtask(closure: *const ClosureHeader) -> f64 {
    let undef = f64::from_bits(0x7FFC_0000_0000_0001); // TAG_UNDEFINED
    unsafe {
        let source = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as usize;
        TEE_PULLING.lock().unwrap().remove(&source);
        let Some((a, b)) = tee_branches_of(source) else {
            return undef;
        };
        // Deliver ONLY when a branch actually has a parked read: a consumer
        // busy with synchronous work between reads must find its next read
        // PENDING at attach (paying the pull cycle, like Node) — never a
        // pre-filled queue. A fast consumer parks before the next cycle runs,
        // keeping Node's one-chunk-per-tick cadence.
        if !tee_branch_demand(a, b) {
            return undef;
        }
        // Pop ONE chunk per cycle — per-chunk reaction cadence, like the
        // spec's per-read pull loop.
        let (chunk, closed, live_pull, is_byte) = {
            let mut g = READABLE_STREAMS.lock().unwrap();
            match g.get_mut(&source) {
                Some(s) => (
                    s.pop_chunk(),
                    s.state == ReadableState::Closed,
                    s.pull_cb != 0,
                    s.is_byte_stream,
                ),
                None => (None, true, false, false),
            }
        };
        match chunk {
            Some(bits) => {
                // Pipeline is warm from the first delivery on — later
                // demand entries skip the cold-start hop.
                TEE_STARTED.lock().unwrap().insert(source);
                // Spec order: branch-a sees the chunk before branch-b,
                // regardless of which branch's read triggered the pull.
                // Byte tees clone the chunk for branch-b (CloneAsUint8Array)
                // so the two branches never share a mutable buffer.
                let b_bits = if is_byte {
                    byob::clone_byte_chunk(bits)
                } else {
                    bits
                };
                tee_deliver(a, bits, is_byte);
                tee_deliver(b, b_bits, is_byte);
                // Chain the next cycle while the source has backlog or a
                // pending close; the demand gate at the cycle's entry keeps
                // pre-fill from ever happening.
                let more = {
                    let source_backlog = {
                        let g = READABLE_STREAMS.lock().unwrap();
                        g.get(&source)
                            .map(|s| !s.chunks.is_empty() || s.state == ReadableState::Closed)
                            .unwrap_or(false)
                    };
                    tee_branch_demand(a, b) || source_backlog
                };
                if more {
                    let close_only = {
                        let g = READABLE_STREAMS.lock().unwrap();
                        g.get(&source)
                            .map(|s| s.chunks.is_empty() && s.state == ReadableState::Closed)
                            .unwrap_or(true)
                    };
                    if is_byte && !close_only {
                        // Byte-stream tee (bytetee.js): Node's CHAINED pulls
                        // pay one extra tick per cycle (the byte path's
                        // clone/view hop); the first demand pull does not,
                        // and neither does the CLOSE-discovery cycle after
                        // the last chunk (node's done lands one tick after
                        // the final delivery pair). Each extra hop cedes a
                        // task-generation to racing promise cascades (the
                        // Next.js module-require chain must win that race).
                        let f = tee_byte_chain_hop as *const u8;
                        perry_runtime::closure::js_register_closure_arity(f, 0);
                        let job = perry_runtime::closure::js_closure_alloc(f, 1);
                        perry_runtime::closure::js_closure_set_capture_ptr(job, 0, source as i64);
                        perry_runtime::builtins::js_queue_microtask(job as i64);
                    } else {
                        tee_schedule_pull(source);
                    }
                }
            }
            None if closed => {
                if is_byte {
                    // Byte-stream tee: Node closes the branches PROMPTLY
                    // (bytetee.js: done right after the last chunk), unlike
                    // the default-stream tee below.
                    tee_close_branches(source);
                    return undef;
                }
                // Node tick parity: the DEFAULT tee branch close lands only
                // after ALL currently-pending promise microtasks (teedone.js:
                // DONE scales with metronome length — t5/t11/t19 for
                // 6/12/20-tick chains), i.e. it travels the nextTick queue,
                // which runs when the microtask queue exhausts. A plain
                // (non-tee) stream's close stays prompt. Defer via nextTick.
                let f = tee_close_tick as *const u8;
                perry_runtime::closure::js_register_closure_arity(f, 0);
                let job = perry_runtime::closure::js_closure_alloc(f, 1);
                perry_runtime::closure::js_closure_set_capture_ptr(job, 0, source as i64);
                perry_runtime::builtins::js_queue_next_tick(job as i64);
            }
            None => {
                if live_pull {
                    super::maybe_pull_force(source);
                }
            }
        }
    }
    undef
}

/// `readable.tee()` — Web Streams `ReadableStreamTee`. The source stays LIVE and
/// is pulled lazily by reads on the two returned branches: a branch read with an
/// empty queue pulls the source (`maybe_pull` routes tee-branch pulls to the
/// source), and each chunk the source enqueues fans out to BOTH branches. The
/// previous implementation snapshot-drained the source's current buffer and
/// closed it, which yielded two empty branches for a pull-driven source (e.g.
/// react-server-dom's RSC flight producer, which only produces on pull) — #5989.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_tee(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    let mut was_locked = false;
    let mut is_byte_stream = false;
    let mut source_state = ReadableState::Readable;
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) if s.reader_handle.is_none() => {
                is_byte_stream = s.is_byte_stream;
                source_state = s.state;
                // Keep the source LIVE — lock it against direct reads but leave
                // its state + queue + pull_cb intact so branch reads can drive
                // it. Buffered chunks are NOT copied into the branches: each
                // travels a tee pull cycle (see `tee_schedule_pull`) so branch
                // reads resolve with Node's tick cadence.
                s.reader_handle = Some(TEE_LOCK_SENTINEL);
            }
            Some(_) => {
                was_locked = true;
            }
            None => {}
        }
    };
    if was_locked {
        throw_type_error("ReadableStream is locked");
    }

    // Only an errored source yields terminal branches immediately. A CLOSED
    // source still has its buffered chunks delivered through pull cycles; the
    // close is discovered by the pull that finds the queue empty.
    let branch_state = match source_state {
        ReadableState::Errored => ReadableState::Errored,
        _ => ReadableState::Readable,
    };
    // An errored source's branches must carry its rejection reason.
    let branch_error_value = if branch_state == ReadableState::Errored {
        READABLE_STREAMS
            .lock()
            .unwrap()
            .get(&id)
            .map(|s| s.error_value)
            .unwrap_or(0)
    } else {
        0
    };
    let id_a = next_stream_id();
    let id_b = next_stream_id();
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        for new_id in [id_a, id_b] {
            g.insert(
                new_id,
                ReadableStreamData {
                    state: branch_state,
                    chunks: VecDeque::new(),
                    chunk_sizes: VecDeque::new(),
                    queue_total_size: 0.0,
                    pending_reads: VecDeque::new(),
                    start_cb: 0,
                    // No own pull_cb: a branch read pulls the SOURCE via the tee
                    // link (see `maybe_pull`). highWaterMark 0 so every read that
                    // finds an empty queue triggers a source pull.
                    pull_cb: 0,
                    cancel_cb: 0,
                    strategy_size_cb: 0,
                    high_water_mark: 0.0,
                    is_byte_stream,
                    pull_returns_byte_chunk: false,
                    pulling: false,
                    started: true,
                    reader_handle: None,
                    error_value: branch_error_value,
                    pending_error_after_chunks: None,
                    canceled: false,
                },
            );
        }
    }
    // Only a still-producible source needs live links; a terminal source's
    // branches already carry its final state + any drained chunks.
    if branch_state == ReadableState::Readable {
        TEE_SOURCE_BRANCHES.lock().unwrap().insert(id, (id_a, id_b));
        let mut bs = TEE_BRANCH_SOURCE.lock().unwrap();
        bs.insert(id_a, id);
        bs.insert(id_b, id);
    } else {
        // #6602: born-Errored branches have no tee lifecycle (no links) and no
        // error_pending ever fires for them — retire now so repeatedly teeing
        // an errored stream can't exhaust the band. Quarantine keeps their
        // registry entries, so reads still reject with the source's error.
        idalloc::retire_readable_terminal(id_a);
        idalloc::retire_readable_terminal(id_b);
    }

    let arr = js_array_alloc(2);
    js_array_push(arr, JSValue::from_bits(f64::to_bits(id_a as f64)));
    js_array_push(arr, JSValue::from_bits(f64::to_bits(id_b as f64)));
    f64::from_bits(JSValue::object_ptr(arr as *mut u8).bits())
}
