//! GC root scanners for promise-related thread-locals and the
//! `Promise.withResolvers` constructor.

use super::async_step::LAST_ASYNC_STEP_THUNKS;
use super::*;

pub fn scan_promise_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_promise_roots_mut(&mut visitor);
}

pub fn scan_promise_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // Scan TASK_QUEUE entries
    TASK_QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        for entry in q.iter_mut() {
            match entry {
                Task::Promise(promise_ptr, value, _, context) => {
                    visitor.visit_raw_mut_ptr_slot(promise_ptr);
                    visitor.visit_nanbox_f64_slot(value);
                    scan_snapshot_roots_mut(context, visitor);
                }
                Task::PromiseAll(state, value, _, context) => {
                    visitor.visit_raw_mut_ptr_slot(&mut state.result_promise);
                    visitor.visit_raw_mut_ptr_slot(&mut state.results_arr);
                    visitor.visit_raw_mut_ptr_slot(&mut state.state_arr);
                    visitor.visit_nanbox_f64_slot(value);
                    scan_snapshot_roots_mut(context, visitor);
                }
                Task::Inline(cb, value, next, _, context) => {
                    visitor.visit_raw_const_ptr_slot(cb);
                    visitor.visit_raw_mut_ptr_slot(next);
                    visitor.visit_nanbox_f64_slot(value);
                    scan_snapshot_roots_mut(context, visitor);
                }
                Task::AsyncStep(cb, value, next, _, context) => {
                    visitor.visit_raw_const_ptr_slot(cb);
                    visitor.visit_raw_mut_ptr_slot(next);
                    visitor.visit_nanbox_f64_slot(value);
                    scan_snapshot_roots_mut(context, visitor);
                }
            }
        }
    });

    PROMISE_CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        let mut moved = Vec::new();
        for (&key, context) in contexts.iter_mut() {
            let mut new_key = key;
            if visitor.visit_metadata_usize_slot(&mut new_key) {
                moved.push((key, new_key));
            }
            scan_snapshot_roots_mut(context, visitor);
        }
        for (old_key, new_key) in moved {
            if let Some(context) = contexts.remove(&old_key) {
                contexts.insert(new_key, context);
            }
        }
    });

    super::combinators::scan_promise_all_states_mut(visitor);

    // Scan SCHEDULED_RESOLVES entries
    super::combinators::SCHEDULED_RESOLVES.with(|q| {
        let mut q = q.borrow_mut();
        for (promise_ptr, value) in q.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(promise_ptr);
            visitor.visit_nanbox_f64_slot(value);
        }
    });
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct TestPromiseScannerSnapshot {
    pub task_promise_ptr: usize,
    pub task_value_bits: u64,
    pub task_context_store_bits: u64,
    pub inline_callback_ptr: usize,
    pub inline_next_ptr: usize,
    pub inline_value_bits: u64,
    pub async_step_callback_ptr: usize,
    pub async_step_next_ptr: usize,
    pub async_step_value_bits: u64,
    pub promise_context_key: usize,
    pub promise_context_store_bits: u64,
    pub scheduled_promise_ptr: usize,
    pub scheduled_value_bits: u64,
}

#[cfg(test)]
pub(crate) fn test_seed_promise_scanner_roots(
    promise_ptr: *mut Promise,
    value: f64,
    context_store: f64,
    callback_ptr: *const crate::closure::ClosureHeader,
    next_ptr: *mut Promise,
) {
    let context = crate::async_context::test_snapshot_with_store(context_store);
    TASK_QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        q.push_back(Task::Promise(promise_ptr, value, true, context.clone()));
        q.push_back(Task::Inline(
            callback_ptr,
            value,
            next_ptr,
            true,
            context.clone(),
        ));
        q.push_back(Task::AsyncStep(
            callback_ptr,
            value,
            next_ptr,
            false,
            context.clone(),
        ));
    });
    PROMISE_CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        contexts.clear();
        contexts.insert(promise_ptr as usize, context.clone());
    });
    super::combinators::SCHEDULED_RESOLVES.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        q.push((promise_ptr, value));
    });
}

