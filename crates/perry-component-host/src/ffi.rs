//! Allocator-safe callback transport. JS exceptions must be caught by the
//! runtime callback; every call returns through Wasmtime before JS can throw.
mod async_calls;
mod callbacks;
mod driver;
mod lifecycle;
use async_calls::*;
use callbacks::{ImportCallback, LifecycleCallback};
use driver::*;
mod http;
use http::*;

use crate::{Host, Import, Imports, Interrupt, Options, ResourceHook};
use anyhow::{bail, ensure, Context, Result};
use perry_abi::component_host::{
    Buffer, Callback, ComponentHostOps, FreeCallback, ResourceCallback, ABI_VERSION, ERROR, OK,
    RELEASED_ERROR,
};
use perry_wit::World;
use std::cell::{Cell, UnsafeCell};
use std::ffi::c_void;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

struct Bridge {
    host: UnsafeCell<Host>,
    busy: Cell<bool>,
    context: Arc<AtomicU64>,
    interrupt: Interrupt,
    pending: UnsafeCell<Option<async_calls::PendingCall>>,
    cleanup_context: Arc<AtomicU64>,
    async_replies: Arc<async_calls::Replies>,
    driver: Option<driver::Actor>,
}

impl Bridge {
    fn entered<R>(
        &self,
        context: u64,
        cleanup_context: u64,
        call: impl FnOnce() -> Result<R>,
    ) -> Result<R> {
        ensure!(
            !self.busy.replace(true),
            "component cannot be re-entered during a call"
        );
        struct Guard<'a>(&'a Bridge, u64, u64);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.context.store(self.1, Ordering::Release);
                self.0.cleanup_context.store(self.2, Ordering::Release);
                self.0.busy.set(false);
            }
        }
        let guard = Guard(
            self,
            self.context.swap(context, Ordering::AcqRel),
            self.cleanup_context.swap(cleanup_context, Ordering::AcqRel),
        );
        let result = call();
        drop(guard);
        result
    }
    fn with<R>(&self, context: u64, call: impl FnOnce(&mut Host) -> Result<R>) -> Result<R> {
        self.entered(context, context, || {
            ensure!(
                self.driver.is_none(),
                "operation requires a legacy component handle"
            );
            ensure!(
                unsafe { (&*self.pending.get()).is_none() },
                "component has a pending async call"
            );
            // The guard precedes every mutable borrow, including reentrant JS callbacks.
            call(unsafe { &mut *self.host.get() })
        })
    }
}

unsafe fn bytes<'a>(pointer: *const u8, len: usize) -> Result<&'a [u8]> {
    ensure!(len == 0 || !pointer.is_null(), "null component byte span");
    Ok(if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(pointer, len)
    })
}

fn output(out: *mut Buffer, value: Vec<u8>) {
    if out.is_null() {
        return;
    }
    let value = value.into_boxed_slice();
    unsafe {
        *out = Buffer {
            len: value.len(),
            data: Box::into_raw(value).cast(),
        };
    }
}

fn respond(out: *mut Buffer, result: Result<Vec<u8>>) -> i32 {
    match result {
        Ok(value) => {
            output(out, value);
            OK
        }
        Err(error) => {
            output(out, format!("{error:#}").into_bytes());
            ERROR
        }
    }
}

/// Release an engine-allocated reply buffer.
///
/// # Safety
/// A non-null buffer must be an unmodified allocation returned by this engine,
/// still owned by the caller and not previously freed. No references to its
/// bytes may be used after this call.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_buffer_drop(buffer: Buffer) {
    if !buffer.data.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            buffer.data,
            buffer.len,
        )));
    }
}

/// A command owns its store for this call only; no callback bridge is exposed.
/// # Safety
/// Nonempty input spans must be valid initialized bytes for this call. `out`
/// must be null or aligned, exclusively writable `Buffer` storage. Release any
/// returned buffer exactly once with `perry_component_host_buffer_drop`.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_run_command(
    wasm: *const u8,
    wasm_len: usize,
    options: *const u8,
    options_len: usize,
    out: *mut Buffer,
) -> i32 {
    respond(
        out,
        (|| {
            let options = if options_len == 0 {
                Options::default()
            } else {
                serde_json::from_slice(bytes(options, options_len)?)?
            };
            let status = crate::run_command(bytes(wasm, wasm_len)?, &options)?;
            Ok(serde_json::to_vec(&status)?)
        })(),
    )
}

