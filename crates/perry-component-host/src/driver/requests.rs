use super::Reply;
use crate::InvocationId;
use anyhow::{ensure, Context, Result};
use futures_channel::{mpsc, oneshot};
use futures_util::task::AtomicWaker;
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context as TaskContext, Poll},
};
use wasmtime::component::Func;

type ReadKey = (bool, String);
type ReadRegistrations = BTreeMap<ReadKey, BTreeMap<InvocationId, Weak<ReadIntent>>>;

pub(super) struct State {
    pub owner: std::thread::ThreadId,
    pub alive: Cell<bool>,
    pub abort: Cell<bool>,
    pub active: Cell<bool>,
    pub outstanding: Cell<usize>,
    pub waker: AtomicWaker,
    replies: RefCell<BTreeMap<InvocationId, oneshot::Sender<Reply>>>,
    reads: RefCell<ReadRegistrations>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            owner: std::thread::current().id(),
            alive: Cell::new(true),
            abort: Cell::new(false),
            active: Cell::new(false),
            outstanding: Cell::new(0),
            waker: AtomicWaker::new(),
            replies: RefCell::new(BTreeMap::new()),
            reads: RefCell::new(BTreeMap::new()),
        }
    }
}
impl State {
    pub fn fail_all(&self, message: &str) {
        let replies = std::mem::take(&mut *self.replies.borrow_mut());
        for reply in replies.into_values() {
            let _ = reply.send(Err(anyhow::anyhow!("{message}")));
        }
    }
    pub fn assert_owner(&self) {
        assert_eq!(
            self.owner,
            std::thread::current().id(),
            "component drivers belong to their creating thread"
        );
    }
}

pub(super) struct Permit(Rc<State>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.outstanding.set(self.0.outstanding.get() - 1);
    }
}

pub(super) struct ReplyPort {
    state: Weak<State>,
    origin: InvocationId,
}
impl ReplyPort {
    pub fn send(self, value: Reply) {
        if let Some(state) = self.state.upgrade() {
            let reply = state.replies.borrow_mut().remove(&self.origin);
            if let Some(reply) = reply {
                let _ = reply.send(value);
            }
        }
    }
}

/// Shared native intent is visible before the close command can be polled.
/// Keep registrations weak so neither completed reads nor clients form a cycle.
pub(super) struct ReadIntent {
    pub cancelled: Cell<bool>,
    pub committed: Cell<bool>,
    pub closed: Cell<bool>,
    key: ReadKey,
    origin: InvocationId,
    state: Weak<State>,
}
impl Drop for ReadIntent {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let mut reads = state.reads.borrow_mut();
            if let Some(entries) = reads.get_mut(&self.key) {
                entries.remove(&self.origin);
                if entries.is_empty() {
                    reads.remove(&self.key);
                }
            }
        }
    }
}

#[derive(Clone)]
pub(super) enum Target {
    Export,
    Read(Value, u32, Option<Rc<ReadIntent>>),
    Close(Value, Vec<Rc<ReadIntent>>),
}
pub(super) struct Request {
    pub target: Target,
    permit: Rc<Permit>,
    pub origin: InvocationId,
    pub interface: String,
    pub name: String,
    pub args: Vec<Value>,
    pub reply: ReplyPort,
    pub started: Rc<Cell<bool>>,
    pub cancelled: Rc<Cell<bool>>,
}
pub(super) struct Command {
    pub target: Target,
    _permit: Rc<Permit>,
    pub origin: InvocationId,
    pub function: Option<Func>,
    pub args: Vec<Value>,
    pub reply: ReplyPort,
    pub started: Rc<Cell<bool>>,
    pub cancelled: Rc<Cell<bool>>,
}
impl Command {
    pub fn new(request: Request, function: Option<Func>) -> Self {
        Self {
            _permit: request.permit,
            target: request.target,
            origin: request.origin,
            function,
            args: request.args,
            reply: request.reply,
            started: request.started,
            cancelled: request.cancelled,
        }
    }
}

