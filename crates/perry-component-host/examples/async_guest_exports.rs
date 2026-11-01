//! Actual Perry guest: generated C scheduler, moving GC and reverse wakes.
#[path = "support/guest_diagnostics.rs"]
mod guest_diagnostics;
#[path = "support/guest_instrument_denial.rs"]
mod guest_instrument_denial;
use anyhow::{ensure, Result};
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{AsyncImports, Driver, DriverCall, Host, Imports, Options, WasiOptions};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
fn tick_calls(driver: &mut Driver, calls: &mut [&mut DriverCall]) {
    if let Some(reply) = block_on(poll_once(driver)) {
        for call in calls {
            if let Some(Err(error)) = block_on(poll_once(&mut **call)) {
                panic!("guest call failed: {error:#}");
            }
        }
        match reply {
            Ok(_) => panic!("guest driver completed unexpectedly"),
            Err(error) => panic!("guest driver failed: {error:#}"),
        }
    }
}
fn tick(driver: &mut Driver) {
    tick_calls(driver, &mut []);
}
fn result(driver: &mut Driver, call: &mut DriverCall) -> Value {
    for _ in 0..128 {
        tick_calls(driver, &mut [call]);
        if let Some(reply) = block_on(poll_once(&mut *call)) {
            return reply.unwrap().unwrap_or(Value::Null);
        }
    }
    panic!("generated guest scheduler did not deliver its result");
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "usage: async_guest_exports <world.wit> <component.wasm> [--moving|--cancel|--instrument-denial]"
    );
    let world = perry_wit::parse_file(Path::new(&args[0]), None)?;
    let bytes = std::fs::read(&args[1])?;
    let moving = args
        .get(2)
        .is_some_and(|arg| matches!(arg.as_str(), "--moving" | "--instrument-denial"));
    let mut wasi = WasiOptions {
        inherit_stdout: true,
        inherit_stderr: true,
        ..WasiOptions::default()
    };
    let _diagnostics = guest_diagnostics::configure(&mut wasi, moving, 312);
    let options = Options {
        // Overlapping calls share one budget, including repeated full guest GC.
        fuel: 2_000_000_000,
        memory_bytes: 256 * 1024 * 1024,
        wasi: Some(wasi),
        ..Options::default()
    };
    let host = Host::new_for_driver(
        &bytes,
        &world,
        &Imports::new(),
        &AsyncImports::new(),
        &options,
        Arc::new(|_| Ok(Vec::new())),
    )?;
    eprintln!("Component compiled and instantiated");
    let (mut driver, client) = host.into_driver()?;
    if args.get(2).is_some_and(|arg| arg == "--instrument-denial") {
        return guest_instrument_denial::plain_rejection(&mut driver, &client);
    }
    if args.get(2).is_some_and(|arg| arg == "--cancel") {
        let input = json!({"text":"cancel", "bytes":[1], "owner":0});
        let cancelled = client.call("", "wait", &[json!(3), input.clone()])?;
        let mut sibling = client.call("", "wait", &[json!(4), input])?;
        for _ in 0..4 {
            tick(&mut driver);
        }
        drop(cancelled);
        let reply = block_on(poll_once(&mut driver)).expect("cancelled store must retire");
        assert!(reply.is_err());
        assert!(block_on(poll_once(&mut sibling))
            .expect("sibling must retire")
            .is_err());
        assert!(!driver.is_active());
        assert!(client.call("", "status", &[]).is_err());
        println!("PASS: dropping a suspended generated guest call retires its store and sibling");
        return Ok(());
    }
    for round in 0..64 {
        let input_a = json!({"text":format!("A 🌍 {round}"), "bytes":[1,2,255], "owner":0});
        let input_b = json!({"text":format!("B 日本語 {round}"), "bytes":[128,0,3,4], "owner":0});
        let mut a = client.call("", "wait", &[json!(1), input_a.clone()])?;
        let mut b = client.call("", "wait", &[json!(2), input_b.clone()])?;
        for _ in 0..4 {
            tick_calls(&mut driver, &mut [&mut a, &mut b]);
        }
        assert!(block_on(poll_once(&mut a)).is_none());
        assert!(block_on(poll_once(&mut b)).is_none());
        let mut wake = client.call("", "wake", &[json!(2)])?;
        assert_eq!(result(&mut driver, &mut wake), Value::Null);
        assert_eq!(
            result(&mut driver, &mut b),
            json!({"text":format!("B 日本語 {round}/2"), "bytes":input_b["bytes"], "owner":2})
        );
        assert!(block_on(poll_once(&mut a)).is_none());
        let mut wake = client.call("", "wake", &[json!(1)])?;
        assert_eq!(result(&mut driver, &mut wake), Value::Null);
        assert_eq!(
            result(&mut driver, &mut a),
            json!({"text":format!("A 🌍 {round}/1"), "bytes":input_a["bytes"], "owner":1})
        );
        let mut immediate = client.call("", "immediate", std::slice::from_ref(&input_a))?;
        assert_eq!(result(&mut driver, &mut immediate), input_a);
        let mut empty = client.call("", "empty", &[])?;
        assert_eq!(result(&mut driver, &mut empty), Value::Null);
        let mut status = client.call("", "status", &[])?;
        assert_eq!(result(&mut driver, &mut status), json!((round + 1) * 2));
        tick(&mut driver);
        assert!(!driver.is_active());
        if round % 16 == 15 {
            eprintln!("Completed {} reverse wake pairs", round + 1);
        }
    }
    let mut sibling = client.call(
        "",
        "wait",
        &[
            json!(3),
            json!({"text":"rejection sibling", "bytes":[0], "owner":0}),
        ],
    )?;
    for _ in 0..4 {
        tick(&mut driver);
    }
    assert!(block_on(poll_once(&mut sibling)).is_none());
    let mut rejected = client.call("", "reject", &[])?;
    let mut retired = false;
    for _ in 0..128 {
        if let Some(reply) = block_on(poll_once(&mut driver)) {
            assert!(reply.is_err());
            retired = true;
            break;
        }
    }
    assert!(retired, "guest rejection must retire its store");
    assert!(block_on(poll_once(&mut rejected))
        .expect("rejection must be delivered")
        .is_err());
    assert!(block_on(poll_once(&mut sibling))
        .expect("sibling must retire")
        .is_err());
    assert!(!driver.is_active());
    assert!(client.call("", "status", &[]).is_err());
    println!("PASS: rejection retires the generated guest store and pending sibling");
    println!("PASS: generated Perry async exports, 64 reverse wake pairs, pending sibling, collecting result getter, structured Unicode/bytes, immediate return, synchronous jobs, idle after every round");
    Ok(())
}