struct Reply(Buffer, FreeCallback);
impl Drop for Reply {
    fn drop(&mut self) {
        unsafe { (self.1)(self.0) }
    }
}

/// Construct an owned component bridge.
///
/// # Safety
/// Nonempty input spans must be readable and initialized for this call. `error`
/// must be null or aligned, exclusively writable `Buffer` storage. Callbacks
/// and their context must remain valid for every embedding call until disposal;
/// they must honor the ABI, return valid buffers for `free_callback`, and never
/// unwind across the C boundary. Use the returned handle on its owning thread
/// (except interrupt) and release it exactly once with the matching drop entry.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_new(
    wasm: *const u8,
    wasm_len: usize,
    contract: *const u8,
    contract_len: usize,
    options: *const u8,
    options_len: usize,
    callback: Option<Callback>,
    free_callback: Option<FreeCallback>,
    resource_callback: Option<ResourceCallback>,
    context: u64,
    error: *mut Buffer,
) -> *mut c_void {
    new_bridge(
        wasm,
        wasm_len,
        contract,
        contract_len,
        options,
        options_len,
        callback.map(ImportCallback::Legacy),
        free_callback,
        resource_callback.map(LifecycleCallback::Legacy),
        context,
        false,
        error,
    )
}

#[allow(clippy::too_many_arguments)]
unsafe fn new_bridge(
    wasm: *const u8,
    wasm_len: usize,
    contract: *const u8,
    contract_len: usize,
    options: *const u8,
    options_len: usize,
    callback: Option<ImportCallback>,
    free_callback: Option<FreeCallback>,
    resource_callback: Option<LifecycleCallback>,
    context: u64,
    persistent: bool,
    error: *mut Buffer,
) -> *mut c_void {
    let result = (|| -> Result<Bridge> {
        let _scope = crate::asynchronous::origin::Scope::enter(None);
        let world: World = serde_json::from_slice(bytes(contract, contract_len)?)?;
        let options: Options = if options_len == 0 {
            Options::default()
        } else {
            serde_json::from_slice(bytes(options, options_len)?)?
        };
        let current = Arc::new(AtomicU64::new(context));
        let mut imports = Imports::new();
        let mut async_imports = crate::AsyncImports::new();
        let async_replies = Arc::new(async_calls::Replies::default());
        let cleanup_context = Arc::new(AtomicU64::new(context));
        for interface in &world.imports {
            if options.wasi.is_some() && interface.name.starts_with("wasi:") {
                continue;
            }
            for function in &interface.functions {
                let callback = callback.context("component imports require a callback")?;
                let free =
                    free_callback.context("component imports require a reply deallocator")?;
                let interface_name = interface.name.clone();
                let function_name = function.name.clone();
                let has_result = function.result.is_some();
                if function.async_ {
                    let handler = async_calls::import(
                        callback,
                        free,
                        current.clone(),
                        async_replies.clone(),
                        interface_name,
                        function_name,
                        has_result,
                    );
                    async_imports
                        .entry(interface.name.clone())
                        .or_default()
                        .insert(function.name.clone(), handler);
                    continue;
                }
                let current = current.clone();
                let failures = async_replies.clone();
                let handler: Import = Arc::new(move |args| {
                    let context = current.load(Ordering::Acquire);
                    ensure!(
                        context != 0,
                        "component import outside a live embedding call"
                    );
                    let args = serde_json::to_vec(args)?;
                    let mut reply = Reply(Buffer::default(), free);
                    let status = callback.call(
                        context,
                        &interface_name,
                        &function_name,
                        &args,
                        &mut reply.0,
                    );
                    failures.callback_status(status);
                    let result = bytes(reply.0.data, reply.0.len)?;
                    if status != OK {
                        bail!("component host import: {}", String::from_utf8_lossy(result));
                    }
                    let value = serde_json::from_slice(result)?;
                    Ok(if has_result { Some(value) } else { None })
                });
                imports
                    .entry(interface.name.clone())
                    .or_default()
                    .insert(function.name.clone(), handler);
            }
        }
        let (resource_hook, lowering_hook) = lifecycle::hooks(
            resource_callback,
            free_callback,
            current.clone(),
            async_replies.clone(),
        );
        let dispatcher = (persistent
            || world
                .imports
                .iter()
                .chain(&world.exports)
                .flat_map(|i| &i.functions)
                .any(|f| f.async_))
        .then(|| Arc::new(crate::asynchronous::dispatch::Dispatcher::default()));
        let mut host = if persistent {
            Host::new_for_driver(
                bytes(wasm, wasm_len)?,
                &world,
                &imports,
                &async_imports,
                &options,
                resource_hook,
            )?
        } else {
            Host::new_with_dispatcher(
                bytes(wasm, wasm_len)?,
                &world,
                &imports,
                &async_imports,
                &options,
                resource_hook,
                dispatcher,
            )?
        };
        if persistent {
            host.live.as_mut().unwrap().store.data_mut().lowering_hook = Some(lowering_hook);
        }
        current.store(0, Ordering::Release);
        let interrupt = host.interrupt_handle();
        let driver = if persistent {
            let owned = Host {
                engine: host.engine.clone(),
                live: host.live.take(),
                dispatcher: host.dispatcher.clone(),
            };
            Some(driver::Actor::new(
                owned,
                current.clone(),
                cleanup_context.clone(),
            )?)
        } else {
            None
        };
        Ok(Bridge {
            host: UnsafeCell::new(host),
            busy: Cell::new(false),
            context: current,
            interrupt,
            pending: UnsafeCell::new(None),
            cleanup_context,
            async_replies,
            driver,
        })
    })();
    match result {
        Ok(bridge) => Box::into_raw(Box::new(bridge)).cast(),
        Err(e) => {
            respond(error, Err(e));
            std::ptr::null_mut()
        }
    }
}

