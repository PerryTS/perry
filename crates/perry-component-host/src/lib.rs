//! Optional native Component Model engine. A host owns its Store, resource
//! handles and limits. No Perry heap pointer crosses this crate boundary.
mod async_values;
mod asynchronous;
mod driver;
pub use async_values::AsyncValueType;
pub use asynchronous::origin::{current_invocation, InvocationId};
pub use driver::{Driver, DriverCall, DriverClient};
mod imports;
pub use asynchronous::{AsyncImport, AsyncImports};
mod cleanup;
mod command;
pub mod http;
pub use command::{run_command, run_command_async};

mod contract;
pub mod ffi;
mod host_resources;
mod limits;
mod resources;
mod transport;
pub use host_resources::{ResourceEvent, ResourceHook, ResourceRef};
mod wasi;
mod wire;
pub use wasi::{Address, Network, Preopen, WasiOptions};

use anyhow::{bail, ensure, Context, Result};
use perry_wit::World;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use wasmtime::component::{Component, Func, Linker, Val};
use wasmtime::{AsContextMut, Config, Engine, Store};

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
    async_values: async_values::Values,
    origins: asynchronous::origin::Origins,
    limits: limits::Limits,
    resources: resources::Resources,
    host_resources: host_resources::HostResources,
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
    dispatcher: Option<Arc<asynchronous::dispatch::Dispatcher>>,
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
        Self::new_with_resource_hook(bytes, world, imports, options, Arc::new(|_| Ok(Vec::new())))
    }

    pub fn new_with_resource_hook(
        bytes: &[u8],
        world: &World,
        imports: &Imports,
        options: &Options,
        resource_hook: ResourceHook,
    ) -> Result<Self> {
        Self::new_with_async_imports(
            bytes,
            world,
            imports,
            &AsyncImports::new(),
            options,
            resource_hook,
        )
    }

    /// Native async imports receive owned values and never retain a Store borrow.
    pub fn new_with_async_imports(
        bytes: &[u8],
        world: &World,
        imports: &Imports,
        async_imports: &AsyncImports,
        options: &Options,
        resource_hook: ResourceHook,
    ) -> Result<Self> {
        Self::new_with_dispatcher(
            bytes,
            world,
            imports,
            async_imports,
            options,
            resource_hook,
            None,
        )
    }

    fn new_with_dispatcher(
        bytes: &[u8],
        world: &World,
        imports: &Imports,
        async_imports: &AsyncImports,
        options: &Options,
        resource_hook: ResourceHook,
        dispatcher: Option<Arc<asynchronous::dispatch::Dispatcher>>,
    ) -> Result<Self> {
        ensure!(
            options.memory_bytes > 0 && options.fuel > 0 && options.instances > 0,
            "component limits must be positive"
        );
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .wasm_component_model_async(true)
            .consume_fuel(true)
            .epoch_interruption(true);
        config.wasm_exceptions(true);
        let engine = Engine::new(&config)?;
        let component = Component::from_binary(&engine, bytes)
            .map_err(anyhow::Error::from)
            .context("compiling component")?;
        let identities = contract::validate(&engine, &component, world, options.wasi.is_some())?;
        let prepared = async_values::Prepared::new(
            &engine,
            bytes,
            component,
            world,
            options.wasi.is_some(),
            &identities,
        )?;
        let component = &prepared.component;
        let async_values = async_values::Values::new()?;
        let host_resources = host_resources::HostResources::new(world, options, resource_hook)?;
        let mut linker = Linker::<StoreData>::new(&engine);
        if options.wasi.is_some() {
            perry_wasi::p2::add_to_linker_sync(&mut linker)?;
            perry_wasi::p3::add_to_linker(&mut linker)?;
        }
        for interface in &world.imports {
            if options.wasi.is_some() && interface.name.starts_with("wasi:") {
                continue;
            }
            let mut namespace = if interface.name.is_empty() {
                linker.root()
            } else {
                linker.instance(&interface.name)?
            };
            for (name, key) in &interface.resources {
                let id = host_resources.types[key];
                if let Some(dispatcher) = &dispatcher {
                    let dispatcher = dispatcher.clone();
                    namespace.resource_async(
                        name,
                        wasmtime::component::ResourceType::host_dynamic(id),
                        move |mut store, rep| {
                            let dispatcher = dispatcher.clone();
                            Box::new(async move {
                                let captured =
                                    asynchronous::origin::capture(store.as_context_mut())?;
                                let id_resource = id;
                                let id = captured.id;
                                asynchronous::origin::captured(
                                    captured,
                                    dispatcher.run(move || {
                                        let _scope = asynchronous::origin::Scope::enter(id);
                                        host_resources::destroy(store, rep, id_resource)
                                    }),
                                )
                                .await?
                                .map_err(wasmtime::Error::from_anyhow)
                            })
                        },
                    )?;
                    continue;
                }
                namespace.resource(
                    name,
                    wasmtime::component::ResourceType::host_dynamic(id),
                    move |mut store, rep| {
                        let captured = asynchronous::origin::capture(store.as_context_mut())?;
                        let _scope = asynchronous::origin::Scope::enter(captured.id);
                        host_resources::destroy(store, rep, id)
                            .map_err(wasmtime::Error::from_anyhow)
                    },
                )?;
            }
            for function in &interface.functions {
                if function.async_ {
                    asynchronous::link_import(
                        &mut namespace,
                        &interface.name,
                        &function.name,
                        imports,
                        async_imports,
                        dispatcher.is_some(),
                    )?;
                    continue;
                }
                let callback = imports
                    .get(&interface.name)
                    .and_then(|items| items.get(&function.name))
                    .with_context(|| {
                        format!("missing import {} / {}", interface.name, function.name)
                    })?
                    .clone();
                imports::link(&mut namespace, &function.name, callback, dispatcher.clone())?;
            }
        }
        if let Some(control) = &prepared.control {
            async_values.link(&mut linker, control)?;
        }
        let limits = limits::Limits::new(options.memory_bytes, options.instances);
        let mut store = Store::new(
            &engine,
            StoreData {
                async_values,
                origins: asynchronous::origin::Origins::default(),
                limits,
                resources: resources::Resources::default(),
                host_resources,
                wasi: options.wasi.as_ref().map(WasiOptions::build).transpose()?,
            },
        );
        store.limiter(|data| &mut data.limits);
        store.set_fuel(options.fuel)?;
        store.set_epoch_deadline(1);
        let initialized = (|| -> Result<_> {
            let needs_async = world
                .imports
                .iter()
                .chain(&world.exports)
                .flat_map(|i| &i.functions)
                .any(|f| f.async_)
                || prepared.control.is_some();
            let instance = if needs_async {
                futures_lite::future::block_on(asynchronous::dispatch::drive(
                    dispatcher.as_ref(),
                    linker.instantiate_async(&mut store, component),
                ))
            } else {
                linker.instantiate(&mut store, component)
            }
            .map_err(anyhow::Error::from)
            .context("instantiating component")?;
            let root = prepared
                .original()
                .map(|name| {
                    instance
                        .get_export_index(&mut store, None, name)
                        .context("missing original component instance")
                })
                .transpose()?;
            let mut adapters = Vec::new();
            for (index, name) in prepared.readers.iter().enumerate() {
                let namespace = instance
                    .get_export_index(&mut store, None, name)
                    .context("missing canonical reader instance")?;
                let read = instance
                    .get_export_index(&mut store, Some(&namespace), "read")
                    .context("missing canonical reader")?;
                let dispose = instance
                    .get_export_index(&mut store, Some(&namespace), "dispose")
                    .context("missing canonical disposal")?;
                let read = instance.get_func(&mut store, read).unwrap();
                let dispose = instance.get_func(&mut store, dispose).unwrap();
                let writer_namespace = instance
                    .get_export_index(&mut store, None, &format!("perry-writer-{index}"))
                    .context("missing canonical writer instance")?;
                let mut writer_func = |name: &str| -> Result<Func> {
                    let index = instance
                        .get_export_index(&mut store, Some(&writer_namespace), name)
                        .context("missing canonical writer function")?;
                    instance
                        .get_func(&mut store, index)
                        .context("invalid canonical writer function")
                };
                let new = writer_func("writer-new")?;
                let write = writer_func("writer-write")?;
                let close = writer_func("writer-close")?;
                let writer_dispose = writer_func("dispose")?;
                let list = write.ty(&store).params().nth(1).unwrap().1;
                let writer = async_values::writers::WriterFunctions {
                    new,
                    write,
                    close,
                    dispose: writer_dispose,
                    list,
                };
                adapters.push(Arc::new(async_values::Adapter::new(
                    &store,
                    read,
                    dispose,
                    prepared.schemas[index].clone(),
                    writer,
                )?));
            }
            store.data_mut().async_values.adapters = adapters;
            let mut exports = BTreeMap::new();
            for interface in &world.exports {
                let namespace = if interface.name.is_empty() {
                    root
                } else {
                    Some(
                        instance
                            .get_export_index(&mut store, root.as_ref(), &interface.name)
                            .context("missing validated interface")?,
                    )
                };
                for function in &interface.functions {
                    let index = instance
                        .get_export_index(&mut store, namespace.as_ref(), &function.name)
                        .context("missing validated function")?;
                    let func = instance
                        .get_func(&mut store, index)
                        .context("export is not callable")?;
                    exports.insert((interface.name.clone(), function.name.clone()), func);
                }
            }
            Ok(exports)
        })();
        let exports = match initialized {
            Ok(exports) => exports,
            Err(error) => {
                let _ = host_resources::dispose(store.as_context_mut());
                return Err(error);
            }
        };
        Ok(Self {
            engine,
            dispatcher,
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
        ensure!(
            live.store.data().async_values.adapters.is_empty(),
            "generic async values require the persistent driver"
        );
        ensure!(!ty.async_(), "async component function requires call_async");
        if self.dispatcher.is_some() {
            return futures_lite::future::block_on(self.call_async(interface, name, args));
        }
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
        let mut results = vec![Val::Bool(false); ty.results().len()];
        live.store.set_fuel(live.fuel)?;
        live.store.set_epoch_deadline(1);
        let outcome = func.call(&mut live.store, &args, &mut results);
        outcome
            .map_err(anyhow::Error::from)
            .context("component call trapped")?;
        match results.as_slice() {
            [] => Ok(None),
            [value] => Ok(Some(transport::lift(
                live.store.as_context_mut(),
                value,
                &ty.results().next().unwrap(),
            )?)),
            _ => bail!("selected WIT function has multiple results"),
        }
    }

    pub fn drop_resource(&mut self, id: &str) -> Result<()> {
        self.drop_resource_tracked(id, &mut false)
    }

    pub(crate) fn drop_resource_tracked(&mut self, id: &str, released: &mut bool) -> Result<()> {
        let live = self.live.as_mut().context("component is disposed")?;
        if live.store.data().host_resources.entries.contains_key(id) {
            return host_resources::drop_owned(live.store.as_context_mut(), id, released);
        }
        let resource = live.store.data_mut().resources.remove(id)?;
        // Ownership is gone even when the destructor traps. Report that
        // separately from rejection, which leaves the wrapper live.
        *released = true;
        live.store.set_fuel(live.fuel)?;
        live.store.set_epoch_deadline(1);
        drop_guest_resource(self.dispatcher.as_ref(), resource, &mut live.store)
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
            if let Err(error) =
                drop_guest_resource(self.dispatcher.as_ref(), resource, &mut live.store)
            {
                first_error.get_or_insert(anyhow::Error::from(error));
            }
        }
        if let Err(error) = host_resources::dispose(live.store.as_context_mut()) {
            first_error.get_or_insert(error);
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn drop_guest_resource(
    dispatcher: Option<&Arc<asynchronous::dispatch::Dispatcher>>,
    resource: wasmtime::component::ResourceAny,
    store: &mut Store<StoreData>,
) -> wasmtime::Result<()> {
    match dispatcher {
        Some(dispatcher) => {
            futures_lite::future::block_on(dispatcher.drive(resource.resource_drop_async(store)))
        }
        None => resource.resource_drop(store),
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

#[cfg(test)]
mod host_resource_tests;

#[cfg(test)]
mod asynchronous_tests;
