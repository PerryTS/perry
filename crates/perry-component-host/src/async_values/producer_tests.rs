use super::integration_tests::{instance, instance_hook, result};
use crate::{AsyncValueType, Driver, DriverClient, Host, Options};
use futures_lite::future::{block_on, poll_once};
use perry_wit::{Type, World};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
thread_local! {
 static LATE:RefCell<Option<(DriverClient,Value,crate::InvocationId,bool)>>=const{RefCell::new(None)};
 static CONTROLS:RefCell<Vec<crate::DriverCall>>=const{RefCell::new(Vec::new())};
 static IMPORT:RefCell<Option<(Value,crate::InvocationId)>>=const{RefCell::new(None)};
}
const NS: &str = "test:producer/data";
fn scalar_host() -> (Driver, DriverClient, World) {
    let source="package test:factory; world plugin { export data: func(value: future<tuple<u64,string,list<u8>>>) -> future<tuple<u64,string,list<u8>>>; export strings: func(value: stream<string>) -> stream<string>; export void: func(value: future) -> future; export units: func(value: stream) -> stream; export nested: func(value: future<future<u32>>) -> future<future<u32>>; }";
    let world = perry_wit::parse(source, None).unwrap();
    let host = Host::new(
        &wat::parse_str(include_str!("factory-echo.component.wat")).unwrap(),
        &world,
        &BTreeMap::new(),
        &Options::default(),
    )
    .unwrap();
    let (driver, client) = host.into_driver().unwrap();
    (driver, client, world)
}
fn pair(driver: &mut Driver, client: &DriverClient, ty: &AsyncValueType) -> (Value, Value) {
    let pair = result(driver, &mut client.create_async_value(ty).unwrap())
        .unwrap()
        .unwrap();
    assert!(pair["writer"]["$writer"].is_string());
    assert!(pair["writer"]["core"].is_null());
    (pair["value"].clone(), pair["writer"].clone())
}
fn owned(driver: &mut Driver, client: &DriverClient) -> Value {
    let value = result(driver, &mut client.call(NS, "make", &[]).unwrap())
        .unwrap()
        .unwrap();
    let packet = result(driver, &mut client.read_async_value(&value, None).unwrap())
        .unwrap()
        .unwrap();
    let owned = if value.get("$stream").is_some() {
        assert_eq!(
            result(driver, &mut client.read_async_value(&value, None).unwrap()).unwrap(),
            Some(json!({"done":true}))
        );
        packet["value"][0].clone()
    } else {
        packet
    };
    for _ in 0..128 {
        assert!(block_on(poll_once(&mut *driver)).is_none());
        if !driver.is_active() {
            return owned;
        }
    }
    panic!("producer did not finish")
}

fn drops(driver: &mut Driver, client: &DriverClient) -> Value {
    result(driver, &mut client.call(NS, "drops", &[]).unwrap())
        .unwrap()
        .unwrap()
}
fn resolve<'a>(world: &'a World, mut ty: &'a Type) -> &'a Type {
    while let Type::Ref(key) = ty {
        ty = &world.types[key].ty;
    }
    ty
}

