//! GC root scanners for promise-related thread-locals and the
//! `Promise.withResolvers` constructor.

#[cfg(test)]
use super::async_step::LAST_ASYNC_STEP_THUNKS;
use super::*;
use std::any::Any;

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
                Task::Microtask {
                    callback,
                    context,
                    async_id,
                    resource,
                    ..
                } => {
                    crate::async_hooks::resource_state::scan_token(async_id, visitor);
                    visitor.visit_nanbox_f64_slot(resource);
                    visitor.visit_raw_const_ptr_slot(callback);
                    scan_snapshot_roots_mut(context, visitor);
                }
                Task::AsyncStep(cb, value, next, _, context, _, id, _) => {
                    crate::async_hooks::resource_state::scan_token(id, visitor);
                    visitor.visit_raw_const_ptr_slot(cb);
                    visitor.visit_raw_mut_ptr_slot(next);
                    visitor.visit_nanbox_f64_slot(value);
                    scan_snapshot_roots_mut(context, visitor);
                }
            }
        }
    });

    super::microtasks::CURRENT_MICROTASK_PROMISE.with(|c| {
        let mut promise = c.get();
        if visitor.visit_raw_mut_ptr_slot(&mut promise) {
            c.set(promise);
        }
    });
    super::microtasks::CURRENT_MICROTASK_CALLBACK.with(|c| {
        let mut callback = c.get();
        if visitor.visit_raw_const_ptr_slot(&mut callback) {
            c.set(callback);
        }
    });
    super::microtasks::CURRENT_MICROTASK_VALUE.with(|c| {
        visitor.visit_cell_f64_slot(c);
    });
    super::microtasks::CURRENT_MICROTASK_NEXT.with(|c| {
        let mut next = c.get();
        if visitor.visit_raw_mut_ptr_slot(&mut next) {
            c.set(next);
        }
    });

    INLINE_TRAP.with(|c| {
        let mut trap = c.get();
        let mut changed = visitor.visit_raw_mut_ptr_slot(&mut trap.trap_next);
        let mut current_step = trap.current_step;
        changed |= visitor.visit_usize_slot(&mut current_step);
        if changed {
            trap.current_step = current_step;
            c.set(trap);
        }
    });

    MICROTASK_PREV_CONTEXTS.with(|stack| {
        for context in stack.borrow_mut().iter_mut() {
            scan_snapshot_roots_mut(context, visitor);
        }
    });

    // Scan SCHEDULED_RESOLVES entries
    super::combinators::SCHEDULED_RESOLVES.with(|q| {
        let mut q = q.borrow_mut();
        for (promise_ptr, value) in q.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(promise_ptr);
            visitor.visit_nanbox_f64_slot(value);
        }
    });
}

const PROMISE_SCAN_TASK_QUEUE: u8 = 0;
const PROMISE_SCAN_CURRENT_MICROTASK_PROMISE: u8 = 1;
const PROMISE_SCAN_CURRENT_MICROTASK_CALLBACK: u8 = 2;
const PROMISE_SCAN_CURRENT_MICROTASK_VALUE: u8 = 3;
const PROMISE_SCAN_CURRENT_MICROTASK_NEXT: u8 = 4;
const PROMISE_SCAN_INLINE_TRAP: u8 = 5;
// #8193 removed PROMISE_SCAN_ASYNC_STEP_GUARD: the only slot it visited was
// `AsyncStepGuard::last_closure`, a write-only raw closure address that this
// pass rekeyed and nothing pruned. The field is gone, so the phase has nothing
// left to scan and the ordinals close up behind it.
const PROMISE_SCAN_PREV_CONTEXTS: u8 = 10;
const PROMISE_SCAN_SCHEDULED_RESOLVES: u8 = 11;
const PROMISE_SCAN_DONE: u8 = 12;

#[derive(Default)]
pub(crate) struct PromiseRootScanState {
    phase: u8,
    index: usize,
    slot: usize,
    context_entry: usize,
    context_store: usize,
}

impl PromiseRootScanState {
    fn advance_to(&mut self, phase: u8) {
        self.phase = phase;
        self.index = 0;
        self.slot = 0;
        self.context_entry = 0;
        self.context_store = 0;
    }

    fn finish_context_item(&mut self) {
        self.slot = 0;
        self.context_entry = 0;
        self.context_store = 0;
    }
}

pub(crate) fn new_promise_root_scan_state() -> Box<dyn Any> {
    Box::<PromiseRootScanState>::default()
}

