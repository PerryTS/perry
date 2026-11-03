use super::*;
use crate::{current_invocation, AsyncImport, AsyncImports, InvocationId, Options};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once, yield_now};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
#[path = "tests/exit_transform.rs"]
mod exit_transform;

type Gates = Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<()>>>>>;
type Observed = Arc<Mutex<Vec<(u64, InvocationId, &'static str)>>>;
struct Probe(Arc<AtomicUsize>, InvocationId);
impl Drop for Probe {
    fn drop(&mut self) {
        assert_eq!(
            current_invocation(),
            Some(self.1),
            "native cancellation must restore its import origin"
        );
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct Fixture {
    early_return: bool,
    sync_tail: bool,
    exit_hook: bool,
    binary_exit_hook: bool,
    persistent_exit: bool,
    omit_return: bool,
    dispatcher: bool,
    spin: u32,
    fuel: Option<u64>,
}
fn host(early_return: bool) -> (Host, Gates, Observed, Arc<AtomicUsize>) {
    fixture(Fixture {
        early_return,
        ..Fixture::default()
    })
}
fn fixture(config: Fixture) -> (Host, Gates, Observed, Arc<AtomicUsize>) {
    let mut source = "package test:driver; interface clock { wait-for: async func(value: u64); } world plugin { import clock; export run: async func(value: u64) -> u64; }".to_string();
    if config.sync_tail {
        source = source.replace("interface clock {", "interface clock { now: func() -> u64;");
    }
    if config.exit_hook || config.binary_exit_hook {
        source = source.replace("interface clock {", "interface clock { exited: func();");
    }
    let world = perry_wit::parse(&source, None).unwrap();
    let heap =
        include_str!("../async_values/heap.wat").replacen("(module", "(core module $heap", 1);
    let mut component = include_str!("../async_values/task_context.component.wat")
        .replace(";; HEAP", &heap)
        .replace("\"clock\"", &format!("\"{}\"", world.imports[0].name));
    if config.early_return {
        component = component.replace("      (call $return (i64.load (local.get $frame)))\n", "");
        component = component.replace("      (local.set $status (call $wait (local.get $value)))", "      (call $return (local.get $value))\n      (local.set $status (call $wait (local.get $value)))");
    }
    if config.omit_return {
        component = component.replace("      (call $return (i64.load (local.get $frame)))\n", "");
    }
    if config.sync_tail {
        component = component.replace(
            "(instance $clock",
            "(instance $clock (export \"now\" (func (result u64)))",
        );
        component = component.replace("  (core func $return", "  (alias export $clock \"now\" (func $now))\n  (core func $now (canon lower (func $now)))\n  (core func $return");
        component = component.replace(
            "  (core module $code",
            "  (core module $code (import \"host\" \"now\" (func $now (result i64)))",
        );
        component = component.replace(
            "      (call $drop-set",
            "      (drop (call $now))\n      (call $drop-set",
        );
        component = component.replace(
            "    (export \"get\"",
            "    (export \"now\" (func $now))\n    (export \"get\"",
        );
    }
    if config.spin > 0 {
        component = component.replace("(param $value i64) (result i32) (local $frame i32) (local $status i32)", &format!("(param $value i64) (result i32) (local $frame i32) (local $status i32) (local $burn i32) (local.set $burn (i32.const {})) (loop $burning (br_if $burning (local.tee $burn (i32.sub (local.get $burn) (i32.const 1)))))", config.spin));
    }
    if config.exit_hook || config.binary_exit_hook {
        component = component.replace(
            "(instance $clock",
            "(instance $clock (export \"exited\" (func))",
        );
    }
    if config.exit_hook {
        component=component.replace("  (core func $return","  (alias export $clock \"exited\" (func $exited))\n  (core func $exited (canon lower (func $exited)))\n  (core func $return");
        component = component.replace(
            "\n  (func (export \"run\")",
            &format!(
                "\n{}\n  (func (export \"run\")",
                include_str!("tests/exit_hook.wat")
            ),
        );
        component = component
            .replace(
                "(canon lift (core func $code \"run\")",
                "(canon lift (core func $tracked \"run\")",
            )
            .replace(
                "(callback (core func $code \"callback\"))",
                "(callback (core func $tracked \"callback\"))",
            );
    }
    let gates: Gates = Arc::default();
    let pending = gates.clone();
    let observed: Observed = Arc::default();
    let observations = observed.clone();
    let drops = Arc::new(AtomicUsize::new(0));
    let dropped = drops.clone();
    let owner = std::thread::current().id();
    let wait: AsyncImport = Arc::new(move |args| {
        assert_eq!(std::thread::current().id(), owner);
        let value = args[0]["$bigint"].as_str().unwrap().parse::<u64>().unwrap();
        let origin = current_invocation().expect("driver import must have an origin");
        observations.lock().unwrap().push((value, origin, "begin"));
        let (send, receive) = oneshot::channel();
        assert!(pending.lock().unwrap().insert(value, send).is_none());
        let observations = observations.clone();
        let guard = Probe(dropped.clone(), origin);
        Box::pin(async move {
            let _guard = guard;
            yield_now().await;
            assert_eq!(current_invocation(), Some(origin));
            receive.await.unwrap()?;
            assert_eq!(std::thread::current().id(), owner);
            assert_eq!(current_invocation(), Some(origin));
            observations.lock().unwrap().push((value, origin, "end"));
            Ok(None)
        })
    });
    let imports: AsyncImports = BTreeMap::from([(
        world.imports[0].name.clone(),
        BTreeMap::from([("wait-for".into(), wait)]),
    )]);
    let mut synchronous = BTreeMap::new();
    if config.sync_tail {
        let observations = observed.clone();
        let now: crate::Import = Arc::new(move |_| {
            assert_eq!(std::thread::current().id(), owner);
            observations.lock().unwrap().push((
                0,
                current_invocation().expect("sync import origin"),
                "sync",
            ));
            Ok(Some(json!({"$bigint":"0"})))
        });
        synchronous.insert(
            world.imports[0].name.clone(),
            BTreeMap::from([("now".into(), now)]),
        );
    }
    if config.exit_hook || config.binary_exit_hook {
        let observations = observed.clone();
        let exited: crate::Import = Arc::new(move |_| {
            assert_eq!(std::thread::current().id(), owner);
            observations.lock().unwrap().push((
                0,
                current_invocation().expect("exit hook must retain its original invocation"),
                "exit",
            ));
            Ok(None)
        });
        synchronous
            .entry(world.imports[0].name.clone())
            .or_insert_with(BTreeMap::new)
            .insert("exited".into(), exited);
    }
    let options = Options {
        fuel: config.fuel.unwrap_or(10_000_000),
        ..Options::default()
    };
    let bytes = wat::parse_str(component).unwrap();
    let bytes = if config.binary_exit_hook {
        exit_transform::wrap_root_function(&bytes, "run", 0, "exited").unwrap()
    } else {
        bytes
    };
    let host = Host::new_prepared(
        &bytes,
        &world,
        &synchronous,
        &imports,
        &options,
        Arc::new(|_| Ok(Vec::new())),
        config
            .dispatcher
            .then(|| Arc::new(dispatch::Dispatcher::default())),
        config.persistent_exit,
    )
    .unwrap();
    (host, gates, observed, drops)
}

fn tick(driver: &mut Driver) {
    assert!(block_on(poll_once(&mut *driver)).is_none());
    assert_eq!(
        current_invocation(),
        None,
        "a pending poll must restore caller ancestry"
    );
}
fn result(driver: &mut Driver, call: &mut DriverCall) -> Reply {
    for _ in 0..128 {
        tick(driver);
        if let Some(result) = block_on(poll_once(&mut *call)) {
            return result;
        }
    }
    panic!("ready driver result was not published");
}
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    for _ in 0..128 {
        if let Some(result) = block_on(poll_once(future.as_mut())) {
            return result;
        }
    }
    panic!("driver failed to reach ready state");
}

#[test]
fn concurrent_calls_keep_their_origins_when_completed_in_reverse_order() {
    let (host, gates, observed, drops) = host(false);
    let (mut driver, client) = host.into_driver().unwrap();
    let mut first = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    let mut second = client.call("", "run", &[json!({"$bigint":"2"})]).unwrap();
    let (id1, id2) = (first.id(), second.id());
    assert_ne!(id1, id2);
    tick(&mut driver);
    assert_eq!(gates.lock().unwrap().len(), 2);
    assert!(driver.is_active());
    gates
        .lock()
        .unwrap()
        .remove(&2)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    assert_eq!(
        result(&mut driver, &mut second).unwrap(),
        Some(json!({"$bigint":"2"}))
    );
    assert!(block_on(poll_once(&mut first)).is_none());
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    assert_eq!(
        result(&mut driver, &mut first).unwrap(),
        Some(json!({"$bigint":"1"}))
    );
    tick(&mut driver);
    assert!(!driver.is_active());
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert_eq!(
        *observed.lock().unwrap(),
        [
            (1, id1, "begin"),
            (2, id2, "begin"),
            (2, id2, "end"),
            (1, id1, "end")
        ]
    );
    drop(client);
    let host = ready(driver).unwrap();
    assert!(host.live.is_some());
}

#[test]
fn results_return_before_background_tasks_exit_and_driver_keeps_polling_imports() {
    let (host, gates, observed, drops) = host(true);
    let (mut driver, client) = host.into_driver().unwrap();
    for value in [1, 2] {
        let mut call = client
            .call("", "run", &[json!({"$bigint":value.to_string()})])
            .unwrap();
        let origin = call.id();
        assert_eq!(
            result(&mut driver, &mut call).unwrap(),
            Some(json!({"$bigint":value.to_string()}))
        );
        drop(call);
        assert_eq!(
            observed.lock().unwrap().last(),
            Some(&(value, origin, "begin"))
        );
        assert!(
            driver.is_active(),
            "post-return work must keep the driver active"
        );
        assert_eq!(drops.load(Ordering::SeqCst), value as usize - 1);
        gates
            .lock()
            .unwrap()
            .remove(&value)
            .unwrap()
            .send(Ok(()))
            .unwrap();
        tick(&mut driver);
        assert_eq!(
            observed.lock().unwrap().last(),
            Some(&(value, origin, "end"))
        );
        assert_eq!(drops.load(Ordering::SeqCst), value as usize);
        assert!(!driver.is_active());
    }
    drop(client);
    assert!(ready(driver).unwrap().live.is_some());
}

#[test]
fn callback_exit_notifications_identify_one_finished_invocation_while_its_sibling_stays_active() {
    for binary_exit_hook in [false, true] {
        let (host, gates, observed, drops) = fixture(Fixture {
            early_return: true,
            exit_hook: !binary_exit_hook,
            binary_exit_hook,
            ..Fixture::default()
        });
        let (mut driver, client) = host.into_driver().unwrap();
        let mut first = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
        let mut second = client.call("", "run", &[json!({"$bigint":"2"})]).unwrap();
        let (a, b) = (first.id(), second.id());
        assert_eq!(
            result(&mut driver, &mut first).unwrap(),
            Some(json!({"$bigint":"1"}))
        );
        assert_eq!(
            result(&mut driver, &mut second).unwrap(),
            Some(json!({"$bigint":"2"}))
        );
        assert!(
            observed
                .lock()
                .unwrap()
                .iter()
                .all(|(_, _, event)| *event != "exit"),
            "task.return is not task exit"
        );
        gates
            .lock()
            .unwrap()
            .remove(&2)
            .unwrap()
            .send(Ok(()))
            .unwrap();
        tick(&mut driver);
        assert!(
            driver.is_active(),
            "the first invocation must still be suspended"
        );
        let exits = observed
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, _, event)| *event == "exit")
            .map(|(_, id, _)| *id)
            .collect::<Vec<_>>();
        assert_eq!(
            exits,
            vec![b],
            "the sibling must finish independently under its own origin"
        );
        assert!(
            client.retained_origins().contains(&b),
            "global retention is not an exit signal"
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        gates
            .lock()
            .unwrap()
            .remove(&1)
            .unwrap()
            .send(Ok(()))
            .unwrap();
        tick(&mut driver);
        assert!(!driver.is_active());
        let exits = observed
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, _, event)| *event == "exit")
            .map(|(_, id, _)| *id)
            .collect::<Vec<_>>();
        assert_eq!(exits, vec![b, a]);
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        drop(client);
        assert!(ready(driver).unwrap().live.is_some());
    }
}

