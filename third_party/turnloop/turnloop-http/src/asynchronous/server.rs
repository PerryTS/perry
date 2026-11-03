//! Owned accept loop and explicit graceful shutdown. Services own each stream.
//!
//! A connection that ends after its final response closes with a lingering close:
//! the response (and HTTP/2 GOAWAY) is flushed, the write side is shut down, peer
//! input is read and discarded until the peer's EOF or the linger deadline, and
//! only then is the socket closed. Closing with unread peer bytes (a pipelined
//! request, HTTP/2 SETTINGS or WINDOW_UPDATE acknowledgements) would send RST,
//! which on macOS and Windows discards the peer's unread copy of that response.
use std::{
    cell::{Cell, RefCell},
    future::{Future, poll_fn},
    io,
    pin::{Pin, pin},
    rc::Rc,
    task::{Poll, Waker},
    time::Duration,
};
use turnloop_io::{
    AsyncIo, Backend, ExecutorHandle, HalfClose, Instant, Listener, turnloop::executor::JoinHandle,
};
/// Connection policy shared with every service through its [`Shutdown`] signal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    /// Longest time a closing connection keeps reading and discarding peer input
    /// after shutting down its write side, like nginx `lingering_timeout`. The
    /// connection closes earlier at the peer's EOF or at the [`Shutdown::stop_by`]
    /// deadline. Zero closes right after the half-close; `Duration::MAX` waits
    /// for the peer's EOF. Default: five seconds.
    pub linger_timeout: Duration,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            linger_timeout: Duration::from_secs(5),
        }
    }
}
/// Cloneable shutdown signal. Waiting futures register their own waker.
#[derive(Clone, Default)]
pub struct Shutdown {
    inner: Rc<State>,
}
#[derive(Default)]
struct State {
    stopped: Cell<bool>,
    options: Cell<Options>,
    deadline: Cell<Option<Instant>>,
    waiters: RefCell<Vec<Option<Waker>>>,
}
/// One retained waiter slot; the slot is reused after the registration drops.
struct Registration<'a> {
    signal: &'a Shutdown,
    index: usize,
}
impl Registration<'_> {
    fn arm(&self, waker: &Waker) {
        let mut waiters = self.signal.inner.waiters.borrow_mut();
        if waiters[self.index]
            .as_ref()
            .is_none_or(|w| !w.will_wake(waker))
        {
            waiters[self.index] = Some(waker.clone());
        }
    }
}
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.signal.inner.waiters.borrow_mut()[self.index] = None;
    }
}
impl Shutdown {
    /// A signal whose services use `options` (for example the linger timeout).
    pub fn with_options(options: Options) -> Self {
        let signal = Self::default();
        signal.inner.options.set(options);
        signal
    }
    /// The connection policy services read when they close.
    pub fn options(&self) -> Options {
        self.inner.options.get()
    }
    pub fn stop(&self) {
        self.inner.stopped.set(true);
        self.wake();
    }
    /// Stop, and close every lingering connection no later than `deadline`,
    /// including connections that are already lingering. An earlier deadline
    /// replaces a later one. In-flight requests still finish; drop the `Server`
    /// to cancel them.
    pub fn stop_by(&self, deadline: Instant) {
        let deadline = self.deadline().map_or(deadline, |d| d.min(deadline));
        self.inner.deadline.set(Some(deadline));
        self.stop();
    }
    /// The overall shutdown deadline set by [`Shutdown::stop_by`].
    pub fn deadline(&self) -> Option<Instant> {
        self.inner.deadline.get()
    }
    pub fn is_stopped(&self) -> bool {
        self.inner.stopped.get()
    }
    fn wake(&self) {
        let mut waiters = self.inner.waiters.borrow_mut();
        for w in waiters.iter_mut().filter_map(Option::take) {
            w.wake();
        }
    }
    fn register(&self) -> Registration<'_> {
        let mut waiters = self.inner.waiters.borrow_mut();
        let index = if let Some(i) = waiters.iter().position(Option::is_none) {
            i
        } else {
            waiters.push(None);
            waiters.len() - 1
        };
        Registration {
            signal: self,
            index,
        }
    }
    /// Race one operation against shutdown. Cancellation drops the operation.
    pub async fn until<F: Future>(&self, future: F) -> Option<F::Output> {
        let registration = self.register();
        let mut future = pin!(future);
        poll_fn(|cx| {
            if self.is_stopped() {
                return Poll::Ready(None);
            }
            registration.arm(cx.waker());
            future.as_mut().poll(cx).map(Some)
        })
        .await
    }
}
/// Lingering close after the final output is flushed: half-close, discard peer
/// input until its EOF or the linger/shutdown deadline, then close. A finished
/// exchange never becomes an error here; a transport whose close fails is dropped,
/// which releases its handle. `scratch` is the connection's retained input buffer.
async fn linger<S: HalfClose>(slot: &mut Option<S>, scratch: &mut Vec<u8>, signal: &Shutdown) {
    let Some(stream) = slot.as_mut() else {
        return;
    };
    let Some(executor) = stream.executor() else {
        if turnloop_io::close(stream).await.is_err() {
            slot.take();
        }
        return;
    };
    let own = executor.now().checked_add(signal.options().linger_timeout);
    let deadline = || match (own, signal.deadline()) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    // Unread request bytes are discarded with the rest of the peer's input.
    scratch.clear();
    scratch.resize(scratch.capacity(), 0);
    let failed = {
        let registration = signal.register();
        let mut close = turnloop_io::linger_close(stream, scratch, deadline());
        poll_fn(|cx| {
            // A later stop_by wakes this registration and moves the deadline.
            registration.arm(cx.waker());
            close.set_deadline(deadline());
            Pin::new(&mut close).poll(cx)
        })
        .await
        .is_err()
    };
    if failed {
        slot.take();
    }
}
/// TCP accept loop. Dropping the server cancels all in-flight services. Calling
/// `shutdown().stop()` stops accepts and asks services to drain; `run` joins them.
pub struct Server<B: Backend> {
    executor: ExecutorHandle<B>,
    listener: Option<Listener<B>>,
    shutdown: Shutdown,
    tasks: Vec<JoinHandle<io::Result<()>>>,
}
impl<B: Backend + 'static> Server<B> {
    pub fn bind(executor: ExecutorHandle<B>, address: std::net::SocketAddr) -> io::Result<Self> {
        Self::bind_with(executor, address, Options::default())
    }
    /// Bind with an explicit connection policy, shared through [`Server::shutdown`].
    pub fn bind_with(
        executor: ExecutorHandle<B>,
        address: std::net::SocketAddr,
        options: Options,
    ) -> io::Result<Self> {
        Ok(Self {
            listener: Some(Listener::bind(&executor, address)?),
            executor,
            shutdown: Shutdown::with_options(options),
            tasks: Vec::new(),
        })
    }
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener
            .as_ref()
            .ok_or_else(|| io::Error::other("server stopped"))?
            .local_addr()
    }
    pub fn shutdown(&self) -> Shutdown {
        self.shutdown.clone()
    }
    pub async fn run<F, Fut>(&mut self, service: F) -> io::Result<()>
    where
        F: Fn(AsyncIo<B>, Shutdown) -> Fut,
        Fut: Future<Output = io::Result<()>> + 'static,
    {
        while let Some(listener) = self.listener.as_ref() {
            // Reap successful and failed services; connection errors do not kill
            // the listener, while their JoinHandles still observe task completion.
            let mut i = 0;
            while i < self.tasks.len() {
                if self.tasks[i].is_finished() {
                    let task = self.tasks.swap_remove(i);
                    let _ = task.await;
                } else {
                    i += 1;
                }
            }
            let Some(stream) = self.shutdown.until(listener.accept()).await else {
                break;
            };
            self.tasks.push(
                self.executor
                    .spawn_local(service(stream?, self.shutdown.clone()))
                    .map_err(turnloop_io::error)?,
            );
        }
        self.listener.take();
        for task in self.tasks.drain(..) {
            task.await
                .map_err(|_| io::Error::other("service cancelled"))??;
        }
        Ok(())
    }
}
impl<B: Backend> Drop for Server<B> {
    fn drop(&mut self) {
        self.shutdown.stop();
        self.tasks.clear();
        self.listener.take();
    }
}

