//! Allocator-safe callback transport. JS exceptions must be caught by the
//! runtime callback; every call returns through Wasmtime before JS can throw.
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
}

impl Bridge {
    fn with<R>(&self, context: u64, call: impl FnOnce(&mut Host) -> Result<R>) -> Result<R> {
        ensure!(
            !self.busy.replace(true),
            "component cannot be re-entered during a call"
        );
        struct Guard<'a>(&'a Bridge, u64);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.context.store(self.1, Ordering::Release);
                self.0.busy.set(false);
            }
        }
        let guard = Guard(self, self.context.swap(context, Ordering::AcqRel));
        // The busy check happens before forming the mutable reference. Nested
        // callbacks may enter other components, but cannot alias this Store.
        let result = call(unsafe { &mut *self.host.get() });
        drop(guard);
        result
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
    let result = (|| -> Result<Bridge> {
        let world: World = serde_json::from_slice(bytes(contract, contract_len)?)?;
        let options: Options = if options_len == 0 {
            Options::default()
        } else {
            serde_json::from_slice(bytes(options, options_len)?)?
        };
        let current = Arc::new(AtomicU64::new(context));
        let mut imports = Imports::new();
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
                let current = current.clone();
                let handler: Import = Arc::new(move |args| {
                    let context = current.load(Ordering::Acquire);
                    ensure!(
                        context != 0,
                        "component import outside a live embedding call"
                    );
                    let args = serde_json::to_vec(args)?;
                    let mut reply = Reply(Buffer::default(), free);
                    let status = callback(
                        context,
                        interface_name.as_ptr(),
                        interface_name.len(),
                        function_name.as_ptr(),
                        function_name.len(),
                        args.as_ptr(),
                        args.len(),
                        &mut reply.0,
                    );
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
        let resource_current = current.clone();
        let resource_hook: ResourceHook = Arc::new(move |event| {
            // Older callers without host resources need no lifecycle channel.
            if resource_callback.is_none() && matches!(event, crate::ResourceEvent::Drain) {
                return Ok(Vec::new());
            }
            let callback =
                resource_callback.context("host resources require a resource callback")?;
            let free = free_callback.context("host resources require a reply deallocator")?;
            let context = resource_current.load(Ordering::Acquire);
            ensure!(context != 0, "resource event outside a live embedding call");
            let event = serde_json::to_vec(&event)?;
            let mut reply = Reply(Buffer::default(), free);
            let status = callback(context, event.as_ptr(), event.len(), &mut reply.0);
            if status != OK {
                bail!(
                    "component resource event: {}",
                    String::from_utf8_lossy(bytes(reply.0.data, reply.0.len)?)
                );
            }
            Ok(serde_json::from_slice(bytes(reply.0.data, reply.0.len)?)?)
        });
        let host = Host::new_with_resource_hook(
            bytes(wasm, wasm_len)?,
            &world,
            &imports,
            &options,
            resource_hook,
        )?;
        current.store(0, Ordering::Release);
        let interrupt = host.interrupt_handle();
        Ok(Bridge {
            host: UnsafeCell::new(host),
            busy: Cell::new(false),
            context: current,
            interrupt,
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
                bridge.with(context, |host| {
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

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_interrupt(handle: *mut c_void) {
    if let Ok(bridge) = bridge(handle) {
        bridge.interrupt.interrupt();
    }
}

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
    if bridge(handle).is_ok_and(|bridge| bridge.busy.get()) {
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

#[no_mangle]
pub unsafe extern "C" fn perry_component_host_drop(handle: *mut c_void) {
    if !handle.is_null() {
        let bridge = Box::from_raw(handle.cast::<Bridge>());
        assert!(
            !bridge.busy.get(),
            "a live component call retains its owner"
        );
        // GC native finalizers must not enter Wasm or invoke JavaScript.
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
}

#[no_mangle]
pub extern "C" fn perry_component_host_register() {
    #[cfg(not(test))]
    unsafe {
        js_runtime_set_component_host_ops(&OPS);
    }
}
