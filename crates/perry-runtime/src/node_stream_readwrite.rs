//! node:stream readable/writable state, split from node_stream.rs for #1987.
#![allow(unused_imports)]
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_alloc_with_shape, js_object_get_field,
    js_object_get_field_by_name_f64, js_object_set_field, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::value::JSValue;
use std::os::raw::c_int;

/// Mark a stream as disturbed (it has been read from / resumed). Backs
/// `Readable.isDisturbed(s)` (#1534).
pub(super) fn mark_disturbed(stream: f64) {
    set_hidden_value(stream, hidden_disturbed_key(), f64::from_bits(TAG_TRUE));
    set_visible_readable_did_read(stream, true);
}

pub(super) unsafe fn own_field_by_key_bytes(obj: *const ObjectHeader, key: &[u8]) -> Option<f64> {
    if obj.is_null() {
        return None;
    }
    let keys = (*obj).keys_array;
    let keys_ptr = keys as usize;
    if keys.is_null() || keys_ptr < 0x10000 {
        return None;
    }
    if gc_type_for_ptr(keys_ptr) != Some(crate::gc::GC_TYPE_ARRAY) {
        return None;
    }

    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65_536 {
        return None;
    }
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if string_value_eq(f64::from_bits(key_val.bits()), key) {
            let value = js_object_get_field(obj, i as u32);
            return if value.bits() == TAG_UNDEFINED {
                None
            } else {
                Some(f64::from_bits(value.bits()))
            };
        }
    }
    None
}

pub(super) fn hidden_key(bytes: &[u8]) -> *mut crate::string::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

pub(super) fn string_value_eq(value: f64, expected: &[u8]) -> bool {
    // SSO-aware byte compare (heap strings AND inline short strings) with no
    // heap materialization. The previous implementation routed SSO values
    // through `js_get_string_pointer_unified`, which allocates a heap copy of
    // the inline bytes per call — a fresh allocation for every SSO key
    // compared, on every object `JSON.stringify` probes for the stream flag
    // keys (#6009).
    let jsval = JSValue::from_bits(value.to_bits());
    unsafe { crate::string::js_string_key_matches_bytes(jsval, expected) }
}

pub(super) fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    // The handle band (EventEmitter ids sit at 0x38000..0x40000,
    // widget/stream handles lower) is never a heap object. The old 0x10000
    // floor let an EventEmitter handle through to the GcHeader probe at
    // raw-8, which is unmapped memory (#4633 SIGSEGV in
    // events.on(emitter, name, { signal }) target validation).
    if crate::value::addr_class::is_handle_band(raw) || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

pub(super) fn get_hidden_value(value: f64, key: *mut crate::string::StringHeader) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key);
    if value.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn is_classic_stream_instance_value(value: f64) -> bool {
    let Some(obj) = object_ptr_from_value(value) else {
        return false;
    };
    unsafe {
        own_field_by_key_bytes(obj, READABLE_FLAG_KEY).is_some()
            || own_field_by_key_bytes(obj, WRITABLE_FLAG_KEY).is_some()
    }
}

pub(crate) fn is_classic_stream_instance_of(value: f64, constructor_name: &str) -> bool {
    if constructor_name == "Stream" {
        return is_classic_stream_instance_value(value);
    }

    let Some(obj) = object_ptr_from_value(value) else {
        return false;
    };
    let Some(constructor) = (unsafe { own_field_by_key_bytes(obj, b"constructor") }) else {
        return false;
    };
    let Some((module, actual)) =
        (unsafe { crate::object::bound_native_callable_module_and_method(constructor) })
    else {
        return false;
    };
    if module != "stream" {
        return false;
    }
    let actual = actual.as_str();

    match constructor_name {
        "Readable" => matches!(actual, "Readable" | "Duplex" | "Transform" | "PassThrough"),
        "Writable" => matches!(actual, "Writable" | "Duplex" | "Transform" | "PassThrough"),
        "Duplex" => matches!(actual, "Duplex" | "Transform" | "PassThrough"),
        "Transform" => matches!(actual, "Transform" | "PassThrough"),
        "PassThrough" => actual == "PassThrough",
        _ => false,
    }
}

pub(super) fn set_hidden_value(
    value: f64,
    key: *mut crate::string::StringHeader,
    field_value: f64,
) {
    if let Some(obj) = object_ptr_from_value(value) {
        js_object_set_field_by_name(obj, key, field_value);
    }
}

pub(super) fn has_truthy_hidden(stream: f64, key: *mut crate::string::StringHeader) -> bool {
    get_hidden_value(stream, key).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

pub(super) fn stream_destroyed(stream: f64) -> bool {
    has_truthy_hidden(stream, hidden_key(b"destroyed"))
}

pub(super) fn set_stream_auto_destroy(stream: f64, opts: f64) {
    let enabled = get_hidden_value(opts, hidden_key(b"autoDestroy"))
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true);
    set_hidden_value(
        stream,
        hidden_stream_auto_destroy_key(),
        f64::from_bits(if enabled { TAG_TRUE } else { TAG_FALSE }),
    );
}

pub(super) fn stream_auto_destroy_enabled(stream: f64) -> bool {
    get_hidden_value(stream, hidden_stream_auto_destroy_key())
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true)
}

pub(super) fn set_stream_emit_close(stream: f64, opts: f64) {
    let enabled = get_hidden_value(opts, hidden_key(b"emitClose"))
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true);
    set_hidden_value(
        stream,
        hidden_stream_emit_close_key(),
        f64::from_bits(if enabled { TAG_TRUE } else { TAG_FALSE }),
    );
}

pub(super) fn stream_emit_close_enabled(stream: f64) -> bool {
    get_hidden_value(stream, hidden_stream_emit_close_key())
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true)
}

pub(super) fn mark_stream_closed_and_emit_close(stream: f64) {
    mark_stream_closed(stream);
    if stream_emit_close_enabled(stream) {
        let _ = emit_stream_event(stream, string_value(b"close"), &[]);
    }
}

pub(super) fn mark_stream_destroyed(stream: f64) {
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_TRUE));
    refresh_readable_aborted_flag(stream);
}

pub(super) fn readable_flowing_value(stream: f64) -> f64 {
    get_hidden_value(stream, readable_flowing_key()).unwrap_or(f64::from_bits(TAG_NULL))
}

pub(super) fn readable_is_flowing(stream: f64) -> bool {
    readable_flowing_value(stream).to_bits() == TAG_TRUE
}

pub(super) fn readable_is_paused(stream: f64) -> bool {
    readable_flowing_value(stream).to_bits() == TAG_FALSE
}

pub(super) fn has_writable_side(stream: f64) -> bool {
    get_hidden_value(stream, hidden_writable_flag_key()).is_some()
}

pub(super) fn should_defer_initial_data_emit(stream: f64) -> bool {
    has_truthy_hidden(stream, hidden_readable_resume_scheduled_key()) && !has_writable_side(stream)
}

pub(super) fn set_readable_flowing(stream: f64, value: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_hidden_value(stream, readable_flowing_key(), value);
    }
}

pub(super) fn ensure_hidden_array(stream: f64, key: *mut crate::string::StringHeader) -> f64 {
    if let Some(value) = get_hidden_value(stream, key) {
        return value;
    }
    let arr = box_pointer(crate::array::js_array_alloc(0) as *const u8);
    set_hidden_value(stream, key, arr);
    arr
}

