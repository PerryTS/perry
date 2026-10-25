use crate::{Driver, DriverCall, Host, Options};
use futures_lite::future::{block_on, poll_once};
use serde_json::{json, Value};
use std::collections::BTreeMap;
const NS: &str = "test:producer/data";
fn driver() -> (Driver, crate::DriverClient) {
    instance(false)
}
fn instance(stream: bool) -> (Driver, crate::DriverClient) {
    let world=perry_wit::parse("package test:producer; interface data { resource thing; make: async func() -> future<own<thing>>; drops: func() -> u32; consume: func(value: own<thing>); } world plugin { export data; }",None).unwrap();
    let mut text = include_str!("producer.component.wat").to_string();
    let world = if stream {
        let source="package test:producer; interface data { resource thing; make: async func() -> stream<own<thing>>; drops: func() -> u32; consume: func(value: own<thing>); } world plugin { export data; }";
        text = text
            .replace("(type $future (future", "(type $future (stream")
            .replace("canon future.", "canon stream.")
            .replace(
                "(func $write (param i32 i32)",
                "(func $write (param i32 i32 i32)",
            )
            .replace(
                "(call $write (global.get $writer) (i32.const 0))",
                "(call $write (global.get $writer) (i32.const 0) (i32.const 1))",
            )
            .replace(
                "(i32.ne (local.get 0) (i32.const 5))",
                "(i32.ne (local.get 0) (i32.const 3))",
            );
        perry_wit::parse(source, None).unwrap()
    } else {
        world
    };
    let bytes = wat::parse_str(text).unwrap();
    let host = Host::new(&bytes, &world, &BTreeMap::new(), &Options::default()).unwrap();
    host.into_driver().unwrap()
}
pub(super) fn result(driver: &mut Driver, call: &mut DriverCall) -> anyhow::Result<Option<Value>> {
    for _ in 0..128 {
        if let Some(result) = block_on(poll_once(&mut *driver)) {
            panic!("driver unexpectedly finished: {:?}", result.err());
        }
        if let Some(result) = block_on(poll_once(&mut *call)) {
            return result;
        }
    }
    panic!("ready component operation did not complete");
}
#[test]
fn composed_native_future_reads_preserve_guest_resource_identity_and_destructor() {
    let (mut driver, client) = driver();
    let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    assert!(value["$future"].is_string());
    let owned = result(
        &mut driver,
        &mut client.read_async_value(&value, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(owned["$resource"].is_string());
    assert!(result(
        &mut driver,
        &mut client.read_async_value(&value, None).unwrap()
    )
    .is_err());
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(0))
    );
    assert!(result(
        &mut driver,
        &mut client
            .call(NS, "consume", std::slice::from_ref(&owned))
            .unwrap()
    )
    .unwrap()
    .is_none());
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[owned]).unwrap()
    )
    .is_err());
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(1))
    );
    let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    assert!(
        result(&mut driver, &mut client.close_async_value(&value).unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(2))
    );
}
#[test]
fn foreign_future_tokens_fail_before_entering_the_engine_and_owner_can_still_read() {
    let (mut first, a) = driver();
    let (mut second, b) = driver();
    let value = result(&mut first, &mut a.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    assert!(result(&mut second, &mut b.read_async_value(&value, None).unwrap()).is_err());
    assert_eq!(
        result(&mut second, &mut b.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(0))
    );
    let owned = result(&mut first, &mut a.read_async_value(&value, None).unwrap())
        .unwrap()
        .unwrap();
    assert!(result(&mut first, &mut a.call(NS, "consume", &[owned]).unwrap()).is_ok());
}

#[test]
fn composed_native_stream_returns_owned_chunk_before_eof_and_closes_unread_values() {
    let (mut driver, client) = instance(true);
    let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    assert!(value["$stream"].is_string());
    let chunk = result(
        &mut driver,
        &mut client.read_async_value(&value, Some(2)).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(chunk["done"], false);
    let owned = chunk["value"][0].clone();
    assert!(owned["$resource"].is_string());
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&value, Some(2)).unwrap()
        )
        .unwrap(),
        Some(json!({"done":true}))
    );
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[owned]).unwrap()
    )
    .is_ok());
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(1))
    );
    let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    assert!(result(&mut driver, &mut client.close_async_value(&value).unwrap()).is_ok());
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(2))
    );
}