#[test]
fn production_factory_future_transfers_nominal_resource_and_cancellation_returns_owner_for_retry() {
    let (mut driver, client) = instance(false);
    let ty = client.async_value_types()[0].clone();
    let (reader, writer) = pair(&mut driver, &client, &ty);
    let original = owned(&mut driver, &client);
    let mut write = client
        .write_async_value(&writer, std::slice::from_ref(&original))
        .unwrap();
    assert!(block_on(poll_once(&mut driver)).is_none());
    assert!(block_on(poll_once(&mut write)).is_none());
    assert!(client.close_async_writer(&writer).is_err());
    let mut duplicate = client
        .write_async_value(&writer, std::slice::from_ref(&original))
        .unwrap();
    assert!(result(&mut driver, &mut duplicate)
        .unwrap_err()
        .to_string()
        .contains("pending write"));
    let mut cancel = client.cancel_async_write(&writer).unwrap();
    assert!(result(&mut driver, &mut cancel).is_ok());
    let cancelled = result(&mut driver, &mut write).unwrap().unwrap();
    assert_eq!(cancelled["cancelled"], true);
    assert_eq!(cancelled["written"], 0);
    assert_eq!(drops(&mut driver, &client), json!(0));
    let returned = cancelled["remaining"][0].clone();
    assert_ne!(returned, original);
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[original]).unwrap()
    )
    .is_err());
    let mut retry = client.write_async_value(&writer, &[returned]).unwrap();
    let received = result(
        &mut driver,
        &mut client.read_async_value(&reader, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    let written = result(&mut driver, &mut retry).unwrap().unwrap();
    assert_eq!(
        written,
        json!({"written":1,"remaining":[],"cancelled":false,"closed":true})
    );
    assert!(client.write_async_value(&writer, &[json!(null)]).is_err());
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[received]).unwrap()
    )
    .is_ok());
    assert_eq!(drops(&mut driver, &client), json!(1));
}

#[test]
fn production_factory_stream_partial_write_returns_unconsumed_resource_and_close_produces_eof() {
    let (mut driver, client) = instance(true);
    let ty = client.async_value_types()[0].clone();
    let (reader, writer) = pair(&mut driver, &client, &ty);
    let first = owned(&mut driver, &client);
    let second = owned(&mut driver, &client);
    let mut write = client.write_async_value(&writer, &[first, second]).unwrap();
    let received = result(
        &mut driver,
        &mut client.read_async_value(&reader, Some(1)).unwrap(),
    )
    .unwrap()
    .unwrap();
    let written = result(&mut driver, &mut write).unwrap().unwrap();
    assert_eq!(written["written"], 1);
    assert_eq!(written["remaining"].as_array().unwrap().len(), 1);
    assert_eq!(written["closed"], false);
    assert!(result(
        &mut driver,
        &mut client
            .call(NS, "consume", &[received["value"][0].clone()])
            .unwrap()
    )
    .is_ok());
    assert!(result(
        &mut driver,
        &mut client
            .call(NS, "consume", &[written["remaining"][0].clone()])
            .unwrap()
    )
    .is_ok());
    assert_eq!(drops(&mut driver, &client), json!(2));
    drop(client.close_async_writer(&writer).unwrap());
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&reader, Some(1)).unwrap()
        )
        .unwrap(),
        Some(json!({"done":true}))
    );
}

#[test]
fn close_pending_stream_writer_discards_unwritten_owner_and_keeps_store_reusable() {
    let (mut driver, client) = instance(true);
    let ty = client.async_value_types()[0].clone();
    let (reader, writer) = pair(&mut driver, &client, &ty);
    let original = owned(&mut driver, &client);
    let mut write = client.write_async_value(&writer, &[original]).unwrap();
    assert!(block_on(poll_once(&mut driver)).is_none());
    assert!(block_on(poll_once(&mut write)).is_none());
    drop(client.close_async_writer(&writer).unwrap());
    assert!(result(&mut driver, &mut write)
        .unwrap_err()
        .to_string()
        .contains("cancelled"));
    assert_eq!(drops(&mut driver, &client), json!(1));
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&reader, None).unwrap()
        )
        .unwrap(),
        Some(json!({"done":true}))
    );
}

