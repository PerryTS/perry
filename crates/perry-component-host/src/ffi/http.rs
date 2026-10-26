//! Nonblocking native HTTP boundary. Polling only transfers owned native
//! resources; the separate invocation enters Wasm on the caller's Store.
use super::*;
use perry_abi::component_host::{WakeCallback, HTTP_PENDING, HTTP_READY};
use perry_wasi::http::{server, Error as HttpError};
use std::{
    future::Future,
    pin::Pin,
    task::{Context as TaskContext, Poll, Wake, Waker},
};
mod options;
#[cfg(test)]
mod tests;

struct Listener {
    server: server::Server,
    alive: Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

type AcceptFuture = Pin<
    Box<dyn Future<Output = std::result::Result<(server::Request, server::Completion), HttpError>>>,
>;
type CompleteFuture = Pin<Box<dyn Future<Output = std::result::Result<(), HttpError>>>>;
enum State {
    Accept(AcceptFuture),
    Ready(Box<server::Request>, server::Completion),
    Completing(CompleteFuture),
    Done,
}
struct Operation {
    state: UnsafeCell<State>,
    busy: Cell<bool>,
    server_alive: Arc<std::sync::atomic::AtomicBool>,
}
impl Operation {
    fn with<R>(&self, f: impl FnOnce(&mut State) -> Result<R>) -> Result<R> {
        ensure!(
            !self.busy.replace(true),
            "HTTP operation cannot be re-entered"
        );
        struct Guard<'a>(&'a Cell<bool>);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let guard = Guard(&self.busy);
        let result = f(unsafe { &mut *self.state.get() });
        drop(guard);
        result
    }
}
struct Notification(WakeCallback, u64);
impl Wake for Notification {
    fn wake(self: Arc<Self>) {
        unsafe { (self.0)(self.1) }
    }
    fn wake_by_ref(self: &Arc<Self>) {
        unsafe { (self.0)(self.1) }
    }
}
unsafe fn operation<'a>(pointer: *mut c_void) -> Result<&'a Operation> {
    ensure!(!pointer.is_null(), "null HTTP operation");
    Ok(&*pointer.cast::<Operation>())
}

/// Transfer one ready exchange into a persistent driver's native queue. This
/// never enters Wasm or an embedding callback. The accept remains caller-owned
/// and consumed after transfer; driver_poll publishes the invocation's result.
/// # Safety
/// Live owner-thread handles and readable interface bytes; writable output
/// freed with buffer_drop. Drop the accept exactly once after submission.
#[no_mangle]
pub unsafe extern "C" fn perry_component_driver_http_submit(
    handle: *mut c_void,
    op: *mut c_void,
    interface: *const u8,
    interface_len: usize,
    out: *mut Buffer,
) -> i32 {
    respond(
        out,
        (|| {
            let interface = std::str::from_utf8(bytes(interface, interface_len)?)?;
            let actor = super::driver::actor(bridge(handle)?)?;
            let operation = operation(op)?;
            ensure!(
                operation.server_alive.load(Ordering::Acquire),
                "HTTP server is closed"
            );
            let id = operation.with(|state| {
                ensure!(
                    matches!(state, State::Ready(..)),
                    "HTTP request is not ready or was already handled"
                );
                let State::Ready(request, completion) = std::mem::replace(state, State::Done)
                else {
                    unreachable!()
                };
                actor.submit_http(interface, *request, completion)
            })?;
            Ok(serde_json::to_vec(
                &serde_json::json!({"id":id.to_string()}),
            )?)
        })(),
    )
}

