//! Synchronous WIT imports may suspend a Wasmtime fiber, but JS executes on
//! the embedding stack through the scoped dispatcher.
use crate::{
    asynchronous::{dispatch::Dispatcher, origin},
    host_resources, transport, Import, StoreData,
};
use anyhow::{ensure, Result};
use std::sync::Arc;
use wasmtime::{
    component::{types::ComponentFunc, LinkerInstance, Val},
    AsContextMut, StoreContextMut,
};

fn invoke(
    mut store: StoreContextMut<'_, StoreData>,
    ty: ComponentFunc,
    params: &[Val],
    results: &mut [Val],
    callback: &Import,
    prepared: Result<std::collections::VecDeque<serde_json::Value>>,
) -> wasmtime::Result<()> {
    let result = (|| -> Result<()> {
        let mut prepared = prepared?;
        let args = params
            .iter()
            .zip(ty.params())
            .map(|(value, (_, ty))| {
                transport::lift_import(store.as_context_mut(), value, &ty, &mut prepared)
            })
            .collect::<Result<Vec<_>>>()?;
        let value = callback(&args);
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
    result.map_err(wasmtime::Error::from_anyhow)
}

struct Borrows<'a> {
    store: StoreContextMut<'a, StoreData>,
    ids: Vec<String>,
    prepared: Option<Result<std::collections::VecDeque<serde_json::Value>>>,
    retiring: Arc<std::sync::atomic::AtomicBool>,
    finished: bool,
}
impl<'a> Borrows<'a> {
    fn new(mut store: StoreContextMut<'a, StoreData>, params: &[Val]) -> Self {
        let checkpoint = store.data().resources.checkpoint();
        let prepared = crate::asynchronous::import_values::prepare(store.as_context_mut(), params);
        let ids = store.data().resources.borrowed_since(checkpoint);
        store.data().borrowed.open(&ids, false);
        let retiring = store.data().retiring.clone();
        Self {
            store,
            ids,
            prepared: Some(prepared),
            retiring,
            finished: false,
        }
    }
    fn invoke(
        &mut self,
        ty: ComponentFunc,
        params: &[Val],
        results: &mut [Val],
        callback: &Import,
    ) -> wasmtime::Result<()> {
        let prepared = self.prepared.take().unwrap();
        invoke(
            self.store.as_context_mut(),
            ty,
            params,
            results,
            callback,
            prepared,
        )
    }
    fn finish(&mut self) -> wasmtime::Result<()> {
        let result = crate::asynchronous::borrows::release(self.store.as_context_mut(), &self.ids);
        self.finished = true;
        result.map_err(wasmtime::Error::from_anyhow)
    }
}
impl Drop for Borrows<'_> {
    fn drop(&mut self) {
        self.store.data().borrowed.close(&self.ids);
        if !self.finished && !self.retiring.load(std::sync::atomic::Ordering::Acquire) {
            let _ = crate::asynchronous::borrows::release(self.store.as_context_mut(), &self.ids);
        }
    }
}

pub(crate) fn link(
    namespace: &mut LinkerInstance<'_, StoreData>,
    name: &str,
    callback: Import,
    dispatcher: Option<Arc<Dispatcher>>,
) -> Result<()> {
    if let Some(dispatcher) = dispatcher {
        namespace.func_new_async(name, move |store, ty, params, results| {
            let callback = callback.clone();
            let dispatcher = dispatcher.clone();
            Box::new(async move {
                let mut store = store;
                let captured = origin::capture(store.as_context_mut())?;
                let id = captured.id;
                let mut borrows = Borrows::new(store, params);
                let outcome = origin::captured(
                    captured,
                    dispatcher.run(|| {
                        let _scope = origin::Scope::enter(id);
                        borrows.invoke(ty, params, results, &callback)
                    }),
                )
                .await?;
                let cleanup = borrows.finish();
                outcome?;
                cleanup
            })
        })?;
    } else {
        namespace.func_new(name, move |store, ty, params, results| {
            let mut store = store;
            let captured = origin::capture(store.as_context_mut())?;
            let _scope = origin::Scope::enter(captured.id);
            let mut borrows = Borrows::new(store, params);
            let outcome = borrows.invoke(ty, params, results, &callback);
            let cleanup = borrows.finish();
            outcome?;
            cleanup
        })?;
    }
    Ok(())
}
