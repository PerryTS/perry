//! Execute generated C/Perry/C futures with three nominal instances of one interface.
use anyhow::{bail, ensure, Context, Result};
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImports, Driver, DriverCall, DriverClient, Host, Import, Imports, Options, ResourceEvent,
    WasiOptions,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const API: &str = "perry:guest-endpoint-labels/api@1.0.0";
const FOREIGN: &str = "perry:guest-endpoint-labels/foreign@1.0.0";
type NativeValues = Arc<Mutex<BTreeMap<String, (String, u32)>>>;

struct Diagnostics(perry_wasi::pipe::MemoryOutputPipe);
impl Drop for Diagnostics {
    fn drop(&mut self) {
        let _ = std::io::stderr().write_all(&self.0.contents());
    }
}

fn result(driver: &mut Driver, call: &mut DriverCall) -> Result<Option<Value>> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(reply) = block_on(poll_once(&mut *driver)) {
            if let Some(reply) = block_on(poll_once(&mut *call)) {
                return reply;
            }
            reply?;
            bail!("labeled-resource driver stopped with pending work");
        }
        if let Some(reply) = block_on(poll_once(&mut *call)) {
            return reply;
        }
        ensure!(
            Instant::now() < deadline,
            "labeled-resource operation timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn call(driver: &mut Driver, client: &DriverClient, name: &str, args: &[Value]) -> Result<Value> {
    Ok(result(driver, &mut client.call(API, name, args)?)?.unwrap_or(Value::Null))
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "usage: guest_endpoint_labels <wit> <component> [--moving]"
    );
    ensure!(args.len() == 2 || args[2] == "--moving", "unknown mode");
    let moving = args.len() == 3;
    let world = perry_wit::parse_file(Path::new(&args[0]), None)?;
    let labels = [FOREIGN, "left", "right"];
    let keys: BTreeSet<_> = labels
        .iter()
        .map(|label| {
            world
                .imports
                .iter()
                .find(|i| i.name == *label)
                .context("labeled import")
                .map(|i| i.resources["item"].clone())
        })
        .collect::<Result<_>>()?;
    ensure!(
        keys.len() == 3,
        "repeated labels must create three nominal resource types"
    );
    let values: NativeValues = Arc::default();
    let drops: Arc<Mutex<BTreeSet<String>>> = Arc::default();
    let reads: Arc<Mutex<BTreeMap<String, u32>>> = Arc::default();
    let mut imports = Imports::new();
    for label in labels {
        let key = world
            .imports
            .iter()
            .find(|i| i.name == label)
            .context("labeled interface")?
            .resources["item"]
            .clone();
        let created = values.clone();
        let make: Import = Arc::new(move |args| {
            let value = u32::try_from(args[0].as_u64().context("resource value")?)?;
            let token = format!("{label}:{value}");
            ensure!(
                created
                    .lock()
                    .unwrap()
                    .insert(token.clone(), (key.clone(), value))
                    .is_none(),
                "duplicate native owner"
            );
            Ok(Some(
                json!({"$hostResource":token,"type":key,"generation":1,"owned":true,"new":true}),
            ))
        });
        let live = values.clone();
        let observed = reads.clone();
        let read: Import = Arc::new(move |args| {
            // Count entry before validation: a wrongly routed callback must fail the witness.
            *observed.lock().unwrap().entry(label.into()).or_default() += 1;
            ensure!(args[0]["owned"] == false);
            let token = args[0]["$hostResource"]
                .as_str()
                .context("native borrowed resource")?;
            ensure!(
                token.starts_with(&format!("{label}:")),
                "wrong nominal callback: {token}"
            );
            let values = live.lock().unwrap();
            let (_, value) = values.get(token).context("resource was already dropped")?;
            Ok(Some(json!(value)))
        });
        imports.insert(
            label.into(),
            BTreeMap::from([
                ("[constructor]item".into(), make),
                ("[method]item.read".into(), read),
            ]),
        );
    }
    let diagnostics =
        moving.then(|| Diagnostics(perry_wasi::pipe::MemoryOutputPipe::new(16 * 1024 * 1024)));
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
            .insert("PERRY_GC_SCHEDULE_SEED".into(), "313".into());
        wasi.env.insert("PERRY_GC_SCHEDULE_RATE".into(), "1".into());
    }
    let live = values.clone();
    let dropped = drops.clone();
    let host = Host::new_for_driver(
        &std::fs::read(&args[1])?,
        &world,
        &imports,
        &AsyncImports::new(),
        &Options {
            fuel: 3_000_000_000,
            memory_bytes: 256 * 1024 * 1024,
            wasi: Some(wasi),
            ..Options::default()
        },
        Arc::new(move |event| {
            if let ResourceEvent::Drop { resource } = event {
                let (key, _) = live
                    .lock()
                    .unwrap()
                    .remove(&resource.token)
                    .context("duplicate native resource drop")?;
                ensure!(key == resource.type_key, "drop changed nominal identity");
                ensure!(
                    dropped.lock().unwrap().insert(resource.token),
                    "native owner dropped twice"
                );
            }
            Ok(Vec::new())
        }),
    )?;
    let (mut driver, client) = host.into_driver()?;
    for (index, value) in [3, 17, 29, 41].into_iter().enumerate() {
        let reader = call(&mut driver, &client, "make-future", &[json!(value)])?;
        let payload = result(&mut driver, &mut client.read_async_value(&reader, None)?)?
            .context("labeled resource future")?;
        ensure!(payload["text"] == "labels 日本語 🍐");
        let owner = &payload["owner"];
        ensure!(owner["owned"] == true);
        let mut borrow = owner.clone();
        borrow["owned"] = json!(false);
        ensure!(call(&mut driver, &client, "resource-value", &[borrow])? == value);
        result(&mut driver, &mut client.drop_resource(owner)?)?;
        ensure!(
            result(&mut driver, &mut client.drop_resource(owner)?).is_err(),
            "duplicate drop accepted"
        );
        let count = (index + 1) as u32;
        ensure!(
            call(&mut driver, &client, "checkpoint", &[])?
                == json!({"completed":count,"rejected":count*8,"failed":0}),
            "producer or rejection counts"
        );
        ensure!(values.lock().unwrap().is_empty(), "native resource leaked");
        ensure!(
            drops.lock().unwrap().len() == (index + 1) * 3,
            "missing exactly-once drops"
        );
        let observed = reads.lock().unwrap();
        for label in labels {
            ensure!(
                observed.get(label) == Some(&(count * 2)),
                "wrong callback count for {label}: {observed:?}"
            );
        }
    }
    println!("PASS: actual repeated-label C/Perry/C resource futures, 32 wrong-owner rejections, 12 exactly-once drops and Store reuse");
    Ok(())
}
