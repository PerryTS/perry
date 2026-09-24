//! The container bindings' executor: one compose-engine future per operation,
//! driven by `perry_container_compose::rt::block_on` on a turnloop worker.
//!
//! Until tokio lane K this module's callers used `crate::common::
//! spawn_for_promise[_deferred]`, which put the future on the tokio
//! current-thread runtime and made `container` imply `async-runtime`. The
//! compose engine no longer uses tokio: its leaves (the backend CLI child
//! processes, timeouts, the async mutex) run on whatever
//! `rt::block_on` loop polls them. So each operation takes a thread from
//! turnloop's `Occupancy::Long` worker set (the same class
//! `perry_ffi_spawn_blocking` uses — a container operation holds its thread
//! for as long as a `docker pull` takes, and must not starve the bounded set
//! bcrypt / zlib run on), creates its own loop there, and settles the promise
//! through the tokio-free `async_bridge` queue — exactly the settle path the
//! tokio arm used. A thread with no event loop, or a refusal at the long
//! set's ceiling, falls back to one plain OS thread.
//!
//! Only the promise bridge (`async-bridge`) is required, so a `container`
//! build links no tokio.

use std::future::Future;

use crate::common::async_bridge::{
    blocking_thread_stack_size, ensure_gc_scanner_registered, ensure_pump_registered,
    pin_promise_for_native_resolution, queue_deferred_resolution, queue_promise_resolution,
    InflightGuard,
};

/// Reject `ptr` with `message`, building the string on the main thread.
fn queue_rejection(ptr: usize, message: String) {
    queue_deferred_resolution(ptr, false, move || {
        let str_ptr = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
        // STRING_TAG, not POINTER_TAG, for proper type identification.
        perry_runtime::JSValue::string_ptr(str_ptr).bits()
    });
}

/// Run `future` to completion off the calling thread; `settle` receives its
/// output, or `Err` when no loop could be created for it. With `keep_alive`
/// the program's event loop stays alive until `settle` has run.
fn run_detached<T, F>(
    future: F,
    keep_alive: bool,
    settle: impl FnOnce(Result<T, String>) + Send + 'static,
) where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
{
    ensure_pump_registered();
    // Held for exactly the operation's run, and released even when the job is
    // dropped unrun, so the event loop stays alive until the result is queued.
    let inflight = keep_alive.then(InflightGuard::new);
    let run = move || {
        let result = perry_container_compose::rt::try_block_on(future)
            .unwrap_or_else(|e| Err(format!("container runtime unavailable: {e}")));
        settle(result);
        drop(inflight);
    };
    // `submit_long` consumes `run` only when it accepts the job; a refusal
    // hands it back through the slot for the thread fallback.
    let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(run)));
    let pool_slot = slot.clone();
    let accepted = perry_runtime::turnloop_pool::submit_long(
        move || {
            if let Some(run) = take(&pool_slot) {
                run();
            }
        },
        |_delivery| {},
    )
    .is_ok();
    if accepted {
        return;
    }
    let thread_slot = slot.clone();
    let spawned = std::thread::Builder::new()
        .name("perry-container".to_string())
        .stack_size(blocking_thread_stack_size())
        .spawn(move || {
            if let Some(run) = take(&thread_slot) {
                run();
            }
        });
    if spawned.is_err() {
        // No thread at all: run inline rather than leave a promise pending.
        if let Some(run) = take(&slot) {
            run();
        }
    }
}

fn take<R>(slot: &std::sync::Mutex<Option<R>>) -> Option<R> {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// Settle `promise_ptr` with `future`'s bits, or reject with its error
/// string. The bits must not be a heap value — use
/// [`spawn_for_promise_deferred`] for strings, arrays and objects.
///
/// # Safety
/// `promise_ptr` must point to a live Perry Promise.
pub(crate) unsafe fn spawn_for_promise<F>(promise_ptr: *mut u8, future: F)
where
    F: Future<Output = Result<u64, String>> + Send + 'static,
{
    ensure_gc_scanner_registered();
    let ptr = promise_ptr as usize;
    // Issue #859: pin before the promise crosses to another thread.
    pin_promise_for_native_resolution(ptr);
    run_detached(future, true, move |result| match result {
        Ok(bits) => queue_promise_resolution(ptr, true, bits),
        Err(message) => queue_rejection(ptr, message),
    });
}

/// Settle `promise_ptr` with `converter(output)`, run on the main thread so it
/// may allocate JS values, or reject with the future's error string.
///
/// # Safety
/// `promise_ptr` must point to a live Perry Promise.
pub(crate) unsafe fn spawn_for_promise_deferred<T, F, C>(
    promise_ptr: *mut u8,
    future: F,
    converter: C,
) where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
    C: FnOnce(T) -> u64 + Send + 'static,
{
    ensure_gc_scanner_registered();
    let ptr = promise_ptr as usize;
    pin_promise_for_native_resolution(ptr);
    run_detached(future, true, move |result| match result {
        Ok(data) => queue_deferred_resolution(ptr, true, move || converter(data)),
        Err(message) => queue_rejection(ptr, message),
    });
}

/// Run `future` in the background with no promise attached (the backend
/// pre-warm and the signal-cleanup watcher). Like the raw tokio `spawn` it
/// replaces, it does not keep the program alive: the watcher never finishes.
pub(crate) fn spawn_detached<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    run_detached(
        async move {
            future.await;
            Ok(())
        },
        false,
        |_: Result<(), String>| {},
    );
}
