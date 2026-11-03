use super::*;
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use serde_json::json;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

const CLOCK: &str = include_str!("../../perry-wasi/src/p3/clock.component.wat");
const WORLD: &str = r#"package test:async-clock@1.0.0;
interface clock { now: func() -> u64; wait-for: async func(how-long: u64); }
world plugin { import clock; export run: async func(nanos: u64) -> u64; }"#;
type Gate = Arc<Mutex<VecDeque<oneshot::Sender<Result<Option<Value>>>>>>;
fn clock_host(
    resource: bool,
    events: Arc<Mutex<Vec<ResourceEvent>>>,
) -> (Host, Gate, Arc<AtomicUsize>) {
    let mut world = WORLD.to_owned();
    let mut component = CLOCK.replace(
        "wasi:clocks/monotonic-clock@0.3.0",
        "test:async-clock/clock@1.0.0",
    );
    if resource {
        world = world
            .replace("interface clock {", "interface clock { resource item;")
            .replace(
                "world plugin { import clock;",
                "world plugin { import clock; use clock.{item};",
            )
            .replace("nanos: u64)", "nanos: u64, owned: own<item>)");
        component = component
            .replace(
                "(instance $clock",
                "(instance $clock\n    (export \"item\" (type $item (sub resource)))",
            )
            .replace(
                "  (alias export $clock \"now\"",
                "  (alias export $clock \"item\" (type $item))\n  (alias export $clock \"now\"",
            )
            .replace(
                "(param $nanos i64) (result i32)",
                "(param $nanos i64) (param $owned i32) (result i32)",
            )
            .replace(
                "(param \"nanos\" u64) (result u64)",
                "(param \"nanos\" u64) (param \"owned\" (own $item)) (result u64)",
            );
    }
    let world = perry_wit::parse(&world, None).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let now_calls = calls.clone();
    let now: Import = Arc::new(move |_| {
        Ok(Some(
            json!({"$bigint":now_calls.fetch_add(100, Ordering::SeqCst).to_string()}),
        ))
    });
    let gates: Gate = Arc::default();
    let wait_gates = gates.clone();
    let thread = std::thread::current().id();
    let wait: AsyncImport = Arc::new(move |args| {
        assert_eq!(std::thread::current().id(), thread);
        assert_eq!(args, vec![json!({"$bigint":"7"})]);
        let (send, receive) = oneshot::channel();
        wait_gates.lock().unwrap().push_back(send);
        Box::pin(async move {
            let result = receive.await.context("wait cancelled")?;
            assert_eq!(std::thread::current().id(), thread);
            result
        })
    });
    let host = Host::new_with_async_imports(
        &wat::parse_str(component).unwrap(),
        &world,
        &BTreeMap::from([(
            "test:async-clock/clock@1.0.0".into(),
            BTreeMap::from([("now".into(), now)]),
        )]),
        &BTreeMap::from([(
            "test:async-clock/clock@1.0.0".into(),
            BTreeMap::from([("wait-for".into(), wait)]),
        )]),
        &Options::default(),
        Arc::new(move |event| {
            events.lock().unwrap().push(event);
            Ok(Vec::new())
        }),
    )
    .unwrap();
    (host, gates, calls)
}

#[test]
fn native_async_calls_suspend_on_owned_imports_then_reuse_the_store() {
    let (mut host, gates, calls) = clock_host(false, Arc::default());
    assert!(host
        .call("", "run", &[json!({"$bigint":"7"})])
        .unwrap_err()
        .to_string()
        .contains("call_async"));
    for _ in 0..2 {
        let args = [json!({"$bigint":"7"})];
        let mut call = Box::pin(host.call_async("", "run", &args));
        assert!(block_on(poll_once(call.as_mut())).is_none());
        let send = gates
            .lock()
            .unwrap()
            .pop_front()
            .expect("async import did not run");
        send.send(Ok(None)).unwrap();
        assert_eq!(block_on(call).unwrap(), Some(json!({"$bigint":"100"})));
        assert!(host.live.is_some());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 400);
    host.dispose().unwrap();
}

#[test]
fn cancelled_and_failed_async_calls_retire_store_and_guest_held_host_payloads() {
    for cancel in [true, false] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (mut host, gates, _) = clock_host(true, events.clone());
        let key = perry_wit::parse(
            &WORLD.replace("interface clock {", "interface clock { resource item;"),
            None,
        )
        .unwrap()
        .imports[0]
            .resources["item"]
            .clone();
        let args = [
            json!({"$bigint":"7"}),
            json!({"$hostResource":"held","type":key,"generation":1,"owned":true,"new":true}),
        ];
        let mut call = Box::pin(host.call_async("", "run", &args));
        assert!(block_on(poll_once(call.as_mut())).is_none());
        let send = gates.lock().unwrap().pop_front().unwrap();
        if cancel {
            drop(call);
            assert!(send.is_canceled());
        } else {
            send.send(Err(anyhow::anyhow!("native wait failed")))
                .unwrap();
            assert!(format!("{:#}", block_on(call).unwrap_err()).contains("native wait failed"));
        }
        assert!(host.live.is_none());
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(
                    |e| matches!(e, ResourceEvent::Drop { resource } if resource.token == "held")
                )
                .count(),
            1
        );
        host.dispose().unwrap();
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, ResourceEvent::Drop { .. }))
                .count(),
            1
        );
    }
}

#[test]
fn production_host_executes_p3_clock_exports_and_rejects_async_contract_mismatch() {
    let world = perry_wit::parse(
        &WORLD
            .replace("test:async-clock@1.0.0", "wasi:clocks@0.3.0")
            .replace("interface clock", "interface monotonic-clock")
            .replace("import clock", "import monotonic-clock"),
        None,
    )
    .unwrap();
    let bytes = wat::parse_str(CLOCK).unwrap();
    let options = Options {
        wasi: Some(WasiOptions::default()),
        ..Options::default()
    };
    let mut wrong = world.clone();
    wrong.exports[0].functions[0].async_ = false;
    assert!(format!(
        "{:#}",
        Host::new(&bytes, &wrong, &Imports::new(), &options)
            .err()
            .unwrap()
    )
    .contains("async calling convention mismatch"));
    let mut host = Host::new(&bytes, &world, &Imports::new(), &options).unwrap();
    let elapsed = block_on(host.call_async("", "run", &[json!({"$bigint":"1000000"})]))
        .unwrap()
        .unwrap();
    assert!(elapsed["$bigint"].as_str().unwrap().parse::<u64>().unwrap() >= 1_000_000);
    host.dispose().unwrap();
}
