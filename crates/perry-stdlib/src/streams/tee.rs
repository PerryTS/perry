//! #5989 `readable.tee()` — Web Streams `ReadableStreamTee`. Split out of
//! `streams.rs` to keep that file under the file-size gate.
//!
//! A tee'd source stream stays LIVE and is driven lazily by reads on its two
//! branches: a branch read with an empty queue pulls the source (`maybe_pull`
//! routes tee-branch pulls to the source), and each chunk the source enqueues
//! fans out to BOTH branches (see the hooks in `controller_enqueue` /
//! `controller_close` / `controller_error`).

use super::{
    build_iter_result, byob, close_pending, error_pending, js_promise_resolve, next_id,
    throw_type_error, ReadableState, ReadableStreamData, NEXT_STREAM_ID, READABLE_STREAMS,
};
use perry_runtime::array::{js_array_alloc, js_array_push};
use perry_runtime::value::JSValue;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

lazy_static::lazy_static! {
    /// Source id -> its two branch ids (so the source's enqueue/close/error
    /// fan out to both branches).
    static ref TEE_SOURCE_BRANCHES: Mutex<HashMap<usize, (usize, usize)>> = Mutex::new(HashMap::new());
    /// Branch id -> its source (so a branch read pulls the source).
    static ref TEE_BRANCH_SOURCE: Mutex<HashMap<usize, usize>> = Mutex::new(HashMap::new());
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
    let mut bs = TEE_BRANCH_SOURCE.lock().unwrap();
    bs.remove(&a);
    bs.remove(&b);
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
    let existing: Vec<u64> = {
        let mut g = READABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) if s.reader_handle.is_none() => {
                is_byte_stream = s.is_byte_stream;
                source_state = s.state;
                let drained = s.drain_chunks();
                // Keep the source LIVE — lock it against direct reads but leave
                // its state + pull_cb intact so branch reads can drive it.
                s.reader_handle = Some(TEE_LOCK_SENTINEL);
                drained
            }
            Some(_) => {
                was_locked = true;
                Vec::new()
            }
            None => Vec::new(),
        }
    };
    if was_locked {
        throw_type_error("ReadableStream is locked");
    }

    // A branch mirrors the source's terminal state; a live source yields live
    // branches whose future chunks arrive by fan-out.
    let branch_state = match source_state {
        ReadableState::Errored => ReadableState::Errored,
        ReadableState::Closed => ReadableState::Closed,
        _ => ReadableState::Readable,
    };
    let id_a = next_id(&NEXT_STREAM_ID);
    let id_b = next_id(&NEXT_STREAM_ID);
    {
        let mut g = READABLE_STREAMS.lock().unwrap();
        for new_id in [id_a, id_b] {
            g.insert(
                new_id,
                ReadableStreamData {
                    state: branch_state,
                    chunks: existing.iter().copied().collect(),
                    chunk_sizes: existing.iter().map(|_| 1.0).collect(),
                    queue_total_size: existing.len() as f64,
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
                    error_value: 0,
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
    }

    let arr = js_array_alloc(2);
    js_array_push(arr, JSValue::from_bits(f64::to_bits(id_a as f64)));
    js_array_push(arr, JSValue::from_bits(f64::to_bits(id_b as f64)));
    f64::from_bits(JSValue::object_ptr(arr as *mut u8).bits())
}
