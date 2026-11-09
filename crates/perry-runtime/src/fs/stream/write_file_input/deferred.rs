//! Resumable iterable writes. Native read turns must run outside the write callback.

use super::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write;

const ITERATOR: usize = 0;
const OPTIONS: usize = 1;
const SINK: usize = 2;
const PENDING: usize = 3;
const ERROR: usize = 4;
const SIGNAL: usize = 5;
const ABORT_LISTENER: usize = 6;

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Pulling,
    WaitingNext,
    HandlingNext,
    WaitingReturn,
}

enum Destination {
    Path { file: std::fs::File, path: String },
    BorrowedFd(i32),
}

struct Operation {
    roots: [f64; 9],
    destination: Option<Destination>,
    pending_path: Option<String>,
    mode: f64,
    encoding: i32,
    phase: Phase,
}

crate::perry_thread_local! {
    static OPERATIONS: RefCell<HashMap<usize, Operation>> = RefCell::new(HashMap::new());
    static NEXT_OPERATION: Cell<usize> = const { Cell::new(1) };
}

fn scan_pending(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    OPERATIONS.with(|operations| {
        for operation in operations.borrow_mut().values_mut() {
            for value in &mut operation.roots {
                visitor.visit_nanbox_f64_slot(value);
            }
        }
    });
}

fn operation_id(closure: *const ClosureHeader) -> usize {
    js_closure_get_capture_ptr(closure, 0) as usize
}

fn snapshot<'a>(
    scope: &'a crate::gc::RuntimeHandleScope,
    id: usize,
) -> Option<Vec<crate::gc::RuntimeHandle<'a>>> {
    let values = OPERATIONS.with(|operations| {
        operations
            .borrow()
            .get(&id)
            .map(|operation| operation.roots)
    })?;
    Some(scope.root_nanbox_f64_slice(&values))
}

fn phase_is(id: usize, phase: Phase) -> bool {
    OPERATIONS.with(|operations| {
        operations
            .borrow()
            .get(&id)
            .is_some_and(|operation| operation.phase == phase)
    })
}

fn transition(id: usize, from: Phase, to: Phase) -> bool {
    OPERATIONS.with(|operations| {
        let mut operations = operations.borrow_mut();
        let Some(operation) = operations.get_mut(&id) else {
            return false;
        };
        if operation.phase != from {
            return false;
        }
        operation.phase = to;
        operation.roots[PENDING] = undefined_value();
        true
    })
}

fn finish(id: usize, success: bool) {
    let operation = OPERATIONS.with(|operations| operations.borrow_mut().remove(&id));
    let Some(operation) = operation else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let roots = scope.root_nanbox_f64_slice(&operation.roots);
    drop(operation.destination);
    if roots[ABORT_LISTENER].get_nanbox_f64().to_bits() != crate::value::TAG_UNDEFINED {
        let event = scope.root_nanbox_f64(string_value(b"abort"));
        if let Some(signal) = crate::event_target_hook_slots::abort_signal_ptr_from_value(
            roots[SIGNAL].get_nanbox_f64(),
        ) {
            crate::event_target_hook_slots::js_abort_signal_remove_listener(
                signal,
                event.get_nanbox_f64(),
                roots[ABORT_LISTENER].get_nanbox_f64(),
            );
        }
    }
    crate::fs::complete_deferred_write(
        roots[SINK].get_nanbox_f64(),
        operation.mode,
        operation.pending_path.as_deref(),
        (!success).then(|| roots[ERROR].get_nanbox_f64()),
    );
}

