//! Actual sync exports stage canonical async imports, with independent native
//! gates, resource loans/owners, post-return futures and interleaved ALS.
use anyhow::{bail, ensure, Context, Result};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImport, AsyncImports, Driver, DriverCall, DriverClient, Host, Import, Imports, Options,
    ResourceEvent, WasiOptions,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const API: &str = "perry:guest-sync-imports/api@1.0.0";
const FOREIGN: &str = "perry:guest-sync-imports/foreign@1.0.0";
type Gates = Arc<Mutex<BTreeMap<(u32, bool), oneshot::Sender<()>>>>;
struct GuestDiagnostics(perry_wasi::pipe::MemoryOutputPipe);
impl Drop for GuestDiagnostics {
    fn drop(&mut self) {
        let _ = std::io::stderr().write_all(&self.0.contents());
    }
}
fn tick(driver: &mut Driver) -> Result<()> {
    if let Some(reply) = block_on(poll_once(driver)) {
        reply?;
        bail!("staged import driver stopped while its client is live");
    }
    Ok(())
}
fn result(driver: &mut Driver, call: &mut DriverCall) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        tick(driver)?;
        if let Some(reply) = block_on(poll_once(&mut *call)) {
            return Ok(reply?.unwrap_or(Value::Null));
        }
        ensure!(Instant::now() < deadline, "staged import timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn call(driver: &mut Driver, client: &DriverClient, name: &str, args: &[Value]) -> Result<Value> {
    result(driver, &mut client.call(API, name, args)?)
        .with_context(|| format!("staged import export {name}"))
}
fn borrowed(value: &Value) -> Value {
    let mut value = value.clone();
    value["owned"] = json!(false);
    value
}
fn release(gates: &Gates, id: u32, exchange: bool) -> Result<()> {
    gates
        .lock()
        .unwrap()
        .remove(&(id, exchange))
        .context("native import did not suspend")?
        .send(())
        .map_err(|_| anyhow::anyhow!("native import was cancelled"))
}
fn verify(driver: &mut Driver, client: &DriverClient, reply: Value, id: u32) -> Result<()> {
    ensure!(reply["context"] == id && reply["text"] == format!("staged-{id} 日本語 🍐"));
    ensure!(reply["bytes"] == json!([0, 128, 255, id]));
    let owner = &reply["owner"];
    ensure!(owner["owned"] == true);
    ensure!(call(driver, client, "resource-value", &[borrowed(owner)])? == id * 2 + 1);
    result(driver, &mut client.drop_resource(owner)?)?;
    ensure!(
        result(driver, &mut client.drop_resource(owner)?).is_err(),
        "owner dropped twice"
    );
    Ok(())
}
fn run(wit: &Path, component: &Path, moving: bool, inline: bool) -> Result<()> {
    let world = perry_wit::parse_file(wit, None)?;
    let api = world
        .exports
        .iter()
        .find(|i| i.name == API)
        .context("public interface")?;
    ensure!(
        api.functions.iter().all(|f| !f.async_),
        "all public exports must be synchronous"
    );
    let key = world
        .imports
        .iter()
        .find(|i| i.name == FOREIGN)
        .context("foreign interface")?
        .resources["item"]
        .clone();
    let values: Arc<Mutex<BTreeMap<String, u32>>> = Arc::default();
    let drops: Arc<Mutex<BTreeSet<String>>> = Arc::default();
    let sealed: Arc<Mutex<BTreeSet<u32>>> = Arc::default();
    let loans: Arc<Mutex<BTreeSet<String>>> = Arc::default();
    let gates: Gates = Arc::default();
    let starts = Arc::new(AtomicUsize::new(0));
    let created = values.clone();
    let make: Import = Arc::new(move |args| {
        let id = args[0].as_u64().context("resource value")? as u32;
        let token = format!("item-{id}");
        ensure!(created.lock().unwrap().insert(token.clone(), id).is_none());
        Ok(Some(
            json!({"$hostResource":token,"type":key,"generation":1,"owned":true,"new":true}),
        ))
    });
    let readable = values.clone();
    let read: Import = Arc::new(move |args| {
        ensure!(args[0]["owned"] == false);
        let token = args[0]["$hostResource"]
            .as_str()
            .context("native resource")?;
        Ok(Some(json!(*readable
            .lock()
            .unwrap()
            .get(token)
            .context("resource already dropped")?)))
    });
    let marked = sealed.clone();
    let seal: Import = Arc::new(move |args| {
        ensure!(marked
            .lock()
            .unwrap()
            .insert(args[0].as_u64().unwrap() as u32));
        Ok(None)
    });
    let imports: Imports = BTreeMap::from([(
        FOREIGN.into(),
        BTreeMap::from([
            ("[constructor]item".into(), make),
            ("[method]item.read".into(), read),
            ("seal".into(), seal),
        ]),
    )]);
    let mut asynchronous = AsyncImports::new();
    for exchange in [false, true] {
        let marked = sealed.clone();
        let readable = values.clone();
        let pending = loans.clone();
        let waiting = gates.clone();
        let started = starts.clone();
        let handler: AsyncImport = Arc::new(move |args| {
            let id = args[0].as_u64().unwrap() as u32;
            assert!(
                marked.lock().unwrap().contains(&id),
                "async import started inside sync JS factory or incoming borrow escaped"
            );
            assert_eq!(args[1]["owned"], exchange);
            let token = args[1]["$hostResource"].as_str().unwrap().to_string();
            let value = readable.lock().unwrap()[&token];
            assert_eq!(value, id * 2 + u32::from(exchange));
            if !exchange {
                assert!(pending.lock().unwrap().insert(token.clone()));
            }
            started.fetch_add(1, Ordering::SeqCst);
            let result = if exchange {
                args[1].clone()
            } else {
                json!(value)
            };
            let receive = if inline {
                None
            } else {
                let (send, receive) = oneshot::channel();
                assert!(waiting
                    .lock()
                    .unwrap()
                    .insert((id, exchange), send)
                    .is_none());
                Some(receive)
            };
            let pending = pending.clone();
            let readable = readable.clone();
            Box::pin(async move {
                if let Some(receive) = receive {
                    receive.await?;
                }
                ensure!(
                    readable.lock().unwrap().get(&token) == Some(&value),
                    "resource died before native import acknowledgement"
                );
                if !exchange {
                    ensure!(pending.lock().unwrap().remove(&token));
                }
                Ok(Some(result))
            })
        });
        asynchronous
            .entry(FOREIGN.into())
            .or_default()
            .insert(if exchange { "exchange" } else { "loan" }.into(), handler);
    }
    let diagnostics =
        moving.then(|| GuestDiagnostics(perry_wasi::pipe::MemoryOutputPipe::new(16 * 1024 * 1024)));
    let mut wasi = WasiOptions {
        inherit_stderr: !moving,
        stderr_buffer: diagnostics.as_ref().map(|d| d.0.clone()),
        ..WasiOptions::default()
    };
    if moving {
        for key in [
            "PERRY_GC_INSTRUMENTS",
            "PERRY_GC_FORCE_EVACUATE",
            "PERRY_GC_VERIFY_EVACUATION",
            "PERRY_GC_DIAG",
        ] {
            wasi.env.insert(key.into(), "1".into());
        }
        wasi.env
            .insert("PERRY_GC_SCHEDULE_SEED".into(), "317".into());
        wasi.env.insert("PERRY_GC_SCHEDULE_RATE".into(), "1".into());
    }
    let live = values.clone();
    let observed = drops.clone();
    let pending = loans.clone();
    let host = Host::new_for_driver(
        &std::fs::read(component)?,
        &world,
        &imports,
        &asynchronous,
        &Options {
            fuel: 3_000_000_000,
            memory_bytes: 256 * 1024 * 1024,
            wasi: Some(wasi),
            ..Options::default()
        },
        Arc::new(move |event| {
            if let ResourceEvent::Drop { resource } = event {
                ensure!(
                    !pending.lock().unwrap().contains(&resource.token),
                    "drop with unacknowledged native loan"
                );
                ensure!(
                    live.lock().unwrap().remove(&resource.token).is_some(),
                    "duplicate native resource drop"
                );
                ensure!(
                    observed.lock().unwrap().insert(resource.token),
                    "native owner dropped twice"
                );
            }
            Ok(Vec::new())
        }),
    )?;
    let (mut driver, client) = host.into_driver()?;
    let guarded = call(&mut driver, &client, "make-resource", &[json!(9999)])?;
    ensure!(call(&mut driver, &client, "guarded", &[borrowed(&guarded)])? == 77);
    ensure!(starts.load(Ordering::SeqCst) == 0 && gates.lock().unwrap().is_empty());
    ensure!(
        call(
            &mut driver,
            &client,
            "resource-value",
            &[borrowed(&guarded)]
        )? == 9999
    );
    result(&mut driver, &mut client.drop_resource(&guarded)?)?;
    for round in 0..8 {
        let a = round * 2 + 1;
        let b = a + 1;
        let fa = call(&mut driver, &client, "make", &[json!(a)])?;
        let fb = call(&mut driver, &client, "make", &[json!(b)])?;
        let mut ra = client.read_async_value(&fa, None)?;
        let mut rb = client.read_async_value(&fb, None)?;
        if !inline {
            let deadline = Instant::now() + Duration::from_secs(45);
            while gates.lock().unwrap().len() != 4 {
                tick(&mut driver)?;
                ensure!(
                    Instant::now() < deadline,
                    "parallel native imports never suspended"
                );
            }
            ensure!(
                block_on(poll_once(&mut ra)).is_none() && block_on(poll_once(&mut rb)).is_none()
            );
            release(&gates, b, true)?;
            release(&gates, a, true)?;
            for _ in 0..16 {
                tick(&mut driver)?;
            }
            ensure!(
                drops.lock().unwrap().len() == 1 + round as usize * 4,
                "owner dropped before loan acknowledgement"
            );
            ensure!(
                block_on(poll_once(&mut ra)).is_none() && block_on(poll_once(&mut rb)).is_none()
            );
            release(&gates, b, false)?;
        }
        let reply = result(&mut driver, &mut rb)?;
        verify(&mut driver, &client, reply, b)?;
        if !inline {
            ensure!(block_on(poll_once(&mut ra)).is_none());
            release(&gates, a, false)?;
        }
        let reply = result(&mut driver, &mut ra)?;
        verify(&mut driver, &client, reply, a)?;
        let counts = call(&mut driver, &client, "checkpoint", &[])?;
        ensure!(
            counts == json!({"completed":b,"failed":0,"blocked":b,"guards":1}),
            "producer/ALS counters: {counts}"
        );
        ensure!(drops.lock().unwrap().len() == 1 + (round as usize + 1) * 4);
    }
    ensure!(values.lock().unwrap().is_empty() && drops.lock().unwrap().len() == 33);
    ensure!(gates.lock().unwrap().is_empty() && loans.lock().unwrap().is_empty());
    ensure!(starts.load(Ordering::SeqCst) == 32 && sealed.lock().unwrap().len() == 16);
    let deadline = Instant::now() + Duration::from_secs(45);
    while driver.is_active() {
        tick(&mut driver)?;
        ensure!(Instant::now() < deadline, "private actor leaked");
    }
    eprintln!("PASS staged import mode inline={inline}: 16 producers, 32 native imports, 33 exactly-once resource drops, no pending actor");
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "usage: guest_sync_imports <wit> <component> [--moving]"
    );
    ensure!(args.len() == 2 || args[2] == "--moving", "unknown mode");
    for inline in [true, false] {
        run(
            Path::new(&args[0]),
            Path::new(&args[1]),
            args.len() == 3,
            inline,
        )?;
    }
    println!("PASS: actual Perry P3 synchronous exports stage async imports, incoming borrow guard, owned loans and transfers, ready and reverse delayed completion, ALS isolation, future results, 33 exactly-once drops per mode and Store reuse");
    Ok(())
}
