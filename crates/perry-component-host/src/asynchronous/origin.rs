//! Invocation identities are native integers, never callback frames or JS roots.
//! Capture guest ancestry before the first host-task yield, then restore the
//! captured identity on every poll instead of querying the ambient stack later.
use super::retention::{Lease, Registry};
use crate::StoreData;
use anyhow::Result;
use std::{
    cell::Cell,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
};
use wasmtime::{component::GuestTaskId, StoreContextMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InvocationId(u64);
impl InvocationId {
    pub fn as_u64(self) -> u64 {
        self.0
    }
    pub(crate) fn fresh() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let previous = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| anyhow::anyhow!("component invocation identity exhausted"))?;
        Ok(Self(previous + 1))
    }
}
thread_local! { static CURRENT: Cell<Option<InvocationId>> = const { Cell::new(None) }; }

/// Identity of the native driver invocation currently executing a callback.
/// It remains the same across host-task suspension; legacy calls have none.
pub fn current_invocation() -> Option<InvocationId> {
    CURRENT.with(Cell::get)
}

pub(crate) struct Scope(Option<InvocationId>, PhantomData<Rc<()>>);
impl Scope {
    pub fn enter(origin: Option<InvocationId>) -> Self {
        Self(CURRENT.with(|c| c.replace(origin)), PhantomData)
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|c| c.set(self.0));
    }
}

struct Scoped<F> {
    origin: Option<InvocationId>,
    future: Option<Pin<Box<F>>>,
    capture: Option<Captured>,
}
impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let _scope = Scope::enter(self.origin);
        self.future.as_mut().unwrap().as_mut().poll(cx)
    }
}
impl<F> Drop for Scoped<F> {
    fn drop(&mut self) {
        let _scope = Scope::enter(self.origin);
        self.future.take();
        // Cleanup in the inner future must still find its retained context.
        self.capture.take();
    }
}

pub(crate) fn scoped<F: Future>(
    origin: Option<InvocationId>,
    future: F,
) -> impl Future<Output = F::Output> {
    Scoped {
        origin,
        future: Some(Box::pin(future)),
        capture: None,
    }
}

/// Captured before yielding off the guest fiber. Its lease outlives the
/// callback future, including cancellation and borrowed-dispatch cleanup.
pub(crate) struct Captured {
    pub id: Option<InvocationId>,
    _lease: Option<Lease>,
}
pub(crate) fn captured<F: Future>(capture: Captured, future: F) -> impl Future<Output = F::Output> {
    Scoped {
        origin: capture.id,
        future: Some(Box::pin(future)),
        capture: Some(capture),
    }
}
#[derive(Default)]
pub(crate) struct Origins {
    pub registry: Registry,
}
impl Drop for Origins {
    fn drop(&mut self) {
        self.registry.clear_roots();
    }
}
impl Origins {
    pub fn register(&mut self, root: GuestTaskId, origin: InvocationId) {
        self.registry.register(root, origin);
    }
    pub fn clear(&mut self) {
        self.registry.clear_roots();
    }
}
pub(crate) fn capture(mut store: StoreContextMut<'_, StoreData>) -> wasmtime::Result<Captured> {
    let registry = store.data().origins.registry.clone();
    let ancestry = if registry.is_empty() {
        Vec::new()
    } else {
        store.async_call_stack()?.collect::<Vec<_>>()
    };
    let (id, lease) = match registry.capture(&ancestry, current_invocation()) {
        Some((id, lease)) => (Some(id), Some(lease)),
        None => (None, None),
    };
    Ok(Captured { id, _lease: lease })
}

#[cfg(test)]
#[path = "origin_tests.rs"]
mod tests;