fn await_result(id: usize, value: f64, returning: bool) {
    let result = crate::exception::catch_js_throw(|| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let promise = if crate::promise::js_value_is_promise(value.get_nanbox_f64()) != 0 {
            crate::value::js_nanbox_get_pointer(value.get_nanbox_f64())
                as *mut crate::promise::Promise
        } else {
            crate::promise::js_promise_resolved(value.get_nanbox_f64())
        };
        let promise = scope.root_raw_mut_ptr(promise);
        let expected = if returning {
            Phase::WaitingReturn
        } else {
            Phase::WaitingNext
        };
        OPERATIONS.with(|operations| {
            if let Some(operation) = operations.borrow_mut().get_mut(&id) {
                if operation.phase == expected {
                    operation.roots[PENDING] =
                        promise.with_mut_ptr(|promise: *mut crate::promise::Promise| {
                            crate::value::js_nanbox_pointer(promise as i64)
                        });
                }
            }
        });
        let (fulfilled, rejected) = if returning {
            (
                crate::fn_info!(return_fulfilled, 1; with_declared(1)),
                crate::fn_info!(return_rejected, 1; with_declared(1)),
            )
        } else {
            (
                crate::fn_info!(next_fulfilled, 1; with_declared(1)),
                crate::fn_info!(next_rejected, 1; with_declared(1)),
            )
        };
        let fulfilled = scope.root_raw_mut_ptr(js_closure_alloc(fulfilled, 1));
        fulfilled.with_mut_ptr(|closure| {
            crate::closure::closure_set_capture_word(closure, 0, id as i64)
        });
        let rejected = scope.root_raw_mut_ptr(js_closure_alloc(rejected, 1));
        rejected.with_mut_ptr(|closure| {
            crate::closure::closure_set_capture_word(closure, 0, id as i64)
        });
        crate::promise::js_promise_attach_handlers(
            promise.get_raw_mut_ptr(),
            fulfilled.get_raw_const_ptr(),
            rejected.get_raw_const_ptr(),
        );
    });
    if let Err(error) = result {
        if returning {
            finish(id, false);
        } else {
            fail(id, error);
        }
    }
}

fn fail(id: usize, error: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(error);
    let destination = OPERATIONS.with(|operations| {
        let mut operations = operations.borrow_mut();
        let operation = operations.get_mut(&id)?;
        if operation.phase == Phase::WaitingReturn {
            return None;
        }
        operation.phase = Phase::WaitingReturn;
        operation.roots[ERROR] = error.get_nanbox_f64();
        operation.roots[PENDING] = undefined_value();
        Some(operation.destination.take())
    });
    let Some(destination) = destination else {
        return;
    };
    drop(destination);
    let Some(roots) = snapshot(&scope, id) else {
        return;
    };
    let returned = crate::exception::catch_js_throw(|| unsafe {
        crate::runtime_state_key!(b"return").call_value(
            roots[ITERATOR].get_nanbox_f64(),
            std::ptr::null(),
            0,
        )
    });
    match returned {
        Ok(value) => await_result(id, value, true),
        Err(_) => finish(id, false),
    }
}

fn pull(id: usize) {
    if !transition(id, Phase::Pulling, Phase::WaitingNext) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let Some(roots) = snapshot(&scope, id) else {
        return;
    };
    let result = crate::exception::catch_js_throw(|| {
        check_write_file_aborted(write_file_signal_from_options(
            roots[OPTIONS].get_nanbox_f64(),
        )?)?;
        Ok(unsafe {
            crate::object::js_native_call_method(
                roots[ITERATOR].get_nanbox_f64(),
                b"next".as_ptr().cast(),
                4,
                std::ptr::null(),
                0,
            )
        })
    });
    match result {
        Ok(Ok(value)) => await_result(id, value, false),
        Ok(Err(error)) | Err(error) => fail(id, error),
    }
}