pub(crate) fn scan_promise_roots_mut_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut dyn Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<PromiseRootScanState>()
        .expect("promise root scanner state type");
    while state.phase != PROMISE_SCAN_DONE {
        let done = match state.phase {
            PROMISE_SCAN_TASK_QUEUE => scan_task_queue_step(visitor, state, remaining),
            PROMISE_SCAN_CURRENT_MICROTASK_PROMISE => {
                scan_current_microtask_promise_step(visitor, remaining)
            }
            PROMISE_SCAN_CURRENT_MICROTASK_CALLBACK => {
                scan_current_microtask_callback_step(visitor, remaining)
            }
            PROMISE_SCAN_CURRENT_MICROTASK_VALUE => {
                scan_current_microtask_value_step(visitor, remaining)
            }
            PROMISE_SCAN_CURRENT_MICROTASK_NEXT => {
                scan_current_microtask_next_step(visitor, remaining)
            }
            PROMISE_SCAN_INLINE_TRAP => scan_inline_trap_step(visitor, state, remaining),
            PROMISE_SCAN_PREV_CONTEXTS => scan_prev_contexts_step(visitor, state, remaining),
            PROMISE_SCAN_SCHEDULED_RESOLVES => {
                scan_scheduled_resolves_step(visitor, state, remaining)
            }
            PROMISE_SCAN_DONE => true,
            _ => true,
        };
        if !done {
            return false;
        }
        state.advance_to(state.phase.saturating_add(1));
    }
    true
}

#[inline]
fn consume_root_work(remaining: &mut usize) -> bool {
    if *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    true
}

fn scan_task_queue_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    TASK_QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        while state.index < q.len() {
            let done = scan_task_step(
                q.get_mut(state.index).expect("task cursor in range"),
                visitor,
                state,
                remaining,
            );
            if !done {
                return false;
            }
            state.index += 1;
            state.finish_context_item();
        }
        true
    })
}

fn scan_task_step(
    task: &mut Task,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    // A task owns its async metadata token independently of its next Promise.
    // Visit before the bounded payload scan; the token's old holder cannot move.
    match task {
        Task::Microtask {
            async_id, resource, ..
        } => {
            crate::async_hooks::resource_state::scan_token(async_id, visitor);
            visitor.visit_nanbox_f64_slot(resource);
        }
        Task::AsyncStep(_, _, _, _, _, _, async_id, _) => {
            crate::async_hooks::resource_state::scan_token(async_id, visitor);
        }
        _ => {}
    }
    match task {
        Task::Promise(promise_ptr, value, _, context) => {
            scan_task_slot_promise(promise_ptr, value, context, visitor, state, remaining)
        }
        Task::PromiseAll(promise_state, value, _, context) => {
            scan_task_slot_promise_all(promise_state, value, context, visitor, state, remaining)
        }
        Task::Inline(cb, value, next, _, context)
        | Task::AsyncStep(cb, value, next, _, context, _, _, _) => {
            scan_task_slot_inline(cb, value, next, context, visitor, state, remaining)
        }
        Task::Microtask {
            callback, context, ..
        } => scan_task_slot_microtask(callback, context, visitor, state, remaining),
    }
}

fn scan_task_slot_promise(
    promise_ptr: &mut *mut Promise,
    value: &mut f64,
    context: &mut AsyncContextSnapshot,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    if state.slot == 0 {
        if !consume_root_work(remaining) {
            return false;
        }
        visitor.visit_raw_mut_ptr_slot(promise_ptr);
        state.slot = 1;
    }
    if state.slot == 1 {
        if !consume_root_work(remaining) {
            return false;
        }
        visitor.visit_nanbox_f64_slot(value);
        state.slot = 2;
    }
    crate::async_context::scan_snapshot_roots_mut_step(
        context,
        visitor,
        &mut state.context_entry,
        &mut state.context_store,
        remaining,
    )
}

fn scan_task_slot_promise_all(
    promise_state: &mut super::combinators::PromiseAllState,
    value: &mut f64,
    context: &mut AsyncContextSnapshot,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    while state.slot < 4 {
        if !consume_root_work(remaining) {
            return false;
        }
        match state.slot {
            0 => visitor.visit_raw_mut_ptr_slot(&mut promise_state.result_promise),
            1 => visitor.visit_raw_mut_ptr_slot(&mut promise_state.results_arr),
            2 => visitor.visit_raw_mut_ptr_slot(&mut promise_state.state_arr),
            3 => visitor.visit_nanbox_f64_slot(value),
            _ => false,
        };
        state.slot += 1;
    }
    crate::async_context::scan_snapshot_roots_mut_step(
        context,
        visitor,
        &mut state.context_entry,
        &mut state.context_store,
        remaining,
    )
}

