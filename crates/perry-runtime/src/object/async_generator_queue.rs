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

/// Which `AsyncGenerator.prototype` method a queued request came from. Only
/// `return` triggers the spec's `Await` of the resume value
/// (`AsyncGeneratorUnwrapYieldResumption` / `AsyncGeneratorAwaitReturn`): a
/// `.return(v)` resume value is awaited (unwrapped) before being delivered as
/// `{ value, done: true }`. `.next(v)` / `.throw(e)` deliver their argument
/// straight through.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestKind {
    NextOrThrow,
    Return,
}

struct AsyncGeneratorRequest {
    original: *const ClosureHeader,
    arg: f64,
    promise: *mut Promise,
    kind: RequestKind,
}

struct AsyncGeneratorQueueState {
    active: bool,
    drain_scheduled: bool,
    started: bool,
    completed: bool,
    original_throw: *const ClosureHeader,
    queue: VecDeque<AsyncGeneratorRequest>,
}

crate::perry_thread_local! {
    static STATES: RefCell<Vec<AsyncGeneratorQueueState>> = const { RefCell::new(Vec::new()) };
}

/// Install the queue wrappers on an async-generator instance's own
/// `next`/`return`/`throw`.
///
/// # Rooting (#7577)
///
/// Called from `js_generator_attach_prototype` /
/// `js_generator_attach_closure_prototype`, both of which now hand in a
/// freshly re-read receiver — which this function then let go stale again.
/// Each `set_method` pair allocates twice (`js_closure_alloc` for the wrapper,
/// `js_object_set_field_by_name` for the keys-array transition), so by the
/// third `set_method` the `obj` bound at entry was two collections old, and the
/// two ORIGINAL closures still waiting to be captured were as well — a wrapper
/// built over a moved `original` calls into retired from-space on the first
/// `gen.return()`.
///
/// Everything is therefore held in a `RuntimeHandleScope` and re-read at each
/// use. `own_closure` reads are allocation-free
/// (`js_object_get_own_field_or_undef` matches by byte slice), so the three
/// lookups can be done up front; it is the writes that move things.
pub(crate) fn wrap_async_generator_instance(obj: *mut ObjectHeader) {
    if obj.is_null() {
        return;
    }
    register_wrapper_arities();

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

    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(js_nanbox_pointer(obj as i64));
    let next_h = scope.root_nanbox_f64(js_nanbox_pointer(next as i64));
    let ret_h = scope.root_nanbox_f64(js_nanbox_pointer(ret as i64));
    let throw_h = scope.root_nanbox_f64(js_nanbox_pointer(throw as i64));
    let closure_now = |h: &crate::gc::RuntimeHandle<'_>| {
        js_nanbox_get_pointer(h.get_nanbox_f64()) as *const ClosureHeader
    };

    let state_id = STATES.with(|states| {
        let mut states = states.borrow_mut();
        let id = states.len() + 1;
        let original_throw = closure_now(&throw_h);
        crate::gc::runtime_write_barrier_root_raw_ptr(original_throw);
        states.push(AsyncGeneratorQueueState {
            active: false,
            drain_scheduled: false,
            started: false,
            completed: false,
            original_throw,
            queue: VecDeque::new(),
        });
        id
    });

    let obj_now = || js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *mut ObjectHeader;

    for (name, original_h, func) in [
        (
            b"next".as_slice(),
            &next_h,
            async_generator_next_wrapper as extern "C" fn(*const ClosureHeader, f64) -> f64,
        ),
        (b"return".as_slice(), &ret_h, async_generator_return_wrapper),
        (b"throw".as_slice(), &throw_h, async_generator_throw_wrapper),
    ] {
        // Inlined rather than routed through `make_method_wrapper` for one
        // reason: that helper takes `original` as a parameter, so the caller
        // would have to bind it BEFORE `js_closure_alloc` runs and the capture
        // store would write a pre-collection address. Reading it from the
        // handle after the allocation is the whole fix.
        let wrapper = js_closure_alloc(func as *const u8, 2);
        js_closure_set_capture_f64(wrapper, 0, state_id as f64);
        js_closure_set_capture_ptr(wrapper, 1, closure_now(original_h) as i64);
        // `set_method` roots both of its pointer arguments before it allocates
        // its key string; nothing between here and the call allocates.
        set_method(obj_now(), name, wrapper);
    }
}

