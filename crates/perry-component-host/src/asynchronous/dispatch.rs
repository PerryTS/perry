//! Scoped host jobs run on the embedding stack after Wasmtime yields its fiber.
//! No JS callback may execute on a Wasmtime stack: Perry's stack guards and
//! exact compiled-frame root walker describe the embedding stack.
use futures_channel::oneshot;
use std::{
    collections::BTreeMap,
    future::Future,
    pin::pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::Poll,
};
type Job = Box<dyn FnOnce() + Send>;
pub(crate) struct Dispatcher {
    owner: std::thread::ThreadId,
    next: AtomicU64,
    jobs: Mutex<BTreeMap<u64, Job>>,
}
impl Default for Dispatcher {
    fn default() -> Self {
        Self {
            owner: std::thread::current().id(),
            next: AtomicU64::new(0),
            jobs: Mutex::default(),
        }
    }
}
struct Registration<'a>(&'a Dispatcher, u64);
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.0.jobs.lock().unwrap().remove(&self.1);
    }
}
impl Dispatcher {
    /// The job can borrow a suspended Wasmtime host call. Its registration is
    /// dropped before that borrow ends, including on fiber cancellation.
    pub async fn run<R: Send>(&self, job: impl FnOnce() -> R + Send) -> wasmtime::Result<R> {
        assert_eq!(
            self.owner,
            std::thread::current().id(),
            "host jobs belong to their embedding thread"
        );
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        assert!(id != u64::MAX, "host dispatch identity exhausted");
        let (send, receive) = oneshot::channel();
        let registration = Registration(self, id);
        let job: Box<dyn FnOnce() + Send + '_> = Box::new(move || {
            let _ = send.send(job());
        });
        // SAFETY: this future owns Registration, which removes the erased
        // closure before its captured borrows can end. drive runs jobs only
        // after the owning future returned Pending, on the same thread. A job
        // is removed from the queue before execution; reentry into the owning
        // Store is rejected by Bridge::entered. Dispatcher is crate-private.
        let job: Job = unsafe { std::mem::transmute(job) };
        self.jobs.lock().unwrap().insert(id, job);
        let result = receive.await;
        drop(registration);
        result.map_err(|_| {
            wasmtime::Error::from_anyhow(anyhow::anyhow!("component host job was cancelled"))
        })
    }

    pub async fn drive<R, F: Future<Output = wasmtime::Result<R>>>(
        &self,
        future: F,
    ) -> wasmtime::Result<R> {
        let mut future = pin!(future);
        std::future::poll_fn(|cx| {
            assert_eq!(
                self.owner,
                std::thread::current().id(),
                "host jobs must run on their embedding thread"
            );
            for _ in 0..64 {
                let result = future.as_mut().poll(cx);
                let jobs = std::mem::take(&mut *self.jobs.lock().unwrap());
                if jobs.is_empty() {
                    return result;
                }
                if let Poll::Ready(result) = result {
                    // A trap in a different guest task can finish the outer
                    // future while a host job is queued. Cancel its borrowed
                    // closure before the Store is retired; never run JS after
                    // that failure or panic instead of reporting the trap.
                    drop(jobs);
                    return Poll::Ready(result.and_then(|_| {
                        Err(wasmtime::Error::from_anyhow(anyhow::anyhow!(
                            "component returned with unfinished host jobs"
                        )))
                    }));
                }
                for job in jobs.into_values() {
                    job();
                }
            }
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await
    }
}

pub(crate) async fn drive<R, F: Future<Output = wasmtime::Result<R>>>(
    dispatcher: Option<&Arc<Dispatcher>>,
    future: F,
) -> wasmtime::Result<R> {
    match dispatcher {
        Some(dispatcher) => dispatcher.drive(future).await,
        None => future.await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_lite::future::{block_on, poll_once};
    #[test]
    fn borrowed_jobs_run_after_yield_and_cancel_before_borrowed_state_dies() {
        let dispatcher = Dispatcher::default();
        let mut value = 7;
        assert_eq!(
            block_on(dispatcher.drive(dispatcher.run(|| {
                value += 1;
                value
            })))
            .unwrap(),
            8
        );
        assert_eq!(value, 8);
        let mut call = Box::pin(dispatcher.run(|| {
            value += 100;
        }));
        assert!(block_on(poll_once(call.as_mut())).is_none());
        assert_eq!(dispatcher.jobs.lock().unwrap().len(), 1);
        drop(call);
        assert!(dispatcher.jobs.lock().unwrap().is_empty());
        assert_eq!(value, 8, "cancelled borrowed job must never execute");
    }

    #[test]
    fn trap_cancels_queued_borrowed_jobs_without_replacing_the_error() {
        let dispatcher = Dispatcher::default();
        let mut called = false;
        let mut child = Box::pin(dispatcher.run(|| {
            called = true;
        }));
        let result = block_on(dispatcher.drive(async {
            assert!(poll_once(child.as_mut()).await.is_none());
            Err::<(), _>(wasmtime::Error::from_anyhow(anyhow::anyhow!(
                "primary guest trap"
            )))
        }));
        assert_eq!(result.unwrap_err().to_string(), "primary guest trap");
        assert!(dispatcher.jobs.lock().unwrap().is_empty());
        assert!(
            block_on(child).is_err(),
            "cancelled reply must fail without panicking"
        );
        assert!(!called);
    }
}
