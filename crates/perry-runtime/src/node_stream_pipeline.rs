//! node:stream — pipeline() / stream.compose() data-flow engine (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
};
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let stages = scope.root_nanbox_f64_slice(stages);
    let arr = scope.root_raw_mut_ptr(crate::array::js_array_alloc(stages.len() as u32));
    for stage in &stages {
        arr.set_raw_mut_ptr(crate::array::js_array_push_f64(
            arr.get_raw_mut_ptr(),
            stage.get_nanbox_f64(),
        ));
    }
    box_pointer(arr.get_raw_const_ptr())
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
    mark_live_pipe_consume_on_emit(src);
    mark_live_pipe_consume_on_emit(dest);
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

#[derive(Clone, Copy)]
pub(super) struct PipelineSettledValue {
    pub(super) value: f64,
    pub(super) fulfilled_promise: bool,
}

pub(super) fn settle_pipeline_value_with_origin(value: f64) -> Result<PipelineSettledValue, f64> {
    let value = crate::promise::adapt_foreign_promise_value(value);
    if crate::promise::js_value_is_promise(value) == 0 {
        return Ok(PipelineSettledValue {
            value,
            fulfilled_promise: false,
        });
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    for _ in 0..10_000 {
        let current = value_handle.get_nanbox_f64();
        if crate::promise::js_value_is_promise(current) == 0 {
            return Ok(PipelineSettledValue {
                value: current,
                fulfilled_promise: true,
            });
        }
        let promise = crate::value::js_nanbox_get_pointer(current) as *mut crate::promise::Promise;
        if promise.is_null() {
            return Ok(PipelineSettledValue {
                value: current,
                fulfilled_promise: false,
            });
        }
        unsafe {
            match (*promise).state {
                crate::promise::PromiseState::Fulfilled => {
                    return Ok(PipelineSettledValue {
                        value: (*promise).value,
                        fulfilled_promise: true,
                    })
                }
                crate::promise::PromiseState::Rejected => {
                    // Reason consumed by direct read (no reaction attached);
                    // mark handled so it is not reported as an unhandled
                    // rejection at program end (#1545).
                    crate::promise::mark_rejection_handled(promise);
                    return Err((*promise).reason);
                }
                crate::promise::PromiseState::Pending => {}
            }
        }

        crate::event_pump::perry_poll();
        let _ = crate::timer::js_timer_tick();
        let _ = crate::timer::js_callback_timer_tick();
        let _ = crate::timer::js_interval_timer_tick();
        if crate::event_pump::perry_has_work() == 0 {
            break;
        }
        crate::event_pump::js_wait_for_event();
    }

    let current = value_handle.get_nanbox_f64();
    if crate::promise::js_value_is_promise(current) == 0 {
        return Ok(PipelineSettledValue {
            value: current,
            fulfilled_promise: true,
        });
    }
    let promise = crate::value::js_nanbox_get_pointer(current) as *mut crate::promise::Promise;
    if promise.is_null() {
        return Ok(PipelineSettledValue {
            value: current,
            fulfilled_promise: false,
        });
    }
    unsafe {
        match (*promise).state {
            crate::promise::PromiseState::Fulfilled => Ok(PipelineSettledValue {
                value: (*promise).value,
                fulfilled_promise: true,
            }),
            crate::promise::PromiseState::Rejected => {
                crate::promise::mark_rejection_handled(promise);
                Err((*promise).reason)
            }
            crate::promise::PromiseState::Pending => Ok(PipelineSettledValue {
                value,
                fulfilled_promise: false,
            }),
        }
    }
}

pub(super) fn settle_pipeline_value(value: f64) -> Result<f64, f64> {
    settle_pipeline_value_with_origin(value).map(|settled| settled.value)
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    match value.get_nanbox_f64().to_bits() {
        TAG_UNDEFINED | TAG_NULL => return Ok(pipeline_empty_chunks()),
        _ => {}
    }
    if !readable_chunks_nonempty(value.get_nanbox_f64()) {
        if let Some(source_iterator) = get_hidden_value(
            value.get_nanbox_f64(),
            hidden_key(READABLE_SOURCE_ITERATOR_KEY),
        ) {
            let source_iterator = scope.root_nanbox_f64(source_iterator);
            if let Some(chunks) =
                collect_pipeline_iterator_chunks(source_iterator.get_nanbox_f64())?
            {
                return Ok(chunks);
            }
        }
    }
    if let Some(result) = js_node_stream_collect_chunks_result(value.get_nanbox_f64()) {
        return result;
    }
    let raw = raw_ptr_from_value(value.get_nanbox_f64());
    if let Some(chunks) = collection_iterable_chunks(raw) {
        return Ok(chunks);
    }
    if let Some(chunks) = collect_pipeline_iterator_chunks(value.get_nanbox_f64())? {
        return Ok(chunks);
    }
    if object_ptr_from_value(value.get_nanbox_f64()).is_some() {
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        let collected =
            crate::promise::js_array_from_async(value.get_nanbox_f64(), undefined, undefined);
        let settled = settle_pipeline_value(collected)?;
        if is_array_like_value(settled) {
            return Ok(settled);
        }
    }
    if is_single_chunk_value(value.get_nanbox_f64()) {
        return Ok(pipeline_single_chunk(value.get_nanbox_f64()));
    }
    Ok(pipeline_empty_chunks())
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let iterable = scope.root_nanbox_f64(iterable);
    let out = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    for _ in 0..100_000 {
        let next_result = catch_pipeline_throw(|| unsafe {
            crate::object::js_native_call_method(
                iterable.get_nanbox_f64(),
                b"next".as_ptr() as *const i8,
                4,
                std::ptr::null(),
                0,
            )
        })?;
        let next_result = settle_pipeline_value(next_result)?;
        let Some((done, value)) = pipeline_iterator_result(next_result) else {
            return Ok(Some(box_pointer(out.get_raw_const_ptr())));
        };
        if done {
            return Ok(Some(box_pointer(out.get_raw_const_ptr())));
        }
        let step_scope = crate::gc::RuntimeHandleScope::new();
        let value = step_scope.root_nanbox_f64(value);
        out.set_raw_mut_ptr(crate::array::js_array_push_f64(
            out.get_raw_mut_ptr(),
            value.get_nanbox_f64(),
        ));
    }
    Ok(Some(box_pointer(out.get_raw_const_ptr())))
}

pub(super) fn call_pipeline_function_stage(
    stage: f64,
    source: f64,
) -> Result<PipelineSettledValue, f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stage = scope.root_nanbox_f64(stage);
    let source = scope.root_nanbox_f64(source);
    if is_array_like_value(source.get_nanbox_f64()) {
        source.set_nanbox_f64(js_node_stream_readable_from(source.get_nanbox_f64()));
    }
    let args = [source.get_nanbox_f64()];
    let result = catch_pipeline_throw(|| unsafe {
        crate::closure::js_native_call_value(stage.get_nanbox_f64(), args.as_ptr(), args.len())
    })?;
    settle_pipeline_value_with_origin(result)
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

extern "C" fn collected_pipeline_error_noop(_closure: *const ClosureHeader, _err: f64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn install_collected_pipeline_error_guards(stages: &[f64]) {
    crate::closure::js_register_closure_arity(collected_pipeline_error_noop as *const u8, 1);
    let scope = crate::gc::RuntimeHandleScope::new();
    let stages = scope.root_nanbox_f64_slice(stages);
    let error = scope.root_nanbox_f64(string_value(b"error"));
    for stage in &stages {
        if is_pipeline_stream(stage.get_nanbox_f64()) {
            let listener = js_closure_alloc(collected_pipeline_error_noop as *const u8, 0);
            let listener = scope.root_raw_mut_ptr(listener);
            add_stream_listener_for_event(
                stage.get_nanbox_f64(),
                error.get_nanbox_f64(),
                box_pointer(listener.get_raw_const_ptr()),
            );
        }
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

pub(super) fn complete_collected_pipeline_with_value(callback: f64, value: f64) {
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
    install_collected_pipeline_error_guards(stages);
    let last = *stages.last().unwrap_or(&f64::from_bits(TAG_UNDEFINED));
    let first = stages[0];
    let mut chunks = if is_callable_value(first) {
        match call_pipeline_function_stage(first, f64::from_bits(TAG_UNDEFINED)) {
            Ok(result) => match collect_pipeline_chunks(result.value) {
                Ok(chunks) => chunks,
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
                    if result.fulfilled_promise {
                        complete_collected_pipeline_with_value(callback, result.value);
                        return last;
                    }
                    if pipeline_stage_has_next(result.value) {
                        if let Err(err) = collect_pipeline_chunks(result.value) {
                            fail_collected_pipeline(stages, callback, err);
                            return last;
                        }
                        complete_collected_pipeline(callback, f64::from_bits(TAG_UNDEFINED));
                    } else {
                        complete_collected_pipeline(callback, result.value);
                    }
                    return last;
                }
                Ok(result) => match collect_pipeline_chunks(result.value) {
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

fn compose_stage_values(stages: f64) -> Vec<f64> {
    if !is_array_like_value(stages) {
        return Vec::new();
    }
    pipeline_chunks_vec(stages)
}

fn compose_source_iterator(value: f64) -> Option<f64> {
    get_hidden_value(value, hidden_key(READABLE_SOURCE_ITERATOR_KEY))
}

fn compose_first_arg_is_source(value: f64) -> bool {
    if is_callable_value(value) {
        return false;
    }
    if is_pipeline_stream(value) {
        return get_hidden_value(value, hidden_writable_flag_key()).is_none()
            || readable_hidden_chunks(value).is_some()
            || compose_source_iterator(value).is_some();
    }
    !matches!(value.to_bits(), TAG_NULL | TAG_UNDEFINED)
        && !is_non_iterable_primitive_for_readable_from(value)
}

fn normalize_compose_source(value: f64) -> f64 {
    if is_pipeline_stream(value) {
        value
    } else {
        js_node_stream_readable_from(value)
    }
}

fn drain_compose_stream_stage(stage: f64) {
    for _ in 0..10_000 {
        let pending_write = writable_length(stage) > 0.0;
        let pending_flush = has_truthy_hidden(stage, hidden_transform_finishing_key());
        let ran = crate::event_pump::perry_poll();
        if !pending_write && !pending_flush && ran == 0 {
            break;
        }
        if !pending_write && !pending_flush && ran == 0 && crate::event_pump::perry_has_work() == 0
        {
            break;
        }
        if (pending_write || pending_flush) && ran == 0 && crate::event_pump::perry_has_work() != 0
        {
            crate::event_pump::js_wait_for_event();
        }
        if writable_length(stage) == 0.0
            && !has_truthy_hidden(stage, hidden_transform_finishing_key())
            && crate::event_pump::perry_has_work() == 0
        {
            break;
        }
    }
}

fn compose_empty_chunks() -> f64 {
    pipeline_empty_chunks()
}

fn compose_copy_chunks(chunks: f64) -> f64 {
    let values = pipeline_chunks_vec(chunks);
    let mut out = crate::array::js_array_alloc(values.len() as u32);
    for value in values {
        out = crate::array::js_array_push_f64(out, value);
    }
    box_pointer(out as *const u8)
}

fn compose_take_stage_output(stage: f64) -> Result<f64, f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stage = scope.root_nanbox_f64(stage);
    drain_compose_stream_stage(stage.get_nanbox_f64());
    if let Some(err) = readable_hidden_error(stage.get_nanbox_f64()) {
        return Err(err);
    }
    let chunks = readable_hidden_chunks(stage.get_nanbox_f64())
        .map(compose_copy_chunks)
        .unwrap_or_else(compose_empty_chunks);
    let chunks = scope.root_nanbox_f64(chunks);
    clear_readable_buffer(stage.get_nanbox_f64());
    clear_pending_readable_chunks(stage.get_nanbox_f64());
    if let Some(err) = readable_hidden_error(stage.get_nanbox_f64()) {
        Err(err)
    } else {
        Ok(chunks.get_nanbox_f64())
    }
}

fn compose_process_stream_stage(stage: f64, chunks: f64, end_stage: bool) -> Result<f64, f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stage = scope.root_nanbox_f64(stage);
    let chunks = scope.root_nanbox_f64(chunks);
    clear_readable_buffer(stage.get_nanbox_f64());
    clear_pending_readable_chunks(stage.get_nanbox_f64());
    let values = pipeline_chunks_vec(chunks.get_nanbox_f64());
    let values = scope.root_nanbox_f64_slice(&values);
    for chunk in &values {
        catch_pipeline_throw(|| {
            write_writable_chunk(
                stage.get_nanbox_f64(),
                chunk.get_nanbox_f64(),
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
            )
        })?;
        drain_compose_stream_stage(stage.get_nanbox_f64());
        if let Some(err) = readable_hidden_error(stage.get_nanbox_f64()) {
            return Err(err);
        }
    }
    if end_stage {
        catch_pipeline_throw(|| {
            finish_stream_with_args(
                stage.get_nanbox_f64(),
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
            );
            f64::from_bits(TAG_UNDEFINED)
        })?;
        drain_compose_stream_stage(stage.get_nanbox_f64());
        if let Some(err) = readable_hidden_error(stage.get_nanbox_f64()) {
            return Err(err);
        }
    }
    compose_take_stage_output(stage.get_nanbox_f64())
}

fn compose_process_callable_stage(stage: f64, chunks: f64) -> Result<f64, f64> {
    call_pipeline_function_stage(stage, chunks)
        .and_then(|result| collect_pipeline_chunks(result.value))
}

fn compose_process_stages(stages: &[f64], input: f64, end_stages: bool) -> Result<f64, f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stages = scope.root_nanbox_f64_slice(stages);
    let chunks = scope.root_nanbox_f64(input);
    for stage in &stages {
        if is_callable_value(stage.get_nanbox_f64()) {
            chunks.set_nanbox_f64(compose_process_callable_stage(
                stage.get_nanbox_f64(),
                chunks.get_nanbox_f64(),
            )?);
            continue;
        }
        if is_pipeline_stream(stage.get_nanbox_f64()) {
            chunks.set_nanbox_f64(compose_process_stream_stage(
                stage.get_nanbox_f64(),
                chunks.get_nanbox_f64(),
                end_stages,
            )?);
            continue;
        }
        chunks.set_nanbox_f64(collect_pipeline_chunks(stage.get_nanbox_f64())?);
    }
    Ok(chunks.get_nanbox_f64())
}

fn compose_push_output(composite: f64, chunks: f64) -> Result<(), f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(composite);
    let chunks = scope.root_nanbox_f64(chunks);
    let values = pipeline_chunks_vec(chunks.get_nanbox_f64());
    let values = scope.root_nanbox_f64_slice(&values);
    for chunk in &values {
        let _ = push_chunk(composite.get_nanbox_f64(), chunk.get_nanbox_f64());
        if let Some(err) = readable_hidden_error(composite.get_nanbox_f64()) {
            return Err(err);
        }
    }
    Ok(())
}

fn compose_destroy_stage_list(stages: f64, err: f64) {
    for stage in compose_stage_values(stages) {
        if is_pipeline_stream(stage) {
            destroy_stream(stage, err);
        }
    }
}

fn fail_composed_duplex(composite: f64, source: f64, stages: f64, err: f64) {
    if stream_destroyed(composite) {
        return;
    }
    if has_truthy_hidden(composite, hidden_key(b"__perryStreamComposePriming")) {
        set_hidden_value(
            composite,
            hidden_key(b"__perryStreamComposePendingError"),
            err,
        );
        return;
    }
    compose_destroy_stage_list(stages, err);
    if is_pipeline_stream(source) {
        destroy_stream(source, err);
    }
    destroy_stream(composite, err);
}

pub(super) extern "C" fn compose_stage_error_callback(
    closure: *const ClosureHeader,
    err: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let composite = js_closure_get_capture_f64(closure, 0);
    let source = js_closure_get_capture_f64(closure, 1);
    let stages = js_closure_get_capture_f64(closure, 2);
    fail_composed_duplex(composite, source, stages, err);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn compose_source_data_callback(
    closure: *const ClosureHeader,
    chunk: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let composite = js_closure_get_capture_f64(closure, 0);
    if stream_destroyed(composite) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let _ = write_writable_chunk(
        composite,
        chunk,
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn compose_source_end_callback(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let composite = js_closure_get_capture_f64(closure, 0);
    if !stream_destroyed(composite) {
        finish_stream_with_args(
            composite,
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn compose_source_error_callback(
    closure: *const ClosureHeader,
    err: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let composite = js_closure_get_capture_f64(closure, 0);
    let source = js_closure_get_capture_f64(closure, 1);
    let stages = js_closure_get_capture_f64(closure, 2);
    fail_composed_duplex(composite, source, stages, err);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn compose_duplex_write_callback(
    closure: *const ClosureHeader,
    chunk: f64,
    _encoding: f64,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let composite = js_closure_get_capture_f64(closure, 0);
    let stages_value = js_closure_get_capture_f64(closure, 1);
    let source = js_closure_get_capture_f64(closure, 2);
    let stages = compose_stage_values(stages_value);
    let result = compose_process_stages(&stages, pipeline_single_chunk(chunk), false)
        .and_then(|chunks| compose_push_output(composite, chunks));
    match result {
        Ok(()) => call_listener_args(composite, cb, &[]),
        Err(err) => {
            fail_composed_duplex(composite, source, stages_value, err);
            call_listener_args(composite, cb, &[err])
        }
    };
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn compose_duplex_final_callback(
    closure: *const ClosureHeader,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let composite = js_closure_get_capture_f64(closure, 0);
    let stages_value = js_closure_get_capture_f64(closure, 1);
    let source = js_closure_get_capture_f64(closure, 2);
    set_hidden_value(composite, hidden_ended_key(), f64::from_bits(TAG_FALSE));
    set_visible_readable(composite, true);
    let stages = compose_stage_values(stages_value);
    let result = compose_process_stages(&stages, compose_empty_chunks(), true)
        .and_then(|chunks| compose_push_output(composite, chunks));
    match result {
        Ok(()) => {
            schedule_readable_end(composite);
            call_listener_args(composite, cb, &[]);
        }
        Err(err) => {
            fail_composed_duplex(composite, source, stages_value, err);
            call_listener_args(composite, cb, &[err]);
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn install_compose_stage_error_listeners(composite: f64, source: f64, stages: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(composite);
    let source = scope.root_nanbox_f64(source);
    let stages = scope.root_nanbox_f64(stages);
    let stage_values = compose_stage_values(stages.get_nanbox_f64());
    let stage_values = scope.root_nanbox_f64_slice(&stage_values);
    let error_event = scope.root_nanbox_f64(string_value(b"error"));
    for stage in &stage_values {
        if !is_pipeline_stream(stage.get_nanbox_f64()) {
            continue;
        }
        let listener = js_closure_alloc(compose_stage_error_callback as *const u8, 3);
        let listener = scope.root_raw_mut_ptr(listener);
        js_closure_set_capture_f64(listener.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
        js_closure_set_capture_f64(listener.get_raw_mut_ptr(), 1, source.get_nanbox_f64());
        js_closure_set_capture_f64(listener.get_raw_mut_ptr(), 2, stages.get_nanbox_f64());
        add_stream_listener_for_event(
            stage.get_nanbox_f64(),
            error_event.get_nanbox_f64(),
            box_pointer(listener.get_raw_const_ptr()),
        );
    }
}

fn install_compose_source_listeners(composite: f64, source: f64, stages: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(composite);
    let source = scope.root_nanbox_f64(source);
    let stages = scope.root_nanbox_f64(stages);
    if !is_pipeline_stream(source.get_nanbox_f64()) {
        return;
    }
    let data = js_closure_alloc(compose_source_data_callback as *const u8, 1);
    let data = scope.root_raw_mut_ptr(data);
    js_closure_set_capture_f64(data.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    add_stream_listener_for_event(
        source.get_nanbox_f64(),
        string_value(b"data"),
        box_pointer(data.get_raw_const_ptr()),
    );

    let end = js_closure_alloc(compose_source_end_callback as *const u8, 1);
    let end = scope.root_raw_mut_ptr(end);
    js_closure_set_capture_f64(end.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    add_stream_listener_for_event(
        source.get_nanbox_f64(),
        string_value(b"end"),
        box_pointer(end.get_raw_const_ptr()),
    );

    install_compose_source_error_listener(
        composite.get_nanbox_f64(),
        source.get_nanbox_f64(),
        stages.get_nanbox_f64(),
    );

    start_pipeline_readable(source.get_nanbox_f64());
}

fn install_compose_source_error_listener(composite: f64, source: f64, stages: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(composite);
    let source = scope.root_nanbox_f64(source);
    let stages = scope.root_nanbox_f64(stages);
    let error = js_closure_alloc(compose_source_error_callback as *const u8, 3);
    let error = scope.root_raw_mut_ptr(error);
    js_closure_set_capture_f64(error.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    js_closure_set_capture_f64(error.get_raw_mut_ptr(), 1, source.get_nanbox_f64());
    js_closure_set_capture_f64(error.get_raw_mut_ptr(), 2, stages.get_nanbox_f64());
    add_stream_listener_for_event(
        source.get_nanbox_f64(),
        string_value(b"error"),
        box_pointer(error.get_raw_const_ptr()),
    );
}

fn install_composed_duplex_callbacks(composite: f64, stages: f64, source: f64, writable: bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(composite);
    let stages = scope.root_nanbox_f64(stages);
    let source = scope.root_nanbox_f64(source);
    let raw = raw_ptr_from_value(composite.get_nanbox_f64());
    if raw < 0x10000 {
        return;
    }
    let write = js_closure_alloc(compose_duplex_write_callback as *const u8, 3);
    let write = scope.root_raw_mut_ptr(write);
    js_closure_set_capture_f64(write.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    js_closure_set_capture_f64(write.get_raw_mut_ptr(), 1, stages.get_nanbox_f64());
    js_closure_set_capture_f64(write.get_raw_mut_ptr(), 2, source.get_nanbox_f64());
    let obj = raw_ptr_from_value(composite.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field_by_name(
        obj,
        hidden_write_key(),
        box_pointer(write.get_raw_const_ptr()),
    );

    let final_cb = js_closure_alloc(compose_duplex_final_callback as *const u8, 3);
    let final_cb = scope.root_raw_mut_ptr(final_cb);
    js_closure_set_capture_f64(final_cb.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    js_closure_set_capture_f64(final_cb.get_raw_mut_ptr(), 1, stages.get_nanbox_f64());
    js_closure_set_capture_f64(final_cb.get_raw_mut_ptr(), 2, source.get_nanbox_f64());
    let obj = raw_ptr_from_value(composite.get_nanbox_f64()) as *mut ObjectHeader;
    js_object_set_field_by_name(
        obj,
        hidden_writable_final_key(),
        box_pointer(final_cb.get_raw_const_ptr()),
    );

    set_hidden_value(
        composite.get_nanbox_f64(),
        hidden_key(b"writableCustomSink"),
        f64::from_bits(TAG_TRUE),
    );
    if !writable {
        set_visible_writable(composite.get_nanbox_f64(), false);
    }
}

fn compose_source_has_snapshot(source: f64) -> bool {
    readable_hidden_chunks(source).is_some() || compose_source_iterator(source).is_some()
}

fn prime_composed_duplex_from_source(composite: f64, source: f64, stages: f64) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(composite);
    let source = scope.root_nanbox_f64(source);
    let stages = scope.root_nanbox_f64(stages);
    prepare_readable_for_iteration(source.get_nanbox_f64());
    let chunks = match collect_pipeline_chunks(source.get_nanbox_f64()) {
        Ok(chunks) => chunks,
        Err(err) => {
            fail_composed_duplex(
                composite.get_nanbox_f64(),
                source.get_nanbox_f64(),
                stages.get_nanbox_f64(),
                err,
            );
            return true;
        }
    };
    let chunks = scope.root_nanbox_f64(chunks);
    let stage_values = compose_stage_values(stages.get_nanbox_f64());
    let stage_values = scope.root_nanbox_f64_slice(&stage_values);
    match compose_process_stages(
        &crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&stage_values),
        chunks.get_nanbox_f64(),
        true,
    )
    .and_then(|chunks| compose_push_output(composite.get_nanbox_f64(), chunks))
    {
        Ok(()) => {
            schedule_readable_end(composite.get_nanbox_f64());
        }
        Err(err) => {
            fail_composed_duplex(
                composite.get_nanbox_f64(),
                source.get_nanbox_f64(),
                stages.get_nanbox_f64(),
                err,
            );
        }
    }
    true
}

fn new_composed_duplex(stages: &[f64], source: Option<f64>, writable: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stages = scope.root_nanbox_f64_slice(stages);
    let source = source.map(|source| scope.root_nanbox_f64(source));
    let composite = scope.root_nanbox_f64(js_node_stream_duplex_new(readable_from_options(
        f64::from_bits(TAG_UNDEFINED),
    )));
    let stages_value = scope.root_nanbox_f64(pipeline_stage_array(
        &crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&stages),
    ));
    let source_value = source
        .as_ref()
        .map(|source| source.get_nanbox_f64())
        .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED));
    install_composed_duplex_callbacks(
        composite.get_nanbox_f64(),
        stages_value.get_nanbox_f64(),
        source_value,
        writable,
    );
    if let Some(source) = source.as_ref() {
        install_compose_stage_error_listeners(
            composite.get_nanbox_f64(),
            source.get_nanbox_f64(),
            stages_value.get_nanbox_f64(),
        );
        if !compose_source_has_snapshot(source.get_nanbox_f64()) {
            install_compose_source_listeners(
                composite.get_nanbox_f64(),
                source.get_nanbox_f64(),
                stages_value.get_nanbox_f64(),
            );
        } else {
            install_compose_source_error_listener(
                composite.get_nanbox_f64(),
                source.get_nanbox_f64(),
                stages_value.get_nanbox_f64(),
            );
            set_hidden_value(
                composite.get_nanbox_f64(),
                hidden_key(b"__perryStreamComposePriming"),
                f64::from_bits(TAG_TRUE),
            );
            prime_composed_duplex_from_source(
                composite.get_nanbox_f64(),
                source.get_nanbox_f64(),
                stages_value.get_nanbox_f64(),
            );
            set_hidden_value(
                composite.get_nanbox_f64(),
                hidden_key(b"__perryStreamComposePriming"),
                f64::from_bits(TAG_FALSE),
            );
            if let Some(err) = get_hidden_value(
                composite.get_nanbox_f64(),
                hidden_key(b"__perryStreamComposePendingError"),
            ) {
                set_hidden_value(
                    composite.get_nanbox_f64(),
                    hidden_key(b"__perryStreamComposePendingError"),
                    f64::from_bits(TAG_UNDEFINED),
                );
                fail_composed_duplex(
                    composite.get_nanbox_f64(),
                    source.get_nanbox_f64(),
                    stages_value.get_nanbox_f64(),
                    err,
                );
            }
        }
    } else {
        install_compose_stage_error_listeners(
            composite.get_nanbox_f64(),
            source_value,
            stages_value.get_nanbox_f64(),
        );
    }
    composite.get_nanbox_f64()
}

pub(super) fn build_node_stream_compose(args: Vec<f64>) -> f64 {
    if args.is_empty() {
        throw_pipeline_missing_streams();
    }
    if args.len() == 1 {
        let only = args[0];
        if is_transform_stream(only) {
            return only;
        }
        if compose_first_arg_is_source(only) {
            let source = normalize_compose_source(only);
            return new_composed_duplex(&[], Some(source), false);
        }
        return new_composed_duplex(&args, None, true);
    }

    if compose_first_arg_is_source(args[0]) {
        let source = normalize_compose_source(args[0]);
        return new_composed_duplex(&args[1..], Some(source), true);
    }

    new_composed_duplex(&args, None, true)
}

#[cold]
pub(super) fn throw_pipeline_missing_streams() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The \"streams\" argument must be specified",
        "ERR_MISSING_ARGS",
    )
}

#[cold]
pub(super) fn throw_pipeline_callback_required(callback: f64) -> ! {
    let received = ["PassThrough", "Transform", "Duplex", "Writable", "Readable"]
        .into_iter()
        .find(|name| is_classic_stream_instance_of(callback, name))
        .map(|name| format!("an instance of {name}"))
        .unwrap_or_else(|| crate::fs::validate::describe_received(callback));
    let message = format!(
        "The \"streams[stream.length - 1]\" property must be of type function. Received {received}"
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

#[cold]
pub(super) fn throw_pipeline_invalid_body(body: f64) -> ! {
    let message = format!(
        "The \"body\" argument must be of type function or an instance of Blob, ReadableStream, WritableStream, Stream, Iterable, AsyncIterable, or Promise or {{ readable, writable }} pair. Received {}",
        crate::fs::validate::describe_received(body)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

#[cold]
pub(super) fn throw_readable_pipe_missing_destination() -> ! {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"Cannot read properties of undefined (reading 'on')",
    )
}
