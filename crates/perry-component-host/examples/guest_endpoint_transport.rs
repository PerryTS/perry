//! Actual Perry-authored P3 library: owned future/stream readers cross generated
//! C/JS adapters. Independent host writers/readers prove the returned endpoints
//! are usable. Guest endpoint I/O and guest-created writers remain separate.
use anyhow::{bail, ensure, Context, Result};
use futures_lite::future::{block_on, poll_once};
use perry_component_host::{
    AsyncImports, Driver, DriverCall, DriverClient, Host, Imports, Options, WasiOptions,
};
use perry_wit::{Type, World};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};

fn result(driver: &mut Driver, call: &mut DriverCall) -> Result<Option<Value>> {
    for _ in 0..1024 {
        if let Some(reply) = block_on(poll_once(&mut *driver)) {
            if let Some(reply) = block_on(poll_once(&mut *call)) {
                return reply;
            }
            reply?;
            bail!("driver exited while a transport operation was pending");
        }
        if let Some(reply) = block_on(poll_once(&mut *call)) {
            return reply;
        }
    }
    bail!("endpoint transport operation did not finish")
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
        .context("fixture function")?;
    let wanted = resolve(world, &function.params[0].ty);
    let (stream, payload) = match wanted {
        Type::Future(ty) => (false, ty.as_deref()),
        Type::Stream(ty) => (true, ty.as_deref()),
        _ => bail!("fixture factory argument is not an endpoint"),
    };
    let ty = client
        .async_value_types()
        .iter()
        .find(|ty| {
            ty.is_stream() == stream
                && ty.payload().map(|t| resolve(world, t)) == payload.map(|t| resolve(world, t))
        })
        .context("source-matched endpoint factory")?
        .clone();
    let pair = result(driver, &mut client.create_async_value(&ty)?)?.context("factory result")?;
    Ok((pair["value"].clone(), pair["writer"].clone()))
}
fn call(driver: &mut Driver, client: &DriverClient, name: &str, args: &[Value]) -> Result<Value> {
    Ok(result(driver, &mut client.call("", name, args)?)?.unwrap_or(Value::Null))
}
fn future(
    driver: &mut Driver,
    client: &DriverClient,
    reader: &Value,
    writer: &Value,
    value: Value,
) -> Result<Option<Value>> {
    let mut write = client.write_async_value(writer, &[value])?;
    let read = result(driver, &mut client.read_async_value(reader, None)?)?;
    ensure!(result(driver, &mut write)?.context("write result")?["written"] == 1);
    Ok(read)
}
fn stream(
    driver: &mut Driver,
    client: &DriverClient,
    reader: &Value,
    writer: &Value,
    values: Vec<Value>,
) -> Result<()> {
    let mut remaining = values.clone();
    let mut received = Vec::new();
    while !remaining.is_empty() {
        let mut write = client.write_async_value(writer, &remaining)?;
        let read = result(driver, &mut client.read_async_value(reader, Some(2))?)?
            .context("stream chunk")?;
        ensure!(read["done"] == false);
        received.extend(
            read["value"]
                .as_array()
                .context("stream payload")?
                .iter()
                .cloned(),
        );
        let written = result(driver, &mut write)?.context("write result")?;
        ensure!(written["written"].as_u64().context("write count")? > 0);
        remaining = written["remaining"]
            .as_array()
            .context("partial remainder")?
            .clone();
    }
    ensure!(received == values);
    result(driver, &mut client.close_async_writer(writer)?)?;
    ensure!(
        result(driver, &mut client.read_async_value(reader, Some(1))?)?
            == Some(json!({"done":true}))
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=3).contains(&args.len()),
        "usage: guest_endpoint_transport <world.wit> <component.wasm> [--moving]"
    );
    ensure!(
        args.get(2).is_none_or(|arg| arg == "--moving"),
        "unknown mode"
    );
    let world = perry_wit::parse_file(Path::new(&args[0]), None)?;
    let mut wasi = WasiOptions {
        inherit_stderr: true,
        inherit_stdout: true,
        ..WasiOptions::default()
    };
    if args.len() == 3 {
        for name in [
            "PERRY_GC_INSTRUMENTS",
            "PERRY_GC_FORCE_EVACUATE",
            "PERRY_GC_VERIFY_EVACUATION",
            "PERRY_GC_DIAG",
        ] {
            wasi.env.insert(name.into(), "1".into());
        }
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
    let payload = json!({"text":"héllo 日本語 🍐", "wide":{"$bigint":"18364758544493064720"}, "letter":"🍐", "bytes":[0,1,128,255]});
    let (reader, writer) = pair(&mut driver, &client, &world, "pass-future")?;
    let returned = call(&mut driver, &client, "pass-future", &[reader])?;
    ensure!(
        future(&mut driver, &client, &returned, &writer, payload.clone())? == Some(payload.clone())
    );
    let (reader, writer) = pair(&mut driver, &client, &world, "pass-stream")?;
    let returned = call(&mut driver, &client, "pass-stream", &[reader])?;
    stream(
        &mut driver,
        &client,
        &returned,
        &writer,
        vec![payload.clone(); 5],
    )?;
    let (a, aw) = pair(&mut driver, &client, &world, "pass-future")?;
    let (b, bw) = pair(&mut driver, &client, &world, "pass-stream")?;
    let returned = call(
        &mut driver,
        &client,
        "pass-pair",
        &[json!({"first":a,"second":b})],
    )?;
    ensure!(
        future(
            &mut driver,
            &client,
            &returned["first"],
            &aw,
            payload.clone()
        )? == Some(payload)
    );
    stream(&mut driver, &client, &returned["second"], &bw, vec![])?;
    for (name, value, expected) in [
        ("character", json!("🍐"), Some(json!("🍐"))),
        ("empty", Value::Null, None),
    ] {
        let (reader, writer) = pair(&mut driver, &client, &world, name)?;
        let returned = call(&mut driver, &client, name, &[reader])?;
        ensure!(future(&mut driver, &client, &returned, &writer, value)? == expected);
    }
    for (name, values) in [
        ("units", vec![Value::Null; 7]),
        (
            "bytes",
            vec![json!(0), json!(1), json!(128), json!(255), json!(17)],
        ),
    ] {
        let (reader, writer) = pair(&mut driver, &client, &world, name)?;
        let returned = call(&mut driver, &client, name, &[reader])?;
        stream(&mut driver, &client, &returned, &writer, values)?;
    }
    let (inner, iw) = pair(&mut driver, &client, &world, "dispose")?;
    let (outer, ow) = pair(&mut driver, &client, &world, "nested-value")?;
    let outer = call(&mut driver, &client, "nested-value", &[outer])?;
    let inner = future(&mut driver, &client, &outer, &ow, inner)?.context("nested future")?;
    ensure!(future(&mut driver, &client, &inner, &iw, json!(42))? == Some(json!(42)));
    for name in ["dispose", "forget"] {
        let (reader, writer) = pair(&mut driver, &client, &world, name)?;
        call(&mut driver, &client, name, &[reader])?;
        ensure!(call(&mut driver, &client, "checkpoint", &[])? == 73);
        let write = result(
            &mut driver,
            &mut client.write_async_value(&writer, &[json!(99)])?,
        )?
        .context("closed reader write")?;
        ensure!(
            write["closed"] == true && write["written"] == 0,
            "{name}: {write}"
        );
    }
    ensure!(
        call(&mut driver, &client, "checkpoint", &[])? == 73,
        "Store must remain usable"
    );
    println!("PASS: actual Perry P3 endpoint transport, compound/char/void/unit/byte/nested values, partial chunks, explicit/GC cleanup and Store reuse");
    Ok(())
}