#[test]
fn private_binary_exit_releases_only_its_completed_root_and_preserves_sibling_context() {
    let (host, gates, _, drops) = fixture(Fixture {
        early_return: true,
        persistent_exit: true,
        ..Fixture::default()
    });
    let (mut driver, client) = host.into_driver().unwrap();
    assert!(client
        .state
        .tracked_exports
        .contains(&(String::new(), "run".into())));
    let mut first = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    let mut second = client.call("", "run", &[json!({"$bigint":"2"})]).unwrap();
    let (a, b) = (first.id(), second.id());
    let mut exited_a = driver.state.exits.wait(a);
    let mut exited_b = driver.state.exits.wait(b);
    result(&mut driver, &mut first).unwrap();
    result(&mut driver, &mut second).unwrap();
    assert!(block_on(poll_once(&mut exited_a)).is_none());
    assert!(block_on(poll_once(&mut exited_b)).is_none());
    gates
        .lock()
        .unwrap()
        .remove(&2)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    tick(&mut driver);
    assert_eq!(block_on(poll_once(&mut exited_b)), Some(Ok(())));
    assert!(block_on(poll_once(&mut exited_a)).is_none());
    assert!(driver.is_active());
    assert!(client.retained_origins().contains(&a));
    assert!(!client.retained_origins().contains(&b));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    tick(&mut driver);
    assert_eq!(block_on(poll_once(&mut exited_a)), Some(Ok(())));
    assert!(!driver.is_active());
    drop(client);
    assert!(ready(driver).unwrap().live.is_some());
}

