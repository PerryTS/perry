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

pub(super) struct State {
    pub owner: std::thread::ThreadId,
    pub alive: Cell<bool>,
    pub abort: Cell<bool>,
    pub active: Cell<bool>,
    pub outstanding: Cell<usize>,
    pub waker: AtomicWaker,
    replies: RefCell<BTreeMap<InvocationId, oneshot::Sender<Reply>>>,
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

pub(super) struct Request {
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
    _permit: Rc<Permit>,
    pub origin: InvocationId,
    pub function: Func,
    pub args: Vec<Value>,
    pub reply: ReplyPort,
    pub started: Rc<Cell<bool>>,
    pub cancelled: Rc<Cell<bool>>,
}
impl Command {
    pub fn new(request: Request, function: Func) -> Self {
        Self {
            _permit: request.permit,
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
        self.send
            .unbounded_send(Request {
                permit: permit.clone(),
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
        })
    }
}
impl Drop for DriverClient {
    fn drop(&mut self) {
        self.state.waker.wake();
    }
}

/// Future for one exported result. It does not own the background guest task.
/// Dropping it before execution skips the call; after execution starts, dropping
/// an unobserved result requests hard Store retirement on the next driver poll.
pub struct DriverCall {
    _permit: Option<Rc<Permit>>,
    origin: InvocationId,
    receive: oneshot::Receiver<Reply>,
    state: Rc<State>,
    started: Rc<Cell<bool>>,
    cancelled: Rc<Cell<bool>>,
    complete: bool,
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
        if !self.complete {
            self.cancelled.set(true);
            if self.started.get() && self.state.alive.get() {
                self.state.abort.set(true);
            }
            self.state.waker.wake();
        }
    }
}
