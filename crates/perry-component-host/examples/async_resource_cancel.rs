//! Resource cancellation crosses real C/Perry/C component boundaries.
use anyhow::{ensure, Context, Result};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImport, AsyncImports, Driver, Host, Import, Imports, Options, WasiOptions,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
};
use wac_graph::{types::Package, CompositionGraph, EncodeOptions};

const FOREIGN: &str = "test:async-guest-resources/foreign@1.0.0";
const INTERNAL: &[&str] = &[
    "cancel-exchange",
    "cancel-loan",
    "cancelled-expired",
    "loan",
    "published",
];

fn compose(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 5,
        "--compose <provider> <perry> <caller> <output>"
    );
    let mut graph = CompositionGraph::new();
    let mut packages = Vec::new();
    for (name, path) in ["provider", "guest", "caller"].into_iter().zip(&args[1..4]) {
        let package = Package::from_file(&format!("test:{name}"), None, path, graph.types_mut())?;
        packages.push(graph.register_package(package)?);
    }
    let provider = graph.instantiate(packages[0]);
    let foreign = graph.alias_instance_export(provider, FOREIGN)?;
    let item = graph.alias_instance_export(foreign, "item")?;
    let guest = graph.instantiate(packages[1]);
    graph.set_instantiation_argument(guest, FOREIGN, foreign)?;
    graph.set_instantiation_argument(guest, "item", item)?;
    let caller = graph.instantiate(packages[2]);
    // Both consumers receive the same instance, including its nominal type.
    graph.set_instantiation_argument(caller, FOREIGN, foreign)?;
    graph.set_instantiation_argument(caller, "item", item)?;
    for name in INTERNAL {
        let function = graph.alias_instance_export(guest, *name)?;
        graph.set_instantiation_argument(caller, *name, function)?;
    }
    let exercise = graph.alias_instance_export(caller, "exercise")?;
    graph.export(exercise, "exercise")?;
    std::fs::write(&args[4], graph.encode(EncodeOptions::default())?)?;
    Ok(())
}

struct Proceed {
    mode: u32,
    base: u32,
    send: oneshot::Sender<Result<Option<Value>>>,
}
#[derive(Default)]
struct Events {
    gates: BTreeMap<u32, oneshot::Sender<()>>,
    proceed: Option<Proceed>,
    submitted: BTreeSet<u32>,
    entered: BTreeSet<u32>,
    ready: BTreeSet<u32>,
    dropped: BTreeMap<u32, usize>,
    acknowledged: BTreeMap<u32, (u32, u32)>,
}
type Shared = Arc<Mutex<Events>>;