/// Streaming HTTP/1 response encoder passed to a service callback. Its retained
/// output is flushed after every event, bounding echo-server body storage.
pub struct Response {
    encoder: Option<crate::http1::Encoder>,
    output: Vec<u8>,
    finished: bool,
    keep_alive: bool,
    status: u16,
}
impl Default for Response {
    fn default() -> Self {
        Self {
            encoder: None,
            output: Vec::with_capacity(65536),
            finished: false,
            keep_alive: true,
            status: 0,
        }
    }
}
impl Response {
    pub fn start(
        &mut self,
        head: &crate::http1::Head,
        length: crate::http1::BodyLength,
    ) -> io::Result<()> {
        if self.encoder.is_some() {
            return Err(io::Error::other("response already started"));
        }
        self.encoder = Some(super::encode_head(head, length, &mut self.output)?);
        self.keep_alive = head.keep_alive && !head.token("connection", "close");
        self.status = head.status;
        Ok(())
    }
    pub fn body(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.encoder
            .as_mut()
            .ok_or_else(|| io::Error::other("response not started"))?
            .body(bytes, &mut self.output)
            .map_err(io::Error::other)
    }
    pub fn finish(&mut self, trailers: &[crate::http1::Header]) -> io::Result<()> {
        self.encoder
            .as_mut()
            .ok_or_else(|| io::Error::other("response not started"))?
            .finish(trailers, &mut self.output)
            .map_err(io::Error::other)?;
        self.finished = true;
        Ok(())
    }
}
/// Serve HTTP/1 keep-alive with streaming request/response callbacks. Idle reads
/// are cancelled on shutdown; an active request finishes before the connection closes.
/// A connection that ends after a response (shutdown, `connection: close` or a
/// non-reusable request) and an idle connection at shutdown close with a lingering
/// close bounded by [`Options::linger_timeout`] and [`Shutdown::stop_by`].
///
/// A request asking to upgrade (or a CONNECT) ends with
/// [`Event::Upgrade`](crate::http1::Event::Upgrade) in place of `End`. A service
/// that declines answers normally and the connection stays HTTP/1. This driver
/// cannot hand the transport over, so a `101` (or a `2xx` to CONNECT) closes the
/// connection after the response. Serve upgrades from [`super::Http1`] directly.
pub async fn http1<S: HalfClose>(
    stream: S,
    shutdown: Shutdown,
    mut service: impl FnMut(crate::http1::Event<'_>, &mut Response) -> io::Result<()>,
) -> io::Result<()> {
    let mut conn = super::Http1::new(stream, crate::http1::Mode::Request);
    let mut response = Response::default();
    loop {
        // Wait for the next request outside the codec: a cancelled codec read
        // releases the transport, while an idle connection at shutdown lingers.
        while conn.input.is_empty() && !shutdown.is_stopped() {
            let stream = conn.stream.as_mut().ok_or_else(super::closed)?;
            match shutdown.until(super::append(stream, &mut conn.input)).await {
                // EOF: the codec below reports a clean or truncated end as before.
                None | Some(Ok(0)) => break,
                Some(read) => {
                    read?;
                }
            }
        }
        let Some(head) = shutdown.until(conn.head()).await else {
            // Stopped before this request started. A stop that interrupted a
            // partial head has already released the transport.
            linger(&mut conn.stream, &mut conn.input, &shutdown).await;
            return Ok(());
        };
        let head = head?;
        let connect = head.method == "CONNECT";
        service(crate::http1::Event::Head(head), &mut response)?;
        turnloop_io::write_all(
            conn.stream.as_mut().ok_or_else(super::closed)?,
            &response.output,
        )
        .await?;
        response.output.clear();
        let mut upgrade = false;
        loop {
            let mut ended = false;
            let received = conn
                .event(|event| {
                    // An upgrade request ends like any other; the service
                    // decides whether to switch by the status it answers.
                    upgrade = matches!(event, crate::http1::Event::Upgrade);
                    ended = upgrade || matches!(event, crate::http1::Event::End);
                    service(event, &mut response)
                })
                .await?;
            turnloop_io::write_all(
                conn.stream.as_mut().ok_or_else(super::closed)?,
                &response.output,
            )
            .await?;
            response.output.clear();
            if !received {
                return Err(io::Error::other("service must handle upgrades explicitly"));
            }
            if ended {
                break;
            }
        }
        if !response.finished {
            return Err(io::Error::other("service did not finish response"));
        }
        // This driver has no handoff, so an accepted upgrade ends the connection
        // rather than parsing the next protocol's bytes as HTTP/1.
        let switched =
            upgrade && (response.status == 101 || connect && (200..300).contains(&response.status));
        if shutdown.is_stopped() || switched || !conn.reusable() || !response.keep_alive {
            linger(&mut conn.stream, &mut conn.input, &shutdown).await;
            return Ok(());
        }
        conn.reset()?;
        response.encoder = None;
        response.finished = false;
    }
}
/// Serve HTTP/2 and flush control frames even on protocol failure. Services own
/// stream-level application state and release capacity after consuming DATA.
/// Shutdown sends GOAWAY and continues processing existing streams until drained;
/// a drained connection then closes with a lingering close (see [`http1`]).
pub async fn http2<S: HalfClose>(
    stream: S,
    shutdown: Shutdown,
    mut service: impl FnMut(&mut crate::http2::Connection, crate::http2::Event<'_>) -> io::Result<()>,
) -> io::Result<()> {
    let mut conn = super::Http2::new(stream, crate::http2::Role::Server)?;
    conn.shutdown = Some(shutdown.clone());
    loop {
        if conn.core.is_drained() {
            conn.flush().await?;
            linger(&mut conn.stream, &mut conn.input, &shutdown).await;
            return Ok(());
        }
        if !conn.event(&mut service).await? {
            return Ok(());
        }
    }
}