pub(crate) fn scan_async_generator_queue_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    STATES.with(|states| {
        for state in states.borrow_mut().iter_mut() {
            visitor.visit_raw_const_ptr_slot(&mut state.original_throw);
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

/// `obj[name] = closure`.
///
/// #7577: `js_string_from_bytes` allocates, so both pointer arguments are
/// rooted across it and re-read at the store. Pre-fix, building the key could
/// move the receiver and the closure, and the store then wrote a moved closure
/// into a moved object using both pre-collection addresses.
fn set_method(obj: *mut ObjectHeader, name: &[u8], closure: *mut ClosureHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_h = scope.root_nanbox_f64(js_nanbox_pointer(obj as i64));
    let closure_h = scope.root_nanbox_f64(js_nanbox_pointer(closure as i64));
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(
        js_nanbox_get_pointer(obj_h.get_nanbox_f64()) as *mut ObjectHeader,
        key,
        closure_h.get_nanbox_f64(),
    );
}

/// #4547: the queue wrappers each take a single `arg` (the value passed to
/// `next`/`return`/`throw`). Without a registered arity, dispatch padding has
/// no declared count, so a 0-arg `gen.return()` / `gen.throw()` read an
/// uninitialized stack slot for `arg` instead of `undefined`. Record arity 1
/// for all three func pointers so the call path pads the missing argument.
fn register_wrapper_arities() {
    crate::perry_thread_local! {
        static REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    REGISTERED.with(|done| {
        if done.get() {
            return;
        }
        done.set(true);
        crate::closure::js_register_closure_arity(async_generator_next_wrapper as *const u8, 1);
        crate::closure::js_register_closure_arity(async_generator_return_wrapper as *const u8, 1);
        crate::closure::js_register_closure_arity(async_generator_throw_wrapper as *const u8, 1);
    });
}

// #7577: `make_method_wrapper(state_id, original, func)` used to live here. Its
// signature was the defect — taking `original` as a parameter forced every
// caller to bind that pointer BEFORE the `js_closure_alloc` inside, so the
// capture store wrote a pre-collection address. The one caller now allocates
// the wrapper first and reads `original` out of its handle afterwards, which
// leaves no correct way to call this helper; per CLAUDE.md's kill-policy the
// losing shape is deleted rather than left for a future caller to reach for.

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

/// True if `obj` is an async-generator instance — i.e. its own `next` is one of
/// this module's request-queue wrappers (installed by
/// `wrap_async_generator_instance`). Sync generator instances also expose own
/// `next`/`return`/`throw`, so a structural "has these methods" check cannot
/// tell the two apart; the queue wrapper is the async brand. Used by the
/// `%AsyncGenerator.prototype%` thunks to reject a sync-generator `this`.
pub(crate) fn is_async_generator_instance(obj: *mut ObjectHeader) -> bool {
    match own_closure(obj, b"next") {
        Some(next) => is_queue_wrapper(next),
        None => false,
    }
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

pub(crate) fn is_async_generator_instance_value(value: f64) -> bool {
    let ptr = crate::value::js_nanbox_get_pointer(value) as usize;
    if ptr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let is_object = unsafe {
        let gc = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
    };
    if !is_object {
        return false;
    }
    own_closure(ptr as *mut ObjectHeader, b"next").is_some_and(is_queue_wrapper)
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
    async_generator_request(closure, arg, RequestKind::NextOrThrow)
}

extern "C" fn async_generator_return_wrapper(closure: *const ClosureHeader, arg: f64) -> f64 {
    async_generator_request(closure, arg, RequestKind::Return)
}

extern "C" fn async_generator_throw_wrapper(closure: *const ClosureHeader, arg: f64) -> f64 {
    async_generator_request(closure, arg, RequestKind::NextOrThrow)
}

fn async_generator_request(closure: *const ClosureHeader, arg: f64, kind: RequestKind) -> f64 {
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
        // `across_const` pairs the allocating call with the re-read, so the
        // closure pointer cannot be bound stale in between (#7341).
        let (promise, original) =
            original_handle.across_const::<ClosureHeader, _>(|| js_promise_new());
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
                    kind,
                });
            } else {
                js_promise_reject(promise, f64::from_bits(TAG_UNDEFINED));
            }
        });
        return boxed_promise(promise);
    }

    // `.return(v)` must Await `v` before resuming/closing the generator (spec
    // `AsyncGeneratorUnwrapYieldResumption` for suspendedYield and
    // `AsyncGeneratorAwaitReturn` for suspendedStart/completed). Route through
    // an explicit output promise so the unwrap happens on the microtask queue.
    if kind == RequestKind::Return {
        let out = js_promise_new();
        dispatch_return_with_await(state_id, original, arg, out);
        return boxed_promise(out);
    }

    let result = call_original(original, arg);
    after_initial_result(state_id, result);
    result
}

