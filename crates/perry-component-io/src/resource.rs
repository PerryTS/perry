//! Driver-local resource ownership behind Send endpoints.
use super::*;
use std::any::Any;
use std::cell::{Cell, RefCell, RefMut};
use std::collections::HashMap;
use std::marker::PhantomData;

#[derive(Default)]
pub(super) struct Resources {
    next: Cell<u64>,
    values: RefCell<HashMap<u64, Box<dyn Any>>>,
}
impl Resources {
    fn insert<T: 'static>(&self, value: T, permit: Permit) -> Result<u64, Error> {
        let id = self.next.get().checked_add(1).ok_or(Error::Capacity)?;
        self.next.set(id);
        self.values.borrow_mut().insert(
            id,
            Box::new(Rc::new(Value {
                value: RefCell::new(value),
                _permit: permit,
            })),
        );
        Ok(id)
    }

    fn get<T: 'static>(&self, id: u64) -> Result<LocalResource<T>, Error> {
        self.values
            .borrow()
            .get(&id)
            .and_then(|v| v.downcast_ref::<Rc<Value<T>>>())
            .cloned()
            .map(LocalResource)
            .ok_or_else(|| Error::Driver("native resource is unavailable".into()))
    }

    pub(super) fn remove(&self, id: u64) {
        // Drop native owners outside the table borrow.
        let value = self.values.borrow_mut().remove(&id);
        drop(value);
    }

    pub(super) fn clear(&self) {
        let values = std::mem::take(&mut *self.values.borrow_mut());
        drop(values);
    }
}

struct Value<T> {
    value: RefCell<T>,
    _permit: Permit,
}

/// A native value accessible only on its owning driver thread. Clones retain
/// the resource and its capacity permit until all in-flight operations finish.
pub struct LocalResource<T>(Rc<Value<T>>);
impl<T> Clone for LocalResource<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> LocalResource<T> {
    /// Borrow for a single synchronous operation or poll. Never hold this guard
    /// across an await: another operation may need the same native resource.
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        self.0.value.borrow_mut()
    }
}

struct Remote {
    id: u64,
    submitter: Submitter,
    cleanup: Arc<AtomicUsize>,
}
impl Drop for Remote {
    fn drop(&mut self) {
        if let Some(inner) = self.submitter.0.upgrade() {
            // Releases require no task permit; even a saturated driver must
            // reclaim its resources. At most one release is queued per resource.
            let _ = inner.send.send(Command::Release(self.id));
            let _ = inner.notify.notify();
        }
    }
}

