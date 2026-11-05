//! Import borrows retain scoped capabilities until accepted forwarding work
//! completes. Only this import's aliases are removed at its settlement.
use crate::{borrowed::Registry, InvocationId, StoreData};
use anyhow::{Context, Result};
use futures_util::FutureExt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use wasmtime::{component::Accessor, AsContextMut, StoreContextMut};

pub(crate) fn release(mut store: StoreContextMut<'_, StoreData>, ids: &[String]) -> Result<()> {
    store.data().borrowed.close(ids);
    let values = store.data_mut().resources.take_borrows(ids);
    let mut first = None;
    for value in values {
        let result: Result<()> = if store.data().host_resources.is_host(value.ty()) {
            value
                .try_into_resource_dynamic(store.as_context_mut())
                .map(|_| ())
                .map_err(anyhow::Error::from)
        } else {
            // Only borrowed aliases reach this path. Wasmtime's maintained
            // async API removes their table entries before its destructor
            // branch, so no guest/embedding code can suspend this operation.
            // Poll inside the Accessor borrow; never extend a Store lifetime.
            // A future engine that suspends here fails closed and retires the
            // component rather than retaining an erased Store borrow.
            (|| {
                anyhow::ensure!(!value.owned(), "import cleanup requires a borrowed alias");
                value
                    .resource_drop_async(store.as_context_mut())
                    .now_or_never()
                    .context("borrow alias cleanup unexpectedly suspended")?
                    .map_err(anyhow::Error::from)
            })()
        };
        if let Err(error) = result {
            first.get_or_insert(error);
        }
    }
    first.map_or(Ok(()), Err)
}
pub(crate) struct Scope<'a> {
    accessor: &'a Accessor<StoreData>,
    ids: Vec<String>,
    registry: Registry,
    retiring: Arc<AtomicBool>,
    finished: bool,
}
impl<'a> Scope<'a> {
    pub fn new(accessor: &'a Accessor<StoreData>, ids: Vec<String>) -> Self {
        let (registry, retiring) = accessor.with(|mut store| {
            let data = store.data_mut();
            data.borrowed.open(&ids, true);
            (data.borrowed.clone(), data.retiring.clone())
        });
        Self {
            accessor,
            ids,
            registry,
            retiring,
            finished: false,
        }
    }
    pub async fn finish(&mut self) -> Result<()> {
        self.registry.close(&self.ids);
        if !self.ids.is_empty() {
            let waiter = InvocationId::fresh()?;
            for id in &self.ids {
                self.registry.wait(format!("lease:{id}"), waiter).await;
            }
        }
        let result = self
            .accessor
            .with(|mut store| release(store.as_context_mut(), &self.ids));
        self.finished = true;
        result
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.registry.close(&self.ids);
        if !self.finished && !self.retiring.load(Ordering::Acquire) {
            let _ = self
                .accessor
                .with(|mut store| release(store.as_context_mut(), &self.ids));
        }
    }
}