pub(super) fn buffer_pending_readable_chunk(stream: f64, chunk: f64) {
    let pending = ensure_hidden_array(stream, hidden_readable_pending_key());
    let arr = raw_ptr_from_value(pending) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, chunk);
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(arr as *const u8),
    );
}

pub(super) fn pending_readable_chunk_count(stream: f64) -> u32 {
    let pending = ensure_hidden_array(stream, hidden_readable_pending_key());
    let arr = raw_ptr_from_value(pending) as *const crate::array::ArrayHeader;
    crate::array::js_array_length(arr)
}

pub(super) fn emit_readable_data(stream: f64, chunk: f64) {
    if stream_destroyed(stream) {
        return;
    }
    emit_readable_data_unchecked(stream, chunk);
}

pub(super) fn emit_readable_data_unchecked(stream: f64, chunk: f64) {
    let Some(chunk) = super::decode_readable_chunk_for_encoding(stream, chunk) else {
        return;
    };
    let _ = emit_stream_event(stream, string_value(b"data"), &[chunk]);
    write_chunk_to_pipe_destinations(stream, chunk);
}

pub(super) fn flush_pending_readable_chunks(stream: f64) {
    if !readable_is_flowing(stream) || stream_destroyed(stream) {
        return;
    }
    let pending = ensure_hidden_array(stream, hidden_readable_pending_key());
    let arr = raw_ptr_from_value(pending) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    if len == 0 {
        return;
    }
    let mut chunks = Vec::with_capacity(len as usize);
    for i in 0..len {
        chunks.push(crate::array::js_array_get_f64(arr, i));
    }
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    for chunk in chunks {
        if !readable_is_flowing(stream) {
            buffer_pending_readable_chunk(stream, chunk);
            continue;
        }
        super::readable_from_promises::consume_readable_buffered_front(stream, chunk);
        emit_readable_data_unchecked(stream, chunk);
    }
    if stream_hidden_ended(stream)
        && pending_readable_chunk_count(stream) == 0
        && !readable_is_paused(stream)
        && !stream_destroyed(stream)
    {
        schedule_readable_end(stream);
    }
}

pub(super) fn readable_data_listener_added(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() || readable_is_paused(stream)
    {
        return;
    }
    set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
    schedule_readable_resume(stream);
}

pub(super) fn readable_listener_added(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return;
    }
    if get_hidden_value(stream, hidden_read_key()).is_some() {
        invoke_read_once(stream);
    }
    schedule_readable_event(stream);
}

pub(super) fn schedule_readable_resume(stream: f64) {
    if has_truthy_hidden(stream, hidden_readable_resume_scheduled_key()) {
        return;
    }
    set_hidden_value(
        stream,
        hidden_readable_resume_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(ns_readable_resume_microtask as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

pub(super) fn pause_readable_stream(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() && !readable_is_paused(stream)
    {
        set_readable_flowing(stream, f64::from_bits(TAG_FALSE));
        let _ = emit_stream_event(stream, string_value(b"pause"), &[]);
    }
    stream
}

pub(super) fn pause_readable_stream_after_unpipe(stream: f64) -> f64 {
    if !stream_hidden_ended(stream) && !has_truthy_hidden(stream, hidden_end_emitted_key()) {
        let _ = pause_readable_stream(stream);
    }
    stream
}

pub(super) fn resume_readable_stream(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
        mark_disturbed(stream);
        flush_pending_readable_chunks(stream);
        schedule_readable_from_drain(stream);
        if stream_hidden_ended(stream)
            && pending_readable_chunk_count(stream) == 0
            && !readable_is_paused(stream)
        {
            schedule_readable_end(stream);
        }
        schedule_readable_resume(stream);
    }
    stream
}

pub(super) fn resume_readable_stream_from_pipe(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() && !stream_destroyed(stream) {
        let was_paused = readable_is_paused(stream);
        set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
        mark_disturbed(stream);
        if was_paused {
            let _ = emit_stream_event(stream, string_value(b"resume"), &[]);
        }
        flush_pending_readable_chunks(stream);
        schedule_readable_from_drain(stream);
        if stream_hidden_ended(stream)
            && pending_readable_chunk_count(stream) == 0
            && !readable_is_paused(stream)
        {
            schedule_readable_end(stream);
        }
    }
    stream
}

pub(super) fn pipe_destinations(stream: f64) -> f64 {
    ensure_hidden_array(stream, hidden_stream_pipes_key())
}

pub(super) fn pipe_no_end_destinations(stream: f64) -> f64 {
    ensure_hidden_array(stream, hidden_stream_pipe_no_end_key())
}

pub(super) fn pipe_destination_contains(stream: f64, dest: f64) -> bool {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        if crate::array::js_array_get_f64(arr, i).to_bits() == dest.to_bits() {
            return true;
        }
    }
    false
}

pub(super) fn pipe_no_end_destination_contains(stream: f64, dest: f64) -> bool {
    let arr_value = pipe_no_end_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        if crate::array::js_array_get_f64(arr, i).to_bits() == dest.to_bits() {
            return true;
        }
    }
    false
}

pub(super) fn add_pipe_destination(stream: f64, dest: f64) {
    if dest.to_bits() == TAG_UNDEFINED {
        return;
    }
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, dest);
    set_hidden_value(
        stream,
        hidden_stream_pipes_key(),
        box_pointer(arr as *const u8),
    );
}

pub(super) fn add_pipe_no_end_destination(stream: f64, dest: f64) {
    if dest.to_bits() == TAG_UNDEFINED || pipe_no_end_destination_contains(stream, dest) {
        return;
    }
    let arr_value = pipe_no_end_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, dest);
    set_hidden_value(
        stream,
        hidden_stream_pipe_no_end_key(),
        box_pointer(arr as *const u8),
    );
}

pub(super) fn pipe_stream_to_destination(stream: f64, dest: f64, end_dest: bool) -> f64 {
    add_pipe_destination(stream, dest);
    if !end_dest {
        add_pipe_no_end_destination(stream, dest);
    }
    // A flowing destination (e.g. a piped-into PassThrough/Duplex) must consume
    // each chunk from its own readable buffer when it emits 'data' live —
    // otherwise the chunk lingers in the buffer and the destination's drain
    // microtask re-emits it, duplicating every piped chunk. `pipeline()` already
    // marks both ends; `pipe()` needs the same on the destination. (matches
    // mark_live_pipe_consume_on_emit usage in node_stream_pipeline.rs)
    mark_live_pipe_consume_on_emit(dest);
    install_pipe_destination_listeners(stream, dest);
    let _ = emit_stream_event(dest, string_value(b"pipe"), &[stream]);
    set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
    let _ = emit_stream_event(stream, string_value(b"resume"), &[]);
    flush_pending_readable_chunks(stream);
    schedule_readable_from_drain(stream);
    dest
}

fn is_small_native_handle_destination(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits >> 48) as u16 != 0x7FFD {
        return false;
    }
    let raw = bits & 0x0000_FFFF_FFFF_FFFF;
    raw > 0 && raw < 0x0010_0000
}

