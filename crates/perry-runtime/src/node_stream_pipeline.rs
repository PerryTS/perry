//! node:stream — pipeline() / stream.compose() data-flow engine (split out of node_stream.rs for the 2000-line
//! file-size gate, #1987). Shares the parent module's constants, hidden-key
//! accessors and state primitives via `use super::*`.
use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_get_field_by_name_f64, ObjectHeader};

#[derive(Clone, Copy)]
pub(super) struct PipelineOptions {
    pub(super) end_final: bool,
    pub(super) signal: Option<f64>,
}

pub(super) extern "C" fn pipeline_success_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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

pub(super) extern "C" fn pipeline_error_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
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

pub(super) extern "C" fn pipeline_close_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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

pub(super) fn is_pipeline_stream(value: f64) -> bool {
    get_hidden_value(value, hidden_readable_flag_key()).is_some()
        || get_hidden_value(value, hidden_writable_flag_key()).is_some()
}

pub(super) fn is_pipeline_options_arg(value: f64) -> bool {
    object_ptr_from_value(value).is_some()
        && !is_pipeline_stream(value)
        && !is_array_like_value(value)
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
    // Destroying a stage runs its listeners, which can collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let stages = scope.root_nanbox_f64(stages);
    let err = scope.root_nanbox_f64(err);
    for i in 0..rooted_array_len(&stages) {
        destroy_stream(rooted_array_at(&stages, i), err.get_nanbox_f64());
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

/// Wire the classic pipeline's completion listeners onto the rooted stage
/// array `stages`. Each listener allocation and each listener-list growth can
/// collect, so every stage, the callback and the shared state are read from
/// handles after it.
pub(super) fn add_pipeline_callback_listeners(
    stages: &crate::gc::RuntimeHandle<'_>,
    callback: &crate::gc::RuntimeHandle<'_>,
    options: PipelineOptions,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(new_pipeline_callback_state());
    let error_event = scope.root_nanbox_f64(literal_string_value(b"error"));
    let close_event = scope.root_nanbox_f64(literal_string_value(b"close"));
    let signal = options.signal.map(|signal| scope.root_nanbox_f64(signal));
    let len = rooted_array_len(stages);
    for i in 0..len {
        let stage = scope.root_nanbox_f64(rooted_array_at(stages, i));
        let listener = rooted_closure(
            &scope,
            crate::fn_info!(pipeline_error_callback, 1; with_declared(1)),
            &[&state, callback, stages],
        );
        add_stream_listener_for_event(
            stage.get_nanbox_f64(),
            error_event.get_nanbox_f64(),
            listener.get_nanbox_f64(),
        );
        if !pipeline_stage_already_complete(stage.get_nanbox_f64()) {
            let close_listener = rooted_closure(
                &scope,
                crate::fn_info!(pipeline_close_callback, 0; with_declared(0)),
                &[&state, callback, stages, &stage],
            );
            add_stream_listener_for_event(
                stage.get_nanbox_f64(),
                close_event.get_nanbox_f64(),
                close_listener.get_nanbox_f64(),
            );
        }
        if let Some(signal) = &signal {
            attach_abort_signal(signal.get_nanbox_f64(), stage.get_nanbox_f64());
        }
    }

    let success_index = if !options.end_final && len >= 2 {
        len - 2
    } else {
        len - 1
    };
    let success_stage = scope.root_nanbox_f64(rooted_array_at(stages, success_index));
    let success_event =
        if get_hidden_value(success_stage.get_nanbox_f64(), hidden_writable_flag_key()).is_some()
            && options.end_final
        {
            literal_string_value(b"finish")
        } else {
            literal_string_value(b"end")
        };
    let success_event = scope.root_nanbox_f64(success_event);
    let success = rooted_closure(
        &scope,
        crate::fn_info!(pipeline_success_callback, 0; with_declared(0)),
        &[&state, callback],
    );
    add_stream_listener_for_event(
        success_stage.get_nanbox_f64(),
        success_event.get_nanbox_f64(),
        success.get_nanbox_f64(),
    );
}

pub(super) fn wire_pipeline_pair(src: f64, dest: f64, end_dest: bool) {
    // The `'pipe'` and `'resume'` listeners can collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = scope.root_nanbox_f64(src);
    let dest = scope.root_nanbox_f64(dest);
    add_pipe_destination(src.get_nanbox_f64(), dest.get_nanbox_f64());
    if !end_dest {
        add_pipe_no_end_destination(src.get_nanbox_f64(), dest.get_nanbox_f64());
    }
    install_pipe_destination_listeners(src.get_nanbox_f64(), dest.get_nanbox_f64());
    let pipe = literal_string_value(b"pipe");
    let _ = emit_stream_event(dest.get_nanbox_f64(), pipe, &[src.get_nanbox_f64()]);
    set_readable_flowing(src.get_nanbox_f64(), f64::from_bits(TAG_TRUE));
    let resume = literal_string_value(b"resume");
    let _ = emit_stream_event(src.get_nanbox_f64(), resume, &[]);
}

pub(super) fn pipeline_stage_has_next(value: f64) -> bool {
    let Some(obj) = object_ptr_from_value(value) else {
        return false;
    };
    unsafe {
        own_field_by_key_bytes(obj as *const ObjectHeader, b"next").is_some_and(is_callable_value)
    }
}

pub(super) fn pipeline_needs_collected_path(stages: &crate::gc::RuntimeHandle<'_>) -> bool {
    let len = rooted_array_len(stages);
    (0..len).any(|i| is_callable_value(rooted_array_at(stages, i)))
        || (len > 0 && {
            let first = rooted_array_at(stages, 0);
            !is_pipeline_stream(first) && pipeline_stage_has_next(first)
        })
}

pub(super) fn pipeline_empty_chunks() -> f64 {
    box_pointer(crate::array::js_array_alloc(0) as *const u8)
}

pub(super) fn pipeline_single_chunk(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let arr = rooted_empty_array(&scope);
    rooted_array_push(&arr, value.get_nanbox_f64());
    arr.get_nanbox_f64()
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

        // The steps of a compiled `await` busy-wait (codegen `fs_await.rs`).
        // This wait often runs inside a microtask (a stream drained from a
        // job, as every drain in a worker is), and only the await-loop drain
        // runs jobs when re-entered; `perry_poll`'s drain did not, so a source
        // whose next value needs one more job (an async generator) never
        // settled. The await-loop timer tick also fires inside a timer.
        crate::promise::microtasks::js_promise_run_microtasks_await_loop();
        crate::stdlib_pump::js_run_stdlib_pump();
        let _ = crate::timer::js_await_loop_tick_timers();
        // The steps above run jobs and can collect, which moves the promise:
        // re-read it from its handle before checking whether they settled it.
        let promise = crate::value::js_nanbox_get_pointer(value_handle.get_nanbox_f64())
            as *mut crate::promise::Promise;
        if crate::promise::js_promise_state(promise) != 0 {
            continue;
        }
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
                value: current,
                fulfilled_promise: false,
            }),
        }
    }
}