#[test]
fn private_exit_candidate_does_not_hide_vm_rejection_after_callback_returns() {
    let (host, gates, _, _) = fixture(Fixture {
        persistent_exit: true,
        omit_return: true,
        ..Fixture::default()
    });
    let (mut driver, client) = host.into_driver().unwrap();
    let mut call = client.call("", "run", &[json!({"$bigint":"2"})]).unwrap();
    let mut exited = driver.state.exits.wait(call.id());
    tick(&mut driver);
    gates
        .lock()
        .unwrap()
        .remove(&2)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    let error = loop {
        if let Some(result) = block_on(poll_once(&mut driver)) {
            break result.err().unwrap();
        }
    };
    assert!(
        format!("{error:#}").contains("failed to produce a result"),
        "{error:#}"
    );
    assert!(
        block_on(poll_once(&mut exited)).unwrap().is_err(),
        "candidate must never be acknowledged"
    );
    assert!(ready(&mut call).is_err());
}

#[test]
fn preflight_errors_and_cancelled_queued_calls_leave_the_component_reusable() {
    let (host, gates, _, _) = host(false);
    let (mut driver, client) = host.into_driver().unwrap();
    let cancelled = client.call("", "run", &[json!({"$bigint":"99"})]).unwrap();
    drop(cancelled);
    let mut missing = client.call("", "missing", &[]).unwrap();
    assert!(result(&mut driver, &mut missing).is_err());
    let mut invalid = client.call("", "run", &[json!("invalid")]).unwrap();
    assert!(result(&mut driver, &mut invalid).is_err());
    tick(&mut driver);
    assert!(gates.lock().unwrap().is_empty());
    assert!(!driver.is_active());
    let mut call = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    tick(&mut driver);
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    assert_eq!(
        result(&mut driver, &mut call).unwrap(),
        Some(json!({"$bigint":"1"}))
    );
    drop(client);
    assert!(ready(driver).unwrap().live.is_some());
}

