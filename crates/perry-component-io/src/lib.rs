//! Native-only I/O boundary for component providers.
//!
//! Wasmtime resources must be Send, while Turnloop's executor and I/O futures
//! belong to one thread. A driver owns that thread; submitted factories create
//! their local futures there. Only owned native inputs/results cross the channel.
//! No Perry heap pointer, JavaScript value, callback or Wasmtime Store may cross
//! this boundary. Guest code and canonical lifting stay on the calling agent.
//!
//! The work limit includes queued and running tasks. Dropping a Request cancels
//! its future; native I/O follows the lifetime of its owners. A separate resource
//! budget retains persistent non-Send values between calls through Send handles.
//! Each live resource also reserves two bounded cleanup tasks so teardown can
//! progress when ordinary tasks are saturated. Cleanup retains its resource.
//! Dropping the Driver cancels every task, releases the native resources and
//! joins its owner, even when resource handles remain on the calling thread.
//! There is no process-global runtime or detached provider future.
use futures_channel::oneshot;
use futures_util::future::{AbortHandle, Abortable};
use futures_util::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::thread;
use turnloop::backend::Platform;
use turnloop::{
    Config, ExecutorConfig, ExecutorHandle, JoinHandle, LocalExecutor, Notifier, Timeout,
};

/// Failure of the provider task itself. The operation's typed result is separate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Cancelled,
    Stopped,
    Capacity,
    /// Only recoverable in panic=unwind builds; shipping Perry uses panic=abort.
    Panicked,
    Driver(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "component I/O: {self:?}")
    }
}
impl std::error::Error for Error {}

mod resource;
pub use resource::{LocalResource, Resource, ResourceRequest};

type Factory = Box<
    dyn FnOnce(&LocalExecutor<Platform>, &Rc<resource::Resources>) -> Option<JoinHandle<()>> + Send,
>;
enum Command {
    Submit(Factory),
    Release(u64),
    Stop,
}
struct Inner {
    send: mpsc::Sender<Command>,
    notify: Notifier,
    live: Arc<AtomicUsize>,
    resources: Arc<AtomicUsize>,
    stopped: AtomicBool,
    limit: usize,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.shutdown();
    }
}
impl Inner {
    fn shutdown(&self) {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.send.send(Command::Stop);
        let _ = self.notify.notify();
        let worker = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(worker) = worker {
            // Factories must not capture the Driver. In particular its final
            // owner must never be a task running on the driver itself.
            if worker.thread().id() != thread::current().id() {
                let _ = worker.join();
            }
        }
    }
}