fn consume_result(id: usize, value: f64) -> Result<bool, f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let Some(roots) = snapshot(&scope, id) else {
        return Ok(false);
    };
    check_write_file_aborted(write_file_signal_from_options(
        roots[OPTIONS].get_nanbox_f64(),
    )?)?;
    let Some((chunk, done)) = iterator_result_value_done(value.get_nanbox_f64()) else {
        throw_plain_type_error_value("Iterator result is not an object");
    };
    let chunk = scope.root_nanbox_f64(chunk);
    if done {
        return Ok(true);
    }
    let encoding = OPERATIONS.with(|operations| {
        operations
            .borrow()
            .get(&id)
            .map_or(0, |operation| operation.encoding)
    });
    let bytes = write_file_chunk_bytes(chunk.get_nanbox_f64(), encoding)?;
    // No JS or managed allocation runs under this borrow; error values are built afterward.
    let written = OPERATIONS.with(|operations| {
        let mut operations = operations.borrow_mut();
        let Some(operation) = operations.get_mut(&id) else {
            return Ok(None);
        };
        if operation.phase != Phase::HandlingNext {
            return Ok(None);
        }
        match operation.destination.as_mut() {
            Some(Destination::BorrowedFd(fd)) => Ok(Some(*fd)),
            Some(Destination::Path { file, path }) => file
                .write_all(&bytes)
                .map(|()| None)
                .map_err(|error| (error, path.clone())),
            None => Ok(None),
        }
    });
    match written {
        Ok(Some(fd)) => write_fd_chunk_result(fd, &bytes, false).map_err(|error| error.value())?,
        Ok(None) => {}
        Err((error, path)) => return Err(unsafe { build_fs_error_value(&error, "write", &path) }),
    }
    Ok(false)
}

extern "C" fn next_fulfilled(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let id = operation_id(closure);
    if !transition(id, Phase::WaitingNext, Phase::HandlingNext) {
        return undefined_value();
    }
    match crate::exception::catch_js_throw(|| consume_result(id, value)) {
        Ok(Ok(true)) if phase_is(id, Phase::HandlingNext) => finish(id, true),
        Ok(Ok(false)) => {
            if transition(id, Phase::HandlingNext, Phase::Pulling) {
                pull(id);
            }
        }
        Ok(Err(error)) | Err(error) => fail(id, error),
        _ => {}
    }
    undefined_value()
}

extern "C" fn next_rejected(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    error: f64,
) -> f64 {
    let id = operation_id(closure);
    if transition(id, Phase::WaitingNext, Phase::HandlingNext) {
        fail(id, error);
    }
    undefined_value()
}

extern "C" fn return_fulfilled(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _value: f64,
) -> f64 {
    let id = operation_id(closure);
    if phase_is(id, Phase::WaitingReturn) {
        finish(id, false);
    }
    undefined_value()
}

extern "C" fn return_rejected(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _reason: f64,
) -> f64 {
    return_fulfilled(closure, _this, _reason)
}

extern "C" fn abort(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _event: f64,
) -> f64 {
    let id = operation_id(closure);
    let active = OPERATIONS.with(|operations| {
        operations
            .borrow()
            .get(&id)
            .is_some_and(|operation| operation.phase != Phase::WaitingReturn)
    });
    if active {
        let error = crate::event_target_hook_slots::js_abort_error_value();
        fail(id, error);
    }
    undefined_value()
}

extern "C" fn pull_tick(closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    pull(operation_id(closure));
    undefined_value()
}

fn start(id: usize) {
    let result = crate::exception::catch_js_throw(|| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let Some(roots) = snapshot(&scope, id) else {
            return Ok(());
        };
        if roots[SIGNAL].get_nanbox_f64().to_bits() != crate::value::TAG_UNDEFINED {
            let listener = scope.root_raw_mut_ptr(js_closure_alloc(
                crate::fn_info!(abort, 1; with_declared(1)),
                1,
            ));
            listener.with_mut_ptr(|closure| {
                crate::closure::closure_set_capture_word(closure, 0, id as i64)
            });
            OPERATIONS.with(|operations| {
                if let Some(operation) = operations.borrow_mut().get_mut(&id) {
                    operation.roots[ABORT_LISTENER] =
                        listener.with_mut_ptr(|listener: *mut ClosureHeader| {
                            crate::value::js_nanbox_pointer(listener as i64)
                        });
                }
            });
            let event = scope.root_string_ptr(js_string_from_bytes(b"abort".as_ptr(), 5));
            if let Some(signal) = crate::event_target_hook_slots::abort_signal_ptr_from_value(
                roots[SIGNAL].get_nanbox_f64(),
            ) {
                unsafe {
                    crate::event_target_hook_slots::js_event_target_add_event_listener(
                        signal,
                        event.get_raw_const_ptr(),
                        listener.get_raw_mut_ptr::<ClosureHeader>() as i64,
                    );
                }
            }
            check_write_file_aborted(write_file_signal_from_options(
                roots[OPTIONS].get_nanbox_f64(),
            )?)?;
        }
        let closure = scope.root_raw_mut_ptr(js_closure_alloc(
            crate::fn_info!(pull_tick, 0; with_declared(0)),
            1,
        ));
        closure.with_mut_ptr(|closure| {
            crate::closure::closure_set_capture_word(closure, 0, id as i64)
        });
        crate::builtins::js_queue_microtask(closure.get_raw_mut_ptr::<ClosureHeader>() as i64);
        Ok(())
    });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) | Err(error) => fail(id, error),
    }
}