#[test]
fn dropping_started_calls_retires_store_and_cancels_native_imports() {
    let (host, gates, _, drops) = host(false);
    let (mut driver, client) = host.into_driver().unwrap();
    let call = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    tick(&mut driver);
    drop(call);
    assert!(ready(&mut driver).is_err());
    assert!(!driver.is_active());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .is_err());
    assert!(client.call("", "run", &[]).is_err());
    assert_eq!(current_invocation(), None);
}

#[test]
fn a_failed_import_retires_sibling_calls_and_restores_embedding_context() {
    let (host, gates, _, drops) = host(false);
    let (mut driver, client) = host.into_driver().unwrap();
    let mut first = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    let mut second = client.call("", "run", &[json!({"$bigint":"2"})]).unwrap();
    tick(&mut driver);
    gates
        .lock()
        .unwrap()
        .remove(&2)
        .unwrap()
        .send(Err(anyhow::anyhow!("primary import rejection")))
        .unwrap();
    assert!(ready(&mut driver).is_err());
    let error = ready(&mut second).unwrap_err();
    assert!(format!("{error:#}").contains("primary import rejection"));
    assert!(ready(&mut first).is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert!(gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .is_err());
    assert_eq!(current_invocation(), None);
}

#[test]
fn queue_quota_survives_cancellation_and_is_released_when_requests_are_drained() {
    let (host, gates, _, _) = host(false);
    let (mut driver, client) = host.into_driver().unwrap();
    for _ in 0..64 {
        drop(client.call("", "run", &[json!({"$bigint":"1"})]).unwrap());
    }
    assert!(client.call("", "run", &[]).is_err());
    tick(&mut driver);
    assert!(gates.lock().unwrap().is_empty());
    let cancelled = client.call("", "run", &[]).unwrap();
    drop(cancelled);
    tick(&mut driver);
    drop(client);
    assert!(ready(driver).unwrap().live.is_some());
}

#[test]
fn dropping_last_client_with_background_work_retires_the_component() {
    let (host, gates, _, drops) = host(true);
    let (mut driver, client) = host.into_driver().unwrap();
    let mut call = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    assert!(result(&mut driver, &mut call).is_ok());
    drop(client);
    assert!(ready(driver).is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .is_err());
}

#[test]
fn synchronous_import_dispatch_preserves_each_suspended_call_origin() {
    let (host, gates, observed, _) = fixture(Fixture {
        sync_tail: true,
        dispatcher: true,
        ..Fixture::default()
    });
    let (mut driver, client) = host.into_driver().unwrap();
    let mut first = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    let mut second = client.call("", "run", &[json!({"$bigint":"2"})]).unwrap();
    tick(&mut driver);
    gates
        .lock()
        .unwrap()
        .remove(&2)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    assert!(result(&mut driver, &mut second).is_ok());
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    assert!(result(&mut driver, &mut first).is_ok());
    let sync = observed
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, _, kind)| *kind == "sync")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(sync, [(0, second.id(), "sync"), (0, first.id(), "sync")]);
    drop(client);
    assert!(ready(driver).unwrap().live.is_some());
}

