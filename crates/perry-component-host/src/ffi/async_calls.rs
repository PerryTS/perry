//! Owner-thread async calls; native request channels never carry Perry values.
#[cfg(test)]
mod tests;
use super::*;
use crate::{AsyncImport, Host};
use futures_channel::oneshot;
use perry_abi::component_host::{WakeCallback, ASYNC_PENDING};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Mutex,
    task::{Context as TaskContext, Poll, Wake, Waker},
};

type ReplyResult = Result<Option<serde_json::Value>>;
#[derive(Default)]
pub(super) struct Replies {
    next: AtomicU64,
    entries: Mutex<BTreeMap<u64, PendingReply>>,
    failures: Mutex<Vec<u64>>,
    failure_serial: AtomicU64,
}
struct PendingReply {
    origin: u64,
    send: oneshot::Sender<ReplyResult>,
}
impl Replies {
    pub(super) fn failure_serial(&self) -> u64 {
        self.failure_serial.load(Ordering::Relaxed)
    }
    pub(super) fn failed(&self, origin: u64) {
        self.failure_serial.fetch_add(1, Ordering::Relaxed);
        let mut failures = self.failures.lock().unwrap();
        if !failures.contains(&origin) {
            failures.push(origin);
        }
    }
    pub(super) fn callback_status(&self, status: i32) {
        if status != OK && status != ASYNC_PENDING {
            self.failed(crate::current_invocation().map_or(0, |id| id.as_u64()));
        }
    }
    pub(super) fn take_failures(&self) -> Vec<String> {
        std::mem::take(&mut *self.failures.lock().unwrap())
            .into_iter()
            .map(|id| id.to_string())
            .collect()
    }
}
struct Request {
    id: u64,
    replies: Arc<Replies>,
}
impl Drop for Request {
    fn drop(&mut self) {
        self.replies.entries.lock().unwrap().remove(&self.id);
    }
}

pub(super) fn import(
    callback: ImportCallback,
    free: FreeCallback,
    context: Arc<AtomicU64>,
    replies: Arc<Replies>,
    interface: String,
    function: String,
    has_result: bool,
) -> AsyncImport {
    Arc::new(move |args| {
        let current = context.load(Ordering::Acquire);
        let setup = (|| -> Result<_> {
            ensure!(current != 0, "async import outside a live embedding call");
            let id = replies
                .next
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| anyhow::anyhow!("async import identity exhausted"))?
                + 1;
            let (send, receive) = oneshot::channel();
            {
                let mut entries = replies.entries.lock().unwrap();
                ensure!(entries.len() < 64, "too many pending component imports");
                entries.insert(
                    id,
                    PendingReply {
                        origin: crate::current_invocation().map_or(0, |id| id.as_u64()),
                        send,
                    },
                );
            }
            let request = Request {
                id,
                replies: replies.clone(),
            };
            let args = serde_json::to_vec(
                &serde_json::json!({"$asyncRequest":id.to_string(),"args":args}),
            )?;
            let mut reply = Reply(Buffer::default(), free);
            let status =
                unsafe { callback.call(current, &interface, &function, &args, &mut reply.0) };
            replies.callback_status(status);
            let result = unsafe { bytes(reply.0.data, reply.0.len)? };
            match status {
                ASYNC_PENDING => Ok((request, receive, None)),
                OK => Ok((
                    request,
                    receive,
                    Some(Ok(if has_result {
                        Some(serde_json::from_slice(result)?)
                    } else {
                        None
                    })),
                )),
                _ => Ok((
                    request,
                    receive,
                    Some(Err(anyhow::anyhow!(
                        "component host import: {}",
                        String::from_utf8_lossy(result)
                    ))),
                )),
            }
        })();
        Box::pin(async move {
            let (request, receive, immediate) = setup?;
            let result = match immediate {
                Some(value) => value,
                None => receive.await.context("component import cancelled")?,
            };
            drop(request);
            result.map(|value| if has_result { value } else { None })
        })
    })
}