/// Await the `.return(v)` value, then invoke the original return closure with
/// the unwrapped value and settle `out` from its result. A rejected await
/// reports the rejection through `out` (the generator is left closed by the
/// fulfill path's `call_original`, never resumed).
fn dispatch_return_with_await(
    state_id: usize,
    original: *const ClosureHeader,
    arg: f64,
    out: *mut Promise,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let original_h = scope.root_raw_const_ptr(original);
    let out_h = scope.root_raw_mut_ptr(out);
    let arg_h = scope.root_nanbox_f64(arg);
    let arg_promise = match crate::promise::js_promise_resolved_catching(arg_h.get_nanbox_f64()) {
        Ok(promise) => promise,
        Err(reason) => {
            let suspended_throw = STATES.with(|states| {
                states
                    .borrow()
                    .get(state_id - 1)
                    .filter(|state| state.started && !state.completed)
                    .map(|state| state.original_throw)
            });
            if let Some(throw_original) = suspended_throw {
                let throw_h = scope.root_raw_const_ptr(throw_original);
                let result = throw_h.with_const_ptr(|throw| call_original(throw, reason));
                out_h.with_mut_ptr(|out| after_queued_result(state_id, out, result));
            } else {
                out_h.with_mut_ptr(|out| {
                    finish_after_immediate_queued_result(state_id, out, false, reason)
                });
            }
            return;
        }
    };
    let arg_promise_h = scope.root_raw_mut_ptr(arg_promise);
    let fulfill = make_return_step_wrapper(state_id, &original_h, &out_h, true);
    let fulfill_h = scope.root_raw_mut_ptr(fulfill);
    let reject = make_return_step_wrapper(state_id, &original_h, &out_h, false);
    arg_promise_h.with_mut_ptr(|arg_promise| {
        fulfill_h
            .with_mut_ptr(|fulfill| js_promise_attach_settle_listener(arg_promise, fulfill, reject))
    });
}

fn make_return_step_wrapper(
    state_id: usize,
    original: &crate::gc::RuntimeHandle<'_>,
    out: &crate::gc::RuntimeHandle<'_>,
    is_fulfilled: bool,
) -> *mut ClosureHeader {
    let func = if is_fulfilled {
        async_generator_return_step_fulfill as *const u8
    } else {
        async_generator_return_step_reject as *const u8
    };
    let wrapper = js_closure_alloc(func, 3);
    js_closure_set_capture_f64(wrapper, 0, state_id as f64);
    original.with_const_ptr::<ClosureHeader, _>(|original| {
        js_closure_set_capture_ptr(wrapper, 1, original as i64)
    });
    out.with_mut_ptr::<Promise, _>(|out| js_closure_set_capture_ptr(wrapper, 2, out as i64));
    wrapper
}