pub(crate) fn try_start_deferred_write_file(
    path: f64,
    data: f64,
    options: f64,
    sink: f64,
    mode: f64,
    pending_path: Option<String>,
) -> Result<bool, f64> {
    if is_direct_write_data(data) {
        return Ok(false);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let path = scope.root_nanbox_f64(path);
    let data = scope.root_nanbox_f64(data);
    let options = scope.root_nanbox_f64(options);
    let sink = scope.root_nanbox_f64(sink);
    validate::validate_path_or_fd("path", path.get_nanbox_f64(), "write");
    validate::validate_string_or_object_options("options", options.get_nanbox_f64());
    validate_write_file_data_source(data.get_nanbox_f64())?;
    let signal = write_file_signal_from_options(options.get_nanbox_f64())?;
    check_write_file_aborted(signal)?;
    let signal = scope.root_nanbox_f64(if signal.is_some() {
        unsafe {
            options_field_value(options.get_nanbox_f64(), b"signal")
                .map_or_else(undefined_value, |value| f64::from_bits(value.bits()))
        }
    } else {
        undefined_value()
    });
    let encoding = encoding_tag_from_options(options.get_nanbox_f64());
    let destination = if let Some(fd) = numeric_fd_value(path.get_nanbox_f64()) {
        Destination::BorrowedFd(fd)
    } else {
        let decoded = unsafe { decode_path_value(path.get_nanbox_f64()) }
            .unwrap_or_else(|| validate::throw_invalid_path_arg("path", path.get_nanbox_f64()));
        let flag = file_options_flag(options.get_nanbox_f64(), "w");
        let mode = write_mode_from_options(options.get_nanbox_f64());
        let file = open_file_for_write_flag(&decoded, &flag, mode)
            .map_err(|error| unsafe { build_fs_error_value(&error, "open", &decoded) })?;
        Destination::Path {
            file,
            path: decoded,
        }
    };
    // Keep the owned File outside the throw trap so a user iterator throw cannot skip its drop.
    let iterator = match crate::exception::catch_js_throw(|| {
        if let Some(id) = fs_read_stream_id(data.get_nanbox_f64()) {
            if let Some(error) = stored_node_error_value(id) {
                return Err(error);
            }
        }
        write_file_iterator_for_value(data.get_nanbox_f64())
            .ok_or_else(|| write_file_data_type_error(data.get_nanbox_f64()))
    }) {
        Ok(result) => result?,
        Err(error) => return Err(error),
    };
    let iterator = scope.root_nanbox_f64(iterator);
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::fs::pending_iterable_writes",
        scan_pending,
    );
    let id = NEXT_OPERATION.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });
    OPERATIONS.with(|operations| {
        operations.borrow_mut().insert(
            id,
            Operation {
                roots: [
                    iterator.get_nanbox_f64(),
                    options.get_nanbox_f64(),
                    sink.get_nanbox_f64(),
                    undefined_value(),
                    undefined_value(),
                    signal.get_nanbox_f64(),
                    undefined_value(),
                    data.get_nanbox_f64(),
                    path.get_nanbox_f64(),
                ],
                destination: Some(destination),
                pending_path,
                mode,
                encoding,
                phase: Phase::Pulling,
            },
        );
    });
    start(id);
    Ok(true)
}

#[cfg(test)]
mod relocation_tests;
