//! node:stream — pipeline() / stream.compose() data-flow engine (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
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

#[derive(Clone, Copy)]
pub(super) struct PipelineOptions {
    pub(super) end_final: bool,
    pub(super) signal: Option<f64>,
}

pub(super) extern "C" fn pipeline_success_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    if !mark_pipeline_callback_called(state) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn pipeline_error_callback(closure: *const ClosureHeader, err: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    let stages = js_closure_get_capture_f64(closure, 2);
    if !mark_pipeline_callback_called(state) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    destroy_pipeline_stages(stages, err);
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[err]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn pipeline_close_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stage = js_closure_get_capture_f64(closure, 3);
    if pipeline_stage_already_complete(stage) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let state = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    let stages = js_closure_get_capture_f64(closure, 2);
    if !mark_pipeline_callback_called(state) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let err = pipeline_premature_close_error();
    destroy_pipeline_stages(stages, err);
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[err]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn pipeline_args(args: *const crate::array::ArrayHeader) -> Vec<f64> {
    if args.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(args);
    let mut values = Vec::with_capacity(len as usize);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(args, i));
    }
    values
}

pub(super) fn pipeline_array_like_values(value: f64) -> Vec<f64> {
    if !is_array_like_value(value) {
        return Vec::new();
    }
    let arr = raw_ptr_from_value(value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut values = Vec::with_capacity(len as usize);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(arr, i));
    }
    values
}

pub(super) fn is_pipeline_stream(value: f64) -> bool {
    get_hidden_value(value, hidden_readable_flag_key()).is_some()
        || get_hidden_value(value, hidden_writable_flag_key()).is_some()
}

pub(super) fn is_pipeline_options_arg(value: f64) -> bool {
    object_ptr_from_value(value).is_some()
        && !is_pipeline_stream(value)
        && !is_array_like_value(value)
}

pub(super) fn pipeline_options_from_arg(value: f64) -> PipelineOptions {
    let end_final = get_hidden_value(value, hidden_key(b"end"))
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true);
    PipelineOptions {
        end_final,
        signal: options_signal(value),
    }
}

pub(super) fn pipe_options_end(value: f64) -> bool {
    get_hidden_value(value, hidden_key(b"end"))
        .map(|v| v.to_bits() != TAG_FALSE)
        .unwrap_or(true)
}

pub(super) fn normalize_pipeline_source(value: f64, index: usize) -> f64 {
    if index == 0
        && !is_pipeline_stream(value)
        && !is_non_iterable_primitive_for_readable_from(value)
    {
        js_node_stream_readable_from(value)
    } else {
        value
    }
}

pub(super) fn pipeline_stage_array(stages: &[f64]) -> f64 {
    let mut arr = crate::array::js_array_alloc(stages.len() as u32);
    for stage in stages {
        arr = crate::array::js_array_push_f64(arr, *stage);
    }
    box_pointer(arr as *const u8)
}

pub(super) fn new_pipeline_callback_state() -> f64 {
    let state = js_object_alloc(0, 0);
    let value = box_pointer(state as *const u8);
    set_hidden_value(
        value,
        hidden_pipeline_callback_done_key(),
        f64::from_bits(TAG_FALSE),
    );
    value
}

pub(super) fn mark_pipeline_callback_called(state: f64) -> bool {
    if has_truthy_hidden(state, hidden_pipeline_callback_done_key()) {
        return false;
    }
    set_hidden_value(
        state,
        hidden_pipeline_callback_done_key(),
        f64::from_bits(TAG_TRUE),
    );
    true
}

pub(super) fn destroy_pipeline_stages(stages: f64, err: f64) {
    if !is_array_like_value(stages) {
        return;
    }
    let arr = raw_ptr_from_value(stages) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        destroy_stream(crate::array::js_array_get_f64(arr, i), err);
    }
}

pub(super) fn pipeline_premature_close_error() -> f64 {
    let msg = b"Premature close";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_STREAM_PREMATURE_CLOSE");
    let err = crate::error::js_error_new_with_message(s);
    crate::value::js_nanbox_pointer(err as i64)
}

pub(super) fn pipeline_stage_already_complete(stage: f64) -> bool {
    stream_hidden_ended(stage)
        || has_truthy_hidden(stage, hidden_end_emitted_key())
        || has_truthy_hidden(stage, hidden_finish_emitted_key())
}