/// Cloneable submission endpoint. All methods and callbacks stay on its thread.
#[derive(Clone)]
pub struct DriverClient {
    send: mpsc::UnboundedSender<Request>,
    state: Rc<State>,
}
impl DriverClient {
    pub(super) fn new(send: mpsc::UnboundedSender<Request>, state: Rc<State>) -> Self {
        Self { send, state }
    }
    /// Queue an owned argument snapshot without running guest or callback code.
    /// At most 64 unconsumed calls may exist at once. Poll the driver to execute.
    pub fn call(&self, interface: &str, name: &str, args: &[Value]) -> Result<DriverCall> {
        self.submit(interface, name, args, Target::Export)
    }
    /// Read a Store-local future or stream descriptor, including nested values.
    /// Futures yield one payload (None for void); streams yield
    /// {done:false,value:[...]} chunks and finally {done:true}. A future is
    /// consumed by its read; a stream keeps its descriptor until EOF or close.
    /// Capacity defaults to 1 for futures and 4096 for streams (maximum 65536).
    /// Only one read may be pending per descriptor. A rejected preflight leaves
    /// it usable; dropping a started read's observer retires the Store.
    pub fn read_async_value(&self, value: &Value, max_elements: Option<u32>) -> Result<DriverCall> {
        let (_, stream) = crate::async_values::values::token(value)?;
        let max = max_elements.unwrap_or(if stream { 4096 } else { 1 });
        ensure!(
            (1..=65536).contains(&max) && (stream || max == 1),
            "invalid async read capacity"
        );
        self.submit("", "", &[], Target::Read(value.clone(), max, None))
    }
    /// Close an owned descriptor or cancel its pending read without retiring
    /// the Store. Accepted closes execute even if their observer is dropped.
    /// Queuing close before a pending read commits prevents payload publication
    /// and disposes copied owned values using their original resource types.
    pub fn close_async_value(&self, value: &Value) -> Result<DriverCall> {
        crate::async_values::values::token(value)?;
        self.submit("", "", &[], Target::Close(value.clone(), Vec::new()))
    }
    fn submit(
        &self,
        interface: &str,
        name: &str,
        args: &[Value],
        mut target: Target,
    ) -> Result<DriverCall> {
        self.state.assert_owner();
        ensure!(
            self.state.alive.get() && !self.state.abort.get(),
            "component driver is retired"
        );
        ensure!(
            self.state.outstanding.get() < 64,
            "too many pending component calls"
        );
        let origin = InvocationId::fresh()?;
        let (reply, receive) = oneshot::channel();
        self.state.replies.borrow_mut().insert(origin, reply);
        let reply = ReplyPort {
            state: Rc::downgrade(&self.state),
            origin,
        };
        self.state.outstanding.set(self.state.outstanding.get() + 1);
        let permit = Rc::new(Permit(self.state.clone()));
        let started = Rc::new(Cell::new(false));
        let cancelled = Rc::new(Cell::new(false));
        match &mut target {
            Target::Read(value, _, slot) => {
                let (token, stream) = crate::async_values::values::token(value)?;
                let key = (stream, token.to_string());
                let intent = Rc::new(ReadIntent {
                    cancelled: Cell::new(false),
                    committed: Cell::new(false),
                    closed: Cell::new(false),
                    key: key.clone(),
                    origin,
                    state: Rc::downgrade(&self.state),
                });
                self.state
                    .reads
                    .borrow_mut()
                    .entry(key)
                    .or_default()
                    .insert(origin, Rc::downgrade(&intent));
                *slot = Some(intent);
            }
            Target::Close(value, intents) => {
                let (token, stream) = crate::async_values::values::token(value)?;
                if let Some(entries) = self.state.reads.borrow().get(&(stream, token.to_string())) {
                    for intent in entries.values().filter_map(Weak::upgrade) {
                        if !intent.committed.get() {
                            intent.cancelled.set(true);
                            intents.push(intent);
                        }
                    }
                }
            }
            Target::Export => {}
        }

        let close = matches!(target, Target::Close(..));
        self.send
            .unbounded_send(Request {
                permit: permit.clone(),
                target,
                origin,
                interface: interface.into(),
                name: name.into(),
                args: args.to_vec(),
                reply,
                started: started.clone(),
                cancelled: cancelled.clone(),
            })
            .map_err(|_| {
                self.state.replies.borrow_mut().remove(&origin);
                anyhow::anyhow!("component driver is retired")
            })?;
        self.state.active.set(true);
        self.state.waker.wake();
        Ok(DriverCall {
            _permit: Some(permit),
            origin,
            receive,
            state: self.state.clone(),
            started,
            cancelled,
            complete: false,
            close,
        })
    }
}
impl Drop for DriverClient {
    fn drop(&mut self) {
        self.state.waker.wake();
    }
}

/// Future for one export, read or close result. It does not own background guest work.
/// Dropping it before execution skips the call; after execution starts, dropping
/// an unobserved result requests hard Store retirement on the next driver poll.
/// Accepted close requests always execute, even if their observer is dropped.
pub struct DriverCall {
    _permit: Option<Rc<Permit>>,
    origin: InvocationId,
    receive: oneshot::Receiver<Reply>,
    state: Rc<State>,
    started: Rc<Cell<bool>>,
    cancelled: Rc<Cell<bool>>,
    complete: bool,
    close: bool,
}
impl DriverCall {
    pub fn id(&self) -> InvocationId {
        self.origin
    }
}
impl Future for DriverCall {
    type Output = Reply;
    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Reply> {
        self.state.assert_owner();
        match Pin::new(&mut self.receive).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.complete = true;
                self._permit.take();
                Poll::Ready(
                    result.context("component driver retired before publishing this result")?,
                )
            }
        }
    }
}
impl Drop for DriverCall {
    fn drop(&mut self) {
        self.state.assert_owner();
        if !self.complete && !self.close {
            self.cancelled.set(true);
            if self.started.get() && self.state.alive.get() {
                self.state.abort.set(true);
            }
            self.state.waker.wake();
        }
    }
}
