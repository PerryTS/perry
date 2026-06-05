//! Request queue wrapper for lowered `async function*` instances.
//!
//! Perry lowers a generator call to an object with own `next` / `return` /
//! `throw` closures. For async generators those closures already return
//! promises, but calling a second method in the same stack used to resume the
//! state machine synchronously. ECMAScript async generators queue requests:
//! same-stack follow-up requests resume from the microtask queue.

use super::{js_object_get_own_field_or_undef, js_object_set_field_by_name, ObjectHeader};
use crate::closure::{
    js_closure_alloc, js_closure_call1, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::promise::{
    js_promise_attach_settle_listener, js_promise_new, js_promise_reject, js_promise_resolve,
    js_value_is_promise, Promise, PromiseState,
};
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_UNDEFINED};
use std::cell::RefCell;
use std::collections::VecDeque;

struct AsyncGeneratorRequest {
    original: *const ClosureHeader,
    arg: f64,
    promise: *mut Promise,
}

struct AsyncGeneratorQueueState {
    active: bool,
    drain_scheduled: bool,
    queue: VecDeque<AsyncGeneratorRequest>,
}

thread_local! {
    static STATES: RefCell<Vec<AsyncGeneratorQueueState>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn wrap_async_generator_instance(obj: *mut ObjectHeader) {
    if obj.is_null() {
        return;
    }

    let Some(next) = own_closure(obj, b"next") else {
        return;
    };
    if is_queue_wrapper(next) {
        return;
    }
    let Some(ret) = own_closure(obj, b"return") else {
        return;
    };
    let Some(throw) = own_closure(obj, b"throw") else {
        return;
    };

    let state_id = STATES.with(|states| {
        let mut states = states.borrow_mut();
        let id = states.len() + 1;
        states.push(AsyncGeneratorQueueState {
            active: false,
            drain_scheduled: false,
            queue: VecDeque::new(),
        });
        id
    });

    set_method(
        obj,
        b"next",
        make_method_wrapper(state_id, next, async_generator_next_wrapper),
    );
    set_method(
        obj,
        b"return",
        make_method_wrapper(state_id, ret, async_generator_return_wrapper),
    );
    set_method(
        obj,
        b"throw",
        make_method_wrapper(state_id, throw, async_generator_throw_wrapper),
    );
}

pub(crate) fn scan_async_generator_queue_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    STATES.with(|states| {
        for state in states.borrow_mut().iter_mut() {
            for request in state.queue.iter_mut() {
                visitor.visit_raw_const_ptr_slot(&mut request.original);
                visitor.visit_nanbox_f64_slot(&mut request.arg);
                visitor.visit_raw_mut_ptr_slot(&mut request.promise);
            }
        }
    });
}

fn own_closure(obj: *mut ObjectHeader, name: &[u8]) -> Option<*const ClosureHeader> {
    let value =
        js_object_get_own_field_or_undef(js_nanbox_pointer(obj as i64), name.as_ptr(), name.len());
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_pointer() {
        let ptr = js_value.as_pointer::<ClosureHeader>();
        if crate::closure::is_closure_ptr(ptr as usize) {
            return Some(ptr);
        }
    }
    None
}

fn set_method(obj: *mut ObjectHeader, name: &[u8], closure: *mut ClosureHeader) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, js_nanbox_pointer(closure as i64));
}

fn make_method_wrapper(
    state_id: usize,
    original: *const ClosureHeader,
    func: extern "C" fn(*const ClosureHeader, f64) -> f64,
) -> *mut ClosureHeader {
    let wrapper = js_closure_alloc(func as *const u8, 2);
    js_closure_set_capture_f64(wrapper, 0, state_id as f64);
    js_closure_set_capture_ptr(wrapper, 1, original as i64);
    wrapper
}

fn make_settle_wrapper(
    state_id: usize,
    out: *mut Promise,
    is_fulfilled: bool,
) -> *mut ClosureHeader {
    let func = if is_fulfilled {
        async_generator_settle_fulfill as *const u8
    } else {
        async_generator_settle_reject as *const u8
    };
    let wrapper = js_closure_alloc(func, 2);
    js_closure_set_capture_f64(wrapper, 0, state_id as f64);
    js_closure_set_capture_ptr(wrapper, 1, out as i64);
    wrapper
}