#[test]
fn production_factories_reject_foreign_types_writers_and_invalid_payloads_without_consuming_owners()
{
    let (mut first, a) = instance(false);
    let (mut second, b) = instance(false);
    let at = a.async_value_types()[0].clone();
    let bt = b.async_value_types()[0].clone();
    assert!(b.create_async_value(&at).is_err());
    let (reader, writer) = pair(&mut first, &a, &at);
    for action in [
        b.write_async_value(&writer, &[json!(0)]),
        b.cancel_async_write(&writer),
        b.close_async_writer(&writer),
        b.abort_async_writer(&writer),
    ] {
        assert!(action.is_err());
    }
    let original = owned(&mut first, &a);
    let mut invalid = a.write_async_value(&writer, &[json!(3)]).unwrap();
    assert!(result(&mut first, &mut invalid).is_err());
    assert!(a.write_async_value(&writer, &[]).is_err());
    assert!(a
        .write_async_value(&writer, &[original.clone(), original.clone()])
        .is_err());
    let mut write = a.write_async_value(&writer, &[original]).unwrap();
    let received = result(&mut first, &mut a.read_async_value(&reader, None).unwrap())
        .unwrap()
        .unwrap();
    assert!(result(&mut first, &mut write).is_ok());
    assert!(result(&mut first, &mut a.call(NS, "consume", &[received]).unwrap()).is_ok());
    assert_eq!(drops(&mut first, &a), json!(1));
    let (reader, writer) = pair(&mut second, &b, &bt);
    assert!(result(&mut second, &mut b.close_async_value(&reader).unwrap()).is_ok());
    let original = owned(&mut second, &b);
    let returned = result(
        &mut second,
        &mut b.write_async_value(&writer, &[original]).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(returned["closed"], true);
    assert_eq!(returned["written"], 0);
    assert!(result(
        &mut second,
        &mut b
            .call(NS, "consume", &[returned["remaining"][0].clone()])
            .unwrap()
    )
    .is_ok());
    assert_eq!(drops(&mut second, &b), json!(1));
}

#[test]
fn production_factory_compound_and_void_futures_pass_through_original_guest_exports() {
    let (mut driver, client, world) = scalar_host();
    let types = client.async_value_types().to_vec();
    for (name, ty) in [
        (
            "data",
            types
                .iter()
                .find(|t| {
                    matches!(
                        t.payload().map(|t| resolve(&world, t)),
                        Some(Type::Tuple(_))
                    )
                })
                .unwrap(),
        ),
        (
            "void",
            types
                .iter()
                .find(|t| !t.is_stream() && t.payload().is_none())
                .unwrap(),
        ),
    ] {
        let (reader, writer) = pair(&mut driver, &client, ty);
        let reader = result(&mut driver, &mut client.call("", name, &[reader]).unwrap())
            .unwrap()
            .unwrap();
        let value = if name == "void" {
            Value::Null
        } else {
            json!([{"$bigint":"18364758544493064720"},"héllo 🌍",(0..65537u32).map(|i|(i%256) as u8).collect::<Vec<_>>()])
        };
        let mut write = client
            .write_async_value(&writer, std::slice::from_ref(&value))
            .unwrap();
        let read = result(
            &mut driver,
            &mut client.read_async_value(&reader, None).unwrap(),
        )
        .unwrap();
        assert_eq!(read, if name == "void" { None } else { Some(value) });
        assert_eq!(
            result(&mut driver, &mut write).unwrap().unwrap()["written"],
            1
        );
    }
}

#[test]
fn production_factory_unit_stream_and_queued_cancellation_preserve_counts() {
    let (mut driver, client, _) = scalar_host();
    let ty = client
        .async_value_types()
        .iter()
        .find(|t| t.is_stream() && t.payload().is_none())
        .unwrap()
        .clone();
    let (reader, writer) = pair(&mut driver, &client, &ty);
    let mut cancelled = client
        .write_async_value(&writer, &vec![Value::Null; 7])
        .unwrap();
    drop(client.cancel_async_write(&writer).unwrap());
    let result_cancelled = result(&mut driver, &mut cancelled).unwrap().unwrap();
    assert_eq!(result_cancelled["written"], 0);
    assert_eq!(
        result_cancelled["remaining"],
        json!([null, null, null, null, null, null, null])
    );
    let mut remaining = 7;
    for _ in 0..3 {
        let mut write = client
            .write_async_value(&writer, &vec![Value::Null; remaining])
            .unwrap();
        let read = result(
            &mut driver,
            &mut client.read_async_value(&reader, Some(3)).unwrap(),
        )
        .unwrap()
        .unwrap();
        let count = remaining.min(3);
        assert_eq!(read["value"], Value::Array(vec![Value::Null; count]));
        remaining -= count;
        let written = result(&mut driver, &mut write).unwrap().unwrap();
        assert_eq!(written["written"], count);
        assert_eq!(written["remaining"].as_array().unwrap().len(), remaining);
    }
    assert_eq!(remaining, 0);
    assert!(result(
        &mut driver,
        &mut client.close_async_writer(&writer).unwrap()
    )
    .is_ok());
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&reader, None).unwrap()
        )
        .unwrap(),
        Some(json!({"done":true}))
    );
}