fn call_small_native_pipe_method(dest: f64, method: &'static [u8], args: &[f64]) -> f64 {
    unsafe {
        crate::object::js_native_call_method(
            dest,
            method.as_ptr() as *const i8,
            method.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

pub(super) fn remove_pipe_no_end_destination_once(stream: f64, dest: f64) -> bool {
    let arr_value = pipe_no_end_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut out = crate::array::js_array_alloc(len.saturating_sub(1));
    let mut found = false;
    for i in 0..len {
        let current = crate::array::js_array_get_f64(arr, i);
        if !found && current.to_bits() == dest.to_bits() {
            found = true;
        } else {
            out = crate::array::js_array_push_f64(out, current);
        }
    }
    if found {
        set_hidden_value(
            stream,
            hidden_stream_pipe_no_end_key(),
            box_pointer(out as *const u8),
        );
    }
    found
}

pub(super) fn unpipe_destination(stream: f64, dest: f64) -> bool {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut out = crate::array::js_array_alloc(len.saturating_sub(1));
    let mut found = false;
    for i in 0..len {
        let current = crate::array::js_array_get_f64(arr, i);
        if !found && current.to_bits() == dest.to_bits() {
            found = true;
        } else {
            out = crate::array::js_array_push_f64(out, current);
        }
    }
    if found {
        set_hidden_value(
            stream,
            hidden_stream_pipes_key(),
            box_pointer(out as *const u8),
        );
        remove_pipe_no_end_destination_once(stream, dest);
        let _ = emit_stream_event(dest, string_value(b"unpipe"), &[stream]);
        if crate::array::js_array_length(out) == 0 {
            let _ = pause_readable_stream_after_unpipe(stream);
        }
    }
    found
}

pub(super) fn unpipe_all_destinations(stream: f64) {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut dests = Vec::with_capacity(len as usize);
    for i in 0..len {
        dests.push(crate::array::js_array_get_f64(arr, i));
    }
    set_hidden_value(
        stream,
        hidden_stream_pipes_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_hidden_value(
        stream,
        hidden_stream_pipe_no_end_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    let _ = pause_readable_stream_after_unpipe(stream);
    for dest in dests {
        let _ = emit_stream_event(dest, string_value(b"unpipe"), &[stream]);
    }
}

pub(super) fn write_chunk_to_pipe_destinations(stream: f64, chunk: f64) {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut dests = Vec::with_capacity(len as usize);
    for i in 0..len {
        dests.push(crate::array::js_array_get_f64(arr, i));
    }
    for dest in dests {
        if is_small_native_handle_destination(dest) {
            let ret = call_small_native_pipe_method(dest, b"write", &[chunk]);
            if ret.to_bits() == TAG_FALSE {
                let _ = pause_readable_stream(stream);
            }
            continue;
        }
        let ret = write_writable_chunk(
            dest,
            chunk,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
        if ret.to_bits() == TAG_FALSE {
            let _ = pause_readable_stream(stream);
            if writable_length(dest) == 0.0 {
                let _ = resume_readable_stream(stream);
            } else {
                add_pipe_drain_listener(stream, dest);
            }
        }
    }
}

pub(super) fn end_pipe_destinations(stream: f64) {
    let arr_value = pipe_destinations(stream);
    let arr = raw_ptr_from_value(arr_value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut dests = Vec::with_capacity(len as usize);
    for i in 0..len {
        dests.push(crate::array::js_array_get_f64(arr, i));
    }
    for dest in dests {
        if is_small_native_handle_destination(dest) {
            let _ = call_small_native_pipe_method(dest, b"end", &[]);
            continue;
        }
        if stream_destroyed(dest) || has_truthy_hidden(dest, hidden_finish_emitted_key()) {
            continue;
        }
        if pipe_no_end_destination_contains(stream, dest) {
            continue;
        }
        request_pipe_destination_finish(dest);
    }
}

pub(super) fn schedule_readable_from_drain(stream: f64) {
    if readable_hidden_chunks(stream).is_none()
        || has_truthy_hidden(stream, hidden_drain_scheduled_key())
        || has_truthy_hidden(
            stream,
            super::readable_from_promises::hidden_readable_from_promise_pending_key(),
        )
        || readable_is_paused(stream)
        || stream_destroyed(stream)
    {
        return;
    }
    set_hidden_value(
        stream,
        hidden_drain_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(ns_readable_from_drain as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

pub(super) fn schedule_readable_event(stream: f64) {
    if get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0) <= 0.0
        || !readable_chunks_nonempty(stream)
    {
        return;
    }
    queue_readable_event(stream);
}

pub(super) fn queue_readable_event(stream: f64) {
    if has_truthy_hidden(stream, hidden_readable_scheduled_key())
        || stream_listener_count_for_event(stream, string_value(b"readable")) == 0
    {
        return;
    }
    set_hidden_value(
        stream,
        hidden_readable_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(ns_readable_event_microtask as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

pub(super) fn cancel_readable_event(stream: f64) {
    set_hidden_value(
        stream,
        hidden_readable_scheduled_key(),
        f64::from_bits(TAG_FALSE),
    );
}

pub(super) fn schedule_readable_end(stream: f64) {
    if has_truthy_hidden(stream, hidden_end_emitted_key())
        || has_truthy_hidden(stream, hidden_end_scheduled_key())
    {
        return;
    }
    set_hidden_value(stream, hidden_end_scheduled_key(), f64::from_bits(TAG_TRUE));
    let closure = js_closure_alloc(ns_readable_end_microtask as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    crate::builtins::js_queue_microtask(closure as i64);
}

pub(super) fn schedule_writable_finish(stream: f64, callback: Option<f64>) {
    if has_truthy_hidden(stream, hidden_finish_emitted_key())
        || has_truthy_hidden(stream, hidden_finish_scheduled_key())
        || has_truthy_hidden(stream, hidden_writable_final_pending_key())
    {
        return;
    }
    if let Some(final_callback) = writable_hidden_final(stream) {
        if !has_truthy_hidden(stream, hidden_writable_final_invoked_key()) {
            set_hidden_value(
                stream,
                hidden_writable_final_invoked_key(),
                f64::from_bits(TAG_TRUE),
            );
            set_hidden_value(
                stream,
                hidden_writable_final_pending_key(),
                f64::from_bits(TAG_TRUE),
            );
            let cb = js_closure_alloc(ns_writable_final_callback_done as *const u8, 2);
            js_closure_set_capture_f64(cb, 0, stream);
            js_closure_set_capture_f64(
                cb,
                1,
                callback.unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED)),
            );
            let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
            let prev_this = crate::object::js_implicit_this_set(stream);
            unsafe {
                let _ =
                    crate::closure::js_native_call_value(final_callback, [cb_value].as_ptr(), 1);
            }
            crate::object::js_implicit_this_set(prev_this);
            return;
        }
    }
    set_hidden_value(
        stream,
        hidden_finish_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(ns_writable_finish_microtask as *const u8, 2);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    js_closure_set_capture_ptr(
        closure,
        1,
        callback
            .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
            .to_bits() as i64,
    );
    crate::builtins::js_queue_microtask(closure as i64);
}

pub(super) fn schedule_writable_finish_then_transform_end(stream: f64, callback: Option<f64>) {
    schedule_writable_finish(stream, callback);
    let finish_ready = has_truthy_hidden(stream, hidden_finish_scheduled_key())
        || has_truthy_hidden(stream, hidden_finish_emitted_key());
    let final_pending = has_truthy_hidden(stream, hidden_writable_final_pending_key());
    if is_transform_stream(stream) && finish_ready && !final_pending {
        schedule_readable_end(stream);
    }
}

pub(super) fn set_pending_writable_finish_callback(stream: f64, callback: Option<f64>) {
    let value = callback.unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED));
    set_hidden_value(stream, hidden_writable_pending_finish_callback_key(), value);
}

pub(super) fn take_pending_writable_finish_callback(stream: f64) -> Option<f64> {
    let value = get_hidden_value(stream, hidden_writable_pending_finish_callback_key());
    set_hidden_value(
        stream,
        hidden_writable_pending_finish_callback_key(),
        f64::from_bits(TAG_UNDEFINED),
    );
    value.filter(|v| is_callable_value(*v))
}

pub(super) fn schedule_pending_writable_finish_if_ready(stream: f64) {
    if !stream_hidden_ended(stream)
        || writable_length(stream) > 0.0
        || has_truthy_hidden(stream, hidden_finish_emitted_key())
        || has_truthy_hidden(stream, hidden_finish_scheduled_key())
    {
        return;
    }
    let callback = take_pending_writable_finish_callback(stream);
    schedule_writable_finish_then_transform_end(stream, callback);
}

pub(super) fn emit_readable_end_once(stream: f64) {
    if !has_truthy_hidden(stream, hidden_end_emitted_key()) {
        if pending_readable_chunk_count(stream) > 0 {
            if !readable_is_paused(stream) {
                flush_pending_readable_chunks(stream);
            }
            if pending_readable_chunk_count(stream) > 0 || readable_is_paused(stream) {
                return;
            }
        } else if readable_is_paused(stream) {
            return;
        }
        set_hidden_value(stream, hidden_end_emitted_key(), f64::from_bits(TAG_TRUE));
        mark_stream_ended(stream);
        refresh_readable_aborted_flag(stream);
        let _ = emit_stream_event(stream, string_value(b"end"), &[]);
        end_pipe_destinations(stream);
        // autoDestroy tears readable-only streams down after 'end'. Duplex
        // streams defer `close` until both readable `end` and writable
        // `finish` have fired; whichever side finishes second performs the
        // close. Refs node-suite/stream/readable/closed-flag.
        if stream_auto_destroy_enabled(stream) {
            let writable_pending = get_hidden_value(stream, hidden_writable_flag_key()).is_some()
                && !has_truthy_hidden(stream, hidden_finish_emitted_key());
            if !writable_pending {
                destroy_stream(stream, f64::from_bits(TAG_UNDEFINED));
            }
        }
    }
}

pub(super) fn push_readable_buffered_chunk(stream: f64, chunk: f64) {
    let existing = readable_hidden_chunks(stream).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        box_pointer(arr as *const u8)
    });
    if !is_array_like_value(existing) {
        return;
    }
    let arr = raw_ptr_from_value(existing) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, chunk);
    set_hidden_value(stream, hidden_chunks_key(), box_pointer(arr as *const u8));
}

pub(super) fn unshift_readable_buffered_chunk(stream: f64, chunk: f64) {
    let existing = readable_hidden_chunks(stream).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        box_pointer(arr as *const u8)
    });
    if !is_array_like_value(existing) {
        return;
    }
    let arr = raw_ptr_from_value(existing) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_unshift_f64(arr, chunk);
    set_hidden_value(stream, hidden_chunks_key(), box_pointer(arr as *const u8));
}

pub(super) fn unshift_pending_readable_chunk(stream: f64, chunk: f64) {
    let pending = ensure_hidden_array(stream, hidden_readable_pending_key());
    let arr = raw_ptr_from_value(pending) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_unshift_f64(arr, chunk);
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(arr as *const u8),
    );
}

pub(super) fn clear_readable_buffer(stream: f64) {
    set_hidden_value(
        stream,
        hidden_chunks_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    set_hidden_value(stream, hidden_buffered_key(), 0.0);
    set_hidden_value(stream, hidden_key(b"readableLength"), 0.0);
}

pub(super) fn clear_pending_readable_chunks(stream: f64) {
    set_hidden_value(
        stream,
        hidden_readable_pending_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
}

pub(super) fn drain_readable_from_events(stream: f64) {
    if !readable_is_flowing(stream) || stream_destroyed(stream) {
        return;
    }
    let data_event = string_value(b"data");
    let end_event = string_value(b"end");
    if stream_listener_count_for_event(stream, data_event) == 0
        && stream_listener_count_for_event(stream, end_event) == 0
        && crate::array::js_array_length(
            raw_ptr_from_value(pipe_destinations(stream)) as *const crate::array::ArrayHeader
        ) == 0
    {
        return;
    }
    if let Some(chunks) = readable_hidden_chunks(stream) {
        let mut values = Vec::new();
        push_chunk_values(chunks, &mut values, 0);
        if !values.is_empty() {
            mark_disturbed(stream);
        }
        let mut emit_destroyed_tail = false;
        for chunk in values {
            if !readable_is_flowing(stream) {
                return;
            }
            if stream_destroyed(stream) {
                if !emit_destroyed_tail {
                    return;
                }
                super::readable_from_promises::consume_readable_buffered_front(stream, chunk);
                emit_readable_data_unchecked(stream, chunk);
                return;
            }
            if super::readable_from_promises::attach_readable_from_promise_chunk(stream, chunk) {
                return;
            }
            super::readable_from_promises::consume_readable_buffered_front(stream, chunk);
            emit_readable_data_unchecked(stream, chunk);
            if stream_destroyed(stream) {
                emit_destroyed_tail = true;
            }
        }
    }
    if !stream_destroyed(stream) {
        emit_readable_end_once(stream);
    }
}

pub(super) fn is_array_like_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return false;
    }
    unsafe {
        matches!(
            gc_type_for_ptr(raw),
            Some(crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY)
        )
    }
}

pub(super) fn readable_hidden_chunks(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_chunks_key())
}

pub(super) fn readable_object_mode(value: f64) -> bool {
    has_truthy_hidden(value, hidden_key(b"readableObjectMode"))
}

pub(super) fn readable_chunks_nonempty(stream: f64) -> bool {
    let Some(chunks) = readable_hidden_chunks(stream) else {
        return false;
    };
    if is_array_like_value(chunks) {
        let raw = raw_ptr_from_value(chunks);
        return raw >= 0x10000
            && crate::array::js_array_length(raw as *const crate::array::ArrayHeader) > 0;
    }
    is_single_chunk_value(chunks)
}

pub(super) fn readable_hidden_error(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_error_key())
}

pub(super) fn stream_hidden_ended(value: f64) -> bool {
    get_hidden_value(value, hidden_ended_key()).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

pub(super) fn readable_aborted_value(stream: f64) -> f64 {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return f64::from_bits(TAG_FALSE);
    }
    let destroyed = has_truthy_hidden(stream, hidden_key(b"destroyed"));
    let errored = readable_hidden_error(stream).is_some();
    let ended = stream_hidden_ended(stream) || has_truthy_hidden(stream, hidden_end_emitted_key());
    if (destroyed || errored) && !ended {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

pub(super) fn refresh_readable_aborted_flag(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_some() {
        set_hidden_value(
            stream,
            hidden_key(b"readableAborted"),
            readable_aborted_value(stream),
        );
    }
}

pub(super) fn writable_hidden_write(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_write_key())
}

pub(super) fn writable_hidden_writev(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_writev_key())
}

pub(super) fn transform_hidden_callback(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_transform_callback_key())
}

pub(super) fn transform_hidden_flush(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_transform_flush_key())
}

