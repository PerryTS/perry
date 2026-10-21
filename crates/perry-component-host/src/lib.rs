//! Optional native Component Model engine. A host owns its Store, resource
//! handles and limits. No Perry heap pointer crosses this crate boundary.
mod contract;
pub mod ffi;
mod limits;
mod resources;
mod wasi;
mod wire;
pub use wasi::{Address, Network, Preopen, WasiOptions};

use anyhow::{bail, ensure, Context, Result};
use perry_wit::World;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use wasmtime::component::{Component, Func, Linker, Val};
use wasmtime::{Config, Engine, Store};

pub type Import = Arc<dyn Fn(&[Value]) -> Result<Option<Value>> + Send + Sync>;
pub type Imports = BTreeMap<String, BTreeMap<String, Import>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Options {
    /// Maximum total size of guest linear memories, in bytes.
    pub memory_bytes: usize,
    /// Fuel for initialization and each exported call/destructor.
    pub fuel: u64,
    /// Maximum core instances used by one component/composition.
    pub instances: usize,
    /// Opt-in Preview 2 providers with per-instance capabilities.
    pub wasi: Option<WasiOptions>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024,
            fuel: 10_000_000,
            instances: 64,
            wasi: None,
        }
    }
}

struct StoreData {
    limits: limits::Limits,
    resources: resources::Resources,
    wasi: Option<wasi::State>,
}

struct Live {
    store: Store<StoreData>,
    exports: BTreeMap<(String, String), Func>,
    fuel: u64,
}

pub struct Host {
    live: Option<Live>,
    engine: Engine,
}

/// Independent of the calling thread. Incrementing the epoch interrupts this
/// component's guest execution; another component has a different engine.
#[derive(Clone)]
pub struct Interrupt(Engine);
impl Interrupt {
    pub fn interrupt(&self) {
        self.0.increment_epoch();
    }
}

