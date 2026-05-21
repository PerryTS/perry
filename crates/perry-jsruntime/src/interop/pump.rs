//! Event-loop pump driving V8 progress from Perry's outer event loop.
//!
//! Includes the foreign-promise adapter machinery that bridges V8
//! `Promise` handles into Perry's native `Promise` queue, plus
//! `jsruntime_process_pending` / `jsruntime_has_active_handles` —
//! the two callbacks `js_runtime_init` registers with `perry-runtime`.

use super::*;

use deno_core::v8;
use std::cell::RefCell;
use std::task::{Context as TaskContext, Poll, RawWaker, RawWakerVTable, Waker};

pub(crate) const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(crate) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

pub(crate) struct ForeignPromiseAdapter {
    pub(crate) handle_id: u64,
    pub(crate) native_promise: *mut perry_runtime::promise::Promise,
}

thread_local! {
    pub(crate) static FOREIGN_PROMISE_ADAPTERS: RefCell<Vec<ForeignPromiseAdapter>> = const { RefCell::new(Vec::new()) };
    pub(crate) static PENDING_JSRUNTIME_TICKS: RefCell<Vec<v8::Global<v8::PromiseResolver>>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C" {
    pub(crate) fn js_register_jsruntime_pump(f: extern "C" fn() -> i32);
    pub(crate) fn js_register_jsruntime_has_active(f: extern "C" fn() -> i32);
}

pub(crate) fn boxed_native_promise(promise: *mut perry_runtime::promise::Promise) -> f64 {
    f64::from_bits(POINTER_TAG | (promise as u64 & POINTER_MASK))
}

pub(crate) fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

unsafe fn notifying_waker_clone(_: *const ()) -> RawWaker {
    notifying_raw_waker()
}

unsafe fn notifying_waker_wake(_: *const ()) {
    perry_runtime::event_pump::js_notify_main_thread();
}

unsafe fn notifying_waker_wake_by_ref(_: *const ()) {
    perry_runtime::event_pump::js_notify_main_thread();
}

unsafe fn notifying_waker_drop(_: *const ()) {}

static NOTIFYING_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    notifying_waker_clone,
    notifying_waker_wake,
    notifying_waker_wake_by_ref,
    notifying_waker_drop,
);

pub(crate) fn notifying_raw_waker() -> RawWaker {
    RawWaker::new(std::ptr::null(), &NOTIFYING_WAKER_VTABLE)
}

pub(crate) fn notifying_waker() -> Waker {
    unsafe { Waker::from_raw(notifying_raw_waker()) }
}

enum AdapterSettlement {
    Resolve(u64, *mut perry_runtime::promise::Promise, f64),
    Reject(u64, *mut perry_runtime::promise::Promise, f64),
}

fn collect_foreign_promise_settlements(state: &mut JsRuntimeState) -> Vec<AdapterSettlement> {
    deno_core::scope!(scope, &mut state.runtime);
    FOREIGN_PROMISE_ADAPTERS.with(|adapters| {
        let mut adapters = adapters.borrow_mut();
        let mut settlements = Vec::new();
        let mut i = 0;
        while i < adapters.len() {
            let adapter = &adapters[i];
            let settlement = match get_js_handle(scope, adapter.handle_id) {
                Some(v8_val) if v8_val.is_promise() => {
                    let promise = v8::Local::<v8::Promise>::try_from(v8_val).unwrap();
                    match promise.state() {
                        v8::PromiseState::Fulfilled => {
                            let result = promise.result(scope);
                            Some(AdapterSettlement::Resolve(
                                adapter.handle_id,
                                adapter.native_promise,
                                v8_to_native(scope, result),
                            ))
                        }
                        v8::PromiseState::Rejected => {
                            let reason = promise.result(scope);
                            Some(AdapterSettlement::Reject(
                                adapter.handle_id,
                                adapter.native_promise,
                                v8_to_native(scope, reason),
                            ))
                        }
                        v8::PromiseState::Pending => None,
                    }
                }
                Some(v8_val) => Some(AdapterSettlement::Resolve(
                    adapter.handle_id,
                    adapter.native_promise,
                    v8_to_native(scope, v8_val),
                )),
                None => Some(AdapterSettlement::Reject(
                    adapter.handle_id,
                    adapter.native_promise,
                    undefined_value(),
                )),
            };

            if let Some(settlement) = settlement {
                settlements.push(settlement);
                adapters.remove(i);
            } else {
                i += 1;
            }
        }
        settlements
    })
}