#[test]
fn overlapping_calls_cannot_replenish_the_shared_guest_fuel_budget() {
    let (host, gates, _, drops) = fixture(Fixture {
        early_return: true,
        spin: 500,
        fuel: Some(10_000),
        ..Fixture::default()
    });
    let (mut driver, client) = host.into_driver().unwrap();
    let mut exhausted = false;
    for value in 1..=16 {
        let mut call = client
            .call("", "run", &[json!({"$bigint":value.to_string()})])
            .unwrap();
        for turn in 0..128 {
            if let Some(result) = block_on(poll_once(&mut driver)) {
                let error = result.err().expect("live client must not finish normally");
                assert!(format!("{error:#}").contains("fuel"), "{error:#}");
                exhausted = true;
                break;
            }
            if let Some(result) = block_on(poll_once(&mut call)) {
                assert!(result.is_ok());
                break;
            }
            assert!(turn < 127, "ready exported result must be published");
        }
        if exhausted {
            break;
        }
        assert!(driver.is_active());
    }
    assert!(
        exhausted,
        "new overlapping requests must not refill old tasks' fuel"
    );
    let pending = std::mem::take(&mut *gates.lock().unwrap());
    assert!(!pending.is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), pending.len());
    assert!(pending.into_values().all(|send| send.send(Ok(())).is_err()));
}

