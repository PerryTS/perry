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

type ReadKey = (u8, String);
struct WriterMeta {
    stream: bool,
    closing: bool,
}
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
    writers: RefCell<BTreeMap<String, WriterMeta>>,
    pub types: Vec<crate::AsyncValueType>,
    retention: crate::asynchronous::retention::Registry,
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
            writers: RefCell::new(BTreeMap::new()),
            types: Vec::new(),
            retention: Default::default(),
        }
    }
}
impl State {
    pub fn new(
        types: Vec<crate::AsyncValueType>,
        retention: crate::asynchronous::retention::Registry,
    ) -> Self {
        Self {
            types,
            retention,
            ..Self::default()
        }
    }
    pub fn register_writer(&self, token: String, stream: bool) {
        self.writers.borrow_mut().insert(
            token,
            WriterMeta {
                stream,
                closing: false,
            },
        );
    }
    pub fn remove_writer(&self, token: &str) {
        self.writers.borrow_mut().remove(token);
    }
    pub fn fail_all(&self, message: &str) {
        self.writers.borrow_mut().clear();
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
    pub close: Cell<bool>,
    pub operation: Cell<Option<u32>>,
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WriterAction {
    Cancel,
    Close,
    Abort,
}

#[derive(Clone)]
pub(super) enum Target {
    Export,
    Factory(crate::AsyncValueType),
    Resource(crate::cleanup::CollectedResource, bool),
    Write(Value, Option<Rc<ReadIntent>>),
    WriterControl(Value, WriterAction, Vec<Rc<ReadIntent>>),
    Read(Value, u32, Option<Rc<ReadIntent>>),
    Close(Value, Vec<Rc<ReadIntent>>),
    CollectAsync(Value, Vec<Rc<ReadIntent>>),
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
    pub fn state(&self) -> &State {
        &self._permit.0
    }
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
    /// Dispose one owned resource through a private canonical helper. Construct
    /// the Host with new_for_driver to enable this operation. An accepted drop
    /// executes even if its observer is discarded; destructor traps retire the
    /// Store, while invalid/borrowed/foreign tokens reject without changing it.
    pub fn drop_resource(&self, value: &Value) -> Result<DriverCall> {
        ensure!(
            value["owned"] == true,
            "cannot dispose a borrowed resource descriptor"
        );
        let guest = value.get("$resource").is_some();
        let host = value.get("$hostResource").is_some();
        ensure!(
            guest != host,
            "expected exactly one resource descriptor kind"
        );
        let request = if guest {
            crate::cleanup::CollectedResource::Guest {
                token: value["$resource"]
                    .as_str()
                    .context("invalid guest resource token")?
                    .into(),
            }
        } else {
            ensure!(
                value["new"] != true,
                "host resource is not registered with the component"
            );
            crate::cleanup::CollectedResource::Host {
                resource: serde_json::from_value(serde_json::json!({
                    "token":value["$hostResource"],"type":value["type"],"generation":value["generation"],
                }))?,
            }
        };
        self.submit("", "", &[], Target::Resource(request, true))
    }
    /// Deferred collector request: {kind:"guest",token} or {kind:"host",resource}.
    /// Ignore stale, transferred and old-generation owners instead of reclaiming
    /// ownership from the guest. Accepted cleanup survives observer drop.
    pub fn collect_resource(&self, request: &Value) -> Result<DriverCall> {
        let request = serde_json::from_value(request.clone())?;
        self.submit("", "", &[], Target::Resource(request, false))
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
    /// Deferred cleanup of a readable future/stream. Ignore a descriptor that
    /// was already consumed, closed or transferred. Live pending reads cancel
    /// using the same ownership protocol as close_async_value.
    pub fn collect_async_value(&self, value: &Value) -> Result<DriverCall> {
        crate::async_values::values::token(value)?;
        self.submit("", "", &[], Target::CollectAsync(value.clone(), Vec::new()))
    }
    /// IDs whose callback context may still be needed on later driver polls.
    /// Includes queued/unpublished requests, conservative guest-root bindings
    /// and captured host futures. Completed root bindings can remain until an
    /// engine task ID is reused or the Store becomes quiescent; this is not a
    /// per-task exit notification. Retain separate context for results not yet
    /// consumed by the embedding. Store retirement clears guest roots;
    /// deferred callback leases remain until their cleanup has finished.
    pub fn retained_origins(&self) -> Vec<InvocationId> {
        self.state.assert_owner();
        let mut origins = self.state.retention.snapshot();
        origins.extend(self.state.replies.borrow().keys().copied());
        origins.into_iter().collect()
    }
    /// Factory types are branded to this component and retain WIT payload aliases.
    pub fn async_value_types(&self) -> &[crate::AsyncValueType] {
        self.state.assert_owner();
        &self.state.types
    }
    /// Create a readable descriptor and a private owned writer descriptor:
    /// {value: future_or_stream, writer: writer_descriptor}.
    pub fn create_async_value(&self, ty: &crate::AsyncValueType) -> Result<DriverCall> {
        self.state.assert_owner();
        ensure!(
            self.state.types.contains(ty),
            "async factory type belongs to another component"
        );
        self.submit("", "", &[], Target::Factory(ty.clone()))
    }
    fn writer(&self, value: &Value) -> Result<bool> {
        self.state.assert_owner();
        let token = crate::async_values::writers::token(value)?;
        let writers = self.state.writers.borrow();
        let writer = writers
            .get(token)
            .context("writer is closed or belongs to another component")?;
        ensure!(!writer.closing, "writer is closing or being aborted");
        Ok(writer.stream)
    }
    /// Write a bounded list. A future requires one element; void/unit payloads
    /// use null placeholders. Returns {written,remaining,cancelled,closed}.
    /// Ownership transfers only after all values pass preflight. A partial or
    /// cancelled write returns its remaining values. After canonical transfer,
    /// those values receive fresh owned descriptors.
    pub fn write_async_value(&self, writer: &Value, values: &[Value]) -> Result<DriverCall> {
        let stream = self.writer(writer)?;
        ensure!(
            (1..=65536).contains(&values.len()) && (stream || values.len() == 1),
            "invalid async write count"
        );
        self.submit("", "", values, Target::Write(writer.clone(), None))
    }
    /// Cancel only writes already submitted; retain the writer for retry if
    /// it has not completed. Await the write to recover its unconsumed values.
    pub fn cancel_async_write(&self, writer: &Value) -> Result<DriverCall> {
        self.writer(writer)?;
        self.submit(
            "",
            "",
            &[],
            Target::WriterControl(writer.clone(), WriterAction::Cancel, Vec::new()),
        )
    }
    /// Close a stream writer, cancelling pending writes and disposing their
    /// unconsumed values. Accepted close requests survive observer drop.
    pub fn close_async_writer(&self, writer: &Value) -> Result<DriverCall> {
        ensure!(
            self.writer(writer)?,
            "an unwritten future writer must be completed or explicitly aborted"
        );
        self.submit(
            "",
            "",
            &[],
            Target::WriterControl(writer.clone(), WriterAction::Close, Vec::new()),
        )
    }
    /// Abort a producer by retiring its Store and cancelling sibling guest
    /// work. This is the explicit failure path for an unwritten future writer.
    pub fn abort_async_writer(&self, writer: &Value) -> Result<DriverCall> {
        self.writer(writer)?;
        self.submit(
            "",
            "",
            &[],
            Target::WriterControl(writer.clone(), WriterAction::Abort, Vec::new()),
        )
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
            Target::Read(value, _, slot) | Target::Write(value, slot) => {
                let key = if value.get("$writer").is_some() {
                    (2, crate::async_values::writers::token(value)?.to_string())
                } else {
                    let (token, stream) = crate::async_values::values::token(value)?;
                    (u8::from(stream), token.to_string())
                };
                let intent = Rc::new(ReadIntent {
                    cancelled: Cell::new(false),
                    committed: Cell::new(false),
                    closed: Cell::new(false),
                    close: Cell::new(false),
                    operation: Cell::new(None),
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
            Target::Close(value, intents) | Target::CollectAsync(value, intents) => {
                let (token, stream) = crate::async_values::values::token(value)?;
                if let Some(entries) = self
                    .state
                    .reads
                    .borrow()
                    .get(&(u8::from(stream), token.to_string()))
                {
                    for intent in entries.values().filter_map(Weak::upgrade) {
                        if !intent.committed.get() {
                            intent.cancelled.set(true);
                            intents.push(intent);
                        }
                    }
                }
            }
            Target::WriterControl(value, action, intents) => {
                let token = crate::async_values::writers::token(value)?;
                if let Some(entries) = self.state.reads.borrow().get(&(2, token.to_string())) {
                    for intent in entries.values().filter_map(Weak::upgrade) {
                        if !intent.committed.get() {
                            if *action != WriterAction::Abort {
                                intent.cancelled.set(true);
                                intent.close.set(*action == WriterAction::Close);
                            }
                            intents.push(intent);
                        }
                    }
                }
                if *action != WriterAction::Cancel {
                    self.state
                        .writers
                        .borrow_mut()
                        .get_mut(token)
                        .context("writer disappeared")?
                        .closing = true;
                }
            }
            Target::Export | Target::Factory(_) | Target::Resource(..) => {}
        }

        let close = matches!(
            target,
            Target::Close(..)
                | Target::CollectAsync(..)
                | Target::WriterControl(..)
                | Target::Resource(..)
        );
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
