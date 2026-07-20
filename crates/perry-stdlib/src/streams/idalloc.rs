//! #6602: Web Streams handle-id allocation with recycling.
//!
//! Ids for streams, readers, writers and transform streams all come from the
//! fixed band `[STREAM_HANDLE_ID_START, STREAM_HANDLE_ID_END)` owned by
//! `perry_runtime::value::addr_class`. The band is only `0x100000` wide and
//! every band-gated classification path stops recognizing a handle past its
//! end, so the old monotonic never-reused counter exhausted it after ~21k
//! requests on a server that mints ~48 stream-family ids per request.
//!
//! Lifecycle: an id is RETIRED when its object reaches a terminal state —
//! readable: errored, or closed with queue and pending reads drained;
//! writable: closed/errored with no write in flight; reader/writer:
//! `releaseLock()` (an attached reader/writer also dies with its terminal
//! stream); transform: its writable side reaching terminal; pipeTo lock ids
//! (never registered anywhere): pipe completion. Retired ids sit in a FIFO
//! quarantine with their registry entries INTACT, so a wrapper still held by
//! user code keeps full post-terminal semantics (`locked`, `getReader()` on a
//! closed stream, error values, promise getters). Only when the quarantine
//! overflows `PERRY_STREAM_ID_QUARANTINE` (default 16384; the debug knob that
//! makes exhaustion tests cheap) is the oldest id EVICTED — registry and
//! side-table entries removed, GC roots dropped — and pushed onto the free
//! list `next_stream_id` reuses. A stale wrapper can only observe the
//! recycling after its id survived a full quarantine's worth of later
//! teardowns; before that it degrades to the registry-miss defaults, the same
//! answers a plain in-band number gets.
//!
//! Locking: the allocator mutex is a LEAF lock — `get_reader` / `get_writer`
//! call `next_stream_id` while holding a registry lock, so nothing here may
//! acquire a registry lock while holding the allocator lock. Eviction's
//! registry cleanup therefore runs BETWEEN two allocator-lock sections; the
//! evicted batch stays in `pooled` throughout, so a concurrent duplicate
//! retire of a mid-eviction id is rejected, and `retire_*` entry checks run
//! before the allocator lock is taken.

use super::transform::TRANSFORM_PAIRS;
use super::{
    byob, expando, tee, ReadableState, WritableState, READABLE_STREAMS, READERS,
    STREAM_HANDLE_ID_END, STREAM_HANDLE_ID_START, TRANSFORM_STREAMS, WRITABLE_STREAMS, WRITERS,
};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

pub(super) const DEFAULT_QUARANTINE: usize = 16384;

struct IdAlloc {
    /// Next never-yet-used id (monotonic bump while fresh ids remain).
    next: usize,
    /// Evicted ids ready for reuse, oldest first.
    free: VecDeque<usize>,
    /// Terminal ids in quarantine, retirement order. Registry entries intact.
    retired: VecDeque<usize>,
    /// Every id currently in `retired` or `free` — the double-retire /
    /// double-free guard. Ids leave on allocation.
    pooled: HashSet<usize>,
    /// Live pipeTo lock ids. Pipe lock ids never own a registry entry, so the
    /// allocator itself tracks their ownership: marked atomically at
    /// allocation, unmarked exactly once at retire. A stale duplicate release
    /// finds no mark and can never touch the id's next life (a registry-
    /// absence probe instead would race the alloc→register window of a reused
    /// id).
    pipe_ids: HashSet<usize>,
}

lazy_static::lazy_static! {
    static ref IDALLOC: Mutex<IdAlloc> = Mutex::new(IdAlloc {
        next: STREAM_HANDLE_ID_START,
        free: VecDeque::new(),
        retired: VecDeque::new(),
        pooled: HashSet::new(),
        pipe_ids: HashSet::new(),
    });
}

/// Quarantine length that triggers eviction of the oldest retired id.
/// 0 = not yet initialized from `PERRY_STREAM_ID_QUARANTINE`.
static QUARANTINE: AtomicUsize = AtomicUsize::new(0);

