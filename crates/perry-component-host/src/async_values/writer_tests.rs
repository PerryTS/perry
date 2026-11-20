use super::{
    reader_tests::{component, control, engine, field, invoke, ready, world},
    reader_types::Resources,
};
use futures_lite::future::{block_on, poll_once};
use perry_wit::Type;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use wasmtime::{
    component::{Func, Linker, Resource, ResourceType, Val},
    Store,
};

fn factory(store: &mut Store<()>, function: Func) -> (Val, Val) {
    let Val::Tuple(pair) = invoke(store, function, &[]) else {
        panic!("factory tuple")
    };
    assert_eq!(pair.len(), 2);
    (pair[0].clone(), pair[1].clone())
}
fn exchange(
    store: &mut Store<()>,
    functions: (Func, Func),
    ends: (Val, Val),
    values: Vec<Val>,
    count: u32,
    capacity: u32,
    id: u32,
) -> (Val, Val) {
    let (read, write) = functions;
    let (input, writer) = ends;
    let write_args = [writer, Val::List(values), Val::U32(count), Val::U32(id)];
    let read_args = [input, Val::U32(capacity), Val::U32(id + 1)];
    let mut written = [Val::Bool(false)];
    let mut received = [Val::Bool(false)];
    ready(store.run_concurrent(async |accessor| {
        futures_util::try_join!(
            write.call_concurrent(accessor, &write_args, &mut written),
            read.call_concurrent(accessor, &read_args, &mut received)
        )?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }))
    .unwrap()
    .unwrap();
    (written[0].clone(), received[0].clone())
}
fn close(store: &mut Store<()>, function: Func, writer: Val) {
    ready(store.run_concurrent(async |accessor| {
        function
            .call_concurrent(accessor, &[writer], &mut [])
            .await?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }))
    .unwrap()
    .unwrap();
}

#[test]
fn canonical_factories_write_void_scalars_and_compound_futures_with_reusable_memory() {
    let cases = vec![
        (None, vec![]),
        (Some(Type::U64), vec![Val::U64(0xfedc_ba98_7654_3210)]),
        (
            Some(Type::Tuple(vec![
                Type::String,
                Type::List(Box::new(Type::U8)),
            ])),
            vec![Val::Tuple(vec![
                Val::String("héllo 🌍".into()),
                Val::List((0..65537u32).map(|i| Val::U8(i as u8)).collect()),
            ])],
        ),
    ];
    for (payload, values) in cases {
        let engine = engine();
        let component = component(
            &engine,
            &world(),
            payload.as_ref(),
            false,
            &Resources::new(),
        );
        let mut linker = Linker::new(&engine);
        let signals = control(&mut linker);
        let mut store = Store::new(&engine, ());
        let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
        let consumer = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
        let new = instance.get_func(&mut store, "writer-new").unwrap();
        let read = consumer.get_func(&mut store, "read").unwrap();
        let write = instance.get_func(&mut store, "writer-write").unwrap();
        let pages = instance.get_func(&mut store, "pages").unwrap();
        let mut plateau = None;
        for id in [1, 3, 5] {
            let (reader, writer) = factory(&mut store, new);
            let (written, received) = exchange(
                &mut store,
                (read, write),
                (reader, writer),
                values.clone(),
                1,
                1,
                id,
            );
            assert_eq!(field(&written, "status"), &Val::U32(0));
            assert_eq!(field(&written, "count"), &Val::U32(1));
            assert_eq!(field(&written, "remaining"), &Val::List(vec![]));
            assert_eq!(field(&received, "count"), &Val::U32(1));
            assert_eq!(field(&received, "values"), &Val::List(values.clone()));
            let now = invoke(&mut store, pages, &[]);
            if let Some(before) = &plateau {
                assert_eq!(&now, before)
            } else {
                plateau = Some(now)
            }
            for id in [id, id + 1] {
                assert!(signals
                    .lock()
                    .unwrap()
                    .remove(&id)
                    .unwrap()
                    .send(())
                    .is_err());
            }
        }
    }
}