fn tick(driver: &mut Driver) -> Result<()> {
    if let Some(reply) = block_on(poll_once(driver)) {
        reply?;
        anyhow::bail!("driver exited before the client");
    }
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--compose") {
        return compose(&args);
    }
    ensure!(
        (2..=3).contains(&args.len()),
        "<world.wit> <composed.wasm> [--moving]"
    );
    let mut world = perry_wit::parse_file(Path::new(&args[0]), Some("supervisor"))?;
    world.imports.retain(|interface| interface.name != FOREIGN);
    for interface in &mut world.imports {
        if interface.name.is_empty() {
            interface
                .functions
                .retain(|f| !INTERNAL.contains(&f.name.as_str()));
        }
    }
    let events: Shared = Arc::default();
    let mut imports = Imports::new();
    let root = imports.entry(String::new()).or_default();
    for name in ["submitted", "entered", "ready", "dropped"] {
        let observed = events.clone();
        let callback: Import = Arc::new(move |args| {
            let id = args[0].as_u64().context("observer ID")? as u32;
            let mut events = observed.lock().unwrap();
            match name {
                "submitted" => ensure!(events.submitted.insert(id)),
                "entered" => ensure!(events.entered.insert(id)),
                "dropped" => {
                    let count = events.dropped.entry(id).or_default();
                    *count += 1;
                    ensure!(*count == 1, "resource {id} dropped twice");
                }
                "ready" => {
                    ensure!(events.ready.insert(id));
                    let proceed = events
                        .proceed
                        .take()
                        .context("caller cancellation gate missing")?;
                    ensure!(proceed.mode == 2 && proceed.base + 10 == id);
                    drop(events);
                    proceed
                        .send
                        .send(Ok(Some(json!(1))))
                        .map_err(|_| anyhow::anyhow!("caller cancellation observer dropped"))?;
                    return Ok(None);
                }
                _ => unreachable!(),
            }
            Ok(None)
        });
        root.insert(name.into(), callback);
    }
    let observed = events.clone();
    let ack: Import = Arc::new(move |args| {
        let mode = args[0].as_u64().unwrap() as u32;
        let base = args[1].as_u64().unwrap() as u32;
        let state = args[2].as_u64().unwrap() as u32;
        let mut events = observed.lock().unwrap();
        ensure!(
            events.dropped.get(&(base + 1)) == Some(&1),
            "input owner was not released at acknowledgement"
        );
        ensure!(
            !events.dropped.contains_key(&(base + 2)),
            "live sibling lost its owner"
        );
        ensure!(
            !events.gates[&(base + 11)].is_canceled(),
            "sibling was cancelled"
        );
        match mode {
            0 => {
                ensure!(events.submitted.contains(&(base + 10)));
                ensure!(
                    !events.entered.contains(&(base + 10)),
                    "backpressured callee consumed its arguments"
                );
                ensure!(!events.gates.contains_key(&(base + 10)));
            }
            1 => {
                ensure!(
                    events.gates[&(base + 10)].is_canceled(),
                    "cancelled borrow still has native work"
                );
                ensure!(!events.entered.contains(&(base + 10)));
            }
            2 => {
                ensure!(
                    events.ready.contains(&(base + 10)),
                    "no owned result raced cancellation"
                );
                ensure!(events.entered.contains(&(base + 10)));
                ensure!(
                    events.dropped.get(&(base + 1 + 1000000)) == Some(&1),
                    "returned owner was leaked or published without cleanup"
                );
            }
            _ => anyhow::bail!("unknown scenario"),
        }
        ensure!(events.acknowledged.insert(base, (mode, state)).is_none());
        Ok(None)
    });
    root.insert("acknowledged".into(), ack);
    let mut asynchronous = AsyncImports::new();
    let root = asynchronous.entry(String::new()).or_default();
    for name in ["gate", "wait"] {
        let observed = events.clone();
        let callback: AsyncImport = Arc::new(move |args| {
            let id = args[0].as_u64().unwrap() as u32;
            let (send, receive) = oneshot::channel();
            assert!(observed.lock().unwrap().gates.insert(id, send).is_none());
            Box::pin(async move {
                receive.await?;
                Ok(None)
            })
        });
        root.insert(name.into(), callback);
    }
    let observed = events.clone();
    let proceed: AsyncImport = Arc::new(move |args| {
        let (send, receive) = oneshot::channel();
        let mut events = observed.lock().unwrap();
        assert!(events.proceed.is_none());
        events.proceed = Some(Proceed {
            mode: args[0].as_u64().unwrap() as u32,
            base: args[1].as_u64().unwrap() as u32,
            send,
        });
        Box::pin(async move { receive.await? })
    });
    root.insert("proceed".into(), proceed);
    let mut wasi = WasiOptions {
        inherit_stderr: true,
        ..WasiOptions::default()
    };
    if args.get(2).is_some_and(|arg| arg == "--moving") {
        for key in [
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
            memory_bytes: 256 * 1024 * 1024,
            fuel: 2_000_000_000,
            wasi: Some(wasi),
            ..Options::default()
        },
        Arc::new(|_| Ok(Vec::new())),
    )?;
    let (mut driver, client) = host.into_driver()?;
    for mode in 0..3 {
        for round in 0..8 {
            let base = 1000 + mode * 100000 + round * 100;
            let mut call = client.call("", "exercise", &[json!(mode), json!(base)])?;
            let mut started = false;
            for _ in 0..256 {
                tick(&mut driver)?;
                let events = events.lock().unwrap();
                started = events
                    .proceed
                    .as_ref()
                    .is_some_and(|p| p.mode == mode && p.base == base)
                    && events.gates.contains_key(&(base + 11))
                    && match mode {
                        0 => events.submitted.contains(&(base + 10)),
                        1 => events.gates.contains_key(&(base + 10)),
                        _ => {
                            events.submitted.contains(&(base + 10))
                                && events.entered.contains(&(base + 10))
                                && events.gates.contains_key(&(base + 10))
                        }
                    };
                if started {
                    break;
                }
            }
            ensure!(
                started,
                "resource calls did not suspend: mode {mode}, base {base}"
            );
            if mode == 2 {
                events
                    .lock()
                    .unwrap()
                    .gates
                    .remove(&(base + 10))
                    .unwrap()
                    .send(())
                    .unwrap();
            } else {
                events
                    .lock()
                    .unwrap()
                    .proceed
                    .take()
                    .unwrap()
                    .send
                    .send(Ok(Some(json!(1))))
                    .unwrap();
            }
            let mut acked = false;
            for _ in 0..256 {
                tick(&mut driver)?;
                acked = events.lock().unwrap().acknowledged.contains_key(&base);
                if acked {
                    break;
                }
            }
            ensure!(acked, "canonical cancellation never acknowledged");
            ensure!(
                block_on(poll_once(&mut call)).is_none(),
                "sibling stopped being pending"
            );
            if mode == 1 {
                ensure!(
                    events
                        .lock()
                        .unwrap()
                        .gates
                        .remove(&(base + 10))
                        .unwrap()
                        .send(())
                        .is_err(),
                    "late completion revived a cancelled borrow"
                );
            }
            events
                .lock()
                .unwrap()
                .gates
                .remove(&(base + 11))
                .unwrap()
                .send(())
                .unwrap();
            let mut reply = None;
            for _ in 0..256 {
                tick(&mut driver)?;
                if let Some(result) = block_on(poll_once(&mut call)) {
                    reply = Some(result?);
                    break;
                }
            }
            ensure!(reply == Some(Some(json!(77))), "sibling did not finish");
            for _ in 0..256 {
                tick(&mut driver)?;
                if !driver.is_active() {
                    break;
                }
            }
            ensure!(!driver.is_active(), "cancelled resource task did not exit");
            let events = events.lock().unwrap();
            ensure!(events.proceed.is_none() && events.gates.is_empty());
            ensure!(events.dropped.get(&(base + 2)) == Some(&1));
        }
    }
    let events = events.lock().unwrap();
    ensure!(events.acknowledged.len() == 24 && events.dropped.len() == 56);
    let mut race_states = BTreeMap::<u32, usize>::new();
    for (mode, state) in events.acknowledged.values() {
        if *mode == 2 {
            *race_states.entry(*state).or_default() += 1;
        }
    }
    println!(
        "RESOURCE_CANCEL_STATES={}",
        serde_json::to_string(&race_states)?
    );
    println!("PASS: resource canonical cancellation, 8 pre-start owners, 8 suspended borrows, 8 ready owned results, 24 live siblings and reusable Store");
    Ok(())
}