fn quarantine_limit() -> usize {
    let q = QUARANTINE.load(Ordering::Relaxed);
    if q != 0 {
        return q;
    }
    let q = std::env::var("PERRY_STREAM_ID_QUARANTINE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        // Below 2 the reuse distance vanishes; above half the band the
        // allocator can never recycle enough to matter.
        .map(|v| v.clamp(2, (STREAM_HANDLE_ID_END - STREAM_HANDLE_ID_START) / 2))
        .unwrap_or(DEFAULT_QUARANTINE);
    QUARANTINE.store(q, Ordering::Relaxed);
    q
}

#[cfg(test)]
pub(super) fn set_quarantine_limit_for_test(limit: usize) {
    QUARANTINE.store(limit.max(2), Ordering::Relaxed);
}

/// How many times `id` currently sits in the quarantine + free pools
/// (correctness invariant: never more than 1).
#[cfg(test)]
pub(super) fn test_pool_occurrences(id: usize) -> usize {
    let a = IDALLOC.lock().unwrap();
    a.retired.iter().filter(|&&r| r == id).count() + a.free.iter().filter(|&&f| f == id).count()
}

/// Evict and DISCARD everything pooled so a test observes deterministic
/// free-list behavior regardless of what earlier tests retired. The
/// discarded ids are simply never reused — harmless at test scale.
#[cfg(test)]
pub(super) fn test_reset_pools() {
    let retired = {
        let mut a = IDALLOC.lock().unwrap();
        let retired: Vec<usize> = a.retired.drain(..).collect();
        a.free.clear();
        a.pooled.clear();
        retired
    };
    evict_ids(&retired);
}

fn alloc_locked(a: &mut IdAlloc) -> usize {
    if let Some(id) = a.free.pop_front() {
        a.pooled.remove(&id);
        // Free ids are never marked (the mark clears at retire) — defensive.
        a.pipe_ids.remove(&id);
        return id;
    }
    if a.next >= STREAM_HANDLE_ID_END {
        panic!(
            "Web Streams handle id band exhausted: {} ids live or quarantined \
             (see PERRY_STREAM_ID_QUARANTINE)",
            STREAM_HANDLE_ID_END - STREAM_HANDLE_ID_START
        );
    }
    let id = a.next;
    a.next += 1;
    id
}

/// Allocate a stream-family handle id. Prefers recycled ids (their previous
/// registry entries were fully evicted), then fresh monotonic ids. May be
/// called with registry locks held — takes only the allocator (leaf) lock.
pub(super) fn next_stream_id() -> usize {
    alloc_locked(&mut IDALLOC.lock().unwrap())
}

/// Allocate a pipeTo lock id and mark its ownership in the same lock section,
/// so retirement can key on the mark instead of probing registries.
pub(super) fn next_pipe_lock_id() -> usize {
    let mut a = IDALLOC.lock().unwrap();
    let id = alloc_locked(&mut a);
    a.pipe_ids.insert(id);
    id
}

/// Under the allocator lock: pool `id` into quarantine. Returns the overflow
/// batch whose eviction the caller must finish AFTER dropping the lock; the
/// drained ids stay in `pooled` until they reach `free`.
fn pool_retired(a: &mut IdAlloc, id: usize) -> Option<Vec<usize>> {
    if !a.pooled.insert(id) {
        return None;
    }
    a.retired.push_back(id);
    let limit = quarantine_limit();
    if a.retired.len() <= limit {
        return None;
    }
    let n = a.retired.len() - limit;
    Some(a.retired.drain(..n).collect())
}

/// Registry cleanup + free-list handoff for a quarantine overflow batch.
/// Must run with NO locks held (takes registry locks, then the allocator).
fn finish_eviction(evicted: Option<Vec<usize>>) {
    let Some(evicted) = evicted else { return };
    evict_ids(&evicted);
    IDALLOC.lock().unwrap().free.extend(evicted);
}

/// Move `id` into quarantine; once it ages past the quarantine window, evict
/// its registry footprint and hand the id to the free list. Callers must hold
/// NO streams locks and must have verified the id's terminal state.
fn retire(id: usize) {
    if !(STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id) {
        return;
    }
    let evicted = {
        let mut a = IDALLOC.lock().unwrap();
        pool_retired(&mut a, id)
    };
    finish_eviction(evicted);
}

/// Remove every registry and side-table trace of the batch. After this the
/// ids answer like plain in-band numbers until they are reallocated; the GC
/// scanners stop rooting their callbacks, chunks and promises.
fn evict_ids(batch: &[usize]) {
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
    {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
    {
        let mut g = TRANSFORM_STREAMS.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
    {
        let mut g = READERS.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
    {
        let mut g = WRITERS.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
    {
        let mut g = TRANSFORM_PAIRS.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
    {
        let mut g = super::transform::TRANSFORM_WRITE_RELEASES.lock().unwrap();
        for id in batch {
            g.remove(id);
        }
    }
    byob::evict_ids(batch);
    tee::evict_ids(batch);
    for &id in batch {
        expando::stream_expando_clear(id);
    }
}

/// Retire a readable stream that reached a terminal state: errored, or closed
/// with its queue and pending reads drained. A still-attached reader dies
/// with it (a released one goes through [`retire_reader_if_released`]); the
/// tee lock sentinel (`usize::MAX`) fails `retire`'s band guard.
pub(super) fn retire_readable_terminal(stream_id: usize) {
    let reader = {
        let g = READABLE_STREAMS.lock().unwrap();
        match g.get(&stream_id) {
            Some(s)
                if match s.state {
                    ReadableState::Errored => true,
                    ReadableState::Closed => s.chunks.is_empty() && s.pending_reads.is_empty(),
                    ReadableState::Readable => false,
                } =>
            {
                s.reader_handle
            }
            _ => return,
        }
    };
    retire(stream_id);
    if let Some(reader_id) = reader {
        let attached = READERS
            .lock()
            .unwrap()
            .get(&reader_id)
            .map(|r| r.stream_handle == stream_id)
            .unwrap_or(false);
        if attached {
            retire(reader_id);
        }
    }
}

/// Retire a writable stream that reached a terminal state (closed / errored /
/// aborted) with no write in flight and an empty queue. A still-attached
/// writer dies with it, and so does the transform stream whose sink this
/// writable was (its readable side retires through its own terminal path).
pub(super) fn retire_writable_terminal(stream_id: usize) {
    let writer = {
        let g = WRITABLE_STREAMS.lock().unwrap();
        match g.get(&stream_id) {
            Some(s)
                if matches!(s.state, WritableState::Closed | WritableState::Errored)
                    && !s.in_flight
                    && s.write_queue.is_empty() =>
            {
                s.writer_handle
            }
            _ => return,
        }
    };
    retire(stream_id);
    if let Some(writer_id) = writer {
        let attached = WRITERS
            .lock()
            .unwrap()
            .get(&writer_id)
            .map(|w| w.stream_handle == stream_id)
            .unwrap_or(false);
        if attached {
            retire(writer_id);
        }
    }
    let transform_id = TRANSFORM_PAIRS.lock().unwrap().get(&stream_id).copied();
    if let Some(transform_id) = transform_id {
        if TRANSFORM_STREAMS
            .lock()
            .unwrap()
            .contains_key(&transform_id)
        {
            retire(transform_id);
        }
    }
}

/// Retire a reader whose lock was released (`releaseLock()` / iterator
/// return). Post-release calls already answer through the registry-miss arms
/// (`read()` → TypeError, `closed` → resolved promise).
pub(super) fn retire_reader_if_released(reader_id: usize) {
    let released = READERS
        .lock()
        .unwrap()
        .get(&reader_id)
        .map(|r| !r.locked)
        .unwrap_or(false);
    if released {
        retire(reader_id);
    }
}

/// Writer counterpart of [`retire_reader_if_released`].
pub(super) fn retire_writer_if_released(writer_id: usize) {
    let released = WRITERS
        .lock()
        .unwrap()
        .get(&writer_id)
        .map(|w| !w.locked)
        .unwrap_or(false);
    if released {
        retire(writer_id);
    }
}

/// Retire a pipeTo lock id. These ids are stamped into `reader_handle` /
/// `writer_handle` as lock markers but never own a registry entry, so the
/// terminal-state retires above can't see them. Retirement keys on the
/// ownership mark set by [`next_pipe_lock_id`]: the mark clears exactly once,
/// so a stale duplicate release is a no-op and can never retire the id's
/// next life, however far allocation has moved on.
pub(super) fn retire_pipe_lock_id(id: usize) {
    if !(STREAM_HANDLE_ID_START..STREAM_HANDLE_ID_END).contains(&id) {
        return;
    }
    let evicted = {
        let mut a = IDALLOC.lock().unwrap();
        if !a.pipe_ids.remove(&id) {
            return;
        }
        pool_retired(&mut a, id)
    };
    finish_eviction(evicted);
}