fn scan_task_slot_inline(
    cb: &mut *const crate::closure::ClosureHeader,
    value: &mut f64,
    next: &mut *mut Promise,
    context: &mut AsyncContextSnapshot,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    while state.slot < 3 {
        if !consume_root_work(remaining) {
            return false;
        }
        match state.slot {
            0 => visitor.visit_raw_const_ptr_slot(cb),
            1 => visitor.visit_raw_mut_ptr_slot(next),
            2 => visitor.visit_nanbox_f64_slot(value),
            _ => false,
        };
        state.slot += 1;
    }
    crate::async_context::scan_snapshot_roots_mut_step(
        context,
        visitor,
        &mut state.context_entry,
        &mut state.context_store,
        remaining,
    )
}

fn scan_task_slot_microtask(
    callback: &mut *const crate::closure::ClosureHeader,
    context: &mut AsyncContextSnapshot,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    if state.slot == 0 {
        if !consume_root_work(remaining) {
            return false;
        }
        visitor.visit_raw_const_ptr_slot(callback);
        state.slot = 1;
    }
    crate::async_context::scan_snapshot_roots_mut_step(
        context,
        visitor,
        &mut state.context_entry,
        &mut state.context_store,
        remaining,
    )
}

fn scan_current_microtask_promise_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    remaining: &mut usize,
) -> bool {
    if !consume_root_work(remaining) {
        return false;
    }
    super::microtasks::CURRENT_MICROTASK_PROMISE.with(|c| {
        let mut promise = c.get();
        if visitor.visit_raw_mut_ptr_slot(&mut promise) {
            c.set(promise);
        }
    });
    true
}

fn scan_current_microtask_callback_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    remaining: &mut usize,
) -> bool {
    if !consume_root_work(remaining) {
        return false;
    }
    super::microtasks::CURRENT_MICROTASK_CALLBACK.with(|c| {
        let mut callback = c.get();
        if visitor.visit_raw_const_ptr_slot(&mut callback) {
            c.set(callback);
        }
    });
    true
}

fn scan_current_microtask_value_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    remaining: &mut usize,
) -> bool {
    if !consume_root_work(remaining) {
        return false;
    }
    super::microtasks::CURRENT_MICROTASK_VALUE.with(|c| {
        visitor.visit_cell_f64_slot(c);
    });
    true
}

fn scan_current_microtask_next_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    remaining: &mut usize,
) -> bool {
    if !consume_root_work(remaining) {
        return false;
    }
    super::microtasks::CURRENT_MICROTASK_NEXT.with(|c| {
        let mut next = c.get();
        if visitor.visit_raw_mut_ptr_slot(&mut next) {
            c.set(next);
        }
    });
    true
}

fn scan_inline_trap_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    INLINE_TRAP.with(|c| {
        let mut trap = c.get();
        let mut changed = false;
        if state.slot == 0 {
            if !consume_root_work(remaining) {
                return false;
            }
            changed |= visitor.visit_raw_mut_ptr_slot(&mut trap.trap_next);
            state.slot = 1;
        }
        if state.slot == 1 {
            if !consume_root_work(remaining) {
                if changed {
                    c.set(trap);
                }
                return false;
            }
            let mut current_step = trap.current_step;
            changed |= visitor.visit_usize_slot(&mut current_step);
            trap.current_step = current_step;
            state.slot = 2;
        }
        if changed {
            c.set(trap);
        }
        state.slot >= 2
    })
}

fn scan_prev_contexts_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    MICROTASK_PREV_CONTEXTS.with(|stack| {
        let mut stack = stack.borrow_mut();
        while state.index < stack.len() {
            if !crate::async_context::scan_snapshot_roots_mut_step(
                &mut stack[state.index],
                visitor,
                &mut state.context_entry,
                &mut state.context_store,
                remaining,
            ) {
                return false;
            }
            state.index += 1;
            state.context_entry = 0;
            state.context_store = 0;
        }
        true
    })
}

