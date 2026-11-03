//! Promise-watch termination: one rejected `next()`, then a finished iterator.

use super::{
    js_closure_get_capture_f64, reject_promise, remove_abort_listener, resolve_promise_done,
    undefined_value, ClosureHeader, PROMISE_WATCHERS,
};

pub(super) fn abort_error_for_signal(signal: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let signal = scope.root_nanbox_f64(signal);
    let cause =
        crate::event_target_hook_slots::abort_signal_ptr_from_value(signal.get_nanbox_f64())
            .map(|signal| {
                crate::event_target::state::get_slot(
                    signal,
                    crate::event_target::state::SIGNAL_REASON,
                )
            })
            .unwrap_or_else(undefined_value);
    let cause = scope.root_nanbox_f64(cause);
    let error = scope.root_nanbox_f64(crate::event_target_hook_slots::js_abort_error_value());
    crate::node_submodules::set_error_user_prop(
        (error.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize,
        "cause",
        cause.get_nanbox_f64(),
    );
    error.get_nanbox_f64()
}

pub(super) fn abort_promise_watcher(id: usize, reason: f64) -> Vec<*mut crate::promise::Promise> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let reason = scope.root_nanbox_f64(reason);
    let removed = PROMISE_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let state = watchers.get_mut(&id)?;
        let signal = scope.root_nanbox_f64(state.signal);
        let listener = scope.root_nanbox_f64(state.abort_listener);
        let pending: Vec<_> = state
            .pending
            .drain(..)
            .map(|promise| scope.root_raw_mut_ptr(promise))
            .collect();
        let backend = state.backend.take();
        state.active = false;
        state.signal = undefined_value();
        state.abort_listener = undefined_value();
        state.object_value = undefined_value();
        state.closed = true;
        // With no waiting consumer the next call must observe the error once.
        // Otherwise settlement below consumes it, like an async generator throw.
        state.abort_reason = pending.is_empty().then(|| reason.get_nanbox_f64());
        state.queue.clear();
        Some((signal, listener, pending, backend))
    });
    let Some((signal, listener, pending, backend)) = removed else {
        return Vec::new();
    };
    drop(backend);
    // Listener removal may allocate: do it outside the state-map borrow and
    // keep every drained promise rooted until its refreshed pointer is returned.
    remove_abort_listener(signal.get_nanbox_f64(), listener.get_nanbox_f64());
    pending
        .iter()
        .map(|promise| promise.get_raw_mut_ptr::<crate::promise::Promise>())
        .collect()
}

pub(super) fn settle_watch_error(pending: Vec<*mut crate::promise::Promise>, reason: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let reason = scope.root_nanbox_f64(reason);
    let pending: Vec<_> = pending
        .into_iter()
        .map(|promise| scope.root_raw_mut_ptr(promise))
        .collect();
    for (index, promise) in pending.iter().enumerate() {
        let promise = promise.get_raw_mut_ptr::<crate::promise::Promise>();
        if index == 0 {
            reject_promise(promise, reason.get_nanbox_f64());
        } else {
            resolve_promise_done(promise);
        }
    }
}

pub(super) extern "C" fn promise_watcher_abort_impl(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let signal = PROMISE_WATCHERS.with(|watchers| {
        watchers
            .borrow()
            .get(&id)
            .map(|state| state.signal)
            .unwrap_or_else(undefined_value)
    });
    let scope = crate::gc::RuntimeHandleScope::new();
    let reason = scope.root_nanbox_f64(abort_error_for_signal(signal));
    let pending = abort_promise_watcher(id, reason.get_nanbox_f64());
    settle_watch_error(pending, reason.get_nanbox_f64());
    undefined_value()
}