pub(super) fn add_pipeline_callback_listeners(
    stages: &[f64],
    callback: f64,
    options: PipelineOptions,
) {
    let state = new_pipeline_callback_state();
    let stage_array = pipeline_stage_array(stages);
    let error_event = string_value(b"error");
    let close_event = string_value(b"close");
    for stage in stages {
        let listener = js_closure_alloc(pipeline_error_callback as *const u8, 3);
        js_closure_set_capture_f64(listener, 0, state);
        js_closure_set_capture_f64(listener, 1, callback);
        js_closure_set_capture_f64(listener, 2, stage_array);
        add_stream_listener_for_event(*stage, error_event, box_pointer(listener as *const u8));
        if !pipeline_stage_already_complete(*stage) {
            let close_listener = js_closure_alloc(pipeline_close_callback as *const u8, 4);
            js_closure_set_capture_f64(close_listener, 0, state);
            js_closure_set_capture_f64(close_listener, 1, callback);
            js_closure_set_capture_f64(close_listener, 2, stage_array);
            js_closure_set_capture_f64(close_listener, 3, *stage);
            add_stream_listener_for_event(
                *stage,
                close_event,
                box_pointer(close_listener as *const u8),
            );
        }
        if let Some(signal) = options.signal {
            attach_abort_signal(signal, *stage);
        }
    }

    let success_stage = if !options.end_final && stages.len() >= 2 {
        stages[stages.len() - 2]
    } else {
        stages[stages.len() - 1]
    };
    let success_event = if get_hidden_value(success_stage, hidden_writable_flag_key()).is_some()
        && options.end_final
    {
        string_value(b"finish")
    } else {
        string_value(b"end")
    };
    let success = js_closure_alloc(pipeline_success_callback as *const u8, 2);
    js_closure_set_capture_f64(success, 0, state);
    js_closure_set_capture_f64(success, 1, callback);
    add_stream_listener_for_event(
        success_stage,
        success_event,
        box_pointer(success as *const u8),
    );
}

pub(super) fn wire_pipeline_pair(src: f64, dest: f64, end_dest: bool) {
    add_pipe_destination(src, dest);
    if !end_dest {
        add_pipe_no_end_destination(src, dest);
    }
    install_pipe_destination_listeners(src, dest);
    let _ = emit_stream_event(dest, string_value(b"pipe"), &[src]);
    set_readable_flowing(src, f64::from_bits(TAG_TRUE));
    let _ = emit_stream_event(src, string_value(b"resume"), &[]);
}

pub(super) fn pipeline_stage_has_next(value: f64) -> bool {
    let Some(obj) = object_ptr_from_value(value) else {
        return false;
    };
    unsafe {
        own_field_by_key_bytes(obj as *const ObjectHeader, b"next").is_some_and(is_callable_value)
    }
}

pub(super) fn pipeline_needs_collected_path(stages: &[f64]) -> bool {
    stages.iter().any(|stage| is_callable_value(*stage))
        || stages
            .first()
            .is_some_and(|stage| !is_pipeline_stream(*stage) && pipeline_stage_has_next(*stage))
}

pub(super) fn pipeline_empty_chunks() -> f64 {
    box_pointer(crate::array::js_array_alloc(0) as *const u8)
}

pub(super) fn pipeline_single_chunk(value: f64) -> f64 {
    let mut arr = crate::array::js_array_alloc(1);
    arr = crate::array::js_array_push_f64(arr, value);
    box_pointer(arr as *const u8)
}

pub(super) fn settle_pipeline_value(value: f64) -> Result<f64, f64> {
    let value = crate::promise::adapt_foreign_promise_value(value);
    if crate::promise::js_value_is_promise(value) == 0 {
        return Ok(value);
    }
    let promise = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    if promise.is_null() {
        return Ok(value);
    }
    for _ in 0..100_000 {
        unsafe {
            if (*promise).state != crate::promise::PromiseState::Pending {
                break;
            }
        }
        if crate::promise::js_promise_run_microtasks() == 0 {
            break;
        }
    }
    unsafe {
        match (*promise).state {
            crate::promise::PromiseState::Fulfilled => Ok((*promise).value),
            crate::promise::PromiseState::Rejected => Err((*promise).reason),
            crate::promise::PromiseState::Pending => Ok(value),
        }
    }
}

pub(super) fn catch_pipeline_throw(call: impl FnOnce() -> f64) -> Result<f64, f64> {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    if jumped == 0 {
        let value = call();
        crate::exception::js_try_end();
        Ok(value)
    } else {
        let err = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::exception::js_try_end();
        Err(err)
    }
}

