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
) -> wasmtime::Result<()> {
    let result = (|| -> Result<()> {
        let args = params
            .iter()
            .map(|value| transport::lift(store.as_context_mut(), value))
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
    let borrows = store.data_mut().resources.take_borrows();
    let mut cleanup = Ok(());
    for borrow in borrows {
        if let Err(error) = borrow.resource_drop(&mut store) {
            cleanup = Err(error);
        }
    }
    result.map_err(wasmtime::Error::from_anyhow)?;
    cleanup
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
                dispatcher
                    .run(move || {
                        let _scope = origin::Scope::enter(captured);
                        invoke(store, ty, params, results, &callback)
                    })
                    .await?
            })
        })?;
    } else {
        namespace.func_new(name, move |store, ty, params, results| {
            let mut store = store;
            let captured = origin::capture(store.as_context_mut())?;
            let _scope = origin::Scope::enter(captured);
            invoke(store, ty, params, results, &callback)
        })?;
    }
    Ok(())
}