/// A Send handle to a native value which itself need not be Send. It does not
/// retain the Driver. Dropping the last handle releases the driver's table entry;
/// pending operations retain their own local owner until completion/cancellation.
/// No guest heap data or Wasmtime Store may be registered as a native resource.
pub struct Resource<T> {
    remote: Arc<Remote>,
    marker: PhantomData<fn() -> T>,
}
impl<T> Clone for Resource<T> {
    fn clone(&self) -> Self {
        Self {
            remote: self.remote.clone(),
            marker: PhantomData,
        }
    }
}
impl<T: 'static> Resource<T> {
    /// Create another native resource from this one, such as a connection from
    /// a listener. The new value remains on the same driver thread but has an
    /// independent lifetime. Capacity is reserved before invoking the factory,
    /// so admission failure cannot consume a connection from a listen backlog.
    pub fn derive<F, Fut, U, E>(
        &self,
        make: F,
    ) -> Result<ResourceRequest<Result<Resource<U>, E>>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>, LocalResource<T>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<U, E>> + 'static,
        U: 'static,
        E: Send + 'static,
    {
        let remote = self.remote.clone();
        let endpoint = remote.submitter.clone();
        endpoint.register_with(move |io, resources| async move {
            let parent = resources.get(remote.id)?;
            drop(remote);
            Ok(make(io, parent).await)
        })
    }

    /// Create an operation on the native resource's driver thread. Only owned
    /// native arguments/results cross threads; the future and resource stay local.
    pub fn call<F, Fut, R>(&self, make: F) -> Result<ResourceRequest<R>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>, LocalResource<T>) -> Fut + Send + 'static,
        Fut: Future<Output = R> + 'static,
        R: Send + 'static,
    {
        let remote = self.remote.clone();
        let inner = remote.submitter.0.upgrade().ok_or(Error::Stopped)?;
        inner
            .submit_with_resources(move |io, resources| async move {
                let value = resources.get(remote.id)?;
                drop(remote);
                Ok(make(io, value).await)
            })
            .map(ResourceRequest)
    }

    /// Run native teardown even when ordinary task admission is full. Only use
    /// this for cleanup, such as closing the receive/send halves of a socket.
    /// At most two cleanup tasks may be queued/running per resource (including
    /// all cloned handles). Each retains the native resource and its budget
    /// until completion or acknowledged cancellation, bounding executor use.
    pub fn call_cleanup<F, Fut, R>(&self, make: F) -> Result<ResourceRequest<R>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>, LocalResource<T>) -> Fut + Send + 'static,
        Fut: Future<Output = R> + 'static,
        R: Send + 'static,
    {
        let remote = self.remote.clone();
        let inner = remote.submitter.0.upgrade().ok_or(Error::Stopped)?;
        remote
            .cleanup
            .try_update(Ordering::AcqRel, Ordering::Acquire, |live| {
                (live < 2).then_some(live + 1)
            })
            .map_err(|_| Error::Capacity)?;
        let permit = Permit(remote.cleanup.clone());
        inner
            .submit_admitted(permit, move |io, resources| async move {
                let value = resources.get(remote.id)?;
                drop(remote);
                let result = make(io, value.clone()).await;
                // Even a cleanup closure that ignores its argument cannot release
                // the resource budget early and over-admit more cleanup tasks.
                drop(value);
                Ok(result)
            })
            .map(ResourceRequest)
    }
}

impl Submitter {
    /// Construct a persistent native resource on the driver thread. Registrations
    /// and live resources share a bounded resource budget, separate from tasks.
    /// A cancelled registration drops any created value on its owning thread.
    pub fn register<F, Fut, T, E>(
        &self,
        make: F,
    ) -> Result<ResourceRequest<Result<Resource<T>, E>>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, E>> + 'static,
        T: 'static,
        E: Send + 'static,
    {
        self.register_with(move |io, _| async move { Ok(make(io).await) })
    }

    fn register_with<F, Fut, T, E>(
        &self,
        make: F,
    ) -> Result<ResourceRequest<Result<Resource<T>, E>>, Error>
    where
        F: FnOnce(ExecutorHandle<Platform>, Rc<Resources>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Result<T, E>, Error>> + 'static,
        T: 'static,
        E: Send + 'static,
    {
        let inner = self.0.upgrade().ok_or(Error::Stopped)?;
        if inner.stopped.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        inner
            .resources
            .try_update(Ordering::AcqRel, Ordering::Acquire, |live| {
                (live < inner.limit).then_some(live + 1)
            })
            .map_err(|_| Error::Capacity)?;
        let permit = Permit(inner.resources.clone());
        let submitter = self.clone();
        inner
            .submit_with_resources(move |io, resources| async move {
                match make(io, resources.clone()).await? {
                    Ok(value) => {
                        let id = resources.insert(value, permit)?;
                        Ok(Ok(Resource {
                            remote: Arc::new(Remote {
                                id,
                                submitter,
                                cleanup: Arc::new(AtomicUsize::new(0)),
                            }),
                            marker: PhantomData,
                        }))
                    }
                    Err(error) => Ok(Err(error)),
                }
            })
            .map(ResourceRequest)
    }
}

/// A cancellable operation on a driver-owned resource. Awaiting cancellation
/// acknowledges that the local future and its resource references were dropped.
/// It does not destroy a resource still owned by another handle or operation;
/// providers must explicitly manage any operation state retained in that value.
pub struct ResourceRequest<T>(Request<Result<T, Error>>);
impl<T> ResourceRequest<T> {
    pub fn cancel(&self) {
        self.0.cancel();
    }

    pub fn cancellation(&self) -> Cancellation {
        self.0.cancellation()
    }
}
impl<T> Future for ResourceRequest<T> {
    type Output = Result<T, Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0)
            .poll(cx)
            .map(|result| result.and_then(|v| v))
    }
}

#[cfg(test)]
mod tests;
