//! Persistent owner-thread C transport. Submission touches native queues only;
//! poll is the sole entry that runs guest code or embedding callbacks.
use super::*;
use crate::{Driver, DriverCall, DriverClient};
use perry_abi::component_host::{OriginCallback, OriginResourceCallback, WakeCallback};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap, future::Future, pin::Pin, task::Poll};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    Call {
        interface: String,
        function: String,
        args: Vec<Value>,
    },
    Read {
        value: Value,
        capacity: Option<u32>,
    },
    Close {
        value: Value,
    },
    Collect {
        value: Value,
    },
    Create {
        index: usize,
    },
    ResourceDrop {
        value: Value,
    },
    ResourceCollect {
        request: Value,
    },
    Write {
        writer: Value,
        values: Vec<Value>,
    },
    CancelWrite {
        writer: Value,
    },
    CloseWriter {
        writer: Value,
    },
    AbortWriter {
        writer: Value,
    },
}

pub(super) struct Actor {
    // Keep these borrows independent: callbacks can submit while Driver is
    // mutably polled, but must never poll or destroy that Driver recursively.
    driver: RefCell<Option<Pin<Box<Driver>>>>,
    client: DriverClient,
    calls: RefCell<BTreeMap<u64, DriverCall>>,
}
impl Actor {
    pub fn new(host: Host, current: Arc<AtomicU64>, cleanup: Arc<AtomicU64>) -> Result<Self> {
        let (driver, client) = host.into_driver_with_retirement(move || {
            current.store(cleanup.load(Ordering::Acquire), Ordering::Release);
        })?;
        Ok(Self {
            driver: RefCell::new(Some(Box::pin(driver))),
            client,
            calls: RefCell::default(),
        })
    }
    fn submit(&self, request: Request) -> Result<u64> {
        let call = match request {
            Request::Call {
                interface,
                function,
                args,
            } => self.client.call(&interface, &function, &args)?,
            Request::Read { value, capacity } => self.client.read_async_value(&value, capacity)?,
            Request::Close { value } => self.client.close_async_value(&value)?,
            Request::Collect { value } => self.client.collect_async_value(&value)?,
            Request::Create { index } => {
                let ty = self
                    .client
                    .async_value_types()
                    .get(index)
                    .context("unknown async factory type index")?;
                self.client.create_async_value(ty)?
            }
            Request::ResourceDrop { value } => self.client.drop_resource(&value)?,
            Request::ResourceCollect { request } => self.client.collect_resource(&request)?,
            Request::Write { writer, values } => self.client.write_async_value(&writer, &values)?,
            Request::CancelWrite { writer } => self.client.cancel_async_write(&writer)?,
            Request::CloseWriter { writer } => self.client.close_async_writer(&writer)?,
            Request::AbortWriter { writer } => self.client.abort_async_writer(&writer)?,
        };
        let id = call.id().as_u64();
        self.calls.borrow_mut().insert(id, call);
        Ok(id)
    }
    pub fn dispose(&self) {
        // Driver Drop first cancels tasks and retires host-owned resources.
        self.driver.borrow_mut().take();
        self.calls.borrow_mut().clear();
    }
    fn poll(&self, wake: WakeCallback, agent: u64) -> Result<Value> {
        let waker = std::task::Waker::from(Arc::new(async_calls::Notify(wake, agent)));
        let mut cx = std::task::Context::from_waker(&waker);
        let mut failure = None;
        {
            let mut slot = self.driver.borrow_mut();
            if let Some(driver) = slot.as_mut() {
                match driver.as_mut().poll(&mut cx) {
                    Poll::Pending => {}
                    Poll::Ready(Err(error)) => {
                        failure = Some(format!("{error:#}"));
                        slot.take();
                    }
                    Poll::Ready(Ok(mut host)) => {
                        // The actor retains its client, so normal completion
                        // cannot occur until the bridge has been disposed.
                        host.abandon();
                        slot.take();
                        failure = Some("component driver stopped unexpectedly".into());
                    }
                }
            }
        }
        let ids: Vec<_> = self.calls.borrow().keys().copied().collect();
        let mut completed = Vec::new();
        for id in ids {
            let mut call = self.calls.borrow_mut().remove(&id).unwrap();
            match Pin::new(&mut call).poll(&mut cx) {
                Poll::Pending => {
                    self.calls.borrow_mut().insert(id, call);
                }
                Poll::Ready(result) => completed.push(match result {
                    Ok(Some(value)) => {
                        json!({"id":id.to_string(),"ok":true,"returned":true,"value":value})
                    }
                    Ok(None) => json!({"id":id.to_string(),"ok":true,"returned":false}),
                    Err(error) => {
                        json!({"id":id.to_string(),"ok":false,"error":format!("{error:#}")})
                    }
                }),
            }
        }
        let slot = self.driver.borrow();
        let active = slot.as_ref().is_some_and(|driver| driver.is_active());
        let retained: Vec<_> = self
            .client
            .retained_origins()
            .iter()
            .map(|id| id.as_u64().to_string())
            .collect();
        Ok(
            json!({"completed":completed,"retained":retained,"active":active,"retired":slot.is_none(),"error":failure}),
        )
    }
}
fn actor(bridge: &Bridge) -> Result<&Actor> {
    bridge
        .driver
        .as_ref()
        .context("operation requires a persistent driver handle")
}

