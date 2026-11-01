//! Real Perry WASI guest: typed suspended imports, concurrent children and tails.
use anyhow::{ensure, Result};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImport, AsyncImports, Driver, DriverCall, Host, Import, Imports, Options, WasiOptions,
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

const SERVICE: &str = "test:async-guest-imports/service@1.0.0";
type Gates = Arc<Mutex<BTreeMap<u32, oneshot::Sender<Result<Option<Value>>>>>>;
struct Pending {
    finished: bool,
    cancelled: Arc<AtomicUsize>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.finished {
            self.cancelled.fetch_add(1, Ordering::SeqCst);
        }
    }
}
fn tick(driver: &mut Driver, calls: &mut [&mut DriverCall]) {
    if let Some(reply) = block_on(poll_once(driver)) {
        for call in calls {
            if let Some(Err(error)) = block_on(poll_once(&mut **call)) {
                panic!("guest call failed: {error:#}");
            }
        }
        match reply {
            Ok(_) => panic!("unexpected driver completion"),
            Err(error) => panic!("guest driver failed: {error:#}"),
        }
    }
}
fn result(driver: &mut Driver, call: &mut DriverCall) -> Value {
    for _ in 0..256 {
        tick(driver, &mut [call]);
        if let Some(reply) = block_on(poll_once(&mut *call)) {
            return reply.unwrap().unwrap_or(Value::Null);
        }
    }
    panic!("generated import scheduler did not deliver its result");
}
fn idle(driver: &mut Driver) {
    for _ in 0..256 {
        tick(driver, &mut []);
        if !driver.is_active() {
            return;
        }
    }
    panic!("canonical child or post-return task did not exit");
}
fn payload(id: u32) -> Value {
    json!({"text":format!("input {id} 🌍 日本語"),"bytes":[0,127,255,id % 256],"labels":{"tag":"some","value":["héllo","日本語",format!("label{id}")]},"owner":id})
}
fn reply(id: u32) -> Value {
    let mut p = payload(id);
    p["text"] = json!(format!("host {id} 🦀"));
    p
}
fn release(gates: &Gates, id: u32) {
    gates
        .lock()
        .unwrap()
        .remove(&id)
        .expect("missing native gate")
        .send(Ok(Some(reply(id))))
        .unwrap();
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!((2..=3).contains(&args.len()), "usage: async_guest_imports <world.wit> <component.wasm> [--moving|--cancel|--failure|--inline|--inline-moving]");
    let mode = args.get(2).map(String::as_str).unwrap_or("");
    let inline = matches!(mode, "--inline" | "--inline-moving");
    let world = perry_wit::parse_file(Path::new(&args[0]), None)?;
    let bytes = std::fs::read(&args[1])?;
    let gates: Gates = Arc::default();
    let cancelled = Arc::new(AtomicUsize::new(0));
    let empty_calls = Arc::new(AtomicUsize::new(0));
    let many_calls = Arc::new(AtomicUsize::new(0));
    let mut imports = Imports::new();
    let single: Import = Arc::new(|args| {
        ensure!(args.len() == 1);
        Ok(Some(args[0].clone()))
    });
    imports
        .entry(SERVICE.into())
        .or_default()
        .insert("single".into(), single);
    let scalar: Import = Arc::new(|args| Ok(Some(json!(args[0].as_u64().unwrap() + 1))));
    imports
        .entry("".into())
        .or_default()
        .insert("scalar".into(), scalar);
    let empty_count = empty_calls.clone();
    let empty: Import = Arc::new(move |args| {
        ensure!(args.is_empty());
        empty_count.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    });
    imports
        .entry("".into())
        .or_default()
        .insert("empty".into(), empty);
    let mut asynchronous = AsyncImports::new();
    let native_gates = gates.clone();
    let native_cancelled = cancelled.clone();
    let native_count = many_calls.clone();
    let many: AsyncImport = Arc::new(move |args| {
        assert_eq!(args.len(), 6);
        let id = args[0].as_u64().unwrap() as u32;
        for (index, p) in args[1..].iter().enumerate() {
            let mut expected = payload(if id == 6 { 5 } else { id });
            let mut text = expected["text"].as_str().unwrap().to_owned();
            if id == 6 {
                text.push_str("/second");
            }
            if index > 0 {
                text.push_str(["", "/b", "/c", "/d", "/e"][index]);
            }
            expected["text"] = json!(text);
            assert_eq!(p, &expected, "native import argument {index}, call {id}");
        }
        native_count.fetch_add(1, Ordering::SeqCst);
        if inline {
            return Box::pin(async move { Ok(Some(reply(id))) });
        }
        let (send, receive) = oneshot::channel();
        assert!(native_gates.lock().unwrap().insert(id, send).is_none());
        let pending = Pending {
            finished: false,
            cancelled: native_cancelled.clone(),
        };
        Box::pin(async move {
            let mut pending = pending;
            let result = receive.await?;
            pending.finished = true;
            drop(pending);
            result
        })
    });
    asynchronous
        .entry(SERVICE.into())
        .or_default()
        .insert("many".into(), many);
    let mut wasi = WasiOptions {
        inherit_stdout: true,
        inherit_stderr: true,
        ..WasiOptions::default()
    };
    if matches!(mode, "--moving" | "--inline-moving") {
        for name in [
            "PERRY_GC_INSTRUMENTS",
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
    if inline {
        let mut host = Host::new_with_async_imports(
            &bytes,
            &world,
            &imports,
            &asynchronous,
            &options,
            Arc::new(|_| Ok(Vec::new())),
        )?;
        for id in 1..=4 {
            let input = payload(id);
            let actual =
                block_on(host.call_async("", "run", &[json!(id), input.clone()]))?.unwrap();
            let mut expected = reply(id);
            expected["text"] = json!(format!(
                "host {id} 🦀/{}/{id}",
                input["text"].as_str().unwrap()
            ));
            assert_eq!(actual, expected);
            assert_eq!(
                block_on(host.call_async("", "immediate", &[]))?,
                Some(json!(42))
            );
        }
        assert_eq!(empty_calls.load(Ordering::SeqCst), 4);
        println!("PASS: ready Rust imports without embedding dispatch, compound and scalar/void round trips");
        return Ok(());
    }
    let host = Host::new_for_driver(
        &bytes,
        &world,
        &imports,
        &asynchronous,
        &options,
        Arc::new(|_| Ok(Vec::new())),
    )?;
    eprintln!("Import component compiled and instantiated");
    let (mut driver, client) = host.into_driver()?;
    let mut status = client.call("", "status", &[])?;
    assert_eq!(result(&mut driver, &mut status), json!(10000));
    assert_eq!(
        empty_calls.load(Ordering::SeqCst),
        0,
        "initialization guard must precede host I/O"
    );
    let mut guarded = client.call("", "guarded", &[])?;
    assert_eq!(result(&mut driver, &mut guarded), json!(0));
    idle(&mut driver);
    assert_eq!(
        empty_calls.load(Ordering::SeqCst),
        1,
        "a synchronous export's staged import must complete after return"
    );
    for round in 0..64 {
        let input_a = payload(1);
        let input_b = payload(2);
        let mut a = client.call("", "run", &[json!(1), input_a.clone()])?;
        let mut b = client.call("", "run", &[json!(2), input_b.clone()])?;
        for _ in 0..32 {
            tick(&mut driver, &mut [&mut a, &mut b]);
            if gates.lock().unwrap().len() == 2 {
                break;
            }
        }
        assert_eq!(gates.lock().unwrap().len(), 2);
        assert!(block_on(poll_once(&mut a)).is_none());
        assert!(block_on(poll_once(&mut b)).is_none());
        if mode == "--cancel" || mode == "--failure" {
            if mode == "--cancel" {
                drop(a);
            } else {
                gates
                    .lock()
                    .unwrap()
                    .remove(&2)
                    .unwrap()
                    .send(Err(anyhow::anyhow!("expected native import failure")))
                    .unwrap();
            }
            let mut retired = false;
            for _ in 0..256 {
                if let Some(reply) = block_on(poll_once(&mut driver)) {
                    assert!(reply.is_err());
                    retired = true;
                    break;
                }
            }
            assert!(retired);
            assert!(!driver.is_active());
            assert!(block_on(poll_once(&mut b))
                .expect("sibling must fail")
                .is_err());
            assert!(client.call("", "status", &[]).is_err());
            for (id, send) in std::mem::take(&mut *gates.lock().unwrap()) {
                assert!(send.send(Ok(Some(reply(id)))).is_err());
            }
            assert_eq!(
                cancelled.load(Ordering::SeqCst),
                if mode == "--cancel" { 2 } else { 1 }
            );
            println!("PASS: native pending imports cancelled with Store retirement, mode {mode}");
            return Ok(());
        }
        release(&gates, 2);
        let mut expected_b = reply(2);
        expected_b["text"] = json!(format!("host 2 🦀/{}/2", input_b["text"].as_str().unwrap()));
        assert_eq!(result(&mut driver, &mut b), expected_b);
        assert!(block_on(poll_once(&mut a)).is_none());
        release(&gates, 1);
        let mut expected_a = reply(1);
        expected_a["text"] = json!(format!("host 1 🦀/{}/1", input_a["text"].as_str().unwrap()));
        assert_eq!(result(&mut driver, &mut a), expected_a);
        idle(&mut driver);
        if round % 16 == 15 {
            eprintln!("Completed {} import pairs", round + 1);
        }
    }
    let mut parallel = client.call("", "parallel", &[json!(5), payload(5)])?;
    for _ in 0..32 {
        tick(&mut driver, &mut [&mut parallel]);
        if gates.lock().unwrap().len() == 2 {
            break;
        }
    }
    assert_eq!(gates.lock().unwrap().len(), 2);
    release(&gates, 6);
    for _ in 0..8 {
        tick(&mut driver, &mut [&mut parallel]);
    }
    assert!(block_on(poll_once(&mut parallel)).is_none());
    release(&gates, 5);
    assert_eq!(result(&mut driver, &mut parallel), json!(11));
    idle(&mut driver);
    let mut immediate = client.call("", "immediate", &[])?;
    assert_eq!(result(&mut driver, &mut immediate), json!(42));
    idle(&mut driver);
    let mut background = client.call("", "background", &[json!(9), payload(9)])?;
    assert_eq!(result(&mut driver, &mut background), json!(9));
    assert!(
        driver.is_active(),
        "native child must outlive public return"
    );
    assert_eq!(gates.lock().unwrap().len(), 1);
    let mut status = client.call("", "status", &[])?;
    assert_eq!(result(&mut driver, &mut status), json!(10000));
    release(&gates, 9);
    idle(&mut driver);
    let mut status = client.call("", "status", &[])?;
    assert_eq!(result(&mut driver, &mut status), json!(10009));
    idle(&mut driver);
    assert_eq!(many_calls.load(Ordering::SeqCst), 131);
    assert_eq!(empty_calls.load(Ordering::SeqCst), 3);
    assert_eq!(cancelled.load(Ordering::SeqCst), 0);
    println!("PASS: generated Perry async imports, 64 reverse pairs, typed indirect buffers, collecting getters, parallel children, scalar/void, initialization guard, sync-started import, post-return child chain, idle");
    Ok(())
}