extern "C" fn async_generator_return_step_fulfill(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    if let Some(state_id) = state_id_from_wrapper(closure) {
        let original = js_closure_get_capture_ptr(closure, 1) as *const ClosureHeader;
        let out = js_closure_get_capture_ptr(closure, 2) as *mut Promise;
        // Resume the generator's close path with the unwrapped value: runs any
        // pending finallys and yields `{ value, done: true }` (or a
        // finally-overridden completion).
        let result = call_original(original, value);
        after_queued_result(state_id, out, result);
    }
    value
}

extern "C" fn async_generator_return_step_reject(
    closure: *const ClosureHeader,
    reason: f64,
) -> f64 {
    if let Some(state_id) = state_id_from_wrapper(closure) {
        let out = js_closure_get_capture_ptr(closure, 2) as *mut Promise;
        // Await(v) threw: reject this request and resume the queue without
        // touching the generator body (spec `AsyncGeneratorReject`).
        finish_after_immediate_queued_result(state_id, out, false, reason);
    }
    reason
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
        // The async generator is the consumer of this step promise — its
        // rejection (now or later) is observed here, so it is not an unhandled
        // rejection even though the already-settled paths read `reason`
        // directly rather than attaching a reaction.
        crate::promise::mark_rejection_handled(promise);
        let state = unsafe { (*promise).state };
        if state == PromiseState::Pending {
            attach_pending_settle(state_id, promise, std::ptr::null_mut());
            return;
        }
        if state == PromiseState::Fulfilled {
            note_step_settlement(state_id, true, unsafe { (*promise).value });
        } else if state == PromiseState::Rejected {
            note_step_settlement(state_id, false, unsafe { (*promise).reason });
        }
    }
    schedule_drain(state_id);
}

fn after_queued_result(state_id: usize, out: *mut Promise, result: f64) {
    if let Some(promise) = promise_ptr(result) {
        crate::promise::mark_rejection_handled(promise);
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
    // A queued `.return(v)` awaits `v` before resuming the close path, same as
    // the head-of-line case in `async_generator_request`.
    if request.kind == RequestKind::Return {
        dispatch_return_with_await(state_id, original, request.arg, request.promise);
        return;
    }
    let result = call_original(original, request.arg);
    after_queued_result(state_id, request.promise, result);
}

fn finish_after_pending_result(state_id: usize, out: *mut Promise, fulfilled: bool, value: f64) {
    // #6709: settle THIS request's promise BEFORE draining the queue, so its
    // reactions are enqueued ahead of the next request's. Spec order is
    // AsyncGeneratorResolve (settle the front request's promise) *then*
    // AsyncGeneratorDrainQueue (resume the next). Draining first let a
    // synchronously-completing next request — e.g. the terminal
    // `{value: undefined, done: true}` after the last `yield` — resolve and
    // fire its `.then` before this (non-terminal) one, reordering the results
    // of three eagerly-queued `iter.next()` calls. This only surfaced once
    // async-generator `.next()` began returning a *pending* Promise for every
    // `yield` (the spec `AsyncGeneratorYield(? Await(value))` tick) so this
    // pending path is now always taken; before #6709 `.next()` resolved
    // synchronously (busy-wait) and hit the immediate path, which already
    // deferred the drain via `schedule_drain`.
    note_step_settlement(state_id, fulfilled, value);
    settle_out(out, fulfilled, value);
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
}

fn finish_after_immediate_queued_result(
    state_id: usize,
    out: *mut Promise,
    fulfilled: bool,
    value: f64,
) {
    note_step_settlement(state_id, fulfilled, value);
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

fn note_step_settlement(state_id: usize, fulfilled: bool, value: f64) {
    let done = if fulfilled {
        let field = js_object_get_own_field_or_undef(value, b"done".as_ptr(), 4);
        let jv = JSValue::from_bits(field.to_bits());
        (!jv.is_undefined()).then(|| crate::value::js_is_truthy(field) != 0)
    } else {
        None
    };
    STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(state_id - 1) else {
            return;
        };
        if !fulfilled {
            state.completed = true;
        } else if let Some(true) = done {
            state.completed = true;
        } else if let Some(false) = done {
            state.started = true;
        }
    });
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