pub(super) fn collect_pipeline_chunks(value: f64) -> Result<f64, f64> {
    let value = settle_pipeline_value(value)?;
    match value.to_bits() {
        TAG_UNDEFINED | TAG_NULL => return Ok(pipeline_empty_chunks()),
        _ => {}
    }
    if let Some(result) = js_node_stream_collect_chunks_result(value) {
        let chunks = result?;
        if pipeline_should_coalesce_chunks(value) {
            return Ok(pipeline_coalesce_chunks(chunks));
        }
        return Ok(chunks);
    }
    let raw = raw_ptr_from_value(value);
    if let Some(chunks) = collection_iterable_chunks(raw) {
        return Ok(chunks);
    }
    if let Some(chunks) = collect_pipeline_iterator_chunks(value)? {
        return Ok(chunks);
    }
    if object_ptr_from_value(value).is_some() {
        let collected = crate::promise::js_array_from_async(value);
        let settled = settle_pipeline_value(collected)?;
        if is_array_like_value(settled) {
            return Ok(settled);
        }
    }
    if is_single_chunk_value(value) {
        return Ok(pipeline_single_chunk(value));
    }
    Ok(pipeline_empty_chunks())
}

pub(super) fn pipeline_should_coalesce_chunks(value: f64) -> bool {
    is_transform_stream(value)
        && !has_truthy_hidden(value, hidden_key(b"readableObjectMode"))
        && !has_truthy_hidden(value, hidden_writable_object_mode_key())
}

pub(super) fn pipeline_value_to_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value);
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

pub(super) fn pipeline_coalesce_chunks(chunks: f64) -> f64 {
    if !is_array_like_value(chunks) {
        return chunks;
    }
    let arr = raw_ptr_from_value(chunks) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    if len <= 1 {
        return chunks;
    }
    let mut joined = String::new();
    for i in 0..len {
        joined.push_str(&pipeline_value_to_string(crate::array::js_array_get_f64(
            arr, i,
        )));
    }
    let value = string_value(joined.as_bytes());
    pipeline_single_chunk(value)
}

pub(super) fn pipeline_chunks_vec(chunks: f64) -> Vec<f64> {
    let mut values = Vec::new();
    push_chunk_values(chunks, &mut values, 0);
    values
}

pub(super) fn pipeline_iterator_result(value: f64) -> Option<(bool, f64)> {
    let obj = object_ptr_from_value(value)?;
    let done = js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(b"done"));
    let item = js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(b"value"));
    Some((crate::value::js_is_truthy(done) != 0, item))
}

pub(super) fn collect_pipeline_iterator_chunks(iterable: f64) -> Result<Option<f64>, f64> {
    if !pipeline_stage_has_next(iterable) {
        return Ok(None);
    }
    let mut out = crate::array::js_array_alloc(0);
    for _ in 0..100_000 {
        let next_result = catch_pipeline_throw(|| unsafe {
            crate::object::js_native_call_method(
                iterable,
                b"next".as_ptr() as *const i8,
                4,
                std::ptr::null(),
                0,
            )
        })?;
        let next_result = settle_pipeline_value(next_result)?;
        let Some((done, value)) = pipeline_iterator_result(next_result) else {
            return Ok(Some(box_pointer(out as *const u8)));
        };
        if done {
            return Ok(Some(box_pointer(out as *const u8)));
        }
        out = crate::array::js_array_push_f64(out, value);
    }
    Ok(Some(box_pointer(out as *const u8)))
}

pub(super) fn call_pipeline_function_stage(stage: f64, source: f64) -> Result<f64, f64> {
    let args = [source];
    let result = catch_pipeline_throw(|| unsafe {
        crate::closure::js_native_call_value(stage, args.as_ptr(), args.len())
    })?;
    settle_pipeline_value(result)
}