#[test]
fn native_factory_quota_reserves_both_ends_before_running_concurrent_factories() {
    let (mut driver, client, _) = scalar_host();
    let ty = client
        .async_value_types()
        .iter()
        .find(|t| t.is_stream() && t.payload() == Some(&Type::String))
        .unwrap()
        .clone();
    let mut pairs = Vec::new();
    for _ in 0..15 {
        let calls: Vec<_> = (0..32)
            .map(|_| client.create_async_value(&ty).unwrap())
            .collect();
        for mut call in calls {
            pairs.push(result(&mut driver, &mut call).unwrap().unwrap());
        }
    }
    let calls: Vec<_> = (0..64)
        .map(|_| client.create_async_value(&ty).unwrap())
        .collect();
    let mut rejected = 0;
    for mut call in calls {
        match result(&mut driver, &mut call) {
            Ok(Some(value)) => pairs.push(value),
            Err(error) => {
                assert!(error.to_string().contains("too many live"));
                rejected += 1
            }
            _ => panic!("factory result"),
        }
    }
    assert_eq!(pairs.len(), 512);
    assert_eq!(rejected, 32);
    for pair in pairs {
        assert!(result(
            &mut driver,
            &mut client.close_async_value(&pair["value"]).unwrap()
        )
        .is_ok());
        assert!(result(
            &mut driver,
            &mut client.close_async_writer(&pair["writer"]).unwrap()
        )
        .is_ok());
    }
    let (reader, writer) = pair(&mut driver, &client, &ty);
    assert!(result(&mut driver, &mut client.close_async_value(&reader).unwrap()).is_ok());
    assert!(result(
        &mut driver,
        &mut client.close_async_writer(&writer).unwrap()
    )
    .is_ok());
}

#[test]
fn explicit_producer_abort_retires_only_its_component_and_dropping_started_writer_also_retires() {
    let (mut other, client_other) = instance(false);
    for abort in [true, false] {
        let (mut driver, client) = instance(false);
        let ty = client.async_value_types()[0].clone();
        let (_reader, writer) = pair(&mut driver, &client, &ty);
        let owner = owned(&mut driver, &client);
        let mut write = client.write_async_value(&writer, &[owner]).unwrap();
        assert!(block_on(poll_once(&mut driver)).is_none());
        assert!(block_on(poll_once(&mut write)).is_none());
        let mut abort_call = if abort {
            Some(client.abort_async_writer(&writer).unwrap())
        } else {
            None
        };
        if !abort {
            drop(write);
        }
        let mut retired = false;
        for _ in 0..128 {
            if let Some(outcome) = block_on(poll_once(&mut driver)) {
                assert!(outcome.is_err());
                retired = true;
                break;
            }
        }
        assert!(retired);
        if let Some(call) = abort_call.as_mut() {
            assert!(block_on(call)
                .unwrap_err()
                .to_string()
                .contains("producer aborted"));
        }
        assert!(client.call(NS, "drops", &[]).is_err());
    }
    let owner = owned(&mut other, &client_other);
    assert!(result(
        &mut other,
        &mut client_other.call(NS, "consume", &[owner]).unwrap()
    )
    .is_ok());
    assert_eq!(drops(&mut other, &client_other), json!(1));
}