#[test]
fn quiescence_replenishes_fuel_for_a_later_independent_call() {
    let (host, gates, _, _) = fixture(Fixture {
        spin: 500,
        fuel: Some(10_000),
        ..Fixture::default()
    });
    let (mut driver, client) = host.into_driver().unwrap();
    for value in 1..=16 {
        let mut call = client
            .call("", "run", &[json!({"$bigint":value.to_string()})])
            .unwrap();
        tick(&mut driver);
        gates
            .lock()
            .unwrap()
            .remove(&value)
            .unwrap()
            .send(Ok(()))
            .unwrap();
        assert_eq!(
            result(&mut driver, &mut call).unwrap(),
            Some(json!({"$bigint":value.to_string()}))
        );
    }
    drop(client);
    assert!(ready(driver).unwrap().live.is_some());
}

#[test]
fn reused_task_indices_do_not_replace_an_older_background_invocation() {
    let (host, gates, observed, drops) = host(true);
    let (mut driver, client) = host.into_driver().unwrap();
    let mut first = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    let first_origin = first.id();
    assert!(result(&mut driver, &mut first).is_ok());
    for value in 2..=80 {
        let mut call = client
            .call("", "run", &[json!({"$bigint":value.to_string()})])
            .unwrap();
        let origin = call.id();
        assert_ne!(origin, first_origin);
        assert!(result(&mut driver, &mut call).is_ok());
        gates
            .lock()
            .unwrap()
            .remove(&value)
            .unwrap()
            .send(Ok(()))
            .unwrap();
        tick(&mut driver);
        assert_eq!(
            observed.lock().unwrap().last(),
            Some(&(value, origin, "end"))
        );
        assert!(
            driver.is_active(),
            "the first task still owns background work"
        );
    }
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    tick(&mut driver);
    assert_eq!(
        observed.lock().unwrap().last(),
        Some(&(1, first_origin, "end"))
    );
    assert_eq!(drops.load(Ordering::SeqCst), 80);
    assert!(!driver.is_active());
    drop(client);
    assert!(ready(driver).unwrap().live.is_some());
}

#[test]
fn a_post_return_import_failure_is_reported_by_the_persistent_driver() {
    let (host, gates, _, drops) = host(true);
    let (mut driver, client) = host.into_driver().unwrap();
    let mut call = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    assert_eq!(
        result(&mut driver, &mut call).unwrap(),
        Some(json!({"$bigint":"1"}))
    );
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Err(anyhow::anyhow!("post-return rejection")))
        .unwrap();
    let error = ready(driver).err().unwrap();
    assert!(
        format!("{error:#}").contains("post-return rejection"),
        "{error:#}"
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(client.call("", "run", &[]).is_err());
    assert_eq!(current_invocation(), None);
}

