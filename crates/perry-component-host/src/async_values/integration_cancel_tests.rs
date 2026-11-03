use crate::{AsyncImport, Driver, DriverCall, DriverClient, Host, Options};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};
const NS: &str = "test:producer/data";
thread_local! {
    static REENTRANT:RefCell<Option<(DriverClient,Value)>>=const{RefCell::new(None)};
    static CLOSES:RefCell<Vec<DriverCall>>=const{RefCell::new(Vec::new())};
    static AFTER_RETURN:RefCell<Option<(DriverClient,Value,crate::InvocationId)>>=const{RefCell::new(None)};
}
type Gates = Arc<Mutex<VecDeque<oneshot::Sender<()>>>>;
fn setup() -> (Driver, DriverClient, Gates) {
    setup_kind(false)
}
fn setup_kind(stream: bool) -> (Driver, DriverClient, Gates) {
    let kind = if stream { "stream" } else { "future" };
    let source="package test:producer; interface gate { wait: async func(); } interface data { resource thing; make: async func() -> future<own<thing>>; drops: func() -> u32; writes: func() -> u32; consume: func(value: own<thing>); } world plugin { import gate; export data; }";
    let world = perry_wit::parse(
        &source.replace("future<own<thing>>", &format!("{kind}<own<thing>>")),
        None,
    )
    .unwrap();
    let mut producer = include_str!("paused-producer.component.wat").to_string();
    if stream {
        producer = producer
            .replace("(future (own $thing))", "(stream (own $thing))")
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
    }
    let gates: Gates = Arc::default();
    let pending = gates.clone();
    let wait: AsyncImport = Arc::new(move |_| {
        let (send, receive) = oneshot::channel();
        pending.lock().unwrap().push_back(send);
        Box::pin(async move {
            receive.await.unwrap();
            REENTRANT.with(|slot| {
                if let Some((client, value)) = slot.borrow().as_ref() {
                    let close = client.close_async_value(value).unwrap();
                    CLOSES.with(|q| q.borrow_mut().push(close));
                }
            });
            Ok(None)
        })
    });
    let imports = BTreeMap::from([(
        "test:producer/gate".into(),
        BTreeMap::from([("wait".into(), wait)]),
    )]);
    let host = Host::new_with_async_imports(
        &wat::parse_str(producer).unwrap(),
        &world,
        &BTreeMap::new(),
        &imports,
        &Options::default(),
        Arc::new(|event| {
            if matches!(event, crate::ResourceEvent::Drain) {
                AFTER_RETURN.with(|slot| {
                    if let Some((client, value, origin)) = slot.borrow().as_ref() {
                        if crate::current_invocation() == Some(*origin) {
                            let close = client.close_async_value(value).unwrap();
                            CLOSES.with(|q| q.borrow_mut().push(close));
                        }
                    }
                });
            }
            Ok(Vec::new())
        }),
    )
    .unwrap();
    let (driver, client) = host.into_driver().unwrap();
    (driver, client, gates)
}
fn result(driver: &mut Driver, call: &mut DriverCall) -> anyhow::Result<Option<Value>> {
    for _ in 0..128 {
        if let Some(result) = block_on(poll_once(&mut *driver)) {
            panic!("unexpected driver completion: {:?}", result.err());
        }
        if let Some(result) = block_on(poll_once(&mut *call)) {
            return result;
        }
    }
    panic!("ready component operation did not complete");
}
fn drain(driver: &mut Driver) {
    for _ in 0..128 {
        assert!(block_on(poll_once(&mut *driver)).is_none());
        if !driver.is_active() {
            return;
        }
    }
    panic!("signalled guest task did not exit");
}
#[test]
fn pending_read_close_wakes_control_and_reuses_store_after_producer_finishes() {
    let (mut driver, client, gates) = setup();
    for expected in [1, 2] {
        let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
            .unwrap()
            .unwrap();
        let mut read = client.read_async_value(&value, None).unwrap();
        assert!(block_on(poll_once(&mut driver)).is_none());
        assert!(block_on(poll_once(&mut read)).is_none());
        let mut duplicate = client.read_async_value(&value, None).unwrap();
        assert!(result(&mut driver, &mut duplicate)
            .unwrap_err()
            .to_string()
            .contains("pending read"));
        let mut close = client.close_async_value(&value).unwrap();
        assert!(result(&mut driver, &mut close).is_ok());
        assert!(result(&mut driver, &mut read)
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
        assert_eq!(
            result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
            Some(json!(expected - 1))
        );
        gates.lock().unwrap().pop_front().unwrap().send(()).unwrap();
        drain(&mut driver);
        assert_eq!(
            result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
            Some(json!(expected))
        );
        assert_eq!(
            result(&mut driver, &mut client.call(NS, "writes", &[]).unwrap()).unwrap(),
            Some(json!(0))
        );
    }
}
#[test]
fn queued_close_cannot_be_cancelled_by_dropping_its_observer() {
    let (mut driver, client, gates) = setup();
    let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let mut read = client.read_async_value(&value, None).unwrap();
    drop(client.close_async_value(&value).unwrap());
    assert!(result(&mut driver, &mut read).is_err());
    gates.lock().unwrap().pop_front().unwrap().send(()).unwrap();
    drain(&mut driver);
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(1))
    );
}
#[test]
fn close_submitted_from_producer_completion_discards_a_copied_owned_payload_once() {
    let (mut driver, client, gates) = setup();
    let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let mut read = client.read_async_value(&value, None).unwrap();
    assert!(block_on(poll_once(&mut driver)).is_none());
    assert!(block_on(poll_once(&mut read)).is_none());
    REENTRANT.with(|slot| *slot.borrow_mut() = Some((client.clone(), value)));
    gates.lock().unwrap().pop_front().unwrap().send(()).unwrap();
    let cancelled = result(&mut driver, &mut read);
    REENTRANT.with(|slot| slot.borrow_mut().take());
    assert!(cancelled.unwrap_err().to_string().contains("cancelled"));
    for mut close in CLOSES.with(|q| std::mem::take(&mut *q.borrow_mut())) {
        assert!(result(&mut driver, &mut close).is_ok());
    }
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "writes", &[]).unwrap()).unwrap(),
        Some(json!(1)),
        "the payload must have crossed the canonical channel before cancellation was observed"
    );
    assert_eq!(
        result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
        Some(json!(1)),
        "discarding the copied payload must run the original guest destructor exactly once"
    );
}