pub(super) fn writable_hidden_final(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_writable_final_key())
}

pub(super) fn is_transform_stream(stream: f64) -> bool {
    transform_hidden_callback(stream).is_some()
        || transform_hidden_flush(stream).is_some()
        || has_truthy_hidden(stream, hidden_transform_passthrough_key())
}

pub(super) fn finish_transform_stream(stream: f64, callback: Option<f64>) -> bool {
    let Some(flush) = transform_hidden_flush(stream) else {
        return false;
    };
    if has_truthy_hidden(stream, hidden_transform_finishing_key()) {
        return true;
    }
    set_hidden_value(
        stream,
        hidden_transform_finishing_key(),
        f64::from_bits(TAG_TRUE),
    );
    let cb = js_closure_alloc(transform_flush_callback as *const u8, 2);
    js_closure_set_capture_f64(cb, 0, stream);
    js_closure_set_capture_f64(
        cb,
        1,
        callback.unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED)),
    );
    let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
    let prev_this = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(flush, [cb_value].as_ptr(), 1);
    }
    crate::object::js_implicit_this_set(prev_this);
    true
}

pub(super) fn writable_corked_count(value: f64) -> f64 {
    get_hidden_value(value, hidden_writable_corked_key()).unwrap_or(0.0)
}

pub(super) fn writable_length(value: f64) -> f64 {
    get_hidden_value(value, hidden_writable_length_key()).unwrap_or(0.0)
}