type CallFuture = Pin<Box<dyn Future<Output = (Host, ReplyResult)>>>;
pub(super) struct PendingCall {
    pub(super) future: CallFuture,
    pub(super) http: bool,
}
pub(super) struct Notify(pub(super) WakeCallback, pub(super) u64);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        unsafe { (self.0)(self.1) }
    }
    fn wake_by_ref(self: &Arc<Self>) {
        unsafe { (self.0)(self.1) }
    }
}
pub(super) fn poll_call(
    bridge: &Bridge,
    wake: WakeCallback,
    agent: u64,
    out: *mut Buffer,
) -> Result<i32> {
    let waker = Waker::from(Arc::new(Notify(wake, agent)));
    let mut cx = TaskContext::from_waker(&waker);
    let mut pending =
        unsafe { (&mut *bridge.pending.get()).take() }.context("no pending component call")?;
    match pending.future.as_mut().poll(&mut cx) {
        Poll::Pending => {
            unsafe { *bridge.pending.get() = Some(pending) };
            Ok(ASYNC_PENDING)
        }
        Poll::Ready((host, result)) => {
            let retired = host.live.is_none();
            unsafe {
                *bridge.host.get() = host;
            }
            let status = respond(
                out,
                result.and_then(|value| {
                    Ok(serde_json::to_vec(&match value {
                        Some(value) => serde_json::json!({"returned":true,"value":value}),
                        None => serde_json::json!({"returned":false}),
                    })?)
                }),
            );
            Ok(if retired && status == ERROR {
                RELEASED_ERROR
            } else if pending.http && status == ERROR {
                perry_abi::component_host::HTTP_FAILURE
            } else {
                status
            })
        }
    }
}
/// Start and poll once on the owning thread. Guest/JS execution is permitted;
/// the caller supplies live primary and retirement frames for this poll only.
/// # Safety
/// Handle and byte spans must remain valid for this call. Wake must remain valid
/// for process lifetime and only notify the agent. Free output with buffer_drop.
#[no_mangle]
pub unsafe extern "C" fn perry_component_host_call_async_start(
    handle: *mut c_void,
    interface: *const u8,
    interface_len: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    context: u64,
    cleanup_context: u64,
    wake: Option<WakeCallback>,
    agent: u64,
    out: *mut Buffer,
) -> i32 {
    if !out.is_null() {
        *out = Buffer::default();
    }
    let result = (|| -> Result<i32> {
        let interface = std::str::from_utf8(bytes(interface, interface_len)?)?.to_owned();
        let function = std::str::from_utf8(bytes(function, function_len)?)?.to_owned();
        let args: Vec<serde_json::Value> = serde_json::from_slice(bytes(args, args_len)?)?;
        let wake = wake.context("async call requires a native wake callback")?;
        let bridge = bridge(handle)?;
        bridge.entered(context, cleanup_context, || {
            ensure!(
                bridge.driver.is_none(),
                "operation requires a legacy component handle"
            );
            ensure!(
                (&*bridge.pending.get()).is_none(),
                "component has a pending async call"
            );
            let original = &mut *bridge.host.get();
            ensure!(original.live.is_some(), "component is disposed");
            let mut host = Host {
                engine: original.engine.clone(),
                live: original.live.take(),
                dispatcher: original.dispatcher.clone(),
            };
            let current = bridge.context.clone();
            let cleanup = bridge.cleanup_context.clone();
            *bridge.pending.get() = Some(PendingCall {
                http: false,
                future: Box::pin(async move {
                    let result = host
                        .call_async_with_retirement(&interface, &function, &args, move || {
                            current.store(cleanup.load(Ordering::Acquire), Ordering::Release);
                        })
                        .await;
                    (host, result)
                }),
            });
            // A stored future has always been polled: cancellation then takes
            // the hard Store-retirement path rather than normal guest disposal.
            poll_call(bridge, wake, agent, out)
        })
    })();
    match result {
        Ok(status) => status,
        Err(error) => respond(out, Err(error)),
    }
}
/// Resume one pending call under fresh callback frames, without blocking.
/// # Safety
/// Same handle, frame, wake and output requirements as call_async_start.
#[no_mangle]
pub unsafe extern "C" fn perry_component_host_call_async_poll(
    handle: *mut c_void,
    context: u64,
    cleanup_context: u64,
    wake: Option<WakeCallback>,
    agent: u64,
    out: *mut Buffer,
) -> i32 {
    if !out.is_null() {
        *out = Buffer::default();
    }
    let result = (|| -> Result<i32> {
        let bridge = bridge(handle)?;
        let wake = wake.context("async call requires a native wake callback")?;
        bridge.entered(context, cleanup_context, || {
            poll_call(bridge, wake, agent, out)
        })
    })();
    match result {
        Ok(status) => status,
        Err(error) => respond(out, Err(error)),
    }
}
/// Deliver an owned JSON import result; this never executes guest or JS code.
/// # Safety
/// Handle must be live on its owning thread and value a valid readable span;
/// output must be writable and released through buffer_drop.
#[no_mangle]
pub unsafe extern "C" fn perry_component_host_async_import_resolve(
    handle: *mut c_void,
    request: u64,
    success: bool,
    value: *const u8,
    value_len: usize,
    out: *mut Buffer,
) -> i32 {
    respond(
        out,
        (|| -> Result<Vec<u8>> {
            let bridge = bridge(handle)?;
            ensure!(
                !bridge.busy.get(),
                "async import resolution cannot re-enter a component poll"
            );
            let value = bytes(value, value_len)?;
            let result = if success {
                Ok(Some(serde_json::from_slice(value)?))
            } else {
                Err(anyhow::anyhow!(
                    "JavaScript async component import rejected: {}",
                    String::from_utf8_lossy(value)
                ))
            };
            let send = bridge
                .async_replies
                .entries
                .lock()
                .unwrap()
                .remove(&request)
                .context("async import is cancelled, complete, or belongs to another component")?;
            if !success {
                bridge.async_replies.failed(send.origin);
            }
            send.send
                .send(result)
                .map_err(|_| anyhow::anyhow!("async import was cancelled"))?;
            Ok(Vec::new())
        })(),
    )
}
