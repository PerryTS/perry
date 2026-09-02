//! Pump-side stdin write completion (#9493), split out of `reactor.rs` to keep it under the 2000-line cap.
//! A child module, so `use super::*` reaches the reactor's private state.

use super::*;

/// Pump-side completion of queued stdin bytes (#9493): account for them, and
/// once the queue is empty emit `'drain'` and then fire the waiting callbacks
/// in order — Node's `afterWrite` order — and perform the close an `end()`
/// left pending.
pub(super) fn cp_stdin_written(handle: u64, len: usize, broken: bool) {
    let mut callbacks: Vec<u64> = Vec::new();
    let mut emit_drain = false;
    let mut writable_length = 0;
    let mut close_now = false;
    let mut found = false;
    {
        let mut guard = cp_live_lock();
        if let Some(lc) = guard.as_mut().and_then(|m| m.get_mut(&handle)) {
            if let Some(stdin) = lc.stdin.as_mut() {
                found = true;
                stdin.queued = if broken {
                    0
                } else {
                    stdin.queued.saturating_sub(len)
                };
                writable_length = stdin.queued;
                if stdin.queued == 0 {
                    callbacks = std::mem::take(&mut stdin.callbacks);
                    emit_drain = stdin.need_drain && !stdin.end_pending && !broken;
                    stdin.need_drain = false;
                    close_now = stdin.end_pending || broken;
                }
            }
            if close_now {
                // EOF for the child: the drain thread's dup closes with its
                // channel, which this drop ends.
                lc.stdin = None;
            }
        }
    }
    if !found {
        return;
    }
    let Some(cp_bits) = cp_lookup_cp_bits(handle) else {
        return;
    };
    let stream = cp_stdio_stream(f64::from_bits(cp_bits), 0);
    if super::cp_object_ptr(stream).is_none() {
        return;
    }
    cp_set_field(stream, b"writableLength", writable_length as f64);
    if writable_length == 0 {
        cp_set_field(stream, b"writableNeedDrain", TAG_FALSE_F64);
    }
    // The callbacks came out of the registry root; re-root them across the
    // `'drain'` listeners and the calls, which allocate.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handles: Vec<_> = callbacks
        .iter()
        .map(|bits| scope.root_nanbox_f64(f64::from_bits(*bits)))
        .collect();
    if emit_drain {
        cp_emit(stream, "drain", &[]);
    }
    for callback in &handles {
        let args: [f64; 0] = [];
        unsafe {
            let _ =
                crate::closure::js_native_call_value(callback.get_nanbox_f64(), args.as_ptr(), 0);
        }
    }
}

#[inline]
pub(super) fn cp_stdio_stream(cp: f64, fd: usize) -> f64 {
    match fd {
        0 => cp_get_field(cp, b"stdin"),
        1 => cp_get_field(cp, b"stdout"),
        2 => cp_get_field(cp, b"stderr"),
        _ => cp_array_ptr(cp_get_field(cp, b"stdio"))
            .map(|stdio| crate::array::js_array_get_f64(stdio, fd as u32))
            .unwrap_or_else(cp_undefined),
    }
}