pub(super) fn set_writable_length(stream: f64, len: f64) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let len = len.max(0.0);
        set_hidden_value(stream, hidden_writable_length_key(), len);
        set_hidden_value(stream, hidden_key(b"writableLength"), len);
    }
}

pub(super) fn add_writable_length(stream: f64, len: f64) {
    if len > 0.0 {
        set_writable_length(stream, writable_length(stream) + len);
    }
}

pub(super) fn subtract_writable_length(stream: f64, len: f64) {
    if len > 0.0 {
        set_writable_length(stream, writable_length(stream) - len);
    }
}

pub(super) fn writable_need_drain_raw(stream: f64) -> bool {
    has_truthy_hidden(stream, hidden_writable_need_drain_key())
}

pub(super) fn writable_need_drain(stream: f64) -> bool {
    writable_need_drain_raw(stream)
        && !stream_hidden_ended(stream)
        && !has_truthy_hidden(stream, hidden_key(b"destroyed"))
}

pub(super) fn set_writable_need_drain(stream: f64, need_drain: bool) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let value = if need_drain { TAG_TRUE } else { TAG_FALSE };
        set_hidden_value(
            stream,
            hidden_writable_need_drain_key(),
            f64::from_bits(value),
        );
        set_hidden_value(
            stream,
            hidden_key(b"writableNeedDrain"),
            f64::from_bits(value),
        );
    }
}

pub(super) fn set_writable_corked_count(stream: f64, count: f64) {
    if get_hidden_value(stream, hidden_writable_flag_key()).is_some() {
        let count = count.max(0.0);
        set_hidden_value(stream, hidden_writable_corked_key(), count);
        set_hidden_value(stream, hidden_key(b"writableCorked"), count);
    }
}

pub(super) fn cork_stream(stream: f64) -> f64 {
    set_writable_corked_count(stream, writable_corked_count(stream) + 1.0);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn uncork_stream(stream: f64) -> f64 {
    let corked = writable_corked_count(stream);
    if corked > 0.0 {
        set_writable_corked_count(stream, corked - 1.0);
        if corked <= 1.0 {
            flush_writable_buffered(stream);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn buffered_writable_writes(stream: f64) -> Option<f64> {
    get_hidden_value(stream, hidden_writable_buffered_key())
}

pub(super) fn buffer_writable_write(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    let mut buffered = buffered_writable_writes(stream).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        box_pointer(arr as *const u8)
    });
    let arr = raw_ptr_from_value(buffered) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, chunk);
    let arr = crate::array::js_array_push_f64(arr, enc);
    let arr = crate::array::js_array_push_f64(arr, len);
    let arr = crate::array::js_array_push_f64(arr, callback);
    buffered = box_pointer(arr as *const u8);
    set_hidden_value(stream, hidden_writable_buffered_key(), buffered);
}

pub(super) fn writev_record_chunk(chunk: f64, enc: f64) -> (f64, f64) {
    if JSValue::from_bits(chunk.to_bits()).is_any_string() {
        (chunk, enc)
    } else {
        let raw = raw_ptr_from_value(chunk);
        if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
            (chunk, string_value(b"buffer"))
        } else {
            (chunk, enc)
        }
    }
}

pub(super) fn build_writev_chunks(buffered: *const crate::array::ArrayHeader, len: u32) -> f64 {
    let mut chunks = crate::array::js_array_alloc(0);
    let mut i = 0;
    while i < len {
        let chunk = crate::array::js_array_get_f64(buffered, i);
        let enc = if i + 1 < len {
            crate::array::js_array_get_f64(buffered, i + 1)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        let (chunk, encoding) = writev_record_chunk(chunk, enc);
        let record = crate::object::js_object_alloc(0, 2);
        js_object_set_field_by_name(record, hidden_key(b"chunk"), chunk);
        js_object_set_field_by_name(record, hidden_key(b"encoding"), encoding);
        chunks = crate::array::js_array_push_f64(chunks, box_pointer(record as *const u8));
        i += 4;
    }
    box_pointer(chunks as *const u8)
}

pub(super) fn flush_writable_buffered(stream: f64) {
    let Some(buffered) = buffered_writable_writes(stream) else {
        return;
    };
    let raw = raw_ptr_from_value(buffered);
    if raw < 0x10000 {
        return;
    }
    let arr = raw as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    set_hidden_value(
        stream,
        hidden_writable_buffered_key(),
        box_pointer(crate::array::js_array_alloc(0) as *const u8),
    );
    if len > 4 && writable_hidden_writev(stream).is_some() {
        let chunks = build_writev_chunks(arr, len);
        invoke_writable_writev(stream, chunks);
        let mut i = 0;
        while i < len {
            let chunk = crate::array::js_array_get_f64(arr, i);
            let write_len = if i + 2 < len {
                crate::array::js_array_get_f64(arr, i + 2)
            } else {
                writable_chunk_len(stream, chunk)
            };
            let callback = if i + 3 < len {
                crate::array::js_array_get_f64(arr, i + 3)
            } else {
                f64::from_bits(TAG_UNDEFINED)
            };
            emit_writable_chunk(stream, chunk);
            complete_writable_write(stream, write_len, callback, f64::from_bits(TAG_UNDEFINED));
            i += 4;
        }
        return;
    }
    let mut i = 0;
    while i < len {
        let chunk = crate::array::js_array_get_f64(arr, i);
        let enc = if i + 1 < len {
            crate::array::js_array_get_f64(arr, i + 1)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        let write_len = if i + 2 < len {
            crate::array::js_array_get_f64(arr, i + 2)
        } else {
            writable_chunk_len(stream, chunk)
        };
        let callback = if i + 3 < len {
            crate::array::js_array_get_f64(arr, i + 3)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        invoke_writable_write(stream, chunk, enc, write_len, callback);
        emit_writable_chunk(stream, chunk);
        i += 4;
    }
}

pub(super) fn rebind_callback_this(callback: f64, stream: f64) -> f64 {
    f64::from_bits(crate::closure::clone_closure_rebind_this(
        callback.to_bits(),
        stream,
    ))
}

pub(super) fn read_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"read"))
}

