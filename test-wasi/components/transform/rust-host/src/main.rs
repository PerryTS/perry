//! A Rust embedding of a Perry library component, with actual P2 providers.
//! The fixture grants no files, network, environment, arguments or stdio.
mod resources;
mod values;

use anyhow::{ensure, Context, Result};
use perry_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime::component::{Component, ComponentType, Lift, Linker, Lower, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreContextMut, StoreLimits, StoreLimitsBuilder};

#[derive(Debug, PartialEq, ComponentType, Lift, Lower)]
#[component(record)]
struct Request {
    text: String,
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, ComponentType, Lift, Lower)]
#[component(variant)]
enum Failure {
    #[component(name = "invalid-input")]
    InvalidInput(String),
    #[component(name = "unavailable")]
    Unavailable,
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    logs: Vec<String>,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .context("usage: perry-component-rust-host <component.wasm>")?;
    let dual_resources = std::env::args().nth(2).as_deref() == Some("--resources-dual");
    let moving_resources = std::env::args().nth(2).as_deref() == Some("--resources-moving");
    let resource_profile = dual_resources
        || moving_resources
        || std::env::args().nth(2).as_deref() == Some("--resources");
    let all_values = std::env::args().nth(2).as_deref() == Some("--values");
    let composed = std::env::args().nth(2).as_deref() == Some("--composed");
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .wasm_exceptions(true)
        .consume_fuel(true);
    let engine = Engine::new(&config)?;
    let component = Component::from_binary(&engine, &std::fs::read(path)?)?;
    let mut linker = Linker::<State>::new(&engine);
    perry_wasi::p2::add_to_linker_sync(&mut linker)?;
    if resource_profile {
        resources::link(&mut linker)?;
    } else if all_values {
        values::link(&mut linker)?;
    } else {
        linker
            .instance(if composed {
                "perry:transform/audit@1.0.0"
            } else {
                "perry:transform/logging@1.0.0"
            })?
            .func_wrap(
                if composed { "emit" } else { "log" },
                move |mut store: StoreContextMut<'_, State>, (message,): (String,)| {
                    let text = if composed {
                        message.strip_prefix("Rust logger: ").ok_or_else(|| {
                            wasmtime::format_err!("composed Rust logger did not execute")
                        })?
                    } else {
                        &message
                    };
                    wasmtime::ensure!(
                        text == "héllo 🌍" || text.is_empty(),
                        "wrong imported logger text"
                    );
                    store.data_mut().logs.push(message);
                    Ok(())
                },
            )?;
    }
    // Turnloop contexts grant no files, network, environment or stdio by default.
    let mut wasi = WasiCtx::new(64)?;
    if resource_profile {
        wasi.cli.inherit_stderr();
    }
    if moving_resources {
        for name in [
            "PERRY_GC_FORCE_EVACUATE",
            "PERRY_GC_VERIFY_EVACUATION",
            "PERRY_GC_DIAG",
        ] {
            wasi.cli.environment.push((name.into(), "1".into()));
        }
    }
    let mut store = Store::new(
        &engine,
        State {
            wasi,
            table: ResourceTable::new(),
            logs: Vec::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(256 * 1024 * 1024)
                .instances(64)
                .build(),
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_fuel(100_000_000)?;
    let instance = linker.instantiate(&mut store, &component)?;
    if dual_resources {
        return resources::run_dual(&mut store, &instance);
    }
    if resource_profile {
        return resources::run(&mut store, &instance);
    }
    if all_values {
        return values::run(&mut store, &instance);
    }
    let namespace = instance
        .get_export_index(&mut store, None, "perry:transform/transform@1.0.0")
        .context("missing versioned transform interface")?;
    let index = instance
        .get_export_index(&mut store, Some(&namespace), "transform")
        .context("missing transform export")?;
    let transform = instance
        .get_func(&mut store, &index)
        .context("transform is not a function")?
        .typed::<(Request,), (std::result::Result<Request, Failure>,)>(&store)?;
    for _ in 0..50 {
        store.set_fuel(100_000_000)?;
        let (result,) = transform.call(
            &mut store,
            (Request {
                text: "héllo 🌍".into(),
                bytes: vec![0, 127, 255],
            },),
        )?;
        ensure!(
            result
                == Ok(Request {
                    text: "héllo 🌍 / Perry".into(),
                    bytes: vec![255, 127, 0]
                }),
            "wrong structured result: {result:?}"
        );
    }
    store.set_fuel(100_000_000)?;
    let (error,) = transform.call(
        &mut store,
        (Request {
            text: String::new(),
            bytes: Vec::new(),
        },),
    )?;
    ensure!(
        error == Err(Failure::InvalidInput("text must not be empty".into())),
        "wrong typed WIT error: {error:?}"
    );
    ensure!(
        store.data().logs.len() == 51,
        "module initialization or import call count mismatch"
    );
    drop(store);
    if composed {
        println!("PASS direct composition: Perry guest imports Rust logger, 51 audited calls");
    }
    println!("PASS Perry component: Rust host, 51 typed calls, Unicode, bytes, guest GC, imports");
    Ok(())
}