#[test]
fn canonical_stream_writes_return_partial_remainders_and_preserve_unit_counts() {
    for unit in [false, true] {
        let engine = engine();
        let component = component(
            &engine,
            &world(),
            if unit { None } else { Some(&Type::String) },
            true,
            &Resources::new(),
        );
        let mut linker = Linker::new(&engine);
        let signals = control(&mut linker);
        let mut store = Store::new(&engine, ());
        let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
        let consumer = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
        let new = instance.get_func(&mut store, "writer-new").unwrap();
        let read = consumer.get_func(&mut store, "read").unwrap();
        let write = instance.get_func(&mut store, "writer-write").unwrap();
        let drop = instance.get_func(&mut store, "writer-close").unwrap();
        let (mut reader, writer) = factory(&mut store, new);
        let mut remaining = 7;
        let mut values = if unit {
            vec![]
        } else {
            (0..7)
                .map(|i| Val::String(format!("item {i} 🌍")))
                .collect()
        };
        let mut all = Vec::new();
        for id in [1, 3, 5] {
            let (written, received) = exchange(
                &mut store,
                (read, write),
                (reader, writer.clone()),
                values,
                remaining,
                3,
                id,
            );
            let count = remaining.min(3);
            assert_eq!(field(&written, "status"), &Val::U32(0));
            assert_eq!(field(&written, "count"), &Val::U32(count));
            assert_eq!(field(&received, "count"), &Val::U32(count));
            remaining -= count;
            let Val::List(rest) = field(&written, "remaining") else {
                panic!("remaining")
            };
            values = rest.clone();
            assert_eq!(values.len(), if unit { 0 } else { remaining as usize });
            let Val::List(chunk) = field(&received, "values") else {
                panic!("chunk")
            };
            all.extend(chunk.clone());
            let Val::Option(Some(next)) = field(&received, "reader") else {
                panic!("continuation")
            };
            reader = (**next).clone();
            for id in [id, id + 1] {
                assert!(signals
                    .lock()
                    .unwrap()
                    .remove(&id)
                    .unwrap()
                    .send(())
                    .is_err());
            }
        }
        assert_eq!(remaining, 0);
        assert_eq!(
            all,
            if unit {
                vec![]
            } else {
                (0..7)
                    .map(|i| Val::String(format!("item {i} 🌍")))
                    .collect()
            }
        );
        close(&mut store, drop, writer);
        let packet = invoke(&mut store, read, &[reader, Val::U32(3), Val::U32(7)]);
        assert_eq!(field(&packet, "status"), &Val::U32(1));
        assert_eq!(field(&packet, "count"), &Val::U32(0));
        assert_eq!(field(&packet, "reader"), &Val::Option(None));
    }
}