pub(super) fn write_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"write"))
}

pub(super) fn writev_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"writev"))
}

pub(super) fn transform_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"transform"))
}

pub(super) fn transform_flush_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"flush"))
}

pub(super) fn construct_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"construct")).filter(|v| is_callable_value(*v))
}

pub(super) fn destroy_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"destroy")).filter(|v| is_callable_value(*v))
}

pub(super) fn final_callback_from_options(opts: f64) -> Option<f64> {
    get_hidden_value(opts, hidden_key(b"final")).filter(|v| is_callable_value(*v))
}

pub(super) fn install_common_lifecycle_callbacks(stream: f64, opts: f64) {
    if let Some(destroy) = destroy_callback_from_options(opts) {
        set_hidden_value(
            stream,
            hidden_key(STREAM_DESTROY_KEY),
            rebind_callback_this(destroy, stream),
        );
    }
}

pub(super) fn install_writable_lifecycle_callbacks(stream: f64, opts: f64) {
    if let Some(final_callback) = final_callback_from_options(opts) {
        set_hidden_value(
            stream,
            hidden_writable_final_key(),
            rebind_callback_this(final_callback, stream),
        );
        set_hidden_value(
            stream,
            hidden_writable_final_invoked_key(),
            f64::from_bits(TAG_FALSE),
        );
        set_hidden_value(
            stream,
            hidden_writable_final_pending_key(),
            f64::from_bits(TAG_FALSE),
        );
    }
}

pub(super) fn invoke_construct_callback(stream: f64, opts: f64) {
    let Some(construct) = construct_callback_from_options(opts) else {
        return;
    };
    let construct = rebind_callback_this(construct, stream);
    set_hidden_value(stream, hidden_key(STREAM_CONSTRUCT_KEY), construct);
    let cb = js_closure_alloc(ns_construct_callback_done as *const u8, 1);
    js_closure_set_capture_f64(cb, 0, stream);
    let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
    let prev_this = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(construct, [cb_value].as_ptr(), 1);
    }
    crate::object::js_implicit_this_set(prev_this);
}

pub(super) fn invoke_read_once(stream: f64) {
    invoke_read_once_inner(stream, true);
}

/// Like [`invoke_read_once`] but never synthesizes the default `_read`
/// (`ERR_METHOD_NOT_IMPLEMENTED`) error for a bare `Readable`. Passive probes
/// such as `finished()` must observe a stream's state without destroying it —
/// in Node, attaching `finished()` listeners does not call `_read`. Triggering
/// the default error here destroyed an idle stream before a later
/// `destroy(err)` could run, so `finished()` rejected with the default read
/// error instead of the caller's error (#2441 regression / #2462).
pub(super) fn probe_read_once(stream: f64) {
    invoke_read_once_inner(stream, false);
}

fn invoke_read_once_inner(stream: f64, emit_default_error: bool) {
    let Some(read) = get_hidden_value(stream, hidden_read_key()) else {
        if emit_default_error {
            maybe_emit_default_read_error(stream);
        }
        return;
    };
    if get_hidden_value(stream, hidden_read_invoked_key()).is_some() {
        return;
    }
    set_hidden_value(stream, hidden_read_invoked_key(), f64::from_bits(TAG_TRUE));
    let size = get_hidden_value(stream, hidden_hwm_key()).unwrap_or_else(|| default_hwm(false));
    let prev_this = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(read, [size].as_ptr(), 1);
    }
    crate::object::js_implicit_this_set(prev_this);
}

pub(super) fn maybe_emit_default_read_error(stream: f64) {
    if !has_truthy_hidden(stream, hidden_default_read_error_key())
        || readable_hidden_chunks(stream).is_some()
        || stream_hidden_ended(stream)
        || stream_destroyed(stream)
        || get_hidden_value(stream, hidden_read_invoked_key()).is_some()
    {
        return;
    }
    set_hidden_value(stream, hidden_read_invoked_key(), f64::from_bits(TAG_TRUE));
    destroy_stream(stream, readable_default_read_error());
}

/// Test helper: make `stream` behave like a manually-driven Readable that was
/// constructed with a (no-op) `_read`. #2441 made a *bare* Readable (one with
/// no `_read`) raise `ERR_METHOD_NOT_IMPLEMENTED` and self-destroy on the first
/// read — which is Node-correct (Node requires a `_read`). Tests that drive a
/// stream purely via `push()` clear that marker so they exercise their intended
/// push/flow/end lifecycle without tripping the error, exactly as real code
/// would by passing `{ read() {} }` to the constructor.
#[cfg(test)]
pub(crate) fn test_install_manual_read(stream: f64) {
    set_hidden_value(
        stream,
        hidden_default_read_error_key(),
        f64::from_bits(TAG_FALSE),
    );
}

pub(super) fn is_single_chunk_value(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        return true;
    }
    let raw = raw_ptr_from_value(value);
    raw >= 0x10000 && crate::buffer::is_registered_buffer(raw)
}

pub(super) fn is_non_iterable_primitive_for_readable_from(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    (jsval.is_number() || jsval.is_int32() || jsval.is_bool()) && !jsval.is_any_string()
}

pub(super) fn uint8array_byte_chunks(raw: usize) -> f64 {
    let arr = crate::array::js_array_alloc(0);
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return box_pointer(arr as *const u8);
    }
    unsafe {
        let buf = raw as *const crate::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        let mut out = arr;
        for i in 0..len {
            out = crate::array::js_array_push_f64(out, *data.add(i) as f64);
        }
        box_pointer(out as *const u8)
    }
}

pub(super) fn typed_uint8array_byte_chunks(raw: usize) -> Option<f64> {
    if crate::typedarray::lookup_typed_array_kind(raw) != Some(crate::typedarray::KIND_UINT8) {
        return None;
    }
    let ta = raw as *const crate::typedarray::TypedArrayHeader;
    let len = crate::typedarray::js_typed_array_length(ta).max(0) as u32;
    let mut out = crate::array::js_array_alloc(len);
    for i in 0..len {
        out = crate::array::js_array_push_f64(
            out,
            crate::typedarray::js_typed_array_get(ta, i as i32),
        );
    }
    Some(box_pointer(out as *const u8))
}

pub(super) fn collection_iterable_chunks(raw: usize) -> Option<f64> {
    if raw < 0x10000 {
        return None;
    }
    if crate::set::is_registered_set(raw) {
        let chunks = crate::set::js_set_to_array(raw as *const crate::set::SetHeader);
        return Some(box_pointer(chunks as *const u8));
    }
    if crate::map::is_registered_map(raw) {
        let chunks = crate::map::js_map_entries(raw as *const crate::map::MapHeader);
        return Some(box_pointer(chunks as *const u8));
    }
    None
}

pub(super) struct NormalizedReadableInput {
    pub chunks: f64,
    pub source_iterator: Option<f64>,
}

fn normalized_readable_chunks(chunks: f64) -> NormalizedReadableInput {
    NormalizedReadableInput {
        chunks,
        source_iterator: None,
    }
}