/// Construct a persistent driver with origin-explicit callbacks. Origin zero
/// is used during construction. All callbacks run on the owning embedding stack.
/// # Safety
/// Same spans, callback lifetime, allocator and thread rules as host_new. Use
/// host_dispose/host_drop to release this handle. Legacy execution, resource and
/// HTTP entries reject it; driver commands are the execution interface.
#[no_mangle]
pub unsafe extern "C" fn perry_component_driver_new(
    wasm: *const u8,
    wasm_len: usize,
    contract: *const u8,
    contract_len: usize,
    options: *const u8,
    options_len: usize,
    callback: Option<OriginCallback>,
    free_callback: Option<FreeCallback>,
    resource_callback: Option<OriginResourceCallback>,
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
        callback.map(ImportCallback::Origin),
        free_callback,
        resource_callback.map(LifecycleCallback::Origin),
        context,
        true,
        error,
    )
}

/// Discover factory schemas and their indices, scoped to this handle.
/// # Safety
/// Live owner-thread handle; writable output freed with buffer_drop.
#[no_mangle]
pub unsafe extern "C" fn perry_component_driver_types(
    handle: *mut c_void,
    out: *mut Buffer,
) -> i32 {
    respond(
        out,
        (|| {
            let actor = actor(bridge(handle)?)?;
            let types: Vec<_> = actor
                .client
                .async_value_types()
                .iter()
                .enumerate()
                .map(|(index, ty)| json!({"index":index,"schema":ty.wit()}))
                .collect();
            Ok(serde_json::to_vec(&types)?)
        })(),
    )
}

/// Queue one JSON command and return its decimal-string invocation ID. No
/// Wasm or embedding callback executes. May be called from a driver callback.
/// # Safety
/// Live owner-thread handle, initialized readable request span, writable output
/// freed with buffer_drop. Neither submission nor wake may run JS recursively.
#[no_mangle]
pub unsafe extern "C" fn perry_component_driver_submit(
    handle: *mut c_void,
    request: *const u8,
    len: usize,
    out: *mut Buffer,
) -> i32 {
    respond(
        out,
        (|| {
            ensure!(!out.is_null(), "driver submission requires a reply buffer");
            let request = serde_json::from_slice(bytes(request, len)?)?;
            let id = actor(bridge(handle)?)?.submit(request)?;
            Ok(serde_json::to_vec(&json!({"id":id.to_string()}))?)
        })(),
    )
}

/// Run one owner-thread poll under fresh primary/cleanup frame-batch tokens.
/// Return all ready replies before the retained-origin snapshot. Retention is
/// conservative and is not a per-task exit notification. Wake only notifies.
/// # Safety
/// Live owner-thread handle; context tokens are live through this call only.
/// Process-lifetime native wake, writable output freed with buffer_drop. Must
/// consume the returned replies exactly once, including before trimming roots.
#[no_mangle]
pub unsafe extern "C" fn perry_component_driver_poll(
    handle: *mut c_void,
    context: u64,
    cleanup_context: u64,
    wake: Option<WakeCallback>,
    agent: u64,
    out: *mut Buffer,
) -> i32 {
    respond(
        out,
        (|| {
            ensure!(!out.is_null(), "driver poll requires a reply buffer");
            let bridge = bridge(handle)?;
            let wake = wake.context("driver poll requires a native wake callback")?;
            bridge.entered(context, cleanup_context, || {
                let mut result = actor(bridge)?.poll(wake, agent)?;
                // A core start can enqueue an async import without a guest
                // invocation observing its subtask yet. Wasmtime may call
                // that quiescent, but the embedding still owes this reply.
                if result["retired"] == false && bridge.async_replies.is_pending() {
                    result["active"] = json!(true);
                }
                result["failedOrigins"] = json!(bridge.async_replies.take_failures());
                Ok(serde_json::to_vec(&result)?)
            })
        })(),
    )
}

/// Discard one observer. Before execution this skips the request; after start
/// it retires the Store on the next poll. Accepted soft close/cancel commands
/// still execute. Does not run guest code or callbacks.
/// # Safety
/// Live owner-thread handle and writable output freed with buffer_drop.
#[no_mangle]
pub unsafe extern "C" fn perry_component_driver_cancel(
    handle: *mut c_void,
    invocation: u64,
    out: *mut Buffer,
) -> i32 {
    respond(
        out,
        (|| {
            actor(bridge(handle)?)?
                .calls
                .borrow_mut()
                .remove(&invocation)
                .context("unknown or completed component invocation")?;
            Ok(Vec::new())
        })(),
    )
}

#[cfg(test)]
mod tests;
