//! Actual Perry-authored P3 library: owned future/stream readers cross generated
//! C/JS adapters. Independent host writers/readers exercise actual JS endpoint
//! I/O, including guest-created producers progressing after the export returns.
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

// Synchronous canonical exports cannot block on native stderr. The bounded
// guest pipe records the actual Wasm diagnostics; native printing happens
// after execution (also on an early Result error), with native GC controls off.
struct GuestDiagnostics(perry_wasi::pipe::MemoryOutputPipe);
impl Drop for GuestDiagnostics {
    fn drop(&mut self) {
        let _ = std::io::stderr().write_all(&self.0.contents());
    }
}

fn result(driver: &mut Driver, call: &mut DriverCall) -> Result<Option<Value>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
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
        ensure!(
            Instant::now() < deadline,
            "endpoint transport operation did not finish"
        );
        // WASI stdio and socket providers use Turnloop workers. Let their
        // readiness notifications arrive instead of exhausting a CPU loop.
        std::thread::sleep(Duration::from_millis(1));
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
    eprintln!("P3 endpoint case: {name}");
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

fn guest_read_start(
    driver: &mut Driver,
    client: &DriverClient,
    name: &str,
    reader: Value,
) -> Result<DriverCall> {
    let before = call(driver, client, "read-started", &[])?
        .as_u64()
        .context("read counter")?;
    let mut pending = client.call("", name, &[reader])?;
    // A sibling synchronous call must execute while the async read is blocked.
    // It also collects, proving the awaited endpoint and operation are rooted.
    ensure!(
        call(driver, client, "read-started", &[])? == before + 1,
        "guest read did not start"
    );
    ensure!(
        block_on(poll_once(&mut pending)).is_none(),
        "guest read completed before its writer supplied data"
    );
    Ok(pending)
}

fn guest_reads(
    driver: &mut Driver,
    client: &DriverClient,
    world: &World,
    payload: Value,
) -> Result<()> {
    for (name, value, expected) in [
        ("read-future", payload.clone(), payload.clone()),
        ("read-character", json!("🍐"), json!("🍐")),
        ("read-empty", Value::Null, json!(true)),
    ] {
        let (reader, writer) = pair(driver, client, world, name)?;
        let mut pending = guest_read_start(driver, client, name, reader)?;
        let write = result(driver, &mut client.write_async_value(&writer, &[value])?)?
            .context("read fixture write")?;
        ensure!(write["written"] == 1);
        ensure!(
            result(driver, &mut pending)? == Some(expected),
            "{name} result"
        );
    }
    for (name, values, expected) in [
        (
            "read-values",
            vec![payload.clone(); 5],
            json!(vec![payload; 5]),
        ),
        ("read-units", vec![Value::Null; 7], json!(7)),
        (
            "read-bytes",
            vec![json!(0), json!(1), json!(128), json!(255), json!(17)],
            json!([0, 1, 128, 255, 17]),
        ),
    ] {
        let (reader, writer) = pair(driver, client, world, name)?;
        let mut pending = guest_read_start(driver, client, name, reader)?;
        let mut remaining = values;
        let mut chunks = 0;
        while !remaining.is_empty() {
            let write = result(driver, &mut client.write_async_value(&writer, &remaining)?)?
                .context("chunk write")?;
            ensure!(write["written"]
                .as_u64()
                .is_some_and(|count| count > 0 && count <= 2));
            remaining = write["remaining"]
                .as_array()
                .context("remaining chunk")?
                .clone();
            chunks += 1;
        }
        ensure!(chunks >= 3, "the guest must really read partial chunks");
        result(driver, &mut client.close_async_writer(&writer)?)?;
        ensure!(
            result(driver, &mut pending)? == Some(expected),
            "{name} result"
        );
    }
    let (inner, inner_writer) = pair(driver, client, world, "dispose")?;
    let (outer, outer_writer) = pair(driver, client, world, "read-nested")?;
    let mut pending = guest_read_start(driver, client, "read-nested", outer)?;
    result(
        driver,
        &mut client.write_async_value(&outer_writer, &[inner])?,
    )?;
    ensure!(block_on(poll_once(&mut pending)).is_none());
    result(
        driver,
        &mut client.write_async_value(&inner_writer, &[json!(42)])?,
    )?;
    ensure!(result(driver, &mut pending)? == Some(json!(42)));

    let (reader, writer) = pair(driver, client, world, "dispose-pending")?;
    ensure!(call(driver, client, "dispose-pending", &[reader])? == true);
    let closed = result(
        driver,
        &mut client.write_async_value(&writer, &[json!(17)])?,
    )?
    .context("cancelled read writer")?;
    ensure!(closed["closed"] == true && closed["written"] == 0);

    let (reader, writer) = pair(driver, client, world, "abandon-read")?;
    ensure!(call(driver, client, "abandon-read", &[reader])? == true);
    let before = call(driver, client, "read-started", &[])?;
    let mut retry = client.call("", "read-held", &[])?;
    ensure!(call(driver, client, "read-started", &[])?.as_u64() == before.as_u64().map(|n| n + 1));
    ensure!(block_on(poll_once(&mut retry)).is_none());
    result(
        driver,
        &mut client.write_async_value(&writer, &[json!(99)])?,
    )?;
    ensure!(
        result(driver, &mut retry)? == Some(json!(99)),
        "abandoned read must cancel without consuming its retained endpoint"
    );
    Ok(())
}