pub(super) fn write_pipeline_chunks_to_stream(
    stream: f64,
    chunks: f64,
    end_stream: bool,
) -> Result<(), f64> {
    for chunk in pipeline_chunks_vec(chunks) {
        let _ = write_writable_chunk(
            stream,
            chunk,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
        if let Some(err) = readable_hidden_error(stream) {
            return Err(err);
        }
    }
    if end_stream {
        finish_stream_with_args(
            stream,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    if let Some(err) = readable_hidden_error(stream) {
        Err(err)
    } else {
        Ok(())
    }
}

pub(super) fn fail_collected_pipeline(stages: &[f64], callback: f64, err: f64) {
    for stage in stages {
        if is_pipeline_stream(*stage) {
            destroy_stream(*stage, err);
        }
    }
    if is_callable_value(callback) {
        call_listener_args(f64::from_bits(TAG_UNDEFINED), callback, &[err]);
    }
}

pub(super) fn complete_collected_pipeline(callback: f64, value: f64) {
    if is_callable_value(callback) {
        call_listener_args(
            f64::from_bits(TAG_UNDEFINED),
            callback,
            &[f64::from_bits(TAG_UNDEFINED), value],
        );
    }
}

pub(super) fn run_collected_pipeline(
    stages: &[f64],
    callback: f64,
    options: PipelineOptions,
) -> f64 {
    let last = *stages.last().unwrap_or(&f64::from_bits(TAG_UNDEFINED));
    let first = stages[0];
    let mut chunks = if is_callable_value(first) {
        match call_pipeline_function_stage(first, f64::from_bits(TAG_UNDEFINED))
            .and_then(collect_pipeline_chunks)
        {
            Ok(chunks) => chunks,
            Err(err) => {
                fail_collected_pipeline(stages, callback, err);
                return last;
            }
        }
    } else {
        match collect_pipeline_chunks(first) {
            Ok(chunks) => chunks,
            Err(err) => {
                fail_collected_pipeline(stages, callback, err);
                return last;
            }
        }
    };

    for idx in 1..stages.len() {
        let stage = stages[idx];
        let is_last = idx + 1 == stages.len();
        if is_callable_value(stage) {
            match call_pipeline_function_stage(stage, chunks) {
                Ok(result) if is_last => {
                    if pipeline_stage_has_next(result) {
                        if let Err(err) = collect_pipeline_chunks(result) {
                            fail_collected_pipeline(stages, callback, err);
                            return last;
                        }
                        complete_collected_pipeline(callback, f64::from_bits(TAG_UNDEFINED));
                    } else {
                        complete_collected_pipeline(callback, result);
                    }
                    return last;
                }
                Ok(result) => match collect_pipeline_chunks(result) {
                    Ok(next_chunks) => chunks = next_chunks,
                    Err(err) => {
                        fail_collected_pipeline(stages, callback, err);
                        return last;
                    }
                },
                Err(err) => {
                    fail_collected_pipeline(stages, callback, err);
                    return last;
                }
            }
            continue;
        }

        if is_pipeline_stream(stage) {
            let end_stream = options.end_final || !is_last;
            if let Err(err) = write_pipeline_chunks_to_stream(stage, chunks, end_stream) {
                fail_collected_pipeline(stages, callback, err);
                return last;
            }
            if is_last {
                complete_collected_pipeline(callback, f64::from_bits(TAG_UNDEFINED));
                return last;
            }
            match collect_pipeline_chunks(stage) {
                Ok(next_chunks) => chunks = next_chunks,
                Err(err) => {
                    fail_collected_pipeline(stages, callback, err);
                    return last;
                }
            }
        } else {
            match collect_pipeline_chunks(stage) {
                Ok(next_chunks) => chunks = next_chunks,
                Err(err) => {
                    fail_collected_pipeline(stages, callback, err);
                    return last;
                }
            }
            if is_last {
                complete_collected_pipeline(callback, f64::from_bits(TAG_UNDEFINED));
                return last;
            }
        }
    }

    complete_collected_pipeline(callback, f64::from_bits(TAG_UNDEFINED));
    last
}

pub(super) fn start_pipeline_readable(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return;
    }
    set_readable_flowing(stream, f64::from_bits(TAG_TRUE));
    flush_pending_readable_chunks(stream);
    invoke_read_once(stream);
    schedule_readable_from_drain(stream);
    if stream_hidden_ended(stream) || has_truthy_hidden(stream, hidden_end_emitted_key()) {
        end_pipe_destinations(stream);
    }
}

#[cold]
pub(super) fn throw_pipeline_missing_streams() -> ! {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"The \"streams\" argument must be specified",
    )
}

#[cold]
pub(super) fn throw_pipeline_callback_required() -> ! {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"The \"streams[stream.length - 1]\" property must be of type function",
    )
}

#[cold]
pub(super) fn throw_readable_pipe_missing_destination() -> ! {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"Cannot read properties of undefined (reading 'on')",
    )
}