fn settle_foreign_promise_adapters(state: &mut JsRuntimeState) -> i32 {
    let settlements = collect_foreign_promise_settlements(state);
    let count = settlements.len() as i32;
    for settlement in settlements {
        match settlement {
            AdapterSettlement::Resolve(handle_id, promise, value) => {
                if release_js_handle(handle_id) {
                    bump_jsruntime(&JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED);
                }
                bump_jsruntime(&JSRUNTIME_ADAPTERS_RESOLVED);
                perry_runtime::promise::js_promise_resolve(promise, value);
            }
            AdapterSettlement::Reject(handle_id, promise, reason) => {
                if release_js_handle(handle_id) {
                    bump_jsruntime(&JSRUNTIME_FOREIGN_PROMISE_HANDLES_RELEASED);
                }
                bump_jsruntime(&JSRUNTIME_ADAPTERS_REJECTED);
                perry_runtime::promise::js_promise_reject(promise, reason);
            }
        }
    }
    count
}

fn poll_v8_event_loop_once(state: &mut JsRuntimeState) -> i32 {
    let waker = notifying_waker();
    let mut cx = TaskContext::from_waker(&waker);
    match state.runtime.poll_event_loop(&mut cx, Default::default()) {
        Poll::Ready(Ok(())) => {
            // V8 event loop drained — clear the pending flag so the outer
            // loop can exit (assuming no other source — timers, stdlib,
            // http servers — still keeps it alive).
            state.last_poll_was_pending = false;
            0
        }
        Poll::Ready(Err(e)) => {
            state.last_poll_was_pending = false;
            eprintln!("[jsruntime_pump] event loop error: {}", e);
            1
        }
        Poll::Pending => {
            // Refed async op / dyn import / microtask / promise event
            // outstanding. `jsruntime_has_active_handles` reads this flag
            // to keep the outer event loop alive until the op resolves.
            state.last_poll_was_pending = true;
            0
        }
    }
}

fn resolve_pending_jsruntime_ticks(state: &mut JsRuntimeState) -> i32 {
    let resolvers =
        PENDING_JSRUNTIME_TICKS.with(|ticks| ticks.borrow_mut().drain(..).collect::<Vec<_>>());
    if resolvers.is_empty() {
        return 0;
    }

    let count = resolvers.len() as i32;
    deno_core::scope!(scope, &mut state.runtime);
    let undefined = v8::undefined(scope).into();
    for resolver in resolvers {
        let resolver = v8::Local::new(scope, resolver);
        let _ = resolver.resolve(scope, undefined);
    }
    scope.perform_microtask_checkpoint();
    count
}