/// Bind an explicit host listener. Success returns an owned server handle and
/// a JSON local address (including the selected ephemeral port) in `output`.
/// Failure returns null and UTF-8 diagnostic bytes. Binding never executes JS.
/// # Safety
/// Options must be a readable byte span; output must be writable Buffer storage
/// or null. Use the returned handle on its owning thread and drop it once.
/// Free every returned buffer with this engine's buffer-drop entry.
#[no_mangle]
pub unsafe extern "C" fn perry_component_http_server_new(
    options: *const u8,
    options_len: usize,
    out: *mut Buffer,
) -> *mut c_void {
    let result = (|| -> Result<server::Server> {
        let options: options::ServerOptions = serde_json::from_slice(bytes(options, options_len)?)?;
        let transport = options.transport()?;
        Ok(futures_lite::future::block_on(server::Server::bind(
            options.address,
            transport,
            options.max_connections,
        ))?)
    })();
    match result {
        Ok(server) => {
            output(
                out,
                serde_json::to_vec(&server.local_addr().to_string()).unwrap(),
            );
            Box::into_raw(Box::new(Listener {
                server,
                alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            }))
            .cast()
        }
        Err(error) => {
            output(out, format!("{error:#}").into_bytes());
            std::ptr::null_mut()
        }
    }
}
/// Cancel all accepts/exchanges, join the native driver and free this listener.
/// Outstanding operation handles remain valid and report cancellation on poll.
/// # Safety
/// Server must be null or an exclusively owned live handle from server_new;
/// call once on its owner thread. No simultaneous operation may borrow it.
#[no_mangle]
pub unsafe extern "C" fn perry_component_http_server_drop(server: *mut c_void) {
    if !server.is_null() {
        drop(Box::from_raw(server.cast::<Listener>()));
    }
}
/// Start one bounded accept/exchange. This returns immediately, before input.
/// # Safety
/// Server must be a live handle on its owning thread; output must be writable
/// Buffer storage or null. Free returned errors with the engine allocator.
#[no_mangle]
pub unsafe extern "C" fn perry_component_http_accept(
    server: *mut c_void,
    out: *mut Buffer,
) -> *mut c_void {
    if !out.is_null() {
        *out = Buffer::default();
    }
    let result = (|| -> Result<Operation> {
        ensure!(!server.is_null(), "null HTTP server");
        let listener = &*server.cast::<Listener>();
        let pending = listener.server.accept()?;
        Ok(Operation {
            state: UnsafeCell::new(State::Accept(Box::pin(pending.receive()))),
            busy: Cell::new(false),
            server_alive: listener.alive.clone(),
        })
    })();
    match result {
        Ok(operation) => Box::into_raw(Box::new(operation)).cast(),
        Err(error) => {
            output(out, format!("{error:#}").into_bytes());
            std::ptr::null_mut()
        }
    }
}
/// Poll without blocking or executing guest/JS code. PENDING registers a native
/// wake, READY permits one handle call, and OK/a transport error is terminal.
/// READY may be polled repeatedly; terminal operations must be dropped.
/// Invalid-argument/reentrant errors leave state unchanged.
/// # Safety
/// Operation must be live on its owner thread and not concurrently mutated.
/// Wake must remain callable for the process lifetime and may only notify the
/// owning agent; it must never execute JS or touch a heap. Agent is native data.
/// Output must be writable Buffer storage or null, freed by this engine.
#[no_mangle]
pub unsafe extern "C" fn perry_component_http_poll(
    op: *mut c_void,
    wake: Option<WakeCallback>,
    agent: u64,
    out: *mut Buffer,
) -> i32 {
    if !out.is_null() {
        *out = Buffer::default();
    }
    let result = (|| -> Result<i32> {
        let wake = wake.context("HTTP polling requires a native wake callback")?;
        let waker = Waker::from(Arc::new(Notification(wake, agent)));
        let mut context = TaskContext::from_waker(&waker);
        let operation = operation(op)?;
        operation.with(|state| {
            if !operation.server_alive.load(Ordering::Acquire) {
                *state = State::Done;
                bail!("HTTP server is closed");
            }
            match state {
                State::Accept(future) => match future.as_mut().poll(&mut context) {
                    Poll::Pending => Ok(HTTP_PENDING),
                    Poll::Ready(Ok((request, completion))) => {
                        *state = State::Ready(Box::new(request), completion);
                        Ok(HTTP_READY)
                    }
                    Poll::Ready(Err(error)) => {
                        *state = State::Done;
                        Err(error.into())
                    }
                },
                State::Ready(..) => Ok(HTTP_READY),
                State::Completing(future) => match future.as_mut().poll(&mut context) {
                    Poll::Pending => Ok(HTTP_PENDING),
                    Poll::Ready(result) => {
                        *state = State::Done;
                        result?;
                        Ok(OK)
                    }
                },
                State::Done => bail!("HTTP operation is already complete"),
            }
        })
    })();
    match result {
        Ok(status) => status,
        Err(error) => {
            output(out, format!("{error:#}").into_bytes());
            ERROR
        }
    }
}
/// Invoke one ready incoming-handler on its owning Store. After invocation the
/// operation remains live for transport completion, even if the handler fails.
/// RELEASED_ERROR means the failed handler retired its component instance.
/// Reentrant/invalid-bridge rejection leaves an unconsumed request available.
/// A bad export/signature consumes the request but leaves its component alive.
/// # Safety
/// Both handles must be live on their owning thread, with no concurrent access.
/// Interface must be a readable UTF-8 span. Both primary and cleanup callback
/// contexts must remain live throughout this call and catch JS exceptions.
/// Cleanup has a separate exception slot so it cannot overwrite a primary trap. Output is caller-owned
/// Buffer storage or null; returned bytes are freed by the engine allocator.
#[no_mangle]
pub unsafe extern "C" fn perry_component_http_handle(
    handle: *mut c_void,
    op: *mut c_void,
    interface: *const u8,
    interface_len: usize,
    context: u64,
    cleanup_context: u64,
    out: *mut Buffer,
) -> i32 {
    let mut retired = false;
    let result = (|| -> Result<Vec<u8>> {
        let interface = std::str::from_utf8(bytes(interface, interface_len)?)?;
        let operation = operation(op)?;
        ensure!(
            operation.server_alive.load(Ordering::Acquire),
            "HTTP server is closed"
        );
        operation.with(|state| {
            ensure!(
                matches!(state, State::Ready(..)),
                "HTTP request is not ready or was already handled"
            );
            // Check both reentry guards before taking request ownership. Guest
            // callbacks cannot alias this operation or this Wasmtime Store.
            let bridge = bridge(handle)?;
            bridge.with(context, |host| {
                let State::Ready(request, completion) = std::mem::replace(state, State::Done)
                else {
                    unreachable!()
                };
                *state = State::Completing(Box::pin(completion.finish()));
                let result = host.handle_http_with_retirement(
                    interface,
                    request.request,
                    request.response,
                    || bridge.context.store(cleanup_context, Ordering::Release),
                );
                retired = host.live.is_none();
                result
            })
        })?;
        Ok(Vec::new())
    })();
    let status = respond(out, result);
    if retired && status == ERROR {
        RELEASED_ERROR
    } else {
        status
    }
}
/// Cancel and free an operation. Never invokes guest code or JS callbacks.
/// # Safety
/// Operation must be null or an exclusively owned live handle. Drop exactly once
/// on its owner thread, outside any active poll/handle call using this operation.
#[no_mangle]
pub unsafe extern "C" fn perry_component_http_drop(op: *mut c_void) {
    if !op.is_null() {
        let operation = Box::from_raw(op.cast::<Operation>());
        assert!(
            !operation.busy.get(),
            "a live HTTP call retains its operation owner"
        );
        drop(operation);
    }
}