fn make_drain_wrapper(state_id: usize) -> *mut ClosureHeader {
    let wrapper = js_closure_alloc(async_generator_drain_wrapper as *const u8, 1);
    js_closure_set_capture_f64(wrapper, 0, state_id as f64);
    wrapper
}

fn is_queue_wrapper(closure: *const ClosureHeader) -> bool {
    if closure.is_null() {
        return false;
    }
    let func = unsafe { (*closure).func_ptr };
    func == async_generator_next_wrapper as *const u8
        || func == async_generator_return_wrapper as *const u8
        || func == async_generator_throw_wrapper as *const u8
}

fn state_id_from_wrapper(closure: *const ClosureHeader) -> Option<usize> {
    if closure.is_null() {
        return None;
    }
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    if id == 0 {
        None
    } else {
        Some(id)
    }
}

fn original_from_wrapper(closure: *const ClosureHeader) -> *const ClosureHeader {
    js_closure_get_capture_ptr(closure, 1) as *const ClosureHeader
}

extern "C" fn async_generator_next_wrapper(closure: *const ClosureHeader, arg: f64) -> f64 {
    async_generator_request(closure, arg)
}

extern "C" fn async_generator_return_wrapper(closure: *const ClosureHeader, arg: f64) -> f64 {
    async_generator_request(closure, arg)
}

extern "C" fn async_generator_throw_wrapper(closure: *const ClosureHeader, arg: f64) -> f64 {
    async_generator_request(closure, arg)
}

fn async_generator_request(closure: *const ClosureHeader, arg: f64) -> f64 {
    let Some(state_id) = state_id_from_wrapper(closure) else {
        return call_original(original_from_wrapper(closure), arg);
    };
    let original = original_from_wrapper(closure);

    let should_queue = STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(state_id - 1) else {
            return false;
        };
        if state.active || !state.queue.is_empty() {
            return true;
        }
        state.active = true;
        false
    });

    if should_queue {
        let scope = crate::gc::RuntimeHandleScope::new();
        let original_handle = scope.root_raw_const_ptr(original);
        let arg_handle = scope.root_nanbox_f64(arg);
        let promise = js_promise_new();
        let original = original_handle.get_raw_const_ptr::<ClosureHeader>();
        let arg = arg_handle.get_nanbox_f64();
        STATES.with(|states| {
            if let Some(state) = states.borrow_mut().get_mut(state_id - 1) {
                crate::gc::runtime_write_barrier_root_raw_ptr(original);
                crate::gc::runtime_write_barrier_root_nanbox(arg.to_bits());
                crate::gc::runtime_write_barrier_root_raw_ptr(promise);
                state.queue.push_back(AsyncGeneratorRequest {
                    original,
                    arg,
                    promise,
                });
            } else {
                js_promise_reject(promise, f64::from_bits(TAG_UNDEFINED));
            }
        });
        return boxed_promise(promise);
    }

    let result = call_original(original, arg);
    after_initial_result(state_id, result);
    result
}