fn scan_scheduled_resolves_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut PromiseRootScanState,
    remaining: &mut usize,
) -> bool {
    super::combinators::SCHEDULED_RESOLVES.with(|q| {
        let mut q = q.borrow_mut();
        while state.index < q.len() {
            while state.slot < 2 {
                if !consume_root_work(remaining) {
                    return false;
                }
                let (promise_ptr, value) = &mut q[state.index];
                match state.slot {
                    0 => visitor.visit_raw_mut_ptr_slot(promise_ptr),
                    1 => visitor.visit_nanbox_f64_slot(value),
                    _ => false,
                };
                state.slot += 1;
            }
            state.index += 1;
            state.slot = 0;
        }
        true
    })
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct TestPromiseScannerSnapshot {
    pub task_promise_ptr: usize,
    pub task_value_bits: u64,
    pub task_context_store_bits: u64,
    pub current_microtask_promise_ptr: usize,
    pub current_microtask_callback_ptr: usize,
    pub current_microtask_value_bits: u64,
    pub current_microtask_next_ptr: usize,
    pub inline_trap_next_ptr: usize,
    pub inline_trap_step_ptr: usize,
    pub inline_callback_ptr: usize,
    pub inline_next_ptr: usize,
    pub inline_value_bits: u64,
    pub async_step_callback_ptr: usize,
    pub async_step_next_ptr: usize,
    pub async_step_value_bits: u64,
    pub previous_microtask_context_store_bits: u64,
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
        q.enqueue_promise_job(Task::Promise(promise_ptr, value, true, context.clone()));
        q.enqueue_promise_job(Task::Inline(
            callback_ptr,
            value,
            next_ptr,
            true,
            context.clone(),
        ));
        q.enqueue_promise_job(Task::AsyncStep(
            callback_ptr,
            value,
            next_ptr,
            false,
            context.clone(),
            std::ptr::null_mut(),
            0,
            0,
        ));
    });
    MICROTASK_PREV_CONTEXTS.with(|stack| {
        let mut stack = stack.borrow_mut();
        stack.clear();
        stack.push(context.clone());
    });
    super::microtasks::CURRENT_MICROTASK_PROMISE.with(|c| c.set(promise_ptr));
    super::microtasks::CURRENT_MICROTASK_CALLBACK.with(|c| c.set(callback_ptr));
    super::microtasks::CURRENT_MICROTASK_VALUE.with(|c| c.set(value));
    super::microtasks::CURRENT_MICROTASK_NEXT.with(|c| c.set(next_ptr));
    INLINE_TRAP.with(|c| {
        c.set(InlineTrap {
            trap_next: next_ptr,
            current_step: callback_ptr as usize,
            box_activation: std::ptr::null_mut(),
        })
    });
    super::combinators::SCHEDULED_RESOLVES.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        q.push((promise_ptr, value));
    });
}