pub(super) fn normalize_readable_from_input(iterable: f64) -> NormalizedReadableInput {
    if let Some(chunks) = readable_hidden_chunks(iterable) {
        return normalized_readable_chunks(chunks);
    }
    let raw = raw_ptr_from_value(iterable);
    if raw >= 0x10000
        && crate::buffer::is_registered_buffer(raw)
        && crate::buffer::is_uint8array_buffer(raw)
        && !crate::buffer::is_array_buffer(raw)
    {
        return normalized_readable_chunks(uint8array_byte_chunks(raw));
    }
    if let Some(chunks) = typed_uint8array_byte_chunks(raw) {
        return normalized_readable_chunks(chunks);
    }
    if let Some(chunks) = collection_iterable_chunks(raw) {
        return normalized_readable_chunks(chunks);
    }
    if raw >= 0x10000 {
        if let Some(chunks) = unsafe {
            crate::object::arguments_object_to_array(raw as *const crate::object::ObjectHeader)
        } {
            return normalized_readable_chunks(box_pointer(chunks as *const u8));
        }
    }
    if is_array_like_value(iterable) {
        return normalized_readable_chunks(iterable);
    }
    if is_single_chunk_value(iterable) {
        let arr = crate::array::js_array_alloc(1);
        let arr = crate::array::js_array_push_f64(arr, iterable);
        return normalized_readable_chunks(box_pointer(arr as *const u8));
    }
    if let Some((chunks, source_iterator)) = flatten_async_iterable_with_source(iterable) {
        return NormalizedReadableInput {
            chunks: box_pointer(chunks as *const u8),
            source_iterator,
        };
    }
    if let Some((chunks, source_iterator)) = flatten_sync_iterable_value(iterable) {
        return NormalizedReadableInput {
            chunks: box_pointer(chunks as *const u8),
            source_iterator,
        };
    }

    let arr = crate::array::js_array_alloc(1);
    normalized_readable_chunks(box_pointer(arr as *const u8))
}

fn flatten_sync_iterable_value(
    value: f64,
) -> Option<(*mut crate::array::ArrayHeader, Option<f64>)> {
    if has_symbol_async_iterator(value) {
        return None;
    }
    if crate::object::js_util_types_is_generator_object(value).to_bits() == TAG_TRUE {
        return crate::array::sync_iterator_to_array_if_not_async(value)
            .map(|chunks| (chunks, Some(value)));
    }
    let iter = crate::symbol::js_get_iterator(value);
    if iter.to_bits() != value.to_bits() {
        return crate::array::sync_iterator_to_array_if_not_async(iter)
            .map(|chunks| (chunks, Some(iter)));
    }
    None
}

fn has_symbol_async_iterator(value: f64) -> bool {
    let sym = crate::symbol::well_known_symbol("asyncIterator");
    if sym.is_null() {
        return false;
    }
    let sym_value = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
    let method = unsafe { crate::symbol::js_object_get_symbol_property(value, sym_value) };
    is_callable_value(method)
}

pub(super) fn readable_from_options(opts: f64) -> f64 {
    let merged = crate::object::js_object_alloc(0, 2);
    let object_mode =
        get_hidden_value(opts, hidden_key(b"objectMode")).is_none_or(|v| v.to_bits() != TAG_FALSE);
    set_hidden_value(
        box_pointer(merged as *const u8),
        hidden_key(b"objectMode"),
        f64::from_bits(if object_mode { TAG_TRUE } else { TAG_FALSE }),
    );
    let hwm = opt_number(opts, b"highWaterMark").unwrap_or(1.0);
    set_hidden_value(
        box_pointer(merged as *const u8),
        hidden_key(b"highWaterMark"),
        hwm,
    );
    box_pointer(merged as *const u8)
}

pub(super) fn append_string_bytes(value: f64, out: &mut Vec<u8>) {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    append_string_ptr_bytes(ptr, out);
}

pub(super) fn append_string_ptr_bytes(ptr: *const crate::StringHeader, out: &mut Vec<u8>) {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

pub(super) fn append_buffer_bytes(raw: usize, out: &mut Vec<u8>) {
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return;
    }
    unsafe {
        let buf = raw as *const crate::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        out.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

pub(super) fn append_array_chunks(raw: usize, out: &mut Vec<u8>, depth: u8) {
    if raw < 0x10000 {
        return;
    }
    let arr = raw as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let chunk = crate::array::js_array_get_f64(arr, i);
        append_chunk_bytes(chunk, out, depth + 1);
    }
}

pub(super) fn append_chunk_bytes(value: f64, out: &mut Vec<u8>, depth: u8) {
    if depth > 8 {
        return;
    }
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        append_string_bytes(value, out);
        return;
    }
    if jsval.is_int32() {
        out.extend_from_slice(jsval.as_int32().to_string().as_bytes());
        return;
    }
    if jsval.is_number() && value.is_finite() {
        let text = if value.fract() == 0.0 {
            (value as i64).to_string()
        } else {
            value.to_string()
        };
        out.extend_from_slice(text.as_bytes());
        return;
    }

    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 {
        return;
    }
    if crate::buffer::is_registered_buffer(raw) {
        append_buffer_bytes(raw, out);
        return;
    }

    unsafe {
        match gc_type_for_ptr(raw) {
            Some(crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY) => {
                append_array_chunks(raw, out, depth);
            }
            Some(crate::gc::GC_TYPE_OBJECT) => {
                if let Some(chunks) = readable_hidden_chunks(value) {
                    append_chunk_bytes(chunks, out, depth + 1);
                }
            }
            Some(crate::gc::GC_TYPE_STRING) => {
                append_string_ptr_bytes(raw as *const crate::StringHeader, out);
            }
            _ => {}
        }
    }
}

pub(super) fn push_chunk_values(value: f64, out: &mut Vec<f64>, depth: u8) {
    if depth > 8 {
        return;
    }
    if let Some(chunks) = readable_hidden_chunks(value) {
        push_chunk_values(chunks, out, depth + 1);
        return;
    }
    if is_array_like_value(value) {
        let raw = raw_ptr_from_value(value);
        if raw < 0x10000 {
            return;
        }
        let arr = raw as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
            out.push(crate::array::js_array_get_f64(arr, i));
        }
        return;
    }
    if is_single_chunk_value(value) {
        out.push(value);
    }
}

/// Drain the chunk storage Perry attaches in `Readable.from(iterable)`.
pub fn js_node_stream_collect_bytes(stream: f64) -> Vec<u8> {
    js_node_stream_collect_bytes_result(stream).unwrap_or_default()
}

pub fn js_node_stream_collect_chunks_result(stream: f64) -> Option<Result<f64, f64>> {
    invoke_read_once(stream);
    if let Some(err) = readable_hidden_error(stream) {
        return Some(Err(err));
    }
    if let Some(chunks) = readable_hidden_chunks(stream) {
        return Some(Ok(chunks));
    }
    if is_array_like_value(stream) {
        return Some(Ok(stream));
    }
    if is_single_chunk_value(stream) {
        let mut arr = crate::array::js_array_alloc(1);
        arr = crate::array::js_array_push_f64(arr, stream);
        return Some(Ok(box_pointer(arr as *const u8)));
    }
    if get_hidden_value(stream, hidden_read_key()).is_some() {
        let arr = crate::array::js_array_alloc(0);
        return Some(Ok(box_pointer(arr as *const u8)));
    }
    None
}

pub fn js_node_stream_collect_bytes_result(stream: f64) -> Result<Vec<u8>, f64> {
    invoke_read_once(stream);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    let mut out = Vec::new();
    append_chunk_bytes(stream, &mut out, 0);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    Ok(out)
}

