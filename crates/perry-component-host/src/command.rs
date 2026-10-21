//! One-shot WASI P2 command execution, separate from reusable library worlds.
use crate::{limits::Limits, wasi, Options, WasiOptions};
use anyhow::{bail, ensure, Context, Result};
use wasmtime::component::{types::ComponentItem, Component, Linker, Type};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::p2::bindings::sync::CommandPre;
use wasmtime_wasi::{I32Exit, WasiCtxView, WasiView};

struct State {
    limits: Limits,
    wasi: wasi::State,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        self.wasi.view()
    }
}

fn validate_command(component: &Component) -> Result<()> {
    // Match the exact lookup used by the lockfile-pinned Command bindings,
    // including Wasmtime's compatible-version lookup. Do not inspect an
    // arbitrary first cli/run export if the component exports several versions.
    let (_, interface) = component
        .get_export(None, "wasi:cli/run@0.2.12")
        .context("missing wasi:cli/run interface")?;
    let (item, _) = component
        .get_export(Some(&interface), "run")
        .context("missing wasi:cli/run function")?;
    let ComponentItem::ComponentFunc(run) = item else {
        bail!("wasi:cli/run export is not a function");
    };
    ensure!(
        run.params().len() == 0,
        "wasi:cli/run must take no arguments"
    );
    let results: Vec<_> = run.results().collect();
    ensure!(
        matches!(results.as_slice(), [Type::Result(result)] if result.ok().is_none() && result.err().is_none()),
        "wasi:cli/run must return result without payloads"
    );
    Ok(())
}

/// Execute `wasi:cli/run` once in a fresh store. Returned `ok`/`err` become
/// status 0/1; WASI exit requests preserve their status without exiting Perry.
/// Traps, invalid components and capability/configuration errors remain errors.
/// Omitting `options.wasi` uses the same capability defaults as `wasi: {}`.
pub fn run_command(bytes: &[u8], options: &Options) -> Result<i32> {
    ensure!(
        options.memory_bytes > 0 && options.fuel > 0 && options.instances > 0,
        "component limits must be positive"
    );
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .wasm_exceptions(true)
        .consume_fuel(true);
    let engine = Engine::new(&config)?;
    let component = Component::from_binary(&engine, bytes)
        .map_err(anyhow::Error::from)
        .context("compiling WASI command")?;
    // CommandPre checks names, but generated function type checks happen only
    // after instantiation. Validate the signature before an initializer exits.
    validate_command(&component).context("validating wasi:cli/run command export")?;
    let mut linker = Linker::<State>::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
    // Validate the command export before an initializer can request exit.
    // A malformed component must not turn an init-time exit(0) into success.
    let command = CommandPre::new(linker.instantiate_pre(&component)?)
        .map_err(anyhow::Error::from)
        .context("validating wasi:cli/run command export")?;
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
    let result = (|| -> wasmtime::Result<i32> {
        let instance = command.instantiate(&mut store)?;
        let result = instance.wasi_cli_run().call_run(&mut store)?;
        Ok(i32::from(result.is_err()))
    })();
    match result {
        Ok(status) => Ok(status),
        Err(error) => match error.downcast_ref::<I32Exit>() {
            Some(exit) => Ok(exit.0),
            None => Err(anyhow::Error::from(error).context("WASI command trapped")),
        },
    }
    // The fresh store and its provider resources are always released here.
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