#[test]
fn composition_aggregates_imported_host_resource_types_and_preserves_generations() {
    use std::sync::{Arc, Mutex};
    let world=perry_wit::parse("package test:host; interface types { resource thing; make: func() -> own<thing>; } world plugin { import types; use types.{thing}; export make: async func() -> future<own<thing>>; export consume: func(value: own<thing>); }",None).unwrap();
    let key = world.imports[0].resources["thing"].clone();
    let make: crate::Import = Arc::new(move |_| {
        Ok(Some(
            json!({"$hostResource":"host-77","type":key,"generation":1,"new":true,"owned":true}),
        ))
    });
    let imports = BTreeMap::from([(
        "test:host/types".into(),
        BTreeMap::from([("make".into(), make)]),
    )]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let hook: crate::ResourceHook = Arc::new(move |event| {
        recorded.lock().unwrap().push(event);
        Ok(Vec::new())
    });
    let host = Host::new_with_resource_hook(
        &wat::parse_str(include_str!("imported-producer.component.wat")).unwrap(),
        &world,
        &imports,
        &Options::default(),
        hook,
    )
    .unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let future = result(&mut driver, &mut client.call("", "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let owned = result(
        &mut driver,
        &mut client.read_async_value(&future, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(owned["$hostResource"], "host-77");
    assert_eq!(owned["generation"], 2);
    assert!(result(
        &mut driver,
        &mut client.call("", "consume", &[owned]).unwrap()
    )
    .is_ok());
    let future = result(&mut driver, &mut client.call("", "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    assert!(result(&mut driver, &mut client.close_async_value(&future).unwrap()).is_ok());
    assert!(block_on(poll_once(&mut driver)).is_none());
    let drops = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            crate::ResourceEvent::Drop { resource } => Some(resource.generation),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(drops, [2, 1]);
}

#[test]
fn pruned_provider_resource_names_do_not_create_phantom_helper_imports() {
    let mut world=perry_wit::parse("package test:producer; interface data { resource thing; make: async func() -> future<own<thing>>; drops: func() -> u32; consume: func(value: own<thing>); } world plugin { export data; }",None).unwrap();
    // A manifest may retain a provider resource alias after componentization
    // has removed its original interface. The actual exported type remains.
    world.imports.push(perry_wit::Interface {
        name: "wasi:fixture/types@0.3.0".into(),
        functions: Vec::new(),
        resources: world.exports[0].resources.clone(),
    });
    let options = Options {
        wasi: Some(crate::WasiOptions::default()),
        ..Options::default()
    };
    let host = Host::new(
        &wat::parse_str(include_str!("producer.component.wat")).unwrap(),
        &world,
        &BTreeMap::new(),
        &options,
    )
    .unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let future = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let owned = result(
        &mut driver,
        &mut client.read_async_value(&future, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[owned]).unwrap()
    )
    .is_ok());
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(1))
    );
}

#[test]
fn private_control_import_names_do_not_collide_with_application_imports() {
    use std::sync::Arc;
    let source="package test:producer; interface data { resource thing; make: async func() -> future<own<thing>>; drops: func() -> u32; consume: func(value: own<thing>); } world plugin { import perry-async-control-0: interface { wait: async func(id: u32); } export data; }";
    let world = perry_wit::parse(source, None).unwrap();
    let text=include_str!("producer.component.wat").replacen("(component",r#"(component (import "perry-async-control-0" (instance (export "wait" (func async (param "id" u32)))))"#,1);
    let wait: crate::AsyncImport = Arc::new(|_| {
        Box::pin(async { panic!("unused application import must not service helper cancellation") })
    });
    let imports = BTreeMap::from([(
        "perry-async-control-0".into(),
        BTreeMap::from([("wait".into(), wait)]),
    )]);
    let host = Host::new_with_async_imports(
        &wat::parse_str(text).unwrap(),
        &world,
        &BTreeMap::new(),
        &imports,
        &Options::default(),
        Arc::new(|_| Ok(Vec::new())),
    )
    .unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let future = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let owned = result(
        &mut driver,
        &mut client.read_async_value(&future, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[owned]).unwrap()
    )
    .is_ok());
}

#[test]
fn production_future_transport_preserves_unicode_large_bytes_and_nested_resources() {
    let source="package test:compound; interface data { resource thing; record payload { item: own<thing>, text: string, bytes: list<u8> } make: async func() -> future<payload>; drops: func() -> u32; consume: func(value: own<thing>); } world plugin { export data; }";
    let world = perry_wit::parse(source, None).unwrap();
    let bytes = wat::parse_str(include_str!("compound-producer.component.wat")).unwrap();
    let options = Options {
        fuel: 100_000_000,
        ..Options::default()
    };
    let host = Host::new(&bytes, &world, &BTreeMap::new(), &options).unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let ns = "test:compound/data";
    let future = result(&mut driver, &mut client.call(ns, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let payload = result(
        &mut driver,
        &mut client.read_async_value(&future, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(payload["text"], "héllo 🌍");
    let bytes = payload["bytes"].as_array().unwrap();
    assert_eq!(bytes.len(), 3_145_865);
    assert!(bytes
        .iter()
        .enumerate()
        .all(|(i, value)| value == &json!((i.wrapping_mul(31).wrapping_add(7)) as u8)));
    assert!(result(
        &mut driver,
        &mut client
            .call(ns, "consume", std::slice::from_ref(&payload["item"]))
            .unwrap()
    )
    .is_ok());
    assert_eq!(
        result(&mut driver, &mut client.call(ns, "drops", &[]).unwrap()).unwrap(),
        Some(json!(1))
    );
}

#[test]
fn payload_free_futures_and_streams_preserve_void_and_unit_counts() {
    for stream in [false, true] {
        let kind = if stream { "stream" } else { "future" };
        let world=perry_wit::parse(&format!("package test:producer; interface data {{ resource thing; make: async func() -> {kind}; drops: func() -> u32; consume: func(value: own<thing>); }} world plugin {{ export data; }}"),None).unwrap();
        let mut text = include_str!("producer.component.wat").replace(
            "(type $future (future (own $thing)))",
            &format!("(type $future ({kind}))"),
        );
        text=text.replace("      (i32.store (i32.const 0) (call $new-thing (i32.const 77)))\n","").replace("      (if (i32.eq (i32.and (local.get $status) (i32.const 15)) (i32.const 1))\n        (then (call $drop-thing (i32.load (i32.const 0)))))\n","");
        if stream {
            text = text
                .replace("canon future.", "canon stream.")
                .replace(
                    "(func $write (param i32 i32)",
                    "(func $write (param i32 i32 i32)",
                )
                .replace(
                    "(call $write (global.get $writer) (i32.const 0))",
                    "(call $write (global.get $writer) (i32.const 0) (i32.const 7))",
                )
                .replace(
                    "(i32.ne (local.get 0) (i32.const 5))",
                    "(i32.ne (local.get 0) (i32.const 3))",
                )
                .replace(
                    "(global $writer (mut i32) (i32.const 0))",
                    "(global $remaining (mut i32) (i32.const 7)) (global $writer (mut i32) (i32.const 0))",
                )
                .replace(
                    "(func $finish (param $status i32) (result i32)\n",
                    r#"(func $finish (param $status i32) (result i32)
                      (loop $more
                        (global.set $remaining (i32.sub (global.get $remaining) (i32.shr_u (local.get $status) (i32.const 4))))
                        (if (i32.and (i32.eqz (i32.and (local.get $status) (i32.const 15))) (i32.ne (global.get $remaining) (i32.const 0))) (then
                          (local.set $status (call $write (global.get $writer) (i32.const 0) (global.get $remaining)))
                          (if (i32.eq (local.get $status) (i32.const -1)) (then
                            (call $join (global.get $writer) (global.get $set))
                            (return (i32.or (i32.const 2) (i32.shl (global.get $set) (i32.const 4))))))
                          (br $more))))
"#,
                );
        }
        let host = Host::new(
            &wat::parse_str(text).unwrap(),
            &world,
            &BTreeMap::new(),
            &Options::default(),
        )
        .unwrap();
        let (mut driver, client) = host.into_driver().unwrap();
        let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
            .unwrap()
            .unwrap();
        if stream {
            let mut total = 0;
            let mut ended = false;
            for _ in 0..5 {
                let chunk = result(
                    &mut driver,
                    &mut client.read_async_value(&value, Some(3)).unwrap(),
                )
                .unwrap()
                .unwrap();
                if chunk["done"] == true {
                    ended = true;
                    break;
                }
                let values = chunk["value"].as_array().unwrap();
                assert!(values.len() <= 3);
                assert!(values.iter().all(Value::is_null));
                total += values.len();
            }
            assert!(ended);
            assert_eq!(total, 7);
        } else {
            assert!(result(
                &mut driver,
                &mut client.read_async_value(&value, None).unwrap()
            )
            .unwrap()
            .is_none());
        }
        assert_eq!(
            result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
            Some(json!(0))
        );
    }
}

#[test]
fn composed_nested_future_payloads_remain_owned_after_outer_read_cleanup() {
    let world=perry_wit::parse("package test:producer; interface data { resource thing; make: async func() -> future<future<own<thing>>>; drops: func() -> u32; consume: func(value: own<thing>); } world plugin { export data; }",None).unwrap();
    let host = Host::new(
        &wat::parse_str(include_str!("nested-producer.component.wat")).unwrap(),
        &world,
        &BTreeMap::new(),
        &Options::default(),
    )
    .unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let outer = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let inner = result(
        &mut driver,
        &mut client.read_async_value(&outer, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(inner["$future"].is_string());
    let owned = result(
        &mut driver,
        &mut client.read_async_value(&inner, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[owned]).unwrap()
    )
    .is_ok());
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(1))
    );
    let outer = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    assert!(result(&mut driver, &mut client.close_async_value(&outer).unwrap()).is_ok());
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(2))
    );
}
