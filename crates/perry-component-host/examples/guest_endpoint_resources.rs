//! Independent resource owners cross actual Perry future/stream reads and
//! writes through a named interface and synchronous producer facade.
use anyhow::{bail, ensure, Context, Result};
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImports, Driver, DriverCall, DriverClient, Host, Imports, Options, WasiOptions,
};
use perry_wit::{Type, World};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const API: &str = "perry:guest-endpoint-resources/api@1.0.0";
const MIRROR: &str = "perry:guest-endpoint-resources/mirror@1.0.0";

struct GuestDiagnostics(perry_wasi::pipe::MemoryOutputPipe);
impl Drop for GuestDiagnostics {
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
            bail!("resource endpoint driver stopped with pending work");
        }
        if let Some(reply) = block_on(poll_once(&mut *call)) {
            return reply;
        }
        ensure!(
            Instant::now() < deadline,
            "resource endpoint operation timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn call(driver: &mut Driver, client: &DriverClient, name: &str, args: &[Value]) -> Result<Value> {
    Ok(result(driver, &mut client.call(API, name, args)?)
        .with_context(|| format!("resource export {name}"))?
        .unwrap_or(Value::Null))
}
fn counts(driver: &mut Driver, client: &DriverClient, dropped: u32, completed: u32) -> Result<()> {
    let counts = call(driver, client, "checkpoint", &[])?;
    ensure!(
        counts["dropped"] == dropped,
        "drop count: {counts}, expected {dropped}"
    );
    ensure!(
        counts["completed"] == completed && counts["failed"] == 0,
        "producer counts: {counts}"
    );
    Ok(())
}
fn borrowed(value: &Value) -> Value {
    let mut value = value.clone();
    value["owned"] = json!(false);
    value
}
fn stale(driver: &mut Driver, client: &DriverClient, value: &Value) -> Result<()> {
    let mut attempt = client.call(API, "[method]item.read", &[borrowed(value)])?;
    ensure!(
        result(driver, &mut attempt).is_err(),
        "transferred resource remained usable"
    );
    Ok(())
}
fn payload(driver: &mut Driver, client: &DriverClient, id: u32) -> Result<Value> {
    let first = call(driver, client, "[constructor]item", &[json!(id * 2)])?;
    let second = if id % 2 == 0 {
        json!({"tag":"some", "value":call(driver, client, "[constructor]item", &[json!(id * 2 + 1)])?})
    } else {
        json!({"tag":"none"})
    };
    Ok(
        json!({"first":first,"second":second,"text":format!("owner-{id} 日本語 🍐"),"bytes":[0,128,255,id]}),
    )
}
fn payloads(driver: &mut Driver, client: &DriverClient, first: u32, n: u32) -> Result<Vec<Value>> {
    (first..first + n)
        .map(|id| payload(driver, client, id))
        .collect()
}
fn verify_and_drop(
    driver: &mut Driver,
    client: &DriverClient,
    value: &Value,
    id: u32,
) -> Result<u32> {
    ensure!(value["text"] == format!("owner-{id} 日本語 🍐"));
    ensure!(value["bytes"] == json!([0, 128, 255, id]));
    let mut owners = vec![(&value["first"], id * 2)];
    if id % 2 == 0 {
        ensure!(value["second"]["tag"] == "some");
        owners.push((&value["second"]["value"], id * 2 + 1));
    } else {
        ensure!(value["second"] == json!({"tag":"none"}));
    }
    for (owner, expected) in &owners {
        ensure!(owner["owned"] == true);
        ensure!(call(driver, client, "[method]item.read", &[borrowed(owner)])? == *expected);
        result(driver, &mut client.drop_resource(owner)?)?;
        ensure!(
            result(driver, &mut client.drop_resource(owner)?).is_err(),
            "owner dropped twice"
        );
    }
    Ok(owners.len() as u32)
}
fn stream(driver: &mut Driver, client: &DriverClient, reader: &Value) -> Result<Vec<Value>> {
    let mut values = Vec::new();
    loop {
        let chunk = result(driver, &mut client.read_async_value(reader, Some(1))?)?
            .context("stream read")?;
        if chunk["done"] == true {
            return Ok(values);
        }
        let items = chunk["value"].as_array().context("stream payload")?;
        ensure!(
            items.len() == 1,
            "resource stream must make partial progress"
        );
        values.extend(items.iter().cloned());
    }
}
fn resolve<'a>(world: &'a World, mut ty: &'a Type) -> &'a Type {
    while let Type::Ref(key) = ty {
        ty = &world.types[key].ty;
    }
    ty
}
fn pair(
    driver: &mut Driver,
    client: &DriverClient,
    world: &World,
    function: &str,
) -> Result<(Value, Value)> {
    let function = world
        .exports
        .iter()
        .flat_map(|i| &i.functions)
        .find(|f| f.name == function)
        .context("reader function")?;
    let (stream, payload) = match resolve(world, &function.params[0].ty) {
        Type::Future(ty) => (false, ty.as_deref()),
        Type::Stream(ty) => (true, ty.as_deref()),
        _ => bail!("reader argument is not an endpoint"),
    };
    let ty = client
        .async_value_types()
        .iter()
        .find(|ty| {
            ty.is_stream() == stream
                && ty.payload().map(|t| resolve(world, t)) == payload.map(|t| resolve(world, t))
        })
        .context("resource endpoint factory")?
        .clone();
    let pair = result(driver, &mut client.create_async_value(&ty)?)?.context("endpoint pair")?;
    Ok((pair["value"].clone(), pair["writer"].clone()))
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "usage: guest_endpoint_resources <wit> <component> [--moving]"
    );
    ensure!(args.len() == 2 || args[2] == "--moving", "unknown mode");
    let moving = args.len() == 3;
    let world = perry_wit::parse_file(Path::new(&args[0]), None)?;
    for name in [
        "make-future",
        "make-values",
        "make-cancelled",
        "make-closed",
    ] {
        ensure!(
            !world
                .exports
                .iter()
                .flat_map(|i| &i.functions)
                .find(|f| f.name == name)
                .context("factory")?
                .async_
        );
    }
    let diagnostics =
        moving.then(|| GuestDiagnostics(perry_wasi::pipe::MemoryOutputPipe::new(16 * 1024 * 1024)));
    let mut wasi = WasiOptions {
        inherit_stderr: !moving,
        stderr_buffer: diagnostics.as_ref().map(|d| d.0.clone()),
        inherit_stdout: true,
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
    let host = Host::new_for_driver(
        &std::fs::read(&args[1])?,
        &world,
        &Imports::new(),
        &AsyncImports::new(),
        &Options {
            fuel: 3_000_000_000,
            memory_bytes: 256 * 1024 * 1024,
            wasi: Some(wasi),
            ..Options::default()
        },
        Arc::new(|_| Ok(Vec::new())),
    )?;
    let (mut driver, client) = host.into_driver()?;
    let mut dropped = 0;
    counts(&mut driver, &client, dropped, 0)?;

    eprintln!("resource case: synchronous future factory");
    let input = payload(&mut driver, &client, 2)?;
    let reader = call(&mut driver, &client, "make-future", &[input.clone()])?;
    let returned = result(&mut driver, &mut client.read_async_value(&reader, None)?)?
        .context("resource future")?;
    let returned = result(
        &mut driver,
        &mut client.call(MIRROR, "pass-payload", &[returned])?,
    )?
    .context("shared exported record alias")?;
    stale(&mut driver, &client, &input["first"])?;
    counts(&mut driver, &client, dropped, 1)?;
    dropped += verify_and_drop(&mut driver, &client, &returned, 2)?;
    counts(&mut driver, &client, dropped, 1)?;

    for (name, first, completed) in [("make-values", 10, 2), ("make-cancelled", 20, 3)] {
        eprintln!("resource case: {name}");
        let input = payloads(&mut driver, &client, first, 5)?;
        let reader = call(&mut driver, &client, name, &[json!(input)])?;
        // No more guest exports until all five canonical partial reads and EOF.
        let values = stream(&mut driver, &client, &reader)?;
        ensure!(values.len() == 5);
        counts(&mut driver, &client, dropped, completed)?;
        for (index, value) in values.iter().enumerate() {
            stale(&mut driver, &client, &input[index]["first"])?;
            dropped += verify_and_drop(&mut driver, &client, value, first + index as u32)?;
        }
        counts(&mut driver, &client, dropped, completed)?;
    }

    eprintln!("resource case: explicit writer close discards owners");
    let input = payloads(&mut driver, &client, 30, 3)?;
    let reader = call(&mut driver, &client, "make-closed", &[json!(input)])?;
    ensure!(stream(&mut driver, &client, &reader)?.is_empty());
    dropped += 5;
    counts(&mut driver, &client, dropped, 4)?;

    eprintln!("resource case: actual guest future read");
    let input = payload(&mut driver, &client, 40)?;
    let (reader, writer) = pair(&mut driver, &client, &world, "read-future")?;
    let reader = result(
        &mut driver,
        &mut client.call(MIRROR, "pass-future", &[reader])?,
    )?
    .context("shared exported future alias")?;
    let mut pending = client.call(API, "read-future", &[reader])?;
    ensure!(call(&mut driver, &client, "checkpoint", &[])?["started"] == 1);
    ensure!(block_on(poll_once(&mut pending)).is_none());
    let written = result(
        &mut driver,
        &mut client.write_async_value(&writer, &[input.clone()])?,
    )?
    .context("future write")?;
    ensure!(written["written"] == 1);
    let returned = result(&mut driver, &mut pending)?.context("guest read result")?;
    stale(&mut driver, &client, &input["first"])?;
    counts(&mut driver, &client, dropped, 4)?;
    dropped += verify_and_drop(&mut driver, &client, &returned, 40)?;

    eprintln!("resource case: actual guest stream reads with partial host writes");
    let input = payloads(&mut driver, &client, 50, 5)?;
    let (reader, writer) = pair(&mut driver, &client, &world, "read-values")?;
    let mut pending = client.call(API, "read-values", &[reader])?;
    ensure!(call(&mut driver, &client, "checkpoint", &[])?["started"] == 2);
    ensure!(block_on(poll_once(&mut pending)).is_none());
    let mut remaining = input.clone();
    let mut chunks = 0;
    while !remaining.is_empty() {
        let written = result(
            &mut driver,
            &mut client.write_async_value(&writer, &remaining)?,
        )?
        .context("stream write")?;
        ensure!(written["written"] == 1 && written["closed"] == false);
        remaining = written["remaining"]
            .as_array()
            .context("resource suffix")?
            .clone();
        chunks += 1;
    }
    ensure!(chunks == 5);
    result(&mut driver, &mut client.close_async_writer(&writer)?)?;
    let returned = result(&mut driver, &mut pending)?.context("guest stream result")?;
    let values = returned.as_array().context("returned resource payloads")?;
    ensure!(values.len() == 5);
    counts(&mut driver, &client, dropped, 4)?;
    for (index, value) in values.iter().enumerate() {
        stale(&mut driver, &client, &input[index]["first"])?;
        dropped += verify_and_drop(&mut driver, &client, value, 50 + index as u32)?;
    }
    ensure!(dropped == 33, "fixture owner count changed");
    counts(&mut driver, &client, dropped, 4)?;
    let final_counts = call(&mut driver, &client, "checkpoint", &[])?;
    ensure!(final_counts["chunks"].as_u64().is_some_and(|n| n >= 10));
    println!("PASS: actual Perry P3 resource-bearing future/stream reads and writes, named-interface synchronous factories, partial owner suffixes, cancellation retry, close discard, 33 exactly-once drops and Store reuse");
    Ok(())
}
