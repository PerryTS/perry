//! Native producers share the ordinary stream queue, readers, tee and GC scanner.
//! Their pull/cancel hooks contain numeric exchange keys, never heap pointers.
use super::*;

pub(super) struct NativeSource {
    pub(super) promise: Option<*mut Promise>,
    pull: fn(usize),
    cancel: fn(usize),
}

pub(super) struct BodyConsumer {
    pub(super) promise: *mut Promise,
    bytes: Vec<u8>,
    metadata: String,
    finish: fn(Vec<u8>, String) -> Result<f64, f64>,
}

const BODY_READER: usize = usize::MAX - 1;
pub(crate) const NATIVE_BODY_WINDOW: usize = 64 * 1024;

pub(crate) fn alloc_native_readable(
    promise: *mut Promise,
    pull: fn(usize),
    cancel: fn(usize),
) -> RsRef {
    ensure_gc_registered();
    let id = alloc_readable_with_type(0, 0, 0, NATIVE_BODY_WINDOW as f64, true);
    with_rs(RsRef(id), |stream| {
        stream.started = true;
        stream.native_source = Some(NativeSource {
            promise: Some(promise),
            pull,
            cancel,
        });
    })
    .unwrap();
    RsRef(id)
}

pub(crate) fn take_native_promise(stream: RsRef) -> Option<*mut Promise> {
    rs_rec(stream)
        .as_deref_mut()?
        .native_source
        .as_mut()?
        .promise
        .take()
}

pub(crate) fn native_capacity(id: usize) -> usize {
    rs_rec(RsRef(id)).as_deref().map_or(0, |s| {
        if s.state != ReadableState::Readable {
            return 0;
        }
        NATIVE_BODY_WINDOW.saturating_sub(s.queue_total_size as usize)
    })
}

pub(super) fn pull_native(id: usize) -> bool {
    let pull = rs_rec(RsRef(id))
        .as_deref()
        .and_then(|s| s.native_source.as_ref())
        .map(|s| s.pull);
    if let Some(pull) = pull {
        pull(id);
        true
    } else {
        false
    }
}

pub(super) fn cancel_native(id: usize) {
    let cancel = rs_rec(RsRef(id))
        .as_deref_mut()
        .and_then(|s| s.native_source.take())
        .map(|s| s.cancel);
    if let Some(cancel) = cancel {
        cancel(id);
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
            s.reader_handle = Some(BODY_READER);
            s.body_consumer = Some(BodyConsumer {
                promise,
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
            if s.state != ReadableState::Readable && s.chunks.is_empty() {
                s.body_consumer.take().map(|c| (c, s.state, s.error_value))
            } else {
                None
            }
        })
    };
    if let Some((c, state, error)) = terminal {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise = scope.root_raw_mut_ptr(c.promise);
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
        idalloc::retire_readable_terminal(id);
    } else if rs_rec(RsRef(id))
        .as_deref()
        .is_some_and(|s| s.body_consumer.is_some())
    {
        maybe_pull_force(id);
    }
}
