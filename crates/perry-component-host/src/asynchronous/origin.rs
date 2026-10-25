//! Invocation identities are native integers, never callback frames or JS roots.
//! Capture guest ancestry before the first host-task yield, then restore the
//! captured identity on every poll instead of querying the ambient stack later.
use crate::StoreData;
use anyhow::Result;
use std::{
    cell::Cell,
    collections::BTreeMap,
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
    }
}

pub(crate) fn scoped<F: Future>(
    origin: Option<InvocationId>,
    future: F,
) -> impl Future<Output = F::Output> {
    Scoped {
        origin,
        future: Some(Box::pin(future)),
    }
}

#[derive(Default)]
pub(crate) struct Origins {
    roots: BTreeMap<GuestTaskId, InvocationId>,
}
impl Origins {
    pub fn register(&mut self, root: GuestTaskId, origin: InvocationId) {
        // GuestTaskId indices may be reused after a task exits. Replace their
        // registration before the new task runs; host futures retain integers.
        self.roots.insert(root, origin);
    }
    pub fn clear(&mut self) {
        self.roots.clear();
    }
}

pub(crate) fn capture(
    mut store: StoreContextMut<'_, StoreData>,
) -> wasmtime::Result<Option<InvocationId>> {
    if store.data().origins.roots.is_empty() {
        return Ok(None);
    }
    let ancestry = store.async_call_stack()?.collect::<Vec<_>>();
    // The oldest registered ancestor owns this invocation, including helper
    // calls and imported functions invoked after its original task.return.
    Ok(ancestry
        .iter()
        .rev()
        .find_map(|root| store.data().origins.roots.get(root).copied())
        .or_else(current_invocation))
}
