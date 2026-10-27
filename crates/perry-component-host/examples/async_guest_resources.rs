//! Real generated Perry guest resource ownership through Wasmtime's ABI.
use anyhow::{ensure, Context, Result};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImport, AsyncImports, Driver, DriverCall, Host, Import, Imports, Options, ResourceEvent,
    WasiOptions,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
};

const FOREIGN: &str = "test:async-guest-resources/foreign@1.0.0";
const LOCAL: &str = "test:async-guest-resources/local@1.0.0";
type Gates = Arc<Mutex<BTreeMap<u32, oneshot::Sender<()>>>>;

fn tick(driver: &mut Driver) -> Result<()> {
    if let Some(reply) = block_on(poll_once(driver)) {
        reply?;
        anyhow::bail!("driver exited while its client is live");
    }
    Ok(())
}
fn result(driver: &mut Driver, call: &mut DriverCall) -> Result<Value> {
    for _ in 0..256 {
        tick(driver)?;
        if let Some(reply) = block_on(poll_once(&mut *call)) {
            return Ok(reply?.unwrap_or(Value::Null));
        }
    }
    anyhow::bail!("generated resource call did not finish")
}
fn release(gates: &Gates, id: u32) {
    gates
        .lock()
        .unwrap()
        .remove(&id)
        .expect("native import never suspended")
        .send(())
        .unwrap();
}
fn gate(gates: &Gates, id: u32) -> oneshot::Receiver<()> {
    let (send, receive) = oneshot::channel();
    assert!(gates.lock().unwrap().insert(id, send).is_none());
    receive
}
fn borrowed(value: &Value) -> Value {
    let mut value = value.clone();
    value["owned"] = json!(false);
    value
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "usage: async_guest_resources <world.wit> <component.wasm> [--moving]"
    );
    let world = perry_wit::parse_file(Path::new(&args[0]), None)?;
    let key = world
        .imports
        .iter()
        .find(|i| i.name == FOREIGN)
        .context("foreign interface")?
        .resources["item"]
        .clone();
    let values: Arc<Mutex<BTreeMap<String, u32>>> = Arc::default();
    let drops: Arc<Mutex<BTreeMap<String, usize>>> = Arc::default();
    let gates: Gates = Arc::default();
    let mut imports = Imports::new();
    let submitted: Import = Arc::new(|_| Ok(None));
    imports.insert("".into(), BTreeMap::from([("submitted".into(), submitted)]));
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
            .context("borrowed host resource")?;
        Ok(Some(json!(*readable
            .lock()
            .unwrap()
            .get(token)
            .context("resource already dropped")?)))
    });
    imports.insert(
        FOREIGN.into(),
        BTreeMap::from([
            ("[constructor]item".into(), make),
            ("[method]item.read".into(), read),
        ]),
    );
    let mut asynchronous = AsyncImports::new();
    let waiting = gates.clone();
    let wait: AsyncImport = Arc::new(move |args| {
        let receive = gate(&waiting, args[0].as_u64().unwrap() as u32);
        Box::pin(async move {
            receive.await?;
            Ok(None)
        })
    });
    asynchronous.insert("".into(), BTreeMap::from([("wait".into(), wait)]));
    let waiting = gates.clone();
    let exchange: AsyncImport = Arc::new(move |args| {
        assert_eq!(args[1]["owned"], true);
        let value = args[1].clone();
        let receive = gate(&waiting, args[0].as_u64().unwrap() as u32);
        Box::pin(async move {
            receive.await?;
            Ok(Some(value))
        })
    });
    let waiting = gates.clone();
    let readable = values.clone();
    let loan: AsyncImport = Arc::new(move |args| {
        assert_eq!(args[1]["owned"], false);
        let token = args[1]["$hostResource"].as_str().unwrap();
        let value = readable.lock().unwrap()[token];
        let receive = gate(&waiting, args[0].as_u64().unwrap() as u32);
        Box::pin(async move {
            receive.await?;
            Ok(Some(json!(value)))
        })
    });
    asynchronous.insert(
        FOREIGN.into(),
        BTreeMap::from([("exchange".into(), exchange), ("borrow-wait".into(), loan)]),
    );
    let mut wasi = WasiOptions {
        inherit_stderr: true,
        ..WasiOptions::default()
    };
    if args.get(2).is_some_and(|arg| arg == "--moving") {
        for name in [
            "PERRY_GC_FORCE_EVACUATE",
            "PERRY_GC_VERIFY_EVACUATION",
            "PERRY_GC_DIAG",
        ] {
            wasi.env.insert(name.into(), "1".into());
        }
    }
    let options = Options {
        fuel: 2_000_000_000,
        memory_bytes: 256 * 1024 * 1024,
        wasi: Some(wasi),
        ..Options::default()
    };
    let live_values = values.clone();
    let observed_drops = drops.clone();
    let host = Host::new_for_driver(
        &std::fs::read(&args[1])?,
        &world,
        &imports,
        &asynchronous,
        &options,
        Arc::new(move |event| {
            if let ResourceEvent::Drop { resource } = event {
                ensure!(
                    live_values
                        .lock()
                        .unwrap()
                        .remove(&resource.token)
                        .is_some(),
                    "duplicate drop"
                );
                let mut counts = observed_drops.lock().unwrap();
                *counts.entry(resource.token).or_default() += 1;
                ensure!(counts.values().all(|count| *count == 1));
            }
            Ok(Vec::new())
        }),
    )?;
    let (mut driver, client) = host.into_driver()?;
    // A host-created owner passes through a synchronous constructor import
    // and an async export result, then returns through a second async import.
    // Native callers supply fresh descriptors just as the resource hook does.
    let key = world
        .imports
        .iter()
        .find(|i| i.name == FOREIGN)
        .unwrap()
        .resources["item"]
        .clone();
    let fresh = |id: u32| {
        let token = format!("item-{id}");
        assert!(values.lock().unwrap().insert(token.clone(), id).is_none());
        json!({"$hostResource":token,"type":key,"generation":1,"owned":true,"new":true})
    };
    let mut create = client.call("", "create", &[json!(9)])?;
    let created = result(&mut driver, &mut create)?;
    let mut drop_created = client.drop_resource(&created)?;
    result(&mut driver, &mut drop_created)?;
    let mut echo = client.call("", "echo", &[fresh(10)])?;
    let owner = result(&mut driver, &mut echo)?;
    let mut transferred = client.call("", "transferred", &[])?;
    ensure!(result(&mut driver, &mut transferred)? == json!(true));
    let mut exchange = client.call("", "exchange", &[json!(20), owner])?;
    tick(&mut driver)?;
    release(&gates, 20);
    let owner = result(&mut driver, &mut exchange)?;
    let mut transferred = client.call("", "transferred", &[])?;
    ensure!(result(&mut driver, &mut transferred)? == json!(true));
    for round in 0..8 {
        let a = 100 + round * 2;
        let mut first = client.call("", "loan", &[json!(a), borrowed(&owner)])?;
        let mut second = client.call("", "loan", &[json!(a + 1), borrowed(&owner)])?;
        tick(&mut driver)?;
        release(&gates, a + 1);
        ensure!(result(&mut driver, &mut second)? == json!(10));
        ensure!(block_on(poll_once(&mut first)).is_none());
        let mut expired = client.call("", "expired", &[])?;
        ensure!(result(&mut driver, &mut expired)? == json!(true));
        release(&gates, a);
        ensure!(result(&mut driver, &mut first)? == json!(10));
    }
    let mut many = client.call(
        "",
        "loans",
        &[json!(200), json!([borrowed(&owner), borrowed(&owner)])],
    )?;
    tick(&mut driver)?;
    release(&gates, 200);
    ensure!(result(&mut driver, &mut many)? == json!([10, 10]));
    let mut background = client.call("", "loan-background", &[json!(500), borrowed(&owner)])?;
    ensure!(result(&mut driver, &mut background)? == json!(10));
    ensure!(driver.is_active() && gates.lock().unwrap().contains_key(&500));
    let mut expired = client.call("", "expired", &[])?;
    ensure!(result(&mut driver, &mut expired)? == json!(true));
    // Canonical return ends the loan even though a child keeps its task/frame
    // alive. Reclaim the owner before that child's JS continuation runs.
    let mut drop_owner = client.drop_resource(&owner)?;
    result(&mut driver, &mut drop_owner)?;
    release(&gates, 500);
    for _ in 0..256 {
        tick(&mut driver)?;
        if !driver.is_active() {
            break;
        }
    }
    let mut expired = client.call("", "tail-expired", &[])?;
    ensure!(result(&mut driver, &mut expired)? == json!(true));
    let mut loan = client.call("", "import-loan", &[json!(201), fresh(11)])?;
    tick(&mut driver)?;
    release(&gates, 201);
    ensure!(result(&mut driver, &mut loan)? == json!(11));
    let mut make = client.call(LOCAL, "make", &[json!(300)])?;
    tick(&mut driver)?;
    release(&gates, 300);
    let local = result(&mut driver, &mut make)?;
    let mut read = client.call(LOCAL, "read", &[json!(301), borrowed(&local)])?;
    tick(&mut driver)?;
    release(&gates, 301);
    ensure!(result(&mut driver, &mut read)? == json!(300));
    let mut expired = client.call(LOCAL, "expired", &[])?;
    ensure!(result(&mut driver, &mut expired)? == json!(true));
    let mut drop_local = client.drop_resource(&local)?;
    result(&mut driver, &mut drop_local)?;
    for _ in 0..256 {
        tick(&mut driver)?;
        if !driver.is_active() {
            break;
        }
    }
    ensure!(!driver.is_active() && gates.lock().unwrap().is_empty());
    ensure!(values.lock().unwrap().is_empty());
    ensure!(drops.lock().unwrap().len() == 3);
    println!("PASS: generated Perry async resources, owned round trips, 8 reverse borrow pairs, list borrows, import loans and local resources");
    Ok(())
}