#[cfg(test)]
pub(crate) fn test_promise_scanner_snapshot() -> TestPromiseScannerSnapshot {
    let mut snapshot = TestPromiseScannerSnapshot::default();
    TASK_QUEUE.with(|q| {
        let q = q.borrow();
        if let Some(Task::Promise(promise_ptr, value, _, context)) = q.get(0) {
            snapshot.task_promise_ptr = *promise_ptr as usize;
            snapshot.task_value_bits = value.to_bits();
            snapshot.task_context_store_bits =
                crate::async_context::test_snapshot_first_store(context)
                    .map(f64::to_bits)
                    .unwrap_or(0);
        }
        if let Some(Task::Inline(callback_ptr, value, next_ptr, _, _)) = q.get(1) {
            snapshot.inline_callback_ptr = *callback_ptr as usize;
            snapshot.inline_next_ptr = *next_ptr as usize;
            snapshot.inline_value_bits = value.to_bits();
        }
        if let Some(Task::AsyncStep(callback_ptr, value, next_ptr, _, _)) = q.get(2) {
            snapshot.async_step_callback_ptr = *callback_ptr as usize;
            snapshot.async_step_next_ptr = *next_ptr as usize;
            snapshot.async_step_value_bits = value.to_bits();
        }
    });
    PROMISE_CONTEXTS.with(|contexts| {
        let contexts = contexts.borrow();
        if let Some((&key, context)) = contexts.iter().next() {
            snapshot.promise_context_key = key;
            snapshot.promise_context_store_bits =
                crate::async_context::test_snapshot_first_store(context)
                    .map(f64::to_bits)
                    .unwrap_or(0);
        }
    });
    super::combinators::SCHEDULED_RESOLVES.with(|q| {
        let q = q.borrow();
        if let Some((promise_ptr, value)) = q.first() {
            snapshot.scheduled_promise_ptr = *promise_ptr as usize;
            snapshot.scheduled_value_bits = value.to_bits();
        }
    });
    snapshot
}

#[cfg(test)]
pub(crate) fn test_clear_promise_scanner_roots() {
    TASK_QUEUE.with(|q| q.borrow_mut().clear());
    PROMISE_CONTEXTS.with(|contexts| contexts.borrow_mut().clear());
    super::combinators::SCHEDULED_RESOLVES.with(|q| q.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn test_seed_async_step_thunk_cache(
    key: usize,
    fulfill: *mut crate::closure::ClosureHeader,
    reject: *mut crate::closure::ClosureHeader,
) {
    LAST_ASYNC_STEP_THUNKS.with(|c| c.set((key, fulfill, reject)));
}

#[cfg(test)]
pub(crate) fn test_async_step_thunk_cache() -> (usize, usize, usize) {
    LAST_ASYNC_STEP_THUNKS.with(|c| {
        let (key, fulfill, reject) = c.get();
        (key, fulfill as usize, reject as usize)
    })
}

/// Promise.withResolvers<T>() — returns an object with { promise, resolve, reject }.
/// The resolve/reject are closures that settle the promise when called.
#[no_mangle]
pub extern "C" fn js_promise_with_resolvers() -> *mut crate::object::ObjectHeader {
    use crate::closure::js_closure_alloc;
    use crate::object::{js_object_alloc_with_shape, ObjectHeader};

    // Create the pending promise.
    let promise = js_promise_new();
    let promise_box = crate::value::js_nanbox_pointer(promise as i64);

    // Create resolve closure that resolves this promise.
    let resolve_fn = js_closure_alloc(
        with_resolvers_resolve_handler as *const u8,
        1, // 1 capture: the promise pointer
    );
    crate::closure::js_closure_set_capture_f64(resolve_fn, 0, promise_box);
    let resolve_box = crate::value::js_nanbox_pointer(resolve_fn as i64);

    // Create reject closure.
    let reject_fn = js_closure_alloc(with_resolvers_reject_handler as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(reject_fn, 0, promise_box);
    let reject_box = crate::value::js_nanbox_pointer(reject_fn as i64);

    // Build the { promise, resolve, reject } object.
    // Use a 3-field object with packed keys "promise\0resolve\0reject\0".
    let packed = b"promise\0resolve\0reject\0";
    let obj = js_object_alloc_with_shape(
        0xFFF0_0001, // unique shape id
        3,
        packed.as_ptr(),
        packed.len() as u32,
    );

    // Store the three fields.
    unsafe {
        let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut f64;
        *fields.add(0) = promise_box; // .promise
        *fields.add(1) = resolve_box; // .resolve
        *fields.add(2) = reject_box; // .reject
    }

    obj
}

extern "C" fn with_resolvers_resolve_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let promise_box = crate::closure::js_closure_get_capture_f64(closure, 0);
    let promise_ptr = (f64::to_bits(promise_box) & crate::value::POINTER_MASK) as *mut Promise;
    js_promise_resolve(promise_ptr, value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn with_resolvers_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let promise_box = crate::closure::js_closure_get_capture_f64(closure, 0);
    let promise_ptr = (f64::to_bits(promise_box) & crate::value::POINTER_MASK) as *mut Promise;
    js_promise_reject(promise_ptr, value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
