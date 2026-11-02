//! A maintained-C caller tests canonical cancellation after a Perry actor returns.
#[path = "support/guest_diagnostics.rs"]
mod guest_diagnostics;
use anyhow::{bail, ensure, Context, Result};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImport, AsyncImports, Driver, Host, Import, Imports, Options, WasiOptions,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wac_graph::{types::Package, CompositionGraph, EncodeOptions};

type Sender = oneshot::Sender<Result<Option<Value>>>;
type Gate = Arc<Mutex<Option<Sender>>>;

struct Pending {
    aborted: Arc<AtomicUsize>,
    completed: bool,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.completed {
            self.aborted.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn compose(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 5,
        "--compose <producer> <caller> <plan> <output>"
    );
    let plan: Value = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let actor = plan["actor"].as_str().context("private actor name")?;
    let mut graph = CompositionGraph::new();
    let producer =
        Package::from_file("perry:returned-producer", None, &args[1], graph.types_mut())?;
    let producer = graph.register_package(producer)?;
    let caller = Package::from_file("perry:returned-caller", None, &args[2], graph.types_mut())?;
    let caller = graph.register_package(caller)?;
    let producer = graph.instantiate(producer);
    let caller = graph.instantiate(caller);
    for (local, original) in [("start", "guarded"), ("kick", actor)] {
        let export = graph.alias_instance_export(producer, original)?;
        graph.set_instantiation_argument(caller, local, export)?;
    }
    let exercise = graph.alias_instance_export(caller, "exercise")?;
    graph.export(exercise, "exercise")?;
    std::fs::write(&args[4], graph.encode(EncodeOptions::default())?)?;
    Ok(())
}

fn tick(driver: &mut Driver) -> Result<()> {
    if let Some(result) = block_on(poll_once(driver)) {
        result?;
        bail!("returned-actor driver unexpectedly finished");
    }
    Ok(())
}

fn exercise(wit: &Path, component: &[u8], cancel: bool, moving: bool) -> Result<()> {
    let mut world = perry_wit::parse_file(wit, Some("supervisor"))?;
    let plugin = perry_wit::parse_file(wit, Some("plugin"))?;
    let root = world
        .imports
        .iter_mut()
        .find(|i| i.name.is_empty())
        .context("caller imports")?;
    root.functions
        .retain(|f| !matches!(f.name.as_str(), "start" | "kick"));
    root.functions
        .extend(plugin.imports.iter().flat_map(|i| &i.functions).cloned());
    let child: Gate = Arc::default();
    let gate: Gate = Arc::default();
    let aborted = Arc::new(AtomicUsize::new(0));
    let polled = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let marks: Arc<Mutex<Vec<u32>>> = Arc::default();
    let observed = marks.clone();
    let mark: Import = Arc::new(move |args| {
        observed
            .lock()
            .unwrap()
            .push(u32::try_from(args[0].as_u64().context("mark value")?)?);
        Ok(None)
    });
    let mode: Import = Arc::new(move |_| Ok(Some(json!(cancel))));
    let imports = Imports::from([(
        "".into(),
        BTreeMap::from([("mark".into(), mark), ("cancel-mode".into(), mode)]),
    )]);
    let slot = child.clone();
    let count = aborted.clone();
    let entered = polled.clone();
    let finished = completed.clone();
    let empty: AsyncImport = Arc::new(move |_| {
        let (send, receive) = oneshot::channel();
        assert!(slot.lock().unwrap().replace(send).is_none());
        let mut pending = Pending {
            aborted: count.clone(),
            completed: false,
        };
        let entered = entered.clone();
        let finished = finished.clone();
        Box::pin(async move {
            entered.fetch_add(1, Ordering::SeqCst);
            let reply = receive.await?;
            pending.completed = true;
            finished.fetch_add(1, Ordering::SeqCst);
            drop(pending);
            reply
        })
    });
    let slot = gate.clone();
    let proceed: AsyncImport = Arc::new(move |_| {
        let (send, receive) = oneshot::channel();
        assert!(slot.lock().unwrap().replace(send).is_none());
        Box::pin(async move { receive.await? })
    });
    let asynchronous = AsyncImports::from([(
        "".into(),
        BTreeMap::from([("empty".into(), empty), ("proceed".into(), proceed)]),
    )]);
    let mut wasi = WasiOptions {
        inherit_stderr: true,
        ..WasiOptions::default()
    };
    let _diagnostics = guest_diagnostics::configure(&mut wasi, moving, 313);
    let host = Host::new_for_driver(
        component,
        &world,
        &imports,
        &asynchronous,
        &Options {
            fuel: 1_000_000_000,
            memory_bytes: 256 * 1024 * 1024,
            wasi: Some(wasi),
            ..Options::default()
        },
        Arc::new(|_| Ok(Vec::new())),
    )?;
    let (mut driver, client) = host.into_driver()?;
    let mut call = client.call("", "exercise", &[])?;
    let deadline = Instant::now() + Duration::from_secs(45);
    while polled.load(Ordering::SeqCst) != 1 || gate.lock().unwrap().is_none() {
        ensure!(
            Instant::now() < deadline,
            "returned actor did not start its native child"
        );
        tick(&mut driver)?;
        ensure!(
            block_on(poll_once(&mut call)).is_none(),
            "caller returned before native gate"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    ensure!(
        *marks.lock().unwrap() == [1, 2],
        "C caller did not observe RETURNED"
    );
    ensure!(aborted.load(Ordering::SeqCst) == 0 && completed.load(Ordering::SeqCst) == 0);
    ensure!(
        child.lock().unwrap().is_some(),
        "native child is not pending"
    );
    gate.lock()
        .unwrap()
        .take()
        .unwrap()
        .send(Ok(Some(json!(1))))
        .map_err(|_| anyhow::anyhow!("caller gate retired early"))?;
    let outcome = loop {
        ensure!(
            Instant::now() < deadline,
            "returned-actor caller did not finish"
        );
        if let Err(error) = tick(&mut driver) {
            break Err(error);
        }
        if let Some(reply) = block_on(poll_once(&mut call)) {
            break reply;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    if cancel {
        let error = outcome.expect_err("terminal subtask cancellation must trap");
        ensure!(
            error.downcast_ref::<wasmtime::Trap>()
                == Some(&wasmtime::Trap::SubtaskCancelAfterTerminal),
            "wrong canonical trap: {error:#}"
        );
        ensure!(
            *marks.lock().unwrap() == [1, 2, 3],
            "cancellation returned to C"
        );
        drop(call);
        drop(client);
        drop(driver);
        ensure!(
            aborted.load(Ordering::SeqCst) == 1,
            "Store retirement did not drop its native child exactly once"
        );
        ensure!(completed.load(Ordering::SeqCst) == 0);
        ensure!(
            child
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(Ok(None))
                .is_err(),
            "native child remained live after Store retirement"
        );
    } else {
        ensure!(outcome?.is_none(), "void caller result");
        ensure!(*marks.lock().unwrap() == [1, 2, 3, 4]);
        ensure!(
            aborted.load(Ordering::SeqCst) == 0 && completed.load(Ordering::SeqCst) == 0,
            "dropping a returned handle cancelled its live child"
        );
        child
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send(Ok(None))
            .map_err(|_| anyhow::anyhow!("control child retired early"))?;
        while driver.is_active() {
            ensure!(Instant::now() < deadline, "control actor failed to quiesce");
            tick(&mut driver)?;
            std::thread::sleep(Duration::from_millis(1));
        }
        ensure!(completed.load(Ordering::SeqCst) == 1 && aborted.load(Ordering::SeqCst) == 0);
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
        "guest_returned_actor <wit> <composed> [--moving]"
    );
    ensure!(args.len() == 2 || args[2] == "--moving", "unknown mode");
    let moving = args.len() == 3;
    let component = std::fs::read(&args[1])?;
    for _ in 0..4 {
        exercise(Path::new(&args[0]), &component, false, moving)?;
        exercise(Path::new(&args[0]), &component, true, moving)?;
    }
    println!("PASS: returned private actor, four positive handle-drop controls, four SubtaskCancelAfterTerminal traps and four exactly-once native child retirements");
    Ok(())
}