unsafe fn bridge<'a>(handle: *mut c_void) -> Result<&'a Bridge> {
    ensure!(!handle.is_null(), "null component handle");
    Ok(&*handle.cast::<Bridge>())
}

/// Invoke a component export through its owning bridge.
///
/// # Safety
/// `handle` must be null or a live handle returned by this engine, used on its
/// owning thread without concurrent mutation or destruction. Nonempty spans
/// must be valid initialized bytes for this call; `out` must be null or aligned,
/// exclusively writable `Buffer` storage. Callback context must be live. Free
/// a returned buffer exactly once through this engine's buffer-drop entry.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_call(
    handle: *mut c_void,
    interface: *const u8,
    interface_len: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    context: u64,
    out: *mut Buffer,
) -> i32 {
    respond(
        out,
        (|| {
            let interface = std::str::from_utf8(bytes(interface, interface_len)?)?;
            let function = std::str::from_utf8(bytes(function, function_len)?)?;
            let args: Vec<serde_json::Value> = serde_json::from_slice(bytes(args, args_len)?)?;
            let value =
                bridge(handle)?.with(context, |host| host.call(interface, function, &args))?;
            Ok(serde_json::to_vec(&match value {
                Some(value) => serde_json::json!({"returned":true,"value":value}),
                None => serde_json::json!({"returned":false}),
            })?)
        })(),
    )
}

/// Dispose guest resources while retaining the bridge allocation.
///
/// # Safety
/// `handle` must be null or a live engine handle used on its owning thread,
/// without concurrent mutation or destruction. Callback context must remain
/// valid through disposal. `out` must be null or aligned, exclusively writable
/// `Buffer` storage; release returned buffers through this engine.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_dispose(
    handle: *mut c_void,
    context: u64,
    out: *mut Buffer,
) -> i32 {
    let mut disposed = false;
    let status = respond(
        out,
        bridge(handle)
            .and_then(|bridge| {
                bridge.entered(context, context, || {
                    // Cancel suspended guest tasks while the cleanup frame is live.
                    drop(unsafe { (&mut *bridge.pending.get()).take() });
                    if let Some(driver) = &bridge.driver {
                        let before = bridge.async_replies.failure_serial();
                        driver.dispose();
                        disposed = true;
                        ensure!(
                            before == bridge.async_replies.failure_serial(),
                            "component driver disposal callback failed"
                        );
                    }
                    let host = unsafe { &mut *bridge.host.get() };
                    // Host::dispose takes the store before running destructors:
                    // even a trap releases it. A rejected nested call never
                    // reaches this closure and must keep its callback captures.
                    let result = host.dispose();
                    disposed = true;
                    result
                })
            })
            .map(|()| Vec::new()),
    );
    if disposed && status == ERROR {
        RELEASED_ERROR
    } else {
        status
    }
}