extern "C" fn async_generator_drain_wrapper(closure: *const ClosureHeader) -> f64 {
    if let Some(state_id) = state_id_from_wrapper(closure) {
        process_one_queued_request(state_id);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn async_generator_settle_fulfill(closure: *const ClosureHeader, value: f64) -> f64 {
    if let Some(state_id) = state_id_from_wrapper(closure) {
        let out = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
        finish_after_pending_result(state_id, out, true, value);
    }
    value
}

extern "C" fn async_generator_settle_reject(closure: *const ClosureHeader, reason: f64) -> f64 {
    if let Some(state_id) = state_id_from_wrapper(closure) {
        let out = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
        finish_after_pending_result(state_id, out, false, reason);
    }
    reason
}

fn call_original(original: *const ClosureHeader, arg: f64) -> f64 {
    if original.is_null() {
        return boxed_promise(crate::promise::js_promise_rejected(f64::from_bits(
            TAG_UNDEFINED,
        )));
    }
    js_closure_call1(original, arg)
}

fn after_initial_result(state_id: usize, result: f64) {
    if let Some(promise) = promise_ptr(result) {
        let state = unsafe { (*promise).state };
        if state == PromiseState::Pending {
            attach_pending_settle(state_id, promise, std::ptr::null_mut());
            return;
        }
    }
    schedule_drain(state_id);
}

fn after_queued_result(state_id: usize, out: *mut Promise, result: f64) {
    if let Some(promise) = promise_ptr(result) {
        match unsafe { (*promise).state } {
            PromiseState::Pending => {
                attach_pending_settle(state_id, promise, out);
            }
            PromiseState::Fulfilled => {
                let value = unsafe { (*promise).value };
                finish_after_immediate_queued_result(state_id, out, true, value);
            }
            PromiseState::Rejected => {
                let reason = unsafe { (*promise).reason };
                finish_after_immediate_queued_result(state_id, out, false, reason);
            }
        }
    } else {
        finish_after_immediate_queued_result(state_id, out, true, result);
    }
}

fn attach_pending_settle(state_id: usize, promise: *mut Promise, out: *mut Promise) {
    let fulfill = make_settle_wrapper(state_id, out, true);
    let reject = make_settle_wrapper(state_id, out, false);
    js_promise_attach_settle_listener(promise, fulfill, reject);
}

fn process_one_queued_request(state_id: usize) {
    let request_and_original = STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(state_id - 1) else {
            return None;
        };
        state.drain_scheduled = false;
        let Some(request) = state.queue.pop_front() else {
            state.active = false;
            return None;
        };
        let original = request.original;
        Some((request, original))
    });

    let Some((request, original)) = request_and_original else {
        return;
    };
    let result = call_original(original, request.arg);
    after_queued_result(state_id, request.promise, result);
}

fn finish_after_pending_result(state_id: usize, out: *mut Promise, fulfilled: bool, value: f64) {
    let has_queue = STATES.with(|states| {
        states
            .borrow()
            .get(state_id - 1)
            .is_some_and(|state| !state.queue.is_empty())
    });
    if has_queue {
        process_one_queued_request(state_id);
    } else {
        mark_inactive(state_id);
    }
    settle_out(out, fulfilled, value);
}

fn finish_after_immediate_queued_result(
    state_id: usize,
    out: *mut Promise,
    fulfilled: bool,
    value: f64,
) {
    let has_queue = STATES.with(|states| {
        states
            .borrow()
            .get(state_id - 1)
            .is_some_and(|state| !state.queue.is_empty())
    });
    if has_queue {
        schedule_drain(state_id);
    } else {
        mark_inactive(state_id);
    }
    settle_out(out, fulfilled, value);
}

fn mark_inactive(state_id: usize) {
    STATES.with(|states| {
        if let Some(state) = states.borrow_mut().get_mut(state_id - 1) {
            state.active = false;
            state.drain_scheduled = false;
        }
    });
}

fn schedule_drain(state_id: usize) {
    let should_schedule = STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(state_id - 1) else {
            return false;
        };
        if state.drain_scheduled {
            return false;
        }
        state.drain_scheduled = true;
        true
    });
    if should_schedule {
        let closure = make_drain_wrapper(state_id);
        crate::promise::enqueue_queue_microtask(closure as i64);
    }
}

fn settle_out(out: *mut Promise, fulfilled: bool, value: f64) {
    if out.is_null() {
        return;
    }
    if fulfilled {
        js_promise_resolve(out, value);
    } else {
        js_promise_reject(out, value);
    }
}

fn promise_ptr(value: f64) -> Option<*mut Promise> {
    if js_value_is_promise(value) == 0 {
        return None;
    }
    let ptr = js_nanbox_get_pointer(value) as *mut Promise;
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

fn boxed_promise(promise: *mut Promise) -> f64 {
    if promise.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        js_nanbox_pointer(promise as i64)
    }
}
