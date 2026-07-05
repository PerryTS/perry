//! #5437: expando properties on live Web Stream handles. React's
//! `renderToReadableStream` attaches `stream.allReady = promise`; JS objects
//! accept arbitrary properties, so a live stream-band handle must too. Split
//! out of `streams.rs` to keep that file under the file-size gate.

use super::subclass::js_stream_handle_kind;
use super::visit_stream_value_slot;
use std::collections::HashMap;
use std::sync::Mutex;

lazy_static::lazy_static! {
    /// #5437: expando properties attached to live stream handles
    /// (`stream.allReady = promise` in React's renderToReadableStream).
    /// Keyed by stream id; values are NaN-boxed u64 bits, GC-traced in
    /// `scan_expando_roots`.
    static ref STREAM_EXPANDO: Mutex<HashMap<usize, Vec<(String, u64)>>> = Mutex::new(HashMap::new());
}

/// #5437: store an expando property on a live stream handle. Returns 1 when
/// stored. Any live stream-band handle kind accepts expandos (streams,
/// readers, writers) — matching JS objects accepting arbitrary properties.
pub(crate) unsafe extern "C" fn stream_expando_set_hook(
    id: usize,
    key_ptr: *const u8,
    key_len: usize,
    value: f64,
) -> i32 {
    if key_ptr.is_null() || js_stream_handle_kind(id) == 0 {
        return 0;
    }
    let bytes = std::slice::from_raw_parts(key_ptr, key_len);
    let Ok(key) = std::str::from_utf8(bytes) else {
        return 0;
    };
    if let Ok(mut map) = STREAM_EXPANDO.lock() {
        let entry = map.entry(id).or_default();
        if let Some(slot) = entry.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value.to_bits();
        } else {
            entry.push((key.to_string(), value.to_bits()));
        }
        return 1;
    }
    0
}

/// #5437: read an expando property off a stream handle (undefined bits when
/// absent).
pub(crate) fn stream_expando_get(id: usize, key: &str) -> Option<f64> {
    let map = STREAM_EXPANDO.lock().ok()?;
    let entry = map.get(&id)?;
    entry
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, bits)| f64::from_bits(*bits))
}

/// #5437: drop a handle's expando entry when the stream is torn down
/// (closed/errored). Stream ids are monotonic (never reused), so without this
/// the table would grow one entry per stream ever created — an unbounded leak
/// over a long-running SSR server's lifetime, and `scan_expando_roots` would
/// keep those values alive forever. Called from the readable-stream terminal
/// paths, after any `stream.allReady`-style expando has already been consumed
/// during the render.
pub(crate) fn stream_expando_clear(id: usize) {
    if let Ok(mut map) = STREAM_EXPANDO.lock() {
        map.remove(&id);
    }
}

/// #5437: GC-trace expando values. Called from `scan_stream_roots_mut`.
pub(crate) fn scan_expando_roots(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut map) = STREAM_EXPANDO.lock() {
        for entries in map.values_mut() {
            for (_, bits) in entries.iter_mut() {
                visit_stream_value_slot(visitor, bits);
            }
        }
    }
}
