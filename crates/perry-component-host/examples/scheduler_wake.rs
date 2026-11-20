//! Exercise cross-task canonical stream wakeups using a maintained-C fixture.
use anyhow::{ensure, Result};
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{AsyncImports, Driver, DriverCall, Host, Imports, Options};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

fn tick(driver: &mut Driver) {
    assert!(block_on(poll_once(driver)).is_none());
}
fn result(driver: &mut Driver, call: &mut DriverCall) -> Value {
    for _ in 0..128 {
        tick(driver);
        if let Some(reply) = block_on(poll_once(&mut *call)) {
            return reply.unwrap().unwrap_or(Value::Null);
        }
    }
    panic!("canonical wake did not deliver its result");
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: scheduler_wake <world.wit> <component.wasm>"
    );
    let source = std::fs::read_to_string(&args[0])?;
    let bytes = std::fs::read(Path::new(&args[1]))?;
    exercise(&bytes, &perry_wit::parse(&source, None)?)?;
    let public_source = source
        .lines()
        .filter(|line| !line.contains("export signal-type:"))
        .collect::<Vec<_>>()
        .join("\n");
    exercise(&bytes, &perry_wit::parse(&public_source, None)?)?;
    println!("PASS: 128 reverse-order wake pairs, blocked and early wake coalescing, extra-export subtyping, idle after every pair");
    Ok(())
}

fn exercise(bytes: &[u8], world: &perry_wit::World) -> Result<()> {
    let host = Host::new_for_driver(
        bytes,
        world,
        &Imports::new(),
        &AsyncImports::new(),
        &Options::default(),
        Arc::new(|_| Ok(Vec::new())),
    )?;
    let (mut driver, client) = host.into_driver()?;
    for _ in 0..64 {
        let mut a = client.call("", "wait", &[json!(1)])?;
        let mut b = client.call("", "wait", &[json!(2)])?;
        for _ in 0..4 {
            tick(&mut driver);
        }
        assert!(block_on(poll_once(&mut a)).is_none());
        assert!(block_on(poll_once(&mut b)).is_none());
        let mut wake = client.call("", "wake-twice", &[json!(2)])?;
        assert_eq!(result(&mut driver, &mut wake), Value::Null);
        assert_eq!(result(&mut driver, &mut b), json!(102));
        assert!(block_on(poll_once(&mut a)).is_none());
        let mut wake = client.call("", "wake", &[json!(1)])?;
        assert_eq!(result(&mut driver, &mut wake), Value::Null);
        assert_eq!(result(&mut driver, &mut a), json!(101));
        tick(&mut driver);
        assert!(!driver.is_active());
    }
    for _ in 0..2 {
        let mut early = client.call("", "wake", &[json!(3)])?;
        assert_eq!(result(&mut driver, &mut early), Value::Null);
    }
    let mut early = client.call("", "wait", &[json!(3)])?;
    assert_eq!(result(&mut driver, &mut early), json!(103));
    tick(&mut driver);
    assert!(!driver.is_active());
    Ok(())
}
