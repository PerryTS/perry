//! WASI bindings for the extension ABI normally supplied by perry-stdlib.
//! The runtime already owns the rooted native completion tokens and their
//! pump. WASI has one agent, so posted work runs on its next pump turn.

use perry_runtime::promise::{self, native_async::NativeAsyncCompletion, Promise};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;

type Job = Box<dyn FnOnce()>;
extern "C" {
    fn js_register_aux_pump(pump: extern "C" fn() -> i32);
    fn js_register_aux_has_active(active: extern "C" fn() -> i32);
    fn js_run_stdlib_pump();
}
thread_local! {
    static JOBS: RefCell<VecDeque<Job>> = RefCell::new(VecDeque::new());
    static DISPATCHED: Cell<u64> = const { Cell::new(0) };
}
fn enqueue(job: Job) {
    unsafe {
        js_register_aux_pump(pump);
        js_register_aux_has_active(has_active);
    }
    JOBS.with(|q| q.borrow_mut().push_back(job));
    perry_runtime::event_pump::js_notify_main_thread();
}
extern "C" fn has_active() -> i32 {
    JOBS.with(|q| i32::from(!q.borrow().is_empty()))
}
extern "C" fn pump() -> i32 {
    let jobs = JOBS.with(|q| std::mem::take(&mut *q.borrow_mut()));
    let count = jobs.len().min(i32::MAX as usize) as i32;
    for job in jobs {
        job();
        DISPATCHED.with(|n| n.set(n.get().wrapping_add(1)));
    }
    count
}

#[no_mangle]
pub extern "C" fn js_perry_agent_current() -> u64 {
    perry_runtime::agent::current_agent()
}
#[no_mangle]
pub extern "C" fn js_perry_agent_post_available() -> i32 {
    1
}
#[no_mangle]
pub extern "C" fn js_perry_agent_post_dispatched() -> u64 {
    DISPATCHED.with(Cell::get)
}
#[no_mangle]
pub extern "C" fn js_perry_agent_post(
    run: Option<extern "C" fn(*mut c_void)>,
    ctx: *mut c_void,
) -> i32 {
    let Some(run) = run else {
        return -2;
    };
    enqueue(Box::new(move || run(ctx)));
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_new(flags: u32) -> *mut NativeAsyncCompletion {
    promise::js_native_async_completion_new(flags)
}
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_promise(
    token: *mut NativeAsyncCompletion,
) -> *mut Promise {
    promise::js_native_async_completion_promise(token)
}
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_resolve_bits(
    token: *mut NativeAsyncCompletion,
    bits: u64,
) -> i32 {
    promise::js_native_async_completion_resolve_bits(token, bits)
}
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_bits(
    token: *mut NativeAsyncCompletion,
    bits: u64,
) -> i32 {
    promise::js_native_async_completion_reject_bits(token, bits)
}
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_string(
    token: *mut NativeAsyncCompletion,
    data: *const u8,
    len: usize,
) -> i32 {
    promise::js_native_async_completion_reject_string(token, data, len)
}
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_cancel(token: *mut NativeAsyncCompletion) -> i32 {
    promise::js_native_async_completion_cancel(token)
}
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_attach_handle(
    token: *mut NativeAsyncCompletion,
    bits: u64,
    flags: u32,
) -> i32 {
    promise::js_native_async_completion_attach_handle(token, bits, flags)
}
#[no_mangle]
pub extern "C" fn perry_ffi_promise_new() -> *mut Promise {
    perry_ffi_native_async_promise(perry_ffi_native_async_new(0))
}
#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_bits(promise: *mut Promise, bits: u64) {
    promise::js_native_async_completion_resolve_promise_bits(promise, bits);
}
#[no_mangle]
pub extern "C" fn perry_ffi_promise_reject_bits(promise: *mut Promise, bits: u64) {
    promise::js_native_async_completion_reject_promise_bits(promise, bits);
}
#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_deferred(
    promise: *mut Promise,
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void) -> u64,
) {
    enqueue(Box::new(move || {
        perry_ffi_promise_resolve_bits(promise, invoke(ctx))
    }));
}
#[no_mangle]
pub extern "C" fn perry_ffi_promise_reject_deferred(
    promise: *mut Promise,
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void) -> u64,
) {
    enqueue(Box::new(move || {
        perry_ffi_promise_reject_bits(promise, invoke(ctx))
    }));
}
#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking(ctx: *mut c_void, invoke: extern "C" fn(*mut c_void)) {
    enqueue(Box::new(move || invoke(ctx)));
}
#[no_mangle]
pub extern "C" fn perry_ffi_run_pending(_budget_ms: u64) {
    unsafe {
        js_run_stdlib_pump();
    }
}