pub(crate) fn js_node_stream_hidden_error_after_read(stream: f64) -> Option<f64> {
    probe_read_once(stream);
    readable_hidden_error(stream)
}

pub(crate) fn js_node_stream_hidden_error(stream: f64) -> Option<f64> {
    readable_hidden_error(stream)
}

pub(crate) fn js_node_stream_is_classic_stream(stream: f64) -> bool {
    get_hidden_value(stream, hidden_readable_flag_key()).is_some()
        || get_hidden_value(stream, hidden_writable_flag_key()).is_some()
}

pub(crate) fn js_node_stream_has_readable_side(stream: f64) -> bool {
    get_hidden_value(stream, hidden_readable_flag_key()).is_some()
}

pub(crate) fn js_node_stream_has_writable_side(stream: f64) -> bool {
    get_hidden_value(stream, hidden_writable_flag_key()).is_some()
}

pub(crate) fn js_node_stream_readable_side_done(stream: f64) -> bool {
    !js_node_stream_has_readable_side(stream)
        || stream_hidden_ended(stream)
        || has_truthy_hidden(stream, hidden_end_emitted_key())
}

pub(crate) fn js_node_stream_writable_side_done(stream: f64) -> bool {
    !js_node_stream_has_writable_side(stream)
        || has_truthy_hidden(stream, hidden_finish_emitted_key())
}

#[cfg(test)]
pub(crate) fn js_node_stream_is_stub_ended_after_read(stream: f64) -> bool {
    probe_read_once(stream);
    stream_hidden_ended(stream)
}

pub(crate) fn js_node_stream_is_stub_ended(stream: f64) -> bool {
    stream_hidden_ended(stream)
}

#[cfg(test)]
pub(crate) fn test_set_hidden_error(stream: f64, err: f64) {
    set_hidden_value(stream, hidden_error_key(), err);
}

pub(crate) fn js_node_stream_readable_chunks_result(stream: f64) -> Result<Option<Vec<f64>>, f64> {
    invoke_read_once(stream);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    let Some(chunks) = readable_hidden_chunks(stream) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    push_chunk_values(chunks, &mut out, 0);
    if let Some(err) = readable_hidden_error(stream) {
        return Err(err);
    }
    Ok(Some(out))
}

// Method table order determines packed-key order and shape-cache identity.

pub(super) fn readable_methods() -> [(&'static str, StubFn); 39] {
    [
        ("on", cast2(ns_on2)),
        ("once", cast2(ns_once2)),
        ("prependListener", cast2(ns_prepend_listener2)),
        ("prependOnceListener", cast2(ns_prepend_once_listener2)),
        ("off", cast2(ns_off2)),
        ("addListener", cast2(ns_on2)),
        ("removeListener", cast2(ns_remove_listener2)),
        ("removeAllListeners", cast1(ns_remove_all_listeners1)),
        ("emit", cast2(ns_emit_rest)),
        ("setMaxListeners", cast1(ns_set_max_listeners)),
        ("getMaxListeners", cast0(ns_get_max_listeners)),
        ("eventNames", cast0(ns_event_names)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("rawListeners", cast1(ns_raw_listeners)),
        ("read", cast1(ns_read1)),
        ("pipe", cast2(ns_pipe2)),
        ("unpipe", cast1(ns_unpipe1)),
        ("wrap", cast1(ns_wrap1)),
        ("pause", cast0(ns_pause0)),
        ("resume", cast0(ns_resume0)),
        ("destroy", cast1(ns_destroy1)),
        ("setEncoding", cast1(ns_set_encoding1)),
        ("isPaused", cast0(ns_is_paused0)),
        // #1558: async iterator helpers; arities pad missing options args.
        ("toArray", cast1(ns_iter_to_array)),
        ("map", cast2(ns_iter_map)),
        ("filter", cast2(ns_iter_filter)),
        ("reduce", cast3(ns_iter_reduce)),
        ("forEach", cast2(ns_iter_for_each)),
        ("find", cast2(ns_iter_find)),
        ("some", cast2(ns_iter_some)),
        ("every", cast2(ns_iter_every)),
        ("flatMap", cast2(ns_iter_flat_map)),
        ("take", cast1(ns_iter_take)),
        ("drop", cast1(ns_iter_drop)),
        ("iterator", cast1(async_iterator::ns_iterator1)),
        // #1539 — push() backpressure return + readable.compose() instance form.
        ("push", cast1(ns_push1)),
        ("unshift", cast1(ns_unshift1)),
        ("compose", cast1(ns_compose1)),
    ]
}

/// #5137: the bare `EventEmitter` surface — the same 15 listener/emit
/// methods that `readable_methods`/`writable_methods` share, minus all the
/// stream-specific entries. Installed onto `this` by
/// `js_event_emitter_subclass_init` so a source-compiled `class X extends
/// EventEmitter` (e.g. commander's `Command`) gets working
/// `.on`/`.emit`/`.once`/… without routing through the handle-based
/// `js_event_emitter_*` shim. The closures are the generic
/// `ns_*` emitter helpers, which key all state off the receiver object, so
/// they work unchanged on a plain object that never went through a stream
/// constructor.
pub(super) fn emitter_methods() -> [(&'static str, StubFn); 15] {
    [
        ("on", cast2(ns_on2)),
        ("once", cast2(ns_once2)),
        ("prependListener", cast2(ns_prepend_listener2)),
        ("prependOnceListener", cast2(ns_prepend_once_listener2)),
        ("off", cast2(ns_off2)),
        ("addListener", cast2(ns_on2)),
        ("removeListener", cast2(ns_remove_listener2)),
        ("removeAllListeners", cast1(ns_remove_all_listeners1)),
        ("emit", cast2(ns_emit_rest)),
        ("setMaxListeners", cast1(ns_set_max_listeners)),
        ("getMaxListeners", cast0(ns_get_max_listeners)),
        ("eventNames", cast0(ns_event_names)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("rawListeners", cast1(ns_raw_listeners)),
    ]
}

pub(super) fn writable_methods() -> [(&'static str, StubFn); 22] {
    [
        ("on", cast2(ns_on2)),
        ("once", cast2(ns_once2)),
        ("prependListener", cast2(ns_prepend_listener2)),
        ("prependOnceListener", cast2(ns_prepend_once_listener2)),
        ("off", cast2(ns_off2)),
        ("addListener", cast2(ns_on2)),
        ("removeListener", cast2(ns_remove_listener2)),
        ("removeAllListeners", cast1(ns_remove_all_listeners1)),
        ("emit", cast2(ns_emit_rest)),
        ("setMaxListeners", cast1(ns_set_max_listeners)),
        ("getMaxListeners", cast0(ns_get_max_listeners)),
        ("eventNames", cast0(ns_event_names)),
        ("listenerCount", cast1(ns_listener_count)),
        ("listeners", cast1(ns_listeners)),
        ("rawListeners", cast1(ns_raw_listeners)),
        ("write", cast3(ns_write3)),
        ("end", cast3(ns_end3)),
        ("cork", cast0(ns_cork0)),
        ("uncork", cast0(ns_uncork0)),
        ("destroy", cast1(ns_destroy1)),
        ("setDefaultEncoding", cast1(ns_chain1)),
        ("_write", cast3(ns_chain3)),
    ]
}