pub(super) fn settle_pipeline_value(value: f64) -> Result<f64, f64> {
    settle_pipeline_value_with_origin(value).map(|settled| settled.value)
}

pub(super) fn catch_pipeline_throw(call: impl FnOnce() -> f64) -> Result<f64, f64> {
    crate::exception::catch_js_throw(call)
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
            READABLE_SOURCE_ITERATOR_KEY,
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
        crate::closure::js_native_call_value(
            stage.get_nanbox_f64(),
            crate::closure::plain_call_receiver(),
            args.as_ptr(),
            args.len(),
        )
    })?;
    settle_pipeline_value_with_origin(result)
}

pub(super) fn write_pipeline_chunks_to_stream(
    stream: f64,
    chunks: f64,
    end_stream: bool,
) -> Result<(), f64> {
    // Each write runs the stream's `_write`, which can collect: write from a
    // rooted copy of the chunks to the rooted stream.
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let chunks = chunk_values_snapshot(&scope, chunks);
    for i in 0..rooted_array_len(&chunks) {
        let _ = write_writable_chunk(
            stream.get_nanbox_f64(),
            rooted_array_at(&chunks, i),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
        if let Some(err) = readable_hidden_error(stream.get_nanbox_f64()) {
            return Err(err);
        }
    }
    if end_stream {
        finish_stream_with_args(
            stream.get_nanbox_f64(),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    if let Some(err) = readable_hidden_error(stream.get_nanbox_f64()) {
        Err(err)
    } else {
        Ok(())
    }
}

/// Destroy the collected pipeline's stream stages with `err`, then call
/// `callback(err)`. Destroying a stage runs its listeners, which can collect.
pub(super) fn fail_collected_pipeline(
    stages: &crate::gc::RuntimeHandle<'_>,
    callback: &crate::gc::RuntimeHandle<'_>,
    err: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let err = scope.root_nanbox_f64(err);
    for i in 0..rooted_array_len(stages) {
        let stage = rooted_array_at(stages, i);
        if is_pipeline_stream(stage) {
            destroy_stream(stage, err.get_nanbox_f64());
        }
    }
    if is_callable_value(callback.get_nanbox_f64()) {
        call_listener_args(
            f64::from_bits(TAG_UNDEFINED),
            callback.get_nanbox_f64(),
            &[err.get_nanbox_f64()],
        );
    }
}

extern "C" fn collected_pipeline_error_noop(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _err: f64,
) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn install_collected_pipeline_error_guards(stages: &crate::gc::RuntimeHandle<'_>) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(literal_string_value(b"error"));
    for i in 0..rooted_array_len(stages) {
        if is_pipeline_stream(rooted_array_at(stages, i)) {
            let listener = rooted_closure(
                &scope,
                crate::fn_info!(collected_pipeline_error_noop, 1; with_declared(1)),
                &[],
            );
            add_stream_listener_for_event(
                rooted_array_at(stages, i),
                error.get_nanbox_f64(),
                listener.get_nanbox_f64(),
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

/// The collected (non-stream) pipeline: function stages, async iterables and
/// stream stages, run in order on fully collected chunks. Every stage runs
/// user code that can collect, so the stage list, the callback and the
/// current chunks are held in handles and reread after each stage.
pub(super) fn run_collected_pipeline(
    stages: &crate::gc::RuntimeHandle<'_>,
    callback: &crate::gc::RuntimeHandle<'_>,
    options: PipelineOptions,
) -> f64 {
    install_collected_pipeline_error_guards(stages);
    let len = rooted_array_len(stages);
    let last = || rooted_array_at(stages, len - 1);
    let fail = |err: f64| {
        fail_collected_pipeline(stages, callback, err);
        last()
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let first = rooted_array_at(stages, 0);
    let first_chunks = if is_callable_value(first) {
        call_pipeline_function_stage(first, f64::from_bits(TAG_UNDEFINED))
            .and_then(|result| collect_pipeline_chunks(result.value))
    } else {
        collect_pipeline_chunks(first)
    };
    let chunks = match first_chunks {
        Ok(chunks) => scope.root_nanbox_f64(chunks),
        Err(err) => return fail(err),
    };

    for idx in 1..len {
        let stage = rooted_array_at(stages, idx);
        let is_last = idx + 1 == len;
        if is_callable_value(stage) {
            match call_pipeline_function_stage(stage, chunks.get_nanbox_f64()) {
                Ok(result) if is_last => {
                    if result.fulfilled_promise {
                        complete_collected_pipeline(callback.get_nanbox_f64(), result.value);
                        return last();
                    }
                    if pipeline_stage_has_next(result.value) {
                        if let Err(err) = collect_pipeline_chunks(result.value) {
                            return fail(err);
                        }
                        complete_collected_pipeline(
                            callback.get_nanbox_f64(),
                            f64::from_bits(TAG_UNDEFINED),
                        );
                    } else {
                        complete_collected_pipeline(callback.get_nanbox_f64(), result.value);
                    }
                    return last();
                }
                Ok(result) => match collect_pipeline_chunks(result.value) {
                    Ok(next_chunks) => chunks.set_nanbox_f64(next_chunks),
                    Err(err) => return fail(err),
                },
                Err(err) => return fail(err),
            }
            continue;
        }

        if is_pipeline_stream(stage) {
            let end_stream = options.end_final || !is_last;
            if let Err(err) =
                write_pipeline_chunks_to_stream(stage, chunks.get_nanbox_f64(), end_stream)
            {
                return fail(err);
            }
            if is_last {
                complete_collected_pipeline(
                    callback.get_nanbox_f64(),
                    f64::from_bits(TAG_UNDEFINED),
                );
                return last();
            }
            match collect_pipeline_chunks(rooted_array_at(stages, idx)) {
                Ok(next_chunks) => chunks.set_nanbox_f64(next_chunks),
                Err(err) => return fail(err),
            }
        } else {
            match collect_pipeline_chunks(stage) {
                Ok(next_chunks) => chunks.set_nanbox_f64(next_chunks),
                Err(err) => return fail(err),
            }
            if is_last {
                complete_collected_pipeline(
                    callback.get_nanbox_f64(),
                    f64::from_bits(TAG_UNDEFINED),
                );
                return last();
            }
        }
    }

    complete_collected_pipeline(callback.get_nanbox_f64(), f64::from_bits(TAG_UNDEFINED));
    last()
}

pub(super) fn start_pipeline_readable(stream: f64) {
    if get_hidden_value(stream, hidden_readable_flag_key()).is_none() {
        return;
    }
    // Flushing runs `'data'` listeners and `_read`, both of which can collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    set_readable_flowing(stream.get_nanbox_f64(), f64::from_bits(TAG_TRUE));
    flush_pending_readable_chunks(stream.get_nanbox_f64());
    invoke_read_once(stream.get_nanbox_f64());
    schedule_readable_from_drain(stream.get_nanbox_f64());
    if stream_hidden_ended(stream.get_nanbox_f64())
        || has_truthy_hidden(stream.get_nanbox_f64(), hidden_end_emitted_key())
    {
        end_pipe_destinations(stream.get_nanbox_f64());
    }
}

/// The composed duplex's stage list (a GC array from `pipeline_stage_array`),
/// rooted in `scope`. Not an array: an empty list.
fn compose_stage_list<'s>(
    scope: &'s crate::gc::RuntimeHandleScope,
    stages: f64,
) -> crate::gc::RuntimeHandle<'s> {
    if is_array_like_value(stages) {
        scope.root_nanbox_f64(stages)
    } else {
        rooted_empty_array(scope)
    }
}

fn compose_source_iterator(value: f64) -> Option<f64> {
    get_hidden_value(value, READABLE_SOURCE_ITERATOR_KEY)
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
    let scope = crate::gc::RuntimeHandleScope::new();
    chunk_values_snapshot(&scope, chunks).get_nanbox_f64()
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
    let values = chunk_values_snapshot(&scope, chunks.get_nanbox_f64());
    for i in 0..rooted_array_len(&values) {
        catch_pipeline_throw(|| {
            write_writable_chunk(
                stage.get_nanbox_f64(),
                rooted_array_at(&values, i),
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

fn compose_process_stages(
    stages: &crate::gc::RuntimeHandle<'_>,
    input: f64,
    end_stages: bool,
) -> Result<f64, f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let chunks = scope.root_nanbox_f64(input);
    for i in 0..rooted_array_len(stages) {
        let stage = scope.root_nanbox_f64(rooted_array_at(stages, i));
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
    let values = chunk_values_snapshot(&scope, chunks.get_nanbox_f64());
    for i in 0..rooted_array_len(&values) {
        let _ = push_chunk(composite.get_nanbox_f64(), rooted_array_at(&values, i));
        if let Some(err) = readable_hidden_error(composite.get_nanbox_f64()) {
            return Err(err);
        }
    }
    Ok(())
}

fn fail_composed_duplex(composite: f64, source: f64, stages: f64, err: f64) {
    if stream_destroyed(composite) {
        return;
    }
    // Destroying a stream runs its listeners, which can collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(composite);
    let source = scope.root_nanbox_f64(source);
    let err = scope.root_nanbox_f64(err);
    let priming = Slot::ComposePriming;
    if has_truthy_hidden(composite.get_nanbox_f64(), priming) {
        let pending = Slot::ComposePendingError;
        set_hidden_value(composite.get_nanbox_f64(), pending, err.get_nanbox_f64());
        return;
    }
    let stages = compose_stage_list(&scope, stages);
    for i in 0..rooted_array_len(&stages) {
        let stage = rooted_array_at(&stages, i);
        if is_pipeline_stream(stage) {
            destroy_stream(stage, err.get_nanbox_f64());
        }
    }
    if is_pipeline_stream(source.get_nanbox_f64()) {
        destroy_stream(source.get_nanbox_f64(), err.get_nanbox_f64());
    }
    destroy_stream(composite.get_nanbox_f64(), err.get_nanbox_f64());
}

pub(super) extern "C" fn compose_stage_error_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
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
    _this: crate::closure::JsThis,
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

pub(super) extern "C" fn compose_source_end_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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
    _this: crate::closure::JsThis,
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
    _this: crate::closure::JsThis,
    chunk: f64,
    _encoding: f64,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // The stages run user code that can collect.
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let stages = compose_stage_list(&scope, js_closure_get_capture_f64(closure, 1));
    let source = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 2));
    let cb = scope.root_nanbox_f64(cb);
    let input = pipeline_single_chunk(chunk);
    let result = compose_process_stages(&stages, input, false)
        .and_then(|chunks| compose_push_output(composite.get_nanbox_f64(), chunks));
    match result {
        Ok(()) => call_listener_args(composite.get_nanbox_f64(), cb.get_nanbox_f64(), &[]),
        Err(err) => {
            let err = scope.root_nanbox_f64(err);
            fail_composed_duplex(
                composite.get_nanbox_f64(),
                source.get_nanbox_f64(),
                stages.get_nanbox_f64(),
                err.get_nanbox_f64(),
            );
            call_listener_args(
                composite.get_nanbox_f64(),
                cb.get_nanbox_f64(),
                &[err.get_nanbox_f64()],
            )
        }
    };
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn compose_duplex_final_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    cb: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let stages = compose_stage_list(&scope, js_closure_get_capture_f64(closure, 1));
    let source = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 2));
    let cb = scope.root_nanbox_f64(cb);
    set_hidden_value(
        composite.get_nanbox_f64(),
        hidden_ended_key(),
        f64::from_bits(TAG_FALSE),
    );
    set_visible_readable(composite.get_nanbox_f64(), true);
    let input = compose_empty_chunks();
    let result = compose_process_stages(&stages, input, true)
        .and_then(|chunks| compose_push_output(composite.get_nanbox_f64(), chunks));
    match result {
        Ok(()) => {
            schedule_readable_end(composite.get_nanbox_f64());
            call_listener_args(composite.get_nanbox_f64(), cb.get_nanbox_f64(), &[]);
        }
        Err(err) => {
            let err = scope.root_nanbox_f64(err);
            fail_composed_duplex(
                composite.get_nanbox_f64(),
                source.get_nanbox_f64(),
                stages.get_nanbox_f64(),
                err.get_nanbox_f64(),
            );
            call_listener_args(
                composite.get_nanbox_f64(),
                cb.get_nanbox_f64(),
                &[err.get_nanbox_f64()],
            );
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn install_compose_stage_error_listeners(composite: f64, source: f64, stages: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let composite = scope.root_nanbox_f64(composite);
    let source = scope.root_nanbox_f64(source);
    let stages = compose_stage_list(&scope, stages);
    let error_event = scope.root_nanbox_f64(literal_string_value(b"error"));
    for i in 0..rooted_array_len(&stages) {
        let stage = scope.root_nanbox_f64(rooted_array_at(&stages, i));
        if !is_pipeline_stream(stage.get_nanbox_f64()) {
            continue;
        }
        let listener = js_closure_alloc(
            crate::fn_info!(compose_stage_error_callback, 1; with_declared(1)),
            3,
        );
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
    let data = js_closure_alloc(
        crate::fn_info!(compose_source_data_callback, 1; with_declared(1)),
        1,
    );
    let data = scope.root_raw_mut_ptr(data);
    js_closure_set_capture_f64(data.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    add_stream_listener_for_event(
        source.get_nanbox_f64(),
        literal_string_value(b"data"),
        box_pointer(data.get_raw_const_ptr()),
    );

    let end = js_closure_alloc(
        crate::fn_info!(compose_source_end_callback, 0; with_declared(0)),
        1,
    );
    let end = scope.root_raw_mut_ptr(end);
    js_closure_set_capture_f64(end.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    add_stream_listener_for_event(
        source.get_nanbox_f64(),
        literal_string_value(b"end"),
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
    let error = js_closure_alloc(
        crate::fn_info!(compose_source_error_callback, 1; with_declared(1)),
        3,
    );
    let error = scope.root_raw_mut_ptr(error);
    js_closure_set_capture_f64(error.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    js_closure_set_capture_f64(error.get_raw_mut_ptr(), 1, source.get_nanbox_f64());
    js_closure_set_capture_f64(error.get_raw_mut_ptr(), 2, stages.get_nanbox_f64());
    add_stream_listener_for_event(
        source.get_nanbox_f64(),
        literal_string_value(b"error"),
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
    let write = js_closure_alloc(
        crate::fn_info!(compose_duplex_write_callback, 3; with_declared(3)),
        3,
    );
    let write = scope.root_raw_mut_ptr(write);
    js_closure_set_capture_f64(write.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    js_closure_set_capture_f64(write.get_raw_mut_ptr(), 1, stages.get_nanbox_f64());
    js_closure_set_capture_f64(write.get_raw_mut_ptr(), 2, source.get_nanbox_f64());
    set_hidden_value(
        composite.get_nanbox_f64(),
        hidden_write_key(),
        box_pointer(write.get_raw_const_ptr()),
    );

    let final_cb = js_closure_alloc(
        crate::fn_info!(compose_duplex_final_callback, 1; with_declared(1)),
        3,
    );
    let final_cb = scope.root_raw_mut_ptr(final_cb);
    js_closure_set_capture_f64(final_cb.get_raw_mut_ptr(), 0, composite.get_nanbox_f64());
    js_closure_set_capture_f64(final_cb.get_raw_mut_ptr(), 1, stages.get_nanbox_f64());
    js_closure_set_capture_f64(final_cb.get_raw_mut_ptr(), 2, source.get_nanbox_f64());
    set_hidden_value(
        composite.get_nanbox_f64(),
        hidden_writable_final_key(),
        box_pointer(final_cb.get_raw_const_ptr()),
    );

    set_hidden_value(
        composite.get_nanbox_f64(),
        Slot::WritableCustomSink,
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
    let stage_list = compose_stage_list(&scope, stages.get_nanbox_f64());
    match compose_process_stages(&stage_list, chunks.get_nanbox_f64(), true)
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
                Slot::ComposePriming,
                f64::from_bits(TAG_TRUE),
            );
            let primed = catch_pipeline_throw(|| {
                prime_composed_duplex_from_source(
                    composite.get_nanbox_f64(),
                    source.get_nanbox_f64(),
                    stages_value.get_nanbox_f64(),
                );
                f64::from_bits(TAG_UNDEFINED)
            });
            if let Err(err) = primed {
                let err = scope.root_nanbox_f64(err);
                fail_composed_duplex(
                    composite.get_nanbox_f64(),
                    source.get_nanbox_f64(),
                    stages_value.get_nanbox_f64(),
                    err.get_nanbox_f64(),
                );
            }
            set_hidden_value(
                composite.get_nanbox_f64(),
                Slot::ComposePriming,
                f64::from_bits(TAG_FALSE),
            );
            if let Some(err) = get_hidden_value(
                composite.get_nanbox_f64(),
                Slot::ComposePendingError,
            ) {
                let err = scope.root_nanbox_f64(err);
                set_hidden_value(
                    composite.get_nanbox_f64(),
                    Slot::ComposePendingError,
                    f64::from_bits(TAG_UNDEFINED),
                );
                fail_composed_duplex(
                    composite.get_nanbox_f64(),
                    source.get_nanbox_f64(),
                    stages_value.get_nanbox_f64(),
                    err.get_nanbox_f64(),
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
    // `Readable.from` below allocates and can run the source's iterator
    // lookup, so the arguments are rooted before it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let args = scope.root_nanbox_f64_slice(&args);
    let refreshed = |handles: &[crate::gc::RuntimeHandle<'_>]| {
        crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(handles)
    };
    if args.len() == 1 {
        let only = args[0].get_nanbox_f64();
        if is_transform_stream(only) {
            return only;
        }
        if compose_first_arg_is_source(only) {
            let source = normalize_compose_source(only);
            return new_composed_duplex(&[], Some(source), false);
        }
        return new_composed_duplex(&refreshed(&args), None, true);
    }

    if compose_first_arg_is_source(args[0].get_nanbox_f64()) {
        allocation_point();
        let source = scope.root_nanbox_f64(normalize_compose_source(args[0].get_nanbox_f64()));
        // `new_composed_duplex` roots the stages before it allocates.
        return new_composed_duplex(&refreshed(&args[1..]), Some(source.get_nanbox_f64()), true);
    }

    new_composed_duplex(&refreshed(&args), None, true)
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