/// Transfer one ready P3 request to the component's async call driver. Resume
/// with host_call_async_poll and fresh primary/cleanup callback frames. The
/// accept operation becomes consumed; its handle remains owned by the caller.
/// Reentrant or overlapping calls leave the ready request unconsumed.
/// # Safety
/// Handles and interface bytes must be live on their owner thread. Both frame
/// tokens are valid only for this poll; wake must be process-lifetime and only
/// notify the agent. Free output with buffer_drop, and drop the accept once.
#[no_mangle]
pub unsafe extern "C" fn perry_component_http_handle_async_start(
    handle: *mut c_void,
    op: *mut c_void,
    interface: *const u8,
    interface_len: usize,
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
        let wake = wake.context("async HTTP handling requires a native wake callback")?;
        let operation = operation(op)?;
        ensure!(
            operation.server_alive.load(Ordering::Acquire),
            "HTTP server is closed"
        );
        operation.with(|state| {
            ensure!(
                matches!(state, State::Ready(..)),
                "HTTP request is not ready or was already handled"
            );
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
                let State::Ready(request, completion) = std::mem::replace(state, State::Done)
                else {
                    unreachable!()
                };
                let current = bridge.context.clone();
                let cleanup = bridge.cleanup_context.clone();
                *bridge.pending.get() = Some(PendingCall {
                    http: true,
                    future: Box::pin(async move {
                        let result = host
                            .handle_http_async_with_retirement(
                                &interface,
                                *request,
                                completion,
                                move || {
                                    current
                                        .store(cleanup.load(Ordering::Acquire), Ordering::Release);
                                },
                            )
                            .await
                            .and_then(|transport| transport.map(|()| None).map_err(Into::into));
                        (host, result)
                    }),
                });
                // Poll immediately so cancellation owns a started invocation.
                poll_call(bridge, wake, agent, out)
            })
        })
    })();
    match result {
        Ok(status) => status,
        Err(error) => respond(out, Err(error)),
    }
}