#[test]
fn queued_writer_close_preserves_uncommitted_arguments_and_mailbox_rejection_leaves_a_write_alive()
{
    let (mut driver, client) = instance(true);
    let ty = client.async_value_types()[0].clone();
    let (reader, writer) = pair(&mut driver, &client, &ty);
    let owner = owned(&mut driver, &client);
    let mut skipped = client
        .write_async_value(&writer, std::slice::from_ref(&owner))
        .unwrap();
    drop(client.close_async_writer(&writer).unwrap());
    assert!(result(&mut driver, &mut skipped).is_err());
    assert_eq!(drops(&mut driver, &client), json!(0));
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[owner]).unwrap()
    )
    .is_ok());
    assert_eq!(drops(&mut driver, &client), json!(1));
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&reader, None).unwrap()
        )
        .unwrap(),
        Some(json!({"done":true}))
    );
    let (reader, writer) = pair(&mut driver, &client, &ty);
    let owner = owned(&mut driver, &client);
    let mut write = client.write_async_value(&writer, &[owner]).unwrap();
    assert!(block_on(poll_once(&mut driver)).is_none());
    assert!(block_on(poll_once(&mut write)).is_none());
    let calls: Vec<_> = (0..63)
        .map(|_| client.call(NS, "drops", &[]).unwrap())
        .collect();
    assert!(client
        .close_async_writer(&writer)
        .err()
        .unwrap()
        .to_string()
        .contains("too many pending"));
    for mut call in calls {
        assert!(result(&mut driver, &mut call).is_ok());
    }
    let received = result(
        &mut driver,
        &mut client.read_async_value(&reader, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        result(&mut driver, &mut write).unwrap().unwrap()["cancelled"],
        false
    );
    assert!(result(
        &mut driver,
        &mut client
            .call(NS, "consume", &[received["value"][0].clone()])
            .unwrap()
    )
    .is_ok());
    assert_eq!(drops(&mut driver, &client), json!(2));
    assert!(result(
        &mut driver,
        &mut client.close_async_writer(&writer).unwrap()
    )
    .is_ok());
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&reader, None).unwrap()
        )
        .unwrap(),
        Some(json!({"done":true}))
    );
}

#[test]
fn writer_handles_survive_quiescent_host_recovery_and_restart_of_the_driver() {
    let (mut driver, client, _) = scalar_host();
    let ty = client
        .async_value_types()
        .iter()
        .find(|t| t.is_stream() && t.payload() == Some(&Type::String))
        .unwrap()
        .clone();
    let (reader, writer) = pair(&mut driver, &client, &ty);
    drop(client);
    let mut recovered = None;
    for _ in 0..128 {
        if let Some(host) = block_on(poll_once(&mut driver)) {
            recovered = Some(host.unwrap());
            break;
        }
    }
    let host = recovered.expect("quiescent driver returns Host");
    let (mut driver, client) = host.into_driver().unwrap();
    let _second = pair(&mut driver, &client, &ty);
    let mut write = client
        .write_async_value(&writer, &[json!("retained 🌍")])
        .unwrap();
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&reader, None).unwrap()
        )
        .unwrap(),
        Some(json!({"done":false,"value":["retained 🌍"]}))
    );
    assert_eq!(
        result(&mut driver, &mut write).unwrap().unwrap()["written"],
        1
    );
    assert!(result(
        &mut driver,
        &mut client.close_async_writer(&writer).unwrap()
    )
    .is_ok());
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&reader, None).unwrap()
        )
        .unwrap(),
        Some(json!({"done":true}))
    );
}

