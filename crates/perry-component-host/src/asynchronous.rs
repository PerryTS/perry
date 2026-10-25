//! Async calls own the Store across suspension; only Accessor callbacks borrow it.
pub(crate) mod borrows;
pub(crate) mod dispatch;
pub(crate) mod import_values;
pub(crate) mod origin;
pub(crate) mod retention;
use crate::{host_resources, transport, Host, Import, Imports, Live, StoreData};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};
use wasmtime::{
    component::{LinkerInstance, Val},
    AsContextMut,
};

pub type ImportFuture = Pin<Box<dyn Future<Output = Result<Option<Value>>> + Send>>;
pub type AsyncImport = Arc<dyn Fn(Vec<Value>) -> ImportFuture + Send + Sync>;
pub type AsyncImports = BTreeMap<String, BTreeMap<String, AsyncImport>>;

pub(super) fn link_import(
    namespace: &mut LinkerInstance<'_, StoreData>,
    interface: &str,
    name: &str,
    imports: &Imports,
    asynchronous: &AsyncImports,
    embedding_stack: bool,
) -> Result<()> {
    let callback = match asynchronous.get(interface).and_then(|i| i.get(name)) {
        Some(callback) => callback.clone(),
        None => {
            let callback: Import = imports
                .get(interface)
                .and_then(|i| i.get(name))
                .with_context(|| format!("missing async import {interface} / {name}"))?
                .clone();
            Arc::new(move |args: Vec<Value>| -> ImportFuture {
                let result = callback(&args);
                Box::pin(async move { result })
            })
        }
    };
    namespace.func_new_concurrent(name, move |accessor, ty, params, results| {
        let callback = callback.clone();
        Box::pin(async move {
            let captured = accessor.with(|mut store| origin::capture(store.as_context_mut()))?;
            origin::captured(captured, async move {
                let (prepared, ids) = accessor.with(|mut store| {
                    let checkpoint = store.data_mut().resources.checkpoint();
                    let values = import_values::prepare(store.as_context_mut(), params);
                    let ids = store.data_mut().resources.borrowed_since(checkpoint);
                    (values, ids)
                });
                let mut borrows = borrows::Scope::new(accessor, ids);
                // Borrow handles need the initial canonical scope. Only their
                // native conversion runs on the guest fiber; JS follows yield.
                if embedding_stack {
                    futures_lite::future::yield_now().await;
                }
                let args = accessor.with(|mut store| -> Result<Vec<Value>> {
                    let mut prepared = prepared?;
                    params
                        .iter()
                        .zip(ty.params())
                        .map(|(v, (_, ty))| {
                            transport::lift_import(store.as_context_mut(), v, &ty, &mut prepared)
                        })
                        .collect()
                });
                let value = match args {
                    Ok(args) => callback(args).await,
                    Err(error) => Err(error),
                };
                let cleanup = borrows.finish().await;
                accessor.with(|mut store| {
                    let outcome = (|| -> Result<()> {
                        let cleanup = host_resources::drain(store.as_context_mut());
                        let value = value?;
                        cleanup?;
                        let types: Vec<_> = ty.results().collect();
                        ensure!(
                            results.len() == usize::from(value.is_some()),
                            "host import result count mismatch"
                        );
                        if let Some(value) = value {
                            let mut lowering = transport::Lowering::new(store.as_context_mut());
                            results[0] = lowering.value(&value, &types[0])?;
                            lowering.commit()?;
                        }
                        Ok(())
                    })();
                    outcome.map_err(wasmtime::Error::from_anyhow)?;
                    cleanup.map_err(wasmtime::Error::from_anyhow)
                })
            })
            .await
        })
    })?;
    Ok(())
}

/// The guest may still have suspended tasks when a call is cancelled or traps.
/// Destroy its Store before releasing embedding payloads; guest destructors must
/// not execute during hard cancellation. Teardown runs on the calling thread.
pub(super) struct Invocation<'a> {
    pub(super) host: &'a mut Host,
    pub(super) live: Option<Live>,
    pub(super) before_retire: Option<Box<dyn FnOnce() + 'a>>,
}
impl Invocation<'_> {
    pub(super) fn restore(&mut self) {
        self.host.live = self.live.take();
    }
    pub(super) fn retire(&mut self) -> Result<()> {
        let Some(mut live) = self.live.take() else {
            return Ok(());
        };
        if let Some(before) = self.before_retire.take() {
            before();
        }
        live.store
            .data_mut()
            .retiring
            .store(true, std::sync::atomic::Ordering::Release);
        let mut data = live.store.into_data();
        let hook = data.host_resources.hook.clone();
        let entries = std::mem::take(&mut data.host_resources.entries);
        data.host_resources.reps.clear();
        let mut error = None;
        for entry in entries.into_values() {
            if let Err(e) = hook(host_resources::ResourceEvent::Drop {
                resource: entry.reference,
            }) {
                error.get_or_insert(e);
            }
        }
        error.map_or(Ok(()), Err)
    }
}
impl Drop for Invocation<'_> {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}

impl Host {
    /// Invoke a canonical async export. Cancelling this future after it starts
    /// retires the component; successful calls preserve its Store for reuse.
    pub async fn call_async(
        &mut self,
        interface: &str,
        name: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        self.call_async_with_retirement(interface, name, args, || {})
            .await
    }

    pub(crate) async fn call_async_with_retirement<'a>(
        &'a mut self,
        interface: &str,
        name: &str,
        args: &[Value],
        before_retire: impl FnOnce() + 'a,
    ) -> Result<Option<Value>> {
        let dispatcher = self.dispatcher.clone();
        let live = self.live.as_mut().context("component is disposed")?;
        ensure!(
            live.store.data().async_values.adapters.is_empty(),
            "generic async values require the persistent driver"
        );
        let func = *live
            .exports
            .get(&(interface.to_owned(), name.to_owned()))
            .context("function is outside the selected WIT world")?;
        let ty = func.ty(&live.store);
        let params: Vec<_> = ty.params().map(|(_, ty)| ty).collect();
        ensure!(
            params.len() == args.len(),
            "component argument count mismatch"
        );
        let mut lowering = transport::Lowering::new(live.store.as_context_mut());
        let args = args
            .iter()
            .zip(&params)
            .map(|(value, ty)| lowering.value(value, ty))
            .collect::<Result<Vec<_>>>()?;
        lowering.commit()?;
        let mut invocation = Invocation {
            live: self.live.take(),
            host: self,
            before_retire: Some(Box::new(before_retire)),
        };
        let outcome = {
            let live = invocation.live.as_mut().unwrap();
            live.store.set_fuel(live.fuel)?;
            live.store.set_epoch_deadline(1);
            let mut results = vec![Val::Bool(false); ty.results().len()];
            dispatch::drive(
                dispatcher.as_ref(),
                live.store.run_concurrent(async |accessor| {
                    func.call_concurrent(accessor, &args, &mut results).await
                }),
            )
            .await
            .map_err(anyhow::Error::from)
            .and_then(|r| r.map_err(anyhow::Error::from))
            .context("async component call trapped")
            .and_then(|()| match results.as_slice() {
                [] => Ok(None),
                [value] => Ok(Some(transport::lift(
                    live.store.as_context_mut(),
                    value,
                    &ty.results().next().unwrap(),
                )?)),
                _ => anyhow::bail!("selected WIT function has multiple results"),
            })
        };
        if outcome.is_ok() {
            invocation.restore();
        } else {
            let _ = invocation.retire();
        }
        outcome
    }
}