fn guest_writes(driver: &mut Driver, client: &DriverClient, payload: Value) -> Result<()> {
    ensure!(call(driver, client, "writes-completed", &[])? == 0);
    for (name, args, expected) in [
        ("make-future", vec![payload.clone()], Some(payload.clone())),
        (
            "make-sync-future",
            vec![payload.clone()],
            Some(payload.clone()),
        ),
        ("make-character", vec![], Some(json!("🍐"))),
        ("make-empty", vec![], None),
    ] {
        let reader = call(driver, client, name, &args)?;
        // Deliberately no guest export between factory return and these reads.
        // Only the host's canonical read may advance the blocked producer.
        ensure!(
            result(driver, &mut client.read_async_value(&reader, None)?)? == expected,
            "{name} payload"
        );
    }
    for (name, args, expected) in [
        (
            "make-values",
            vec![json!(vec![payload.clone(); 5])],
            json!(vec![payload.clone(); 5]),
        ),
        (
            "make-sync-values",
            vec![json!(vec![payload.clone(); 5])],
            json!(vec![payload; 5]),
        ),
        ("make-units", vec![], json!(vec![Value::Null; 5])),
        ("make-bytes", vec![], json!([0, 1, 128, 255, 17])),
        ("make-cancelled", vec![], json!([17, 18, 19])),
        ("make-closed", vec![], json!([])),
    ] {
        let reader = call(driver, client, name, &args)?;
        let mut received = Vec::new();
        let mut reads = 0;
        loop {
            let chunk = result(driver, &mut client.read_async_value(&reader, Some(2))?)?
                .context("guest-produced stream chunk")?;
            if chunk["done"] == true {
                break;
            }
            let values = chunk["value"]
                .as_array()
                .context("produced payload array")?;
            ensure!(!values.is_empty() && values.len() <= 2);
            received.extend(values.iter().cloned());
            reads += 1;
        }
        ensure!(json!(received) == expected, "{name} payload");
        if name != "make-closed" {
            ensure!(reads >= 2, "host must really consume partial writes");
        }
    }
    let outer = call(driver, client, "make-nested", &[])?;
    let inner = result(driver, &mut client.read_async_value(&outer, None)?)?
        .context("guest-produced nested future")?;
    ensure!(result(driver, &mut client.read_async_value(&inner, None)?)? == Some(json!(42)));
    ensure!(call(driver, client, "writes-failed", &[])? == 0);
    ensure!(call(driver, client, "writes-completed", &[])? == 11);
    ensure!(call(driver, client, "write-chunks", &[])?
        .as_u64()
        .is_some_and(|n| n >= 14));
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (2..=4).contains(&args.len()),
        "usage: guest_endpoint_transport <world.wit> <component.wasm> [--moving] [--trace]"
    );
    ensure!(
        args[2..]
            .iter()
            .all(|arg| matches!(arg.as_str(), "--moving" | "--trace")),
        "unknown mode"
    );
    let world = perry_wit::parse_file(Path::new(&args[0]), None)?;
    for name in ["make-sync-future", "make-sync-values"] {
        let function = world
            .exports
            .iter()
            .flat_map(|i| &i.functions)
            .find(|f| f.name == name)
            .context("synchronous factory")?;
        ensure!(!function.async_, "public factory must remain synchronous");
    }
    let diagnostics = args[2..]
        .iter()
        .any(|arg| arg == "--moving")
        .then(|| GuestDiagnostics(perry_wasi::pipe::MemoryOutputPipe::new(16 * 1024 * 1024)));
    let mut wasi = WasiOptions {
        inherit_stderr: diagnostics.is_none(),
        stderr_buffer: diagnostics.as_ref().map(|output| output.0.clone()),
        inherit_stdout: true,
        ..WasiOptions::default()
    };
    if args[2..].iter().any(|arg| arg == "--moving") {
        for name in [
            "PERRY_GC_INSTRUMENTS",
            "PERRY_GC_FORCE_EVACUATE",
            "PERRY_GC_VERIFY_EVACUATION",
            "PERRY_GC_DIAG",
        ] {
            wasi.env.insert(name.into(), "1".into());
        }
        wasi.env
            .insert("PERRY_GC_SCHEDULE_SEED".into(), "303".into());
        wasi.env.insert("PERRY_GC_SCHEDULE_RATE".into(), "1".into());
    }
    if args[2..].iter().any(|arg| arg == "--trace") {
        for name in ["PERRY_TRACE_THROW", "PERRY_TRACE_ASYNC"] {
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
        )? == Some(payload.clone())
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
    guest_reads(&mut driver, &client, &world, payload.clone())?;
    guest_writes(&mut driver, &client, payload)?;
    ensure!(call(&mut driver, &client, "checkpoint", &[])? == 73);
    println!("PASS: actual Perry P3 endpoint transport and guest reads/writes, compound/char/void/unit/byte/nested values, guest async/sync factories, private post-return producers, partial chunks, cancellation, abandoned-read retry and Store reuse");
    Ok(())
}