impl Host {
    pub fn new(bytes: &[u8], world: &World, imports: &Imports, options: &Options) -> Result<Self> {
        ensure!(
            options.memory_bytes > 0 && options.fuel > 0 && options.instances > 0,
            "component limits must be positive"
        );
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .consume_fuel(true)
            .epoch_interruption(true);
        config.wasm_exceptions(true);
        let engine = Engine::new(&config)?;
        let component = Component::from_binary(&engine, bytes)
            .map_err(anyhow::Error::from)
            .context("compiling component")?;
        contract::validate(&engine, &component, world, options.wasi.is_some())?;
        let mut linker = Linker::<StoreData>::new(&engine);
        if options.wasi.is_some() {
            wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        }
        for interface in &world.imports {
            if options.wasi.is_some() && interface.name.starts_with("wasi:") {
                continue;
            }
            ensure!(
                interface.resources.is_empty(),
                "host-defined resource imports require a resource implementation"
            );
            let mut namespace = if interface.name.is_empty() {
                linker.root()
            } else {
                linker.instance(&interface.name)?
            };
            for function in &interface.functions {
                let callback = imports
                    .get(&interface.name)
                    .and_then(|items| items.get(&function.name))
                    .with_context(|| {
                        format!("missing import {} / {}", interface.name, function.name)
                    })?
                    .clone();
                namespace.func_new(&function.name, move |mut store, ty, params, results| {
                    let result = (|| -> Result<()> {
                        let args = params
                            .iter()
                            .map(|value| wire::lift(value, &mut store.data_mut().resources))
                            .collect::<Result<Vec<_>>>()?;
                        let value = callback(&args)?;
                        let types: Vec<_> = ty.results().collect();
                        ensure!(
                            results.len() == usize::from(value.is_some()),
                            "host import result count mismatch"
                        );
                        if let Some(value) = value {
                            let mut transfers = BTreeSet::new();
                            results[0] = wire::lower(
                                &value,
                                &types[0],
                                &store.data().resources,
                                &mut transfers,
                            )?;
                            store.data_mut().resources.transferred(&transfers);
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
                })?;
            }
        }
        let limits = limits::Limits::new(options.memory_bytes, options.instances);
        let mut store = Store::new(
            &engine,
            StoreData {
                limits,
                resources: resources::Resources::default(),
                wasi: options.wasi.as_ref().map(WasiOptions::build).transpose()?,
            },
        );
        store.limiter(|data| &mut data.limits);
        store.set_fuel(options.fuel)?;
        store.set_epoch_deadline(1);
        let instance = linker
            .instantiate(&mut store, &component)
            .map_err(anyhow::Error::from)
            .context("instantiating component")?;
        let mut exports = BTreeMap::new();
        for interface in &world.exports {
            let namespace = if interface.name.is_empty() {
                None
            } else {
                Some(
                    instance
                        .get_export_index(&mut store, None, &interface.name)
                        .context("missing validated interface")?,
                )
            };
            for function in &interface.functions {
                let index = instance
                    .get_export_index(&mut store, namespace.as_ref(), &function.name)
                    .context("missing validated function")?;
                let func = instance
                    .get_func(&mut store, &index)
                    .context("export is not callable")?;
                exports.insert((interface.name.clone(), function.name.clone()), func);
            }
        }
        Ok(Self {
            engine,
            live: Some(Live {
                store,
                exports,
                fuel: options.fuel,
            }),
        })
    }

    pub fn interrupt_handle(&self) -> Interrupt {
        Interrupt(self.engine.clone())
    }

    pub fn call(&mut self, interface: &str, name: &str, args: &[Value]) -> Result<Option<Value>> {
        let live = self.live.as_mut().context("component is disposed")?;
        let func = *live
            .exports
            .get(&(interface.to_string(), name.to_string()))
            .context("function is outside the selected WIT world")?;
        let ty = func.ty(&live.store);
        let params: Vec<_> = ty.params().map(|(_, ty)| ty).collect();
        ensure!(
            params.len() == args.len(),
            "component argument count mismatch"
        );
        let mut transfers = BTreeSet::new();
        let args = args
            .iter()
            .zip(&params)
            .map(|(value, ty)| wire::lower(value, ty, &live.store.data().resources, &mut transfers))
            .collect::<Result<Vec<_>>>()?;
        let mut results = vec![Val::Bool(false); ty.results().len()];
        live.store.set_fuel(live.fuel)?;
        live.store.set_epoch_deadline(1);
        let outcome = func.call(&mut live.store, &args, &mut results);
        // A call which has entered canonical lowering owns these inputs even
        // when guest execution traps. Stale wrappers must not reuse them.
        live.store.data_mut().resources.transferred(&transfers);
        outcome
            .map_err(anyhow::Error::from)
            .context("component call trapped")?;
        match results.as_slice() {
            [] => Ok(None),
            [value] => Ok(Some(wire::lift(
                value,
                &mut live.store.data_mut().resources,
            )?)),
            _ => bail!("selected WIT function has multiple results"),
        }
    }

    pub fn drop_resource(&mut self, id: &str) -> Result<()> {
        self.drop_resource_tracked(id, &mut false)
    }

    pub(crate) fn drop_resource_tracked(&mut self, id: &str, released: &mut bool) -> Result<()> {
        let live = self.live.as_mut().context("component is disposed")?;
        let resource = live.store.data_mut().resources.remove(id)?;
        // Ownership is gone even when the destructor traps. Report that
        // separately from rejection, which leaves the wrapper live.
        *released = true;
        live.store.set_fuel(live.fuel)?;
        live.store.set_epoch_deadline(1);
        resource
            .resource_drop(&mut live.store)
            .map_err(anyhow::Error::from)
    }

    pub fn dispose(&mut self) -> Result<()> {
        let Some(mut live) = self.live.take() else {
            return Ok(());
        };
        let mut first_error = None;
        for resource in live.store.data_mut().resources.drain() {
            live.store.set_fuel(live.fuel)?;
            live.store.set_epoch_deadline(1);
            if let Err(error) = resource.resource_drop(&mut live.store) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), |error| Err(anyhow::Error::from(error)))
    }
}

impl Host {
    /// Release engine memory without executing guest code. Resource disposal
    /// runs explicitly or in a deferred cleanup job before native teardown.
    pub(crate) fn abandon(&mut self) {
        self.live.take();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.dispose();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod resource_tests;