#[test]
fn native_factory_nested_futures_keep_both_writers_and_exact_payload_types() {
    let (mut driver, client, world) = scalar_host();
    let types = client.async_value_types().to_vec();
    let inner = types
        .iter()
        .find(|t| t.payload().map(|t| resolve(&world, t)) == Some(&Type::U32))
        .unwrap();
    let outer = types
        .iter()
        .find(|t| {
            matches!(
                t.payload().map(|t| resolve(&world, t)),
                Some(Type::Future(_))
            )
        })
        .unwrap();
    let (inner_read, inner_write) = pair(&mut driver, &client, inner);
    let (outer_read, outer_write) = pair(&mut driver, &client, outer);
    let mut outer_call = client
        .write_async_value(&outer_write, std::slice::from_ref(&inner_read))
        .unwrap();
    let nested = result(
        &mut driver,
        &mut client.read_async_value(&outer_read, None).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert!(result(&mut driver, &mut outer_call).is_ok());
    assert!(result(
        &mut driver,
        &mut client.read_async_value(&inner_read, None).unwrap()
    )
    .is_err());
    let mut inner_call = client
        .write_async_value(&inner_write, &[json!(42)])
        .unwrap();
    assert_eq!(
        result(
            &mut driver,
            &mut client.read_async_value(&nested, None).unwrap()
        )
        .unwrap(),
        Some(json!(42))
    );
    assert!(result(&mut driver, &mut inner_call).is_ok());
}

#[test]
fn post_return_writer_controls_win_before_commit_without_dropping_consumed_owners() {
    for close in [false, true] {
        let (mut driver, client) = instance_hook(
            true,
            Arc::new(|event| {
                if matches!(event, crate::ResourceEvent::Drain) {
                    LATE.with(|slot| {
                        let matches = slot.borrow().as_ref().is_some_and(|(_, _, origin, _)| {
                            crate::current_invocation() == Some(*origin)
                        });
                        if matches {
                            let (client, writer, _, close) = slot.borrow_mut().take().unwrap();
                            let control = if close {
                                client.close_async_writer(&writer)
                            } else {
                                client.cancel_async_write(&writer)
                            }
                            .unwrap();
                            CONTROLS.with(|q| q.borrow_mut().push(control));
                        }
                    });
                }
                Ok(Vec::new())
            }),
        );
        let ty = client.async_value_types()[0].clone();
        let (reader, writer) = pair(&mut driver, &client, &ty);
        let first = owned(&mut driver, &client);
        let second = owned(&mut driver, &client);
        let mut write = client.write_async_value(&writer, &[first, second]).unwrap();
        LATE.with(|slot| {
            *slot.borrow_mut() = Some((client.clone(), writer.clone(), write.id(), close))
        });
        let received = result(
            &mut driver,
            &mut client.read_async_value(&reader, Some(1)).unwrap(),
        )
        .unwrap()
        .unwrap();
        let outcome = result(&mut driver, &mut write);
        assert!(
            LATE.with(|slot| slot.borrow().is_none()),
            "the close must have been submitted after helper return"
        );
        if close {
            assert!(outcome.unwrap_err().to_string().contains("cancelled"));
            assert_eq!(drops(&mut driver, &client), json!(1));
        } else {
            let written = outcome.unwrap().unwrap();
            assert_eq!(written["written"], 1);
            assert_eq!(written["cancelled"], true);
            assert_eq!(drops(&mut driver, &client), json!(0));
            assert!(result(
                &mut driver,
                &mut client
                    .call(NS, "consume", &[written["remaining"][0].clone()])
                    .unwrap()
            )
            .is_ok());
        }
        for mut control in CONTROLS.with(|q| std::mem::take(&mut *q.borrow_mut())) {
            assert!(result(&mut driver, &mut control).is_ok());
        }
        assert!(result(
            &mut driver,
            &mut client
                .call(NS, "consume", &[received["value"][0].clone()])
                .unwrap()
        )
        .is_ok());
        assert_eq!(drops(&mut driver, &client), json!(2));
        if !close {
            assert!(result(
                &mut driver,
                &mut client.close_async_writer(&writer).unwrap()
            )
            .is_ok());
        }
        assert_eq!(
            result(
                &mut driver,
                &mut client.read_async_value(&reader, None).unwrap()
            )
            .unwrap(),
            Some(json!({"done":true}))
        );
    }
}

#[test]
fn native_factory_reader_can_be_returned_by_an_import_and_consumed_inside_the_guest() {
    let world=perry_wit::parse("package test:input; interface source { get: func() -> future<u32>; } world plugin { import source; export take: async func() -> u32; }",None).unwrap();
    let get: crate::Import = Arc::new(|args| {
        assert!(args.is_empty());
        let (value, origin) = IMPORT.with(|slot| slot.borrow_mut().take().unwrap());
        assert_eq!(crate::current_invocation(), Some(origin));
        Ok(Some(value))
    });
    let imports = BTreeMap::from([(
        "test:input/source".into(),
        BTreeMap::from([("get".into(), get)]),
    )]);
    let host = Host::new(
        &wat::parse_str(include_str!("factory-import.component.wat")).unwrap(),
        &world,
        &imports,
        &Options::default(),
    )
    .unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let ty = client.async_value_types()[0].clone();
    let (reader, writer) = pair(&mut driver, &client, &ty);
    let mut take = client.call("", "take", &[]).unwrap();
    IMPORT.with(|slot| *slot.borrow_mut() = Some((reader.clone(), take.id())));
    assert!(block_on(poll_once(&mut driver)).is_none());
    assert!(block_on(poll_once(&mut take)).is_none());
    assert!(result(
        &mut driver,
        &mut client.read_async_value(&reader, None).unwrap()
    )
    .is_err());
    let mut write = client.write_async_value(&writer, &[json!(42)]).unwrap();
    assert_eq!(result(&mut driver, &mut take).unwrap(), Some(json!(42)));
    assert_eq!(
        result(&mut driver, &mut write).unwrap().unwrap()["written"],
        1
    );
    assert!(IMPORT.with(|slot| slot.borrow().is_none()));
}

#[test]
fn native_factory_owned_future_is_read_inside_the_original_resource_defining_guest() {
    let world=perry_wit::parse("package test:producer; interface data { resource thing; make: async func() -> future<own<thing>>; take: async func(value: future<own<thing>>) -> own<thing>; drops: func() -> u32; consume: func(value: own<thing>); } world plugin { export data; }",None).unwrap();
    let text = include_str!("producer.component.wat")
        .replace(
            "  (instance $data",
            &format!(
                "{}\n  (instance $data",
                include_str!("factory-owned-consumer.wat")
            ),
        )
        .replace(
            "(instance $data (export \"thing\"",
            "(instance $data (export \"take\" (func $take)) (export \"thing\"",
        );
    let host = Host::new(
        &wat::parse_str(text).unwrap(),
        &world,
        &BTreeMap::new(),
        &Options::default(),
    )
    .unwrap();
    let (mut driver, client) = host.into_driver().unwrap();
    let ty = client.async_value_types()[0].clone();
    let (reader, writer) = pair(&mut driver, &client, &ty);
    let owner = owned(&mut driver, &client);
    let mut take = client.call(NS, "take", &[reader]).unwrap();
    assert!(block_on(poll_once(&mut driver)).is_none());
    assert!(block_on(poll_once(&mut take)).is_none());
    let mut write = client.write_async_value(&writer, &[owner]).unwrap();
    let owner = result(&mut driver, &mut take).unwrap().unwrap();
    assert_eq!(
        result(&mut driver, &mut write).unwrap().unwrap()["written"],
        1
    );
    assert_eq!(drops(&mut driver, &client), json!(0));
    assert!(result(
        &mut driver,
        &mut client.call(NS, "consume", &[owner]).unwrap()
    )
    .is_ok());
    assert_eq!(drops(&mut driver, &client), json!(1));
}
