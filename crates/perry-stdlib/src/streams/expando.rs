//! #5437: expando properties on live Web Stream handles. React's
//! `renderToReadableStream` attaches `stream.allReady = promise`; JS objects
//! accept arbitrary properties, so a live stream-band handle must too. Split
//! out of `streams.rs` to keep that file under the file-size gate. The values
//! live on the record of the handle they were set on (`state::with_expandos`).

use super::state::with_expandos;
use super::subclass::js_stream_handle_kind;

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
    let stored = with_expandos(id, |entries| {
        if let Some(slot) = entries.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value.to_bits();
        } else {
            entries.push((key.to_string(), value.to_bits()));
        }
    });
    i32::from(stored.is_some())
}

/// #5437: read an expando property off a stream handle (undefined bits when
/// absent).
pub(crate) fn stream_expando_get(id: usize, key: &str) -> Option<f64> {
    with_expandos(id, |entries| {
        entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, bits)| f64::from_bits(*bits))
    })
    .flatten()
}

/// #5437: drop the expandos set on `id` itself (a stream reaching its
/// terminal state, a released reader or writer).
pub(crate) fn stream_expando_clear(id: usize) {
    with_expandos(id, |entries| entries.clear());
}
