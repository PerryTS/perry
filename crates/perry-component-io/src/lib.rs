//! Native-only I/O boundary for component providers.
//!
//! Wasmtime resources must be Send, while Turnloop's executor and I/O futures
//! belong to one thread. A driver owns that thread; submitted factories create
//! their local futures there. Only owned native inputs/results cross the channel.
//! No Perry heap pointer, JavaScript value, callback or Wasmtime Store may cross
//! this boundary. Guest code and canonical lifting stay on the calling agent.
//!
//! The work limit includes queued and running tasks. Dropping a Request cancels
//! its future and native I/O; dropping the Driver cancels every task and
//! joins its owner. A resource's task lifetime is therefore explicit, with no
//! process-global runtime or detached futures.
use futures_channel::oneshot;
use futures_util::future::{AbortHandle, Abortable};
use futures_util::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
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

type Factory = Box<dyn FnOnce(&LocalExecutor<Platform>) -> Option<JoinHandle<()>> + Send>;
enum Command {
    Submit(Factory),
    Stop,
}
struct Inner {
    send: mpsc::Sender<Command>,
    notify: Notifier,
    live: Arc<AtomicUsize>,
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
        let (send, receive) = mpsc::channel();
        let (ready_send, ready_receive) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("perry-component-io".into())
            .spawn(move || {
                let executor = LocalExecutor::<Platform>::with_config(
                    Config::default(),
                    ExecutorConfig {
                        tasks: limit,
                        ..ExecutorConfig::default()
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
                let mut tasks = Vec::with_capacity(limit);
                'drive: loop {
                    tasks.retain(|task: &JoinHandle<()>| !task.is_finished());
                    if tasks.is_empty() && !executor.alive() {
                        match receive.recv() {
                            Ok(Command::Submit(make)) => {
                                if let Some(task) = make(&executor) {
                                    tasks.push(task);
                                }
                            }
                            _ => break,
                        }
                    }
                    // Bounded by the admission count, not by an unbounded producer.
                    for _ in 0..limit {
                        match receive.try_recv() {
                            Ok(Command::Submit(make)) => {
                                if let Some(task) = make(&executor) {
                                    tasks.push(task);
                                }
                            }
                            Ok(Command::Stop) | Err(mpsc::TryRecvError::Disconnected) => {
                                break 'drive
                            }
                            Err(mpsc::TryRecvError::Empty) => break,
                        }
                    }
                    if executor.turn(Timeout::Forever).is_err() {
                        break;
                    }
                }
                // Join handles cancel tasks before the executor quiesces its native
                // requests and frees their buffers. Queued factories are dropped too.
                drop(tasks);
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

    pub fn outstanding(&self) -> usize {
        self.0.live.load(Ordering::Acquire)
    }
}
impl Submitter {
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
        if self.stopped.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        self.live
            .try_update(Ordering::AcqRel, Ordering::Acquire, |live| {
                (live < self.limit).then_some(live + 1)
            })
            .map_err(|_| Error::Capacity)?;
        let permit = Permit(self.live.clone());
        let (send, receive) = oneshot::channel();
        let (abort, registration) = AbortHandle::new_pair();
        let factory: Factory = Box::new(move |executor| {
            let handle = executor.handle();
            let future = async move {
                // Invoke the factory inside the caught/abortable future too.
                let future = async move { make(handle).await };
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
impl<T> Request<T> {
    pub fn cancel(&self) {
        self.abort.abort();
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

#[cfg(test)]
mod tests;
