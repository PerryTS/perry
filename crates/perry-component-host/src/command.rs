//! One-shot WASI P2/P3 commands, separate from reusable library worlds.
use crate::{limits::Limits, wasi, Options, WasiOptions};
use anyhow::{bail, ensure, Context, Result};
use perry_wasi::p2::bindings::sync::CommandPre;
use perry_wasi::{I32Exit, WasiCtxView, WasiView};
use wasmtime::component::{
    types::ComponentItem, Component, ComponentExportIndex, InstancePre, Linker, Type, Val,
};
use wasmtime::{Config, Engine, Store};

struct State {
    limits: Limits,
    wasi: wasi::State,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        self.wasi.view()
    }
}

enum Profile {
    P2,
    P3(ComponentExportIndex),
}

fn validate_command(engine: &Engine, component: &Component) -> Result<Profile> {
    // P3 is an explicit interface profile. Do not treat an arbitrary 0.3.x
    // interface name as the pinned 0.3.0 contract.
    let p3 = component
        .component_type()
        .exports(engine)
        .any(|(name, _)| name == "wasi:cli/run@0.3.0");
    let name = if p3 {
        "wasi:cli/run@0.3.0"
    } else {
        "wasi:cli/run@0.2.12"
    };
    // Match the exact lookup used by the lockfile-pinned Command bindings,
    // including Wasmtime's compatible-version lookup. Do not inspect an
    // arbitrary first cli/run export if the component exports several versions.
    let (_, interface) = component
        .get_export(None, name)
        .context("missing wasi:cli/run interface")?;
    let (item, index) = component
        .get_export(Some(&interface), "run")
        .context("missing wasi:cli/run function")?;
    let ComponentItem::ComponentFunc(run) = item else {
        bail!("wasi:cli/run export is not a function");
    };
    ensure!(
        run.async_() == p3,
        "wasi:cli/run has the wrong canonical async calling convention"
    );
    ensure!(
        run.params().len() == 0,
        "wasi:cli/run must take no arguments"
    );
    let results: Vec<_> = run.results().collect();
    ensure!(
        matches!(results.as_slice(), [Type::Result(result)] if result.ok().is_none() && result.err().is_none()),
        "wasi:cli/run must return result without payloads"
    );
    Ok(if p3 { Profile::P3(index) } else { Profile::P2 })
}

/// Execute `wasi:cli/run` once in a fresh store. Returned `ok`/`err` become
/// status 0/1; WASI exit requests preserve their status without exiting Perry.
/// Traps, invalid components and capability/configuration errors remain errors.
/// Omitting `options.wasi` uses the same capability defaults as `wasi: {}`.
pub fn run_command(bytes: &[u8], options: &Options) -> Result<i32> {
    futures_lite::future::block_on(run_command_async(bytes, options))
}

/// Run a command with native P3 task suspension. P2 commands retain synchronous
/// provider I/O. Dropping this future cancels its Store and provider resources.
pub async fn run_command_async(bytes: &[u8], options: &Options) -> Result<i32> {
    Prepared::new(bytes, options)?.run().await
}

/// Destroy suspended guest tasks before their provider context. No embedding
/// callback or managed JS state is present in this one-shot Store.
struct CommandStore(Option<Store<State>>);
impl CommandStore {
    fn get(&mut self) -> &mut Store<State> {
        self.0.as_mut().unwrap()
    }
}
impl Drop for CommandStore {
    fn drop(&mut self) {
        if let Some(store) = self.0.take() {
            drop(store.into_data());
        }
    }
}
struct Prepared {
    store: CommandStore,
    pre: InstancePre<State>,
    profile: Profile,
}
impl Prepared {
    fn new(bytes: &[u8], options: &Options) -> Result<Self> {
        ensure!(
            options.memory_bytes > 0 && options.fuel > 0 && options.instances > 0,
            "component limits must be positive"
        );
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .wasm_component_model_async(true)
            .wasm_exceptions(true)
            .consume_fuel(true);
        let engine = Engine::new(&config)?;
        let component = Component::from_binary(&engine, bytes)
            .map_err(anyhow::Error::from)
            .context("compiling WASI command")?;
        // CommandPre checks names, but generated function type checks happen only
        // after instantiation. Validate the signature before an initializer exits.
        let profile = validate_command(&engine, &component)
            .context("validating wasi:cli/run command export")?;
        let mut linker = Linker::<State>::new(&engine);
        perry_wasi::p2::add_to_linker_sync(&mut linker)?;
        perry_wasi::p3::add_to_linker(&mut linker)?;
        // Validate the command export before an initializer can request exit.
        // A malformed component must not turn an init-time exit(0) into success.
        let pre = linker.instantiate_pre(&component)?;
        if matches!(profile, Profile::P2) {
            CommandPre::new(pre.clone())
                .map_err(anyhow::Error::from)
                .context("validating wasi:cli/run command export")?;
        }
        let defaults = WasiOptions::default();
        let mut store = Store::new(
            &engine,
            State {
                limits: Limits::new(options.memory_bytes, options.instances),
                wasi: options.wasi.as_ref().unwrap_or(&defaults).build()?,
            },
        );
        store.limiter(|state| &mut state.limits);
        // Initialization and run share one budget for the whole command.
        store.set_fuel(options.fuel)?;
        Ok(Self {
            store: CommandStore(Some(store)),
            pre,
            profile,
        })
    }
    async fn run(mut self) -> Result<i32> {
        let store = self.store.get();
        let result: wasmtime::Result<i32> = match self.profile {
            Profile::P2 => (|| {
                let command = CommandPre::new(self.pre)?;
                let instance = command.instantiate(&mut *store)?;
                let result = instance.wasi_cli_run().call_run(&mut *store)?;
                Ok(i32::from(result.is_err()))
            })(),
            Profile::P3(index) => {
                async {
                    let instance = self.pre.instantiate_async(&mut *store).await?;
                    let run = instance.get_func(&mut *store, index).ok_or_else(|| {
                        wasmtime::format_err!("missing validated P3 run function")
                    })?;
                    let mut result = [Val::Bool(false)];
                    store
                        .run_concurrent(async |accessor| {
                            run.call_concurrent(accessor, &[], &mut result).await?;
                            // Publishing a result does not end its task. Keep
                            // driving post-return work, including late traps.
                            std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
                            wasmtime::error::Ok(())
                        })
                        .await??;
                    match &result[0] {
                        Val::Result(Ok(None)) => Ok(0),
                        Val::Result(Err(None)) => Ok(1),
                        _ => Err(wasmtime::format_err!("invalid P3 command result")),
                    }
                }
                .await
            }
        };
        match result {
            Ok(status) => Ok(status),
            Err(error) => match error.downcast_ref::<I32Exit>() {
                Some(exit) => Ok(exit.0),
                None => Err(anyhow::Error::from(error).context("WASI command trapped")),
            },
        }
        // CommandStore retires tasks and releases the native provider context.
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "command_p3_tests.rs"]
mod p3_tests;