#[test]
fn an_idle_driver_is_woken_by_submission_and_by_native_import_readiness() {
    use std::task::{Wake, Waker};
    struct Notify(AtomicUsize);
    impl Wake for Notify {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let (host, gates, _, _) = host(false);
    let (mut driver, client) = host.into_driver().unwrap();
    let notify = Arc::new(Notify(AtomicUsize::new(0)));
    let waker = Waker::from(notify.clone());
    let mut context = TaskContext::from_waker(&waker);
    assert!(Pin::new(&mut driver).poll(&mut context).is_pending());
    assert!(!driver.is_active());
    notify.0.store(0, Ordering::SeqCst);
    let mut call = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    assert!(
        notify.0.load(Ordering::SeqCst) > 0,
        "submission must wake an idle embedding"
    );
    assert!(Pin::new(&mut driver).poll(&mut context).is_pending());
    notify.0.store(0, Ordering::SeqCst);
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    assert!(
        notify.0.load(Ordering::SeqCst) > 0,
        "native readiness must wake the owner"
    );
    assert!(result(&mut driver, &mut call).is_ok());
    tick(&mut driver);
    assert!(!driver.is_active());
    drop(client);
    assert!(ready(driver).unwrap().live.is_some());
}

#[test]
fn retained_origins_include_queued_and_suspended_calls_then_clear_after_completion() {
    let (host, gates, _, _) = host(false);
    let (mut driver, client) = host.into_driver().unwrap();
    assert!(client.retained_origins().is_empty());
    let mut call = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    let origin = call.id();
    assert_eq!(client.retained_origins(), vec![origin]);
    tick(&mut driver);
    assert_eq!(client.retained_origins(), vec![origin]);
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    assert!(result(&mut driver, &mut call).is_ok());
    tick(&mut driver);
    assert!(client.retained_origins().is_empty());
    let skipped = client.call("", "run", &[json!({"$bigint":"2"})]).unwrap();
    let skipped_origin = skipped.id();
    drop(skipped);
    assert_eq!(client.retained_origins(), vec![skipped_origin]);
    tick(&mut driver);
    assert!(client.retained_origins().is_empty());
    let mut invalid = client.call("", "missing", &[]).unwrap();
    assert_eq!(client.retained_origins(), vec![invalid.id()]);
    assert!(result(&mut driver, &mut invalid).is_err());
    tick(&mut driver);
    assert!(client.retained_origins().is_empty());
}

#[test]
fn retained_contexts_stay_bounded_while_an_older_post_return_task_remains_active() {
    let (host, gates, observed, drops) = host(true);
    let (mut driver, client) = host.into_driver().unwrap();
    let mut first = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    let first_origin = first.id();
    assert!(result(&mut driver, &mut first).is_ok());
    let mut completed = Vec::new();
    let mut peak = 0;
    for value in 2..=512 {
        let mut call = client
            .call("", "run", &[json!({"$bigint":value.to_string()})])
            .unwrap();
        let origin = call.id();
        assert!(result(&mut driver, &mut call).is_ok());
        let retained = client.retained_origins();
        assert!(retained.contains(&first_origin));
        assert!(retained.contains(&origin));
        gates
            .lock()
            .unwrap()
            .remove(&value)
            .unwrap()
            .send(Ok(()))
            .unwrap();
        tick(&mut driver);
        completed.push(origin);
        let retained = client.retained_origins();
        peak = peak.max(retained.len());
        assert!(retained.contains(&first_origin));
        assert!(
            retained.len() <= 8,
            "completed origins accumulated behind an active background task: {}",
            retained.len()
        );
        assert!(driver.is_active());
    }
    assert!(
        completed
            .iter()
            .filter(|id| !client.retained_origins().contains(id))
            .count()
            > 500,
        "completed contexts must actually become releasable before global quiescence"
    );
    assert!(peak > 0);
    gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    tick(&mut driver);
    assert_eq!(
        observed.lock().unwrap().last(),
        Some(&(1, first_origin, "end"))
    );
    assert_eq!(drops.load(Ordering::SeqCst), 512);
    assert!(client.retained_origins().is_empty());
}

#[test]
fn retirement_clears_retained_origins_and_old_drivers_do_not_clear_a_restarted_host() {
    let (component, gates, _, drops) = host(false);
    let (mut driver, client) = component.into_driver().unwrap();
    let call = client.call("", "run", &[json!({"$bigint":"1"})]).unwrap();
    let origin = call.id();
    tick(&mut driver);
    assert!(client.retained_origins().contains(&origin));
    drop(call);
    assert!(ready(&mut driver).is_err());
    assert!(client.retained_origins().is_empty());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(gates
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(Ok(()))
        .is_err());
    let (host, gates, _, _) = host(false);
    let (mut old, old_client) = host.into_driver().unwrap();
    // Host recovery requires closing all senders, so retain only the old Driver.
    drop(old_client);
    let host = ready(&mut old).unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let mut call = client.call("", "run", &[json!({"$bigint":"2"})]).unwrap();
    let origin = call.id();
    tick(&mut driver);
    assert_eq!(client.retained_origins(), vec![origin]);
    drop(old);
    assert_eq!(client.retained_origins(), vec![origin]);
    gates
        .lock()
        .unwrap()
        .remove(&2)
        .unwrap()
        .send(Ok(()))
        .unwrap();
    assert!(result(&mut driver, &mut call).is_ok());
    tick(&mut driver);
    assert!(client.retained_origins().is_empty());
}