pub(crate) fn poll_pending_module_evaluations(state: &mut JsRuntimeState) -> i32 {
    let waker = notifying_waker();
    let mut cx = TaskContext::from_waker(&waker);
    let mut completed = Vec::new();

    for (module_id, pending) in state.pending_module_evaluations.iter_mut() {
        match pending.future.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(())) => {
                completed.push((
                    *module_id,
                    pending.canonical_path.display().to_string(),
                    None,
                ));
            }
            Poll::Ready(Err(e)) => {
                completed.push((
                    *module_id,
                    pending.canonical_path.display().to_string(),
                    Some(e.to_string()),
                ));
            }
            Poll::Pending => {}
        }
    }

    let count = completed.len() as i32;
    let mut any_resolved = false;
    for (module_id, path, error) in completed {
        state.pending_module_evaluations.remove(&module_id);
        match error {
            Some(error) => {
                bump_jsruntime(&JSRUNTIME_MODULE_EVALS_REJECTED);
                eprintln!(
                    "[jsruntime_pump] module evaluation error for '{}': {}",
                    path, error
                );
            }
            None => {
                bump_jsruntime(&JSRUNTIME_MODULE_EVALS_RESOLVED);
                any_resolved = true;
            }
        }
    }
    // After any module evaluates, re-assert the Reflect-metadata bridge.
    // The npm `reflect-metadata` package (loaded transitively by NestJS,
    // class-validator, TypeORM, etc.) installs its own `Reflect.defineMetadata`
    // / `getMetadata` etc., which overwrites our wrappers. The bridge JS is
    // idempotent (no-op when our wrapper is already on the descriptor) so
    // running it every time a module finishes evaluating is cheap, and it
    // re-wraps any replacement functions so writes mirror to Perry's
    // `REFLECT_METADATA` store. (#1021 NestJS decorator routing.)
    if any_resolved {
        install_reflect_metadata_bridge(state);
    }
    count
}

pub(crate) extern "C" fn jsruntime_process_pending() -> i32 {
    jsruntime_profile_register();
    bump_jsruntime(&JSRUNTIME_PUMP_TICKS);
    // Enter the shared tokio runtime so async ops (e.g. the V8-fallback
    // `op_perry_http_*` listener) that touch `tokio::net` / `tokio::spawn`
    // can run inside a reactor context. Without this guard, polling an
    // async op that does `TcpListener::bind(...)` panics with "there is
    // no reactor running".
    let tokio_rt = crate::get_tokio_runtime();
    let _enter = tokio_rt.enter();
    with_runtime(|state| {
        let mut ran = poll_v8_event_loop_once(state);
        let resolved_ticks = resolve_pending_jsruntime_ticks(state);
        ran += resolved_ticks;
        if resolved_ticks > 0 {
            ran += poll_v8_event_loop_once(state);
        }
        ran += poll_pending_module_evaluations(state);
        ran += settle_foreign_promise_adapters(state);
        ran
    })
}

pub(crate) extern "C" fn jsruntime_has_active_handles() -> i32 {
    let has_foreign_adapters =
        FOREIGN_PROMISE_ADAPTERS.with(|adapters| !adapters.borrow().is_empty());
    let has_pending_ticks = PENDING_JSRUNTIME_TICKS.with(|ticks| !ticks.borrow().is_empty());
    let has_module_evaluations = JS_RUNTIME.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|state| !state.pending_module_evaluations.is_empty())
    });
    // `last_poll_was_pending` is set by `poll_v8_event_loop_once` whenever
    // deno_core returns `Poll::Pending` — i.e. a refed async op / dyn
    // import / microtask / promise event is still in flight. Without this
    // gate, a top-level `await op_perry_http_listen(port)` (or any other
    // async op invoked from module init) returns to the codegen-emitted
    // outer event loop while its body is still suspended on a tokio
    // worker; the header check then sees no other active source and
    // exits before the op resolves and the listening callback can fire.
    // Pairs with the express smoke at #997.
    let has_pending_v8 = JS_RUNTIME.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|state| state.last_poll_was_pending)
    });
    // Keep the program alive while any V8-fallback `http.createServer`
    // is still listening — without this the outer event loop exits
    // immediately after `server.listen(...)` resolves and the accept
    // loop's tokio task is dropped before serving any requests.
    let has_http_servers = crate::ops::perry_http_active_count() > 0;
    if has_foreign_adapters
        || has_pending_ticks
        || has_module_evaluations
        || has_http_servers
        || has_pending_v8
    {
        1
    } else {
        0
    }
}