/// I/O lifetime owner, held by the component context. Resources use Submitter
/// instead: their tasks must not retain this owner and prevent its shutdown.
pub struct Driver(Arc<Inner>);
impl Drop for Driver {
    fn drop(&mut self) {
        // A Submitter may hold a temporary strong reference during submission.
        // The context still owns teardown: do not wait for that reference to
        // become the final Arc before stopping and joining the native thread.
        self.0.shutdown();
    }
}
/// A resource's submission endpoint does not prolong the context's lifetime.
#[derive(Clone)]
pub struct Submitter(Weak<Inner>);
impl Driver {
    pub fn new(limit: usize) -> Result<Self, Error> {
        if limit == 0 {
            return Err(Error::Capacity);
        }
        let executor_limit = limit.checked_mul(3).ok_or(Error::Capacity)?;
        // Persistent transports can retain cancelled read/write slots until
        // native acknowledgement. Bound staging by this context's capacity,
        // with spare slots for those acknowledgements and teardown operations.
        let operation_limit = limit.checked_mul(4).ok_or(Error::Capacity)?.max(16);
        let (send, receive) = mpsc::channel();
        let (ready_send, ready_receive) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("perry-component-io".into())
            .spawn(move || {
                let executor = LocalExecutor::<Platform>::with_config(
                    Config::default(),
                    ExecutorConfig {
                        tasks: executor_limit,
                        operations: operation_limit,
                        buffer_size: 64 * 1024,
                    },
                );
                let mut executor = match executor {
                    Ok(executor) => executor,
                    Err(error) => {
                        let _ = ready_send.send(Err(Error::Driver(error.to_string())));
                        return;
                    }
                };
                let notify = executor.driver().notifier();
                if ready_send.send(Ok(notify)).is_err() {
                    return;
                }
                let mut tasks = Vec::with_capacity(executor_limit);
                let resources = Rc::new(resource::Resources::default());
                'drive: loop {
                    tasks.retain(|task: &JoinHandle<()>| !task.is_finished());
                    if tasks.is_empty() && !executor.alive() {
                        match receive.recv() {
                            Ok(Command::Submit(make)) => {
                                if let Some(task) = make(&executor, &resources) {
                                    tasks.push(task);
                                }
                            }
                            Ok(Command::Release(id)) => resources.remove(id),
                            _ => break,
                        }
                    }
                    // Task and resource admission bound the queue. A full batch
                    // may leave releases/submissions after a coalesced wake, so
                    // never park until a drain has actually observed it empty.
                    let mut queue_may_have_work = true;
                    for _ in 0..limit {
                        match receive.try_recv() {
                            Ok(Command::Submit(make)) => {
                                if let Some(task) = make(&executor, &resources) {
                                    tasks.push(task);
                                }
                            }
                            Ok(Command::Release(id)) => resources.remove(id),
                            Ok(Command::Stop) | Err(mpsc::TryRecvError::Disconnected) => {
                                break 'drive
                            }
                            Err(mpsc::TryRecvError::Empty) => {
                                queue_may_have_work = false;
                                break;
                            }
                        }
                    }
                    let timeout = if queue_may_have_work {
                        Timeout::Now
                    } else {
                        Timeout::Forever
                    };
                    if executor.turn(timeout).is_err() {
                        break;
                    }
                }
                // Join handles cancel tasks before the executor quiesces its native
                // requests and frees their buffers. Queued factories are dropped too.
                drop(tasks);
                resources.clear();
                drop(executor);
            })
            .map_err(|error| Error::Driver(error.to_string()))?;
        let notify = match ready_receive.recv() {
            Ok(Ok(notify)) => notify,
            other => {
                let _ = worker.join();
                return Err(other.ok().and_then(Result::err).unwrap_or(Error::Stopped));
            }
        };
        Ok(Self(Arc::new(Inner {
            send,
            notify,
            live: Arc::new(AtomicUsize::new(0)),
            resources: Arc::new(AtomicUsize::new(0)),
            stopped: AtomicBool::new(false),
            limit,
            worker: Mutex::new(Some(worker)),
        })))
    }

    /// Construct a local I/O future on the owning thread. Factories and results
    /// must be Send; the future itself may own Turnloop's non-Send adapters.
    /// Never block this thread or capture the Driver in a task. Use `submitter`
    /// for nested submissions, and retain/cancel all resulting Requests.
    pub fn submit<F, Fut, T>(&self, make: F) -> Result<Request<T>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        self.0.submit(make)
    }

    pub fn submitter(&self) -> Submitter {
        Submitter(Arc::downgrade(&self.0))
    }

    /// Native resources, including pending registrations and in-flight owners.
    /// Limited independently of the queued/running task count.
    pub fn resources(&self) -> usize {
        self.0.resources.load(Ordering::Acquire)
    }

    pub fn outstanding(&self) -> usize {
        self.0.live.load(Ordering::Acquire)
    }
}
impl Submitter {
    /// Run owned native work on Turnloop's bounded blocking pool. Cancellation
    /// stops result delivery and cancels queued work; a syscall already running
    /// may finish and retain its owned inputs until then. It cannot be undone.
    /// Never pass guest heap pointers, callbacks, or a Wasmtime Store here.
    pub fn blocking<F, T>(&self, work: F) -> Result<BlockingRequest<T>, Error>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        self.submit(move |io| async move {
            let payload = io
                .blocking(move || Ok(turnloop::Payload::Boxed(Box::new(work()))))
                .await
                .map_err(|e| Error::Driver(e.to_string()))?;
            match payload {
                turnloop::Payload::Boxed(value) => value
                    .downcast::<T>()
                    .map(|value| *value)
                    .map_err(|_| Error::Driver("blocking result type mismatch".into())),
                _ => Err(Error::Driver("unexpected blocking result payload".into())),
            }
        })
        .map(BlockingRequest)
    }

    pub fn submit<F, Fut, T>(&self, make: F) -> Result<Request<T>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        self.0.upgrade().ok_or(Error::Stopped)?.submit(make)
    }
}
impl Inner {
    fn submit<F, Fut, T>(&self, make: F) -> Result<Request<T>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        self.submit_with_resources(move |io, _| make(io))
    }

    fn submit_with_resources<F, Fut, T>(&self, make: F) -> Result<Request<T>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>, Rc<resource::Resources>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        if self.stopped.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        self.live
            .try_update(Ordering::AcqRel, Ordering::Acquire, |live| {
                (live < self.limit).then_some(live + 1)
            })
            .map_err(|_| Error::Capacity)?;
        self.submit_admitted(Permit(self.live.clone()), make)
    }

    fn submit_admitted<F, Fut, T>(&self, permit: Permit, make: F) -> Result<Request<T>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>, Rc<resource::Resources>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + 'static,
        T: Send + 'static,
    {
        if self.stopped.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        let (send, receive) = oneshot::channel();
        let (abort, registration) = AbortHandle::new_pair();
        let factory: Factory = Box::new(move |executor, resources| {
            let resources = resources.clone();
            let handle = executor.handle();
            let future = async move {
                // Invoke the factory inside the caught/abortable future too.
                let future = async move { make(handle, resources).await };
                let result = AssertUnwindSafe(Abortable::new(future, registration))
                    .catch_unwind()
                    .await;
                let result = match result {
                    Ok(Ok(value)) => Ok(value),
                    Ok(Err(_)) => Err(Error::Cancelled),
                    Err(_) => Err(Error::Panicked),
                };
                drop(permit);
                let _ = send.send(result);
            };
            executor.spawn_local(future).ok()
        });
        // Own cancellation before either fallible submission step, including
        // notification after the command has already entered the queue.
        let request = Request { receive, abort };
        self.send
            .send(Command::Submit(factory))
            .map_err(|_| Error::Stopped)?;
        self.notify
            .notify()
            .map_err(|e| Error::Driver(e.to_string()))?;
        Ok(request)
    }
}
struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Cancels its task on drop. Await cancellation to observe that the future and
/// its native resource owners were dropped, rather than just requesting it.
pub struct Request<T> {
    receive: oneshot::Receiver<Result<T, Error>>,
    abort: AbortHandle,
}
/// A cancellation signal for a request whose result future is shared elsewhere.
/// Signalling does not acknowledge teardown; await the request for that.
#[derive(Clone)]
pub struct Cancellation(AbortHandle);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.abort();
    }
}
impl<T> Request<T> {
    pub fn cancel(&self) {
        self.abort.abort();
    }

    pub fn cancellation(&self) -> Cancellation {
        Cancellation(self.abort.clone())
    }
}
impl<T> Future for Request<T> {
    type Output = Result<T, Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.receive).poll(cx) {
            Poll::Ready(Ok(value)) => Poll::Ready(value),
            Poll::Ready(Err(_)) => Poll::Ready(Err(Error::Stopped)),
            Poll::Pending => Poll::Pending,
        }
    }
}
impl<T> Drop for Request<T> {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

/// A cancellable blocking-pool result. Dropping it never borrows guest memory
/// and does not wait for a syscall already running on a native worker.
pub struct BlockingRequest<T>(Request<Result<T, Error>>);
impl<T> BlockingRequest<T> {
    pub fn cancel(&self) {
        self.0.cancel();
    }
}
impl<T> Future for BlockingRequest<T> {
    type Output = Result<T, Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0)
            .poll(cx)
            .map(|result| result.and_then(|v| v))
    }
}

#[cfg(test)]
mod tests;

mod embedding;
#[cfg(any(feature = "standalone", test))]
pub use embedding::standalone;
pub use embedding::{Embedding, Provider, TOKEN};
