//! Compose a C caller around the Perry guest and cancel one canonical call.
use anyhow::{ensure, Result};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImport, AsyncImports, Driver, Host, Import, Imports, Options, WasiOptions,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use wac_graph::{types::Package, CompositionGraph, EncodeOptions};

type Gates = Arc<Mutex<BTreeMap<u32, oneshot::Sender<Result<Option<Value>>>>>>;
struct Pending(Arc<AtomicUsize>, bool);
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.1 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}
fn compose(args: &[String]) -> Result<()> {
    ensure!(args.len() == 4, "--compose <perry> <caller> <output>");
    let mut graph = CompositionGraph::new();
    let perry = Package::from_file("perry:guest", None, &args[1], graph.types_mut())?;
    let perry = graph.register_package(perry)?;
    let caller = Package::from_file("perry:caller", None, &args[2], graph.types_mut())?;
    let caller = graph.register_package(caller)?;
    let perry = graph.instantiate(perry);
    let caller = graph.instantiate(caller);
    for name in ["run", "cancel-probe", "cancel-poke"] {
        let export = graph.alias_instance_export(perry, name)?;
        graph.set_instantiation_argument(caller, name, export)?;
    }
    let exercise = graph.alias_instance_export(caller, "exercise")?;
    graph.export(exercise, "exercise")?;
    std::fs::write(&args[3], graph.encode(EncodeOptions::default())?)?;
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--compose") {
        return compose(&args);
    }
    ensure!(
        (2..=3).contains(&args.len()),
        "<world> <composed> [--moving]"
    );
    let mut world = perry_wit::parse_file(Path::new(&args[0]), None)?;
    for interface in &mut world.imports {
        if interface.name.is_empty() {
            interface
                .functions
                .retain(|f| !matches!(f.name.as_str(), "run" | "cancel-probe" | "cancel-poke"));
        }
    }
    let gates: Gates = Arc::default();
    let cancelled = Arc::new(AtomicUsize::new(0));
    let mut imports = Imports::new();
    let single: Import = Arc::new(|args| Ok(Some(args[0].clone())));
    imports
        .entry("test:async-guest-imports/service@1.0.0".into())
        .or_default()
        .insert("single".into(), single);
    let scalar: Import = Arc::new(|_| Ok(Some(json!(42))));
    let empty: Import = Arc::new(|_| Ok(None));
    imports
        .entry("".into())
        .or_default()
        .extend([("scalar".into(), scalar), ("empty".into(), empty)]);
    let mut asynchronous = AsyncImports::new();
    let native_gates = gates.clone();
    let counter = cancelled.clone();
    let many: AsyncImport = Arc::new(move |args| {
        let id = args[0].as_u64().unwrap() as u32;
        assert!((1..=3).contains(&id));
        assert_eq!(args.len(), 6);
        let (send, receive) = oneshot::channel();
        assert!(native_gates.lock().unwrap().insert(id, send).is_none());
        let pending = Pending(counter.clone(), false);
        Box::pin(async move {
            let mut pending = pending;
            let result = receive.await?;
            pending.1 = true;
            drop(pending);
            result
        })
    });
    asynchronous
        .entry("test:async-guest-imports/service@1.0.0".into())
        .or_default()
        .insert("many".into(), many);
    let native_gates = gates.clone();
    let proceed: AsyncImport = Arc::new(move |_| {
        let (send, receive) = oneshot::channel();
        assert!(native_gates.lock().unwrap().insert(0, send).is_none());
        Box::pin(async move { receive.await? })
    });
    asynchronous
        .entry("".into())
        .or_default()
        .insert("proceed".into(), proceed);
    let mut wasi = WasiOptions {
        inherit_stdout: true,
        inherit_stderr: true,
        ..WasiOptions::default()
    };
    if args.get(2).is_some_and(|s| s == "--moving") {
        for key in [
            "PERRY_GC_INSTRUMENTS",
            "PERRY_GC_FORCE_EVACUATE",
            "PERRY_GC_VERIFY_EVACUATION",
            "PERRY_GC_DIAG",
        ] {
            wasi.env.insert(key.into(), "1".into());
        }
    }
    let host = Host::new_for_driver(
        &std::fs::read(&args[1])?,
        &world,
        &imports,
        &asynchronous,
        &Options {
            fuel: 2_000_000_000,
            memory_bytes: 256 * 1024 * 1024,
            wasi: Some(wasi),
            ..Options::default()
        },
        Arc::new(|_| Ok(Vec::new())),
    )?;
    let (mut driver, client) = host.into_driver()?;
    // Repeated cancellation exercises handle/buffer/frame reuse in one Store.
    for round in 0..8 {
        let mut call = client.call("", "exercise", &[])?;
        let tick = |driver: &mut Driver| {
            if let Some(result) = block_on(poll_once(driver)) {
                match result {
                    Ok(_) => panic!("driver unexpectedly finished"),
                    Err(error) => panic!("driver failed: {error:#}"),
                }
            }
        };
        for _ in 0..256 {
            tick(&mut driver);
            if gates.lock().unwrap().len() == 4 {
                break;
            }
        }
        ensure!(gates.lock().unwrap().len() == 4, "children did not start");
        let mut pending = std::mem::take(&mut *gates.lock().unwrap());
        pending
            .remove(&0)
            .unwrap()
            .send(Ok(Some(json!(1))))
            .unwrap();
        for _ in 0..256 {
            tick(&mut driver);
            if cancelled.load(Ordering::SeqCst) == (round + 1) * 2 {
                break;
            }
        }
        ensure!(
            cancelled.load(Ordering::SeqCst) == (round + 1) * 2,
            "both native child futures must be cancelled"
        );
        ensure!(pending.remove(&1).unwrap().send(Ok(None)).is_err());
        ensure!(pending.remove(&2).unwrap().send(Ok(None)).is_err());
        ensure!(
            block_on(poll_once(&mut call)).is_none(),
            "sibling must remain pending"
        );
        pending.remove(&3).unwrap().send(Ok(Some(json!({"text":"host sibling 🦀", "bytes":[0,127,255], "labels":{"tag":"some","value":["日本語"]}, "owner":3})))).unwrap();
        let mut result = None;
        for _ in 0..256 {
            tick(&mut driver);
            if let Some(value) = block_on(poll_once(&mut call)) {
                result = Some(value?);
                break;
            }
        }
        ensure!(
            result == Some(Some(json!(77))),
            "sibling result missing: {result:?}"
        );
        for _ in 0..256 {
            tick(&mut driver);
            if !driver.is_active() {
                break;
            }
        }
        ensure!(!driver.is_active(), "cancelled task did not exit");
    }
    println!("PASS: cooperative canonical cancellation, 8 rounds, 16 native children, live sibling and reusable Store");
    Ok(())
}