#[cfg(test)]
pub(crate) fn test_seed_many_promise_task_roots(values: &[f64]) {
    TASK_QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        for &value in values {
            q.enqueue_promise_job(Task::Promise(
                std::ptr::null_mut(),
                value,
                true,
                AsyncContextSnapshot::default(),
            ));
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_promise_context(promise_ptr: *mut Promise, context_store: f64) {
    set_promise_context_snapshot(
        promise_ptr,
        crate::async_context::test_snapshot_with_store(context_store),
    );
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
        if let Some(Task::AsyncStep(callback_ptr, value, next_ptr, _, _, _, _, _)) = q.get(2) {
            snapshot.async_step_callback_ptr = *callback_ptr as usize;
            snapshot.async_step_next_ptr = *next_ptr as usize;
            snapshot.async_step_value_bits = value.to_bits();
        }
    });
    super::microtasks::CURRENT_MICROTASK_PROMISE.with(|c| {
        snapshot.current_microtask_promise_ptr = c.get() as usize;
    });
    super::microtasks::CURRENT_MICROTASK_CALLBACK.with(|c| {
        snapshot.current_microtask_callback_ptr = c.get() as usize;
    });
    super::microtasks::CURRENT_MICROTASK_VALUE.with(|c| {
        snapshot.current_microtask_value_bits = c.get().to_bits();
    });
    super::microtasks::CURRENT_MICROTASK_NEXT.with(|c| {
        snapshot.current_microtask_next_ptr = c.get() as usize;
    });
    INLINE_TRAP.with(|c| {
        let trap = c.get();
        snapshot.inline_trap_next_ptr = trap.trap_next as usize;
        snapshot.inline_trap_step_ptr = trap.current_step;
    });
    MICROTASK_PREV_CONTEXTS.with(|stack| {
        snapshot.previous_microtask_context_store_bits = stack
            .borrow()
            .first()
            .and_then(crate::async_context::test_snapshot_first_store)
            .map(f64::to_bits)
            .unwrap_or(0);
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
    MICROTASK_PREV_CONTEXTS.with(|stack| stack.borrow_mut().clear());
    super::microtasks::CURRENT_MICROTASK_PROMISE.with(|c| c.set(std::ptr::null_mut()));
    super::microtasks::CURRENT_MICROTASK_CALLBACK.with(|c| c.set(std::ptr::null()));
    super::microtasks::CURRENT_MICROTASK_VALUE.with(|c| c.set(0.0));
    super::microtasks::CURRENT_MICROTASK_NEXT.with(|c| c.set(std::ptr::null_mut()));
    INLINE_TRAP.with(|c| c.set(InlineTrap::empty()));
    super::combinators::SCHEDULED_RESOLVES.with(|q| q.borrow_mut().clear());
}

/// Test support for the GC death-cleanup tests (gc/tests): park one entry
/// keyed by `promise` in each of the three leak-audited side tables —
/// settle listeners, overflow reactions, Promise.all states.
#[cfg(test)]
pub(crate) fn test_park_promise_side_table_entries(promise: *mut Promise) {
    super::reactions::park_settle_listener(
        promise,
        std::ptr::null(),
        std::ptr::null(),
        capture_context(),
    );
    super::reactions::push_overflow_reaction(
        promise,
        std::ptr::null(),
        std::ptr::null(),
        std::ptr::null_mut(),
        capture_context(),
    );
    unsafe {
        super::combinators::park_all_state(
            promise,
            super::combinators::PromiseAllState {
                result_promise: std::ptr::null_mut(),
                results_arr: std::ptr::null_mut(),
                state_arr: std::ptr::null_mut(),
                index: 0,
            },
        );
    }
}

/// Test support: per-table entry counts keyed by `key` (a promise address) —
/// (settle listeners, overflow reactions, Promise.all states).
#[cfg(test)]
pub(crate) fn test_promise_side_table_counts_for(key: usize) -> (usize, usize, usize) {
    unsafe {
        (
            owned_state::record_count(key as *mut Promise, owned_state::LISTENERS, 3),
            owned_state::record_count(key as *mut Promise, owned_state::OVERFLOW, 4),
            owned_state::record_count(key as *mut Promise, owned_state::ALL_STATES, 4),
        )
    }
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

#[cfg(test)]
pub(crate) fn test_current_microtask_value() -> f64 {
    super::microtasks::CURRENT_MICROTASK_VALUE.with(|c| c.get())
}

/// Promise.withResolvers<T>() — returns an object with { promise, resolve, reject }.
/// The resolve/reject are closures that settle the promise when called.
#[no_mangle]
pub extern "C" fn js_promise_with_resolvers() -> *mut crate::object::ObjectHeader {
    use crate::closure::js_closure_alloc;
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_F473_0);
    obj.set(0, crate::JSValue::pointer(js_promise_new().cast()));
    let resolve = js_closure_alloc(crate::fn_info!(with_resolvers_resolve_handler, 1), 1);
    crate::closure::js_closure_set_capture_f64(resolve, 0, f64::from_bits(obj.get(0).bits()));
    obj.set(1, crate::JSValue::pointer(resolve.cast()));
    let reject = js_closure_alloc(crate::fn_info!(with_resolvers_reject_handler, 1), 1);
    crate::closure::js_closure_set_capture_f64(reject, 0, f64::from_bits(obj.get(0).bits()));
    obj.set(2, crate::JSValue::pointer(reject.cast()));
    obj.finish()
}

#[cfg(test)]
unsafe fn store_with_resolvers_result_fields(
    obj: *mut crate::object::ObjectHeader,
    promise_box: f64,
    resolve_box: f64,
    reject_box: f64,
) {
    crate::object::store_object_field_slot(obj, 0, promise_box.to_bits());
    crate::object::store_object_field_slot(obj, 1, resolve_box.to_bits());
    crate::object::store_object_field_slot(obj, 2, reject_box.to_bits());
}

#[cfg(test)]
pub(crate) unsafe fn test_store_with_resolvers_result_fields(
    obj: *mut crate::object::ObjectHeader,
    promise_box: f64,
    resolve_box: f64,
    reject_box: f64,
) {
    store_with_resolvers_result_fields(obj, promise_box, resolve_box, reject_box);
}

extern "C" fn with_resolvers_resolve_handler(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let promise_box = crate::closure::js_closure_get_capture_f64(closure, 0);
    let promise_ptr = (f64::to_bits(promise_box) & crate::value::POINTER_MASK) as *mut Promise;
    js_promise_resolve(promise_ptr, value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn with_resolvers_reject_handler(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let promise_box = crate::closure::js_closure_get_capture_f64(closure, 0);
    let promise_ptr = (f64::to_bits(promise_box) & crate::value::POINTER_MASK) as *mut Promise;
    js_promise_reject(promise_ptr, value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