struct Item;
struct Owned {
    store: Store<()>,
    new: Func,
    read: Func,
    write: Func,
    dispose: Func,
    drop: Func,
    signals: super::reader_tests::Signals,
    drops: Arc<AtomicUsize>,
}
fn owned_fixture(stream: bool) -> Owned {
    let source="package test:write; interface data { resource item; get: func() -> future<tuple<own<item>,string>>; } world plugin {export data;}";
    let world = perry_wit::parse(
        &source.replace("future<", if stream { "stream<" } else { "future<" }),
        None,
    )
    .unwrap();
    let mut ty = world.exports[0].functions[0].result.as_ref().unwrap();
    while let Type::Ref(key) = ty {
        ty = &world.types[key].ty;
    }
    let (Type::Future(Some(payload)) | Type::Stream(Some(payload))) = ty else {
        panic!("future")
    };
    let resources = world.exports[0]
        .resources
        .values()
        .map(|key| (key.clone(), ("types".into(), "item".into())))
        .collect();
    let engine = engine();
    let component = component(&engine, &world, Some(payload), stream, &resources);
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let drops = Arc::new(AtomicUsize::new(0));
    let dropped = drops.clone();
    linker
        .instance("types")
        .unwrap()
        .resource("item", ResourceType::host::<Item>(), move |_, rep| {
            assert!((77..=78).contains(&rep));
            dropped.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let consumer = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let new = instance.get_func(&mut store, "writer-new").unwrap();
    let read = consumer.get_func(&mut store, "read").unwrap();
    let write = instance.get_func(&mut store, "writer-write").unwrap();
    let dispose = consumer.get_func(&mut store, "dispose").unwrap();
    let drop = instance.get_func(&mut store, "writer-close").unwrap();
    Owned {
        store,
        new,
        read,
        write,
        dispose,
        drop,
        signals,
        drops,
    }
}

#[test]
fn cancelled_future_write_returns_its_owned_payload_for_retry_without_destroying_it() {
    let Owned {
        mut store,
        new,
        read,
        write,
        dispose,
        signals,
        drops,
        ..
    } = owned_fixture(false);
    let (reader, writer) = factory(&mut store, new);
    let resource = Resource::<Item>::new_own(77)
        .try_into_resource_any(&mut store)
        .unwrap();
    let input = [
        writer.clone(),
        Val::List(vec![Val::Tuple(vec![
            Val::Resource(resource),
            Val::String("retry 🌍".into()),
        ])]),
        Val::U32(1),
        Val::U32(1),
    ];
    let mut output = [Val::Bool(false)];
    let mut pending = Box::pin(store.run_concurrent(async |accessor| {
        write.call_concurrent(accessor, &input, &mut output).await?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }));
    assert!(block_on(poll_once(pending.as_mut())).is_none());
    signals
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(())
        .unwrap();
    ready(pending).unwrap().unwrap();
    assert_eq!(field(&output[0], "status"), &Val::U32(2));
    assert_eq!(field(&output[0], "count"), &Val::U32(0));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let Val::List(values) = field(&output[0], "remaining") else {
        panic!("remaining")
    };
    let (written, received) = exchange(
        &mut store,
        (read, write),
        (reader, writer),
        values.clone(),
        1,
        1,
        2,
    );
    assert_eq!(field(&written, "count"), &Val::U32(1));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let Val::List(values) = field(&received, "values") else {
        panic!("values")
    };
    let Val::Tuple(tuple) = &values[0] else {
        panic!("tuple")
    };
    assert_eq!(tuple[1], Val::String("retry 🌍".into()));
    close(&mut store, dispose, Val::List(values.clone()));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn partial_resource_stream_writes_keep_consumed_and_unconsumed_owners_distinct() {
    let Owned {
        mut store,
        new,
        read,
        write,
        dispose,
        drop,
        drops,
        ..
    } = owned_fixture(true);
    let (reader, writer) = factory(&mut store, new);
    let values = (77..=78)
        .map(|rep| {
            Val::Tuple(vec![
                Val::Resource(
                    Resource::<Item>::new_own(rep)
                        .try_into_resource_any(&mut store)
                        .unwrap(),
                ),
                Val::String(format!("owner {rep} 🌍")),
            ])
        })
        .collect();
    let (written, received) = exchange(
        &mut store,
        (read, write),
        (reader, writer.clone()),
        values,
        2,
        1,
        1,
    );
    assert_eq!(field(&written, "count"), &Val::U32(1));
    assert_eq!(field(&received, "count"), &Val::U32(1));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let Val::List(consumed) = field(&received, "values") else {
        panic!("values")
    };
    let Val::List(remaining) = field(&written, "remaining") else {
        panic!("remaining")
    };
    assert_eq!(consumed.len(), 1);
    assert_eq!(remaining.len(), 1);
    let (Val::Tuple(first), Val::Tuple(second)) = (&consumed[0], &remaining[0]) else {
        panic!("tuples")
    };
    assert_eq!(first[1], Val::String("owner 77 🌍".into()));
    assert_eq!(second[1], Val::String("owner 78 🌍".into()));
    close(&mut store, dispose, Val::List(consumed.clone()));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    close(&mut store, dispose, Val::List(remaining.clone()));
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    close(&mut store, drop, writer);
    let Val::Option(Some(reader)) = field(&received, "reader") else {
        panic!("reader")
    };
    let eof = invoke(
        &mut store,
        read,
        &[(**reader).clone(), Val::U32(1), Val::U32(3)],
    );
    assert_eq!(field(&eof, "status"), &Val::U32(1));
    assert_eq!(field(&eof, "count"), &Val::U32(0));
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}

#[test]
fn closed_future_reader_returns_the_unwritten_owner_and_releases_its_writer() {
    let Owned {
        mut store,
        new,
        write,
        dispose,
        drops,
        ..
    } = owned_fixture(false);
    let (reader, writer) = factory(&mut store, new);
    let Val::Future(mut reader) = reader else {
        panic!("reader")
    };
    reader.close(&mut store).unwrap();
    let resource = Resource::<Item>::new_own(77)
        .try_into_resource_any(&mut store)
        .unwrap();
    let packet = invoke(
        &mut store,
        write,
        &[
            writer,
            Val::List(vec![Val::Tuple(vec![
                Val::Resource(resource),
                Val::String("unwritten 🌍".into()),
            ])]),
            Val::U32(1),
            Val::U32(1),
        ],
    );
    assert_eq!(field(&packet, "status"), &Val::U32(1));
    assert_eq!(field(&packet, "count"), &Val::U32(0));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let Val::List(remaining) = field(&packet, "remaining") else {
        panic!("remaining")
    };
    assert_eq!(remaining.len(), 1);
    close(&mut store, dispose, Val::List(remaining.clone()));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