#[test]
fn close_from_post_return_cleanup_wins_before_read_commit_and_drops_payload_once() {
    for stream in [false, true] {
        let (mut driver, client, gates) = setup_kind(stream);
        let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
            .unwrap()
            .unwrap();
        let mut read = client.read_async_value(&value, None).unwrap();
        assert!(block_on(poll_once(&mut driver)).is_none());
        assert!(block_on(poll_once(&mut read)).is_none());
        AFTER_RETURN.with(|slot| *slot.borrow_mut() = Some((client.clone(), value, read.id())));
        gates.lock().unwrap().pop_front().unwrap().send(()).unwrap();
        let cancelled = result(&mut driver, &mut read);
        AFTER_RETURN.with(|slot| slot.borrow_mut().take());
        assert!(cancelled.unwrap_err().to_string().contains("cancelled"));
        for mut close in CLOSES.with(|q| std::mem::take(&mut *q.borrow_mut())) {
            assert!(result(&mut driver, &mut close).is_ok());
        }
        assert_eq!(
            result(&mut driver, &mut client.call(NS, "writes", &[]).unwrap()).unwrap(),
            Some(json!(1))
        );
        assert_eq!(
            result(&mut driver, &mut client.call(NS, "drops", &[]).unwrap()).unwrap(),
            Some(json!(1))
        );
    }
}

#[test]
fn rejected_close_at_mailbox_quota_does_not_cancel_a_pending_read() {
    let (mut driver, client, gates) = setup();
    let value = result(&mut driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let mut read = client.read_async_value(&value, None).unwrap();
    assert!(block_on(poll_once(&mut driver)).is_none());
    assert!(block_on(poll_once(&mut read)).is_none());
    let queued: Vec<_> = (0..63)
        .map(|_| client.call(NS, "drops", &[]).unwrap())
        .collect();
    assert!(client
        .close_async_value(&value)
        .err()
        .unwrap()
        .to_string()
        .contains("too many pending"));
    gates.lock().unwrap().pop_front().unwrap().send(()).unwrap();
    let owned = result(&mut driver, &mut read).unwrap().unwrap();
    for mut call in queued {
        assert!(result(&mut driver, &mut call).is_ok());
    }
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
