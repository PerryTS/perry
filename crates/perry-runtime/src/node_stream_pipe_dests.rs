//! Fan-out from a piped readable to its destinations, split from
//! node_stream_readwrite.rs for the 2000-line file-size gate. Still a child
//! module, so `use super::*` reaches the parent's private items.
//!
//! Every function here walks the destination list while it calls code that
//! can collect: a destination's `write`, its `'unpipe'` and `'end'`
//! listeners. The list is copied once into a GC array held in a handle
//! (#11828, #11882), never into a Rust `Vec` the collector cannot rewrite, and
//! each destination is read from that array just before it is used.

use super::*;
use crate::array::ArrayHeader;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};

/// A copy of `stream`'s destination list, rooted in `scope`. Walking a copy
/// keeps an unpipe from inside a callback from skipping a destination.
fn pipe_destinations_snapshot<'s>(
    scope: &'s RuntimeHandleScope,
    stream: &RuntimeHandle<'_>,
) -> RuntimeHandle<'s> {
    let live =
        || raw_ptr_from_value(pipe_destinations(stream.get_nanbox_f64())) as *const ArrayHeader;
    let dests = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    for i in 0..crate::array::js_array_length(live()) {
        let dest = crate::array::js_array_get_f64(live(), i);
        // `js_array_push_f64` roots its receiver and the value before it can grow.
        dests.set_raw_mut_ptr(
            dests.with_mut_ptr(|arr: *mut ArrayHeader| crate::array::js_array_push_f64(arr, dest)),
        );
    }
    dests
}

fn snapshot_len(dests: &RuntimeHandle<'_>) -> u32 {
    dests.with_const_ptr(|arr: *const ArrayHeader| crate::array::js_array_length(arr))
}

fn snapshot_at(dests: &RuntimeHandle<'_>, i: u32) -> f64 {
    dests.with_const_ptr(|arr: *const ArrayHeader| crate::array::js_array_get_f64(arr, i))
}

pub(in crate::node_stream) fn unpipe_all_destinations(stream: f64) {
    let scope = RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let dests = pipe_destinations_snapshot(&scope, &stream);
    for key in [hidden_stream_pipes_key(), hidden_stream_pipe_no_end_key()] {
        let empty = box_pointer(crate::array::js_array_alloc(0) as *const u8);
        set_hidden_value(stream.get_nanbox_f64(), key, empty);
    }
    let _ = pause_readable_stream_after_unpipe(stream.get_nanbox_f64());
    for i in 0..snapshot_len(&dests) {
        let dest = snapshot_at(&dests, i);
        let _ = emit_stream_event(
            dest,
            literal_string_value(b"unpipe"),
            &[stream.get_nanbox_f64()],
        );
    }
}

pub(in crate::node_stream) fn write_chunk_to_pipe_destinations(stream: f64, chunk: f64) {
    // Each write runs the destination's own code, which can collect (#11828).
    let scope = RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let chunk = scope.root_nanbox_f64(chunk);
    let dests = pipe_destinations_snapshot(&scope, &stream);
    for i in 0..snapshot_len(&dests) {
        let dest = snapshot_at(&dests, i);
        if is_small_native_handle_destination(dest) {
            let ret = call_small_native_pipe_method(dest, b"write", &[chunk.get_nanbox_f64()]);
            if ret.to_bits() == TAG_FALSE {
                let _ = pause_readable_stream(stream.get_nanbox_f64());
                add_pipe_drain_listener(stream.get_nanbox_f64(), snapshot_at(&dests, i));
            }
            continue;
        }
        let dest_scope = RuntimeHandleScope::new();
        let dest = dest_scope.root_nanbox_f64(dest);
        let write = crate::object::js_object_get_own_field_or_undef(
            dest.get_nanbox_f64(),
            b"write".as_ptr(),
            5,
        );
        let ret = if is_callable_value(write) {
            unsafe {
                crate::closure::native_call_value_this(
                    write,
                    crate::closure::JsThis::from_f64(dest.get_nanbox_f64()),
                    [chunk.get_nanbox_f64()].as_ptr(),
                    1,
                )
            }
        } else {
            write_writable_chunk(
                dest.get_nanbox_f64(),
                chunk.get_nanbox_f64(),
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
            )
        };
        if ret.to_bits() == TAG_FALSE {
            let _ = pause_readable_stream(stream.get_nanbox_f64());
            if writable_length(dest.get_nanbox_f64()) == 0.0 {
                let _ = resume_readable_stream(stream.get_nanbox_f64());
            } else {
                add_pipe_drain_listener(stream.get_nanbox_f64(), dest.get_nanbox_f64());
            }
        }
    }
}

pub(in crate::node_stream) fn end_pipe_destinations(stream: f64) {
    // Ending a destination emits its `'end'`/`'finish'` listeners, which can
    // collect.
    let scope = RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let dests = pipe_destinations_snapshot(&scope, &stream);
    for i in 0..snapshot_len(&dests) {
        let dest = snapshot_at(&dests, i);
        if is_small_native_handle_destination(dest) {
            let _ = call_small_native_pipe_method(dest, b"end", &[]);
            continue;
        }
        let dest_scope = RuntimeHandleScope::new();
        let dest = dest_scope.root_nanbox_f64(dest);
        if stream_destroyed(dest.get_nanbox_f64())
            || has_truthy_hidden(dest.get_nanbox_f64(), hidden_finish_emitted_key())
        {
            continue;
        }
        if pipe_no_end_destination_contains(stream.get_nanbox_f64(), dest.get_nanbox_f64()) {
            continue;
        }
        request_pipe_destination_finish(dest.get_nanbox_f64());
    }
}