/// Drop one guest resource and report whether its ownership was released.
///
/// # Safety
/// `handle` must be null or a live engine handle used on its owning thread,
/// without concurrent mutation or destruction. The token span must be readable
/// for `len` bytes, and callback context must be live. `out` must be null or
/// aligned, exclusively writable `Buffer` storage; free replies through this engine.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_resource_drop(
    handle: *mut c_void,
    token: *const u8,
    len: usize,
    context: u64,
    out: *mut Buffer,
) -> i32 {
    let mut released = false;
    let status = respond(
        out,
        (|| {
            let token = std::str::from_utf8(bytes(token, len)?)?;
            bridge(handle)?.with(context, |host| {
                host.drop_resource_tracked(token, &mut released)
            })?;
            Ok(Vec::new())
        })(),
    );
    if released && status == ERROR {
        RELEASED_ERROR
    } else {
        status
    }
}

/// Interrupt guest execution without entering its store.
///
/// # Safety
/// `handle` must be null or a live handle returned by this engine and remain
/// allocated until this call returns. This entry may run concurrently with
/// execution; it must not race bridge destruction.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_interrupt(handle: *mut c_void) {
    if let Ok(bridge) = bridge(handle) {
        bridge.interrupt.interrupt();
    }
}

/// Collect a guest resource, deferring nested calls while the store is busy.
///
/// # Safety
/// `handle` must be null or a live engine handle used on its owning thread,
/// without concurrent mutation or destruction. The request span must be readable
/// for `len` bytes and callback context must be live. `out` must be null or
/// aligned, exclusively writable `Buffer` storage; free replies through this engine.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_resource_collect(
    handle: *mut c_void,
    request: *const u8,
    len: usize,
    context: u64,
    out: *mut Buffer,
) -> i32 {
    // Check before forming a mutable Host reference. Finalization jobs can be
    // delivered by a nested microtask pump inside a guest's host callback.
    if bridge(handle)
        .is_ok_and(|bridge| bridge.busy.get() || unsafe { (&*bridge.pending.get()).is_some() })
    {
        return perry_abi::component_host::DEFERRED;
    }
    let mut released = false;
    let status = respond(
        out,
        (|| {
            let request = serde_json::from_slice(bytes(request, len)?)?;
            bridge(handle)?.with(context, |host| {
                host.collect_resource(request, &mut released)
            })?;
            Ok(Vec::new())
        })(),
    );
    if released && status == ERROR {
        RELEASED_ERROR
    } else {
        status
    }
}

/// Release the bridge without invoking guest code or embedding callbacks.
///
/// # Safety
/// `handle` must be null or the unique, live handle returned by this engine.
/// Call on its owning thread only after all calls and interrupts have returned.
/// No pointer to the bridge may be used after this call.

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_drop(handle: *mut c_void) {
    if !handle.is_null() {
        let bridge = Box::from_raw(handle.cast::<Bridge>());
        assert!(
            !bridge.busy.get(),
            "a live component call retains its owner"
        );
        // GC native finalizers must not enter Wasm or invoke JavaScript.
        bridge.context.store(0, Ordering::Release);
        bridge.cleanup_context.store(0, Ordering::Release);
        drop((&mut *bridge.pending.get()).take());
        if let Some(driver) = &bridge.driver {
            driver.dispose();
        }
        (&mut *bridge.host.get()).abandon();
        drop(bridge);
    }
}

macro_rules! ops {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        pub static OPS: ComponentHostOps = ComponentHostOps {
            abi_version: ABI_VERSION, size: std::mem::size_of::<ComponentHostOps>(), $($name,)*
        };
    };
}
perry_abi::component_host_entry_points!(ops);

#[cfg(test)]
#[path = "ffi_tests.rs"]
mod tests;

#[cfg(not(test))]
extern "C" {
    fn js_runtime_set_component_host_ops(ops: *const ComponentHostOps);
    fn js_runtime_component_io_ops() -> *const perry_abi::component_io::Ops;
}

#[no_mangle]
pub extern "C" fn perry_component_host_register() {
    #[cfg(not(test))]
    unsafe {
        crate::sync_wasi::native_io::install(js_runtime_component_io_ops());
        js_runtime_set_component_host_ops(&OPS);
    }
}
