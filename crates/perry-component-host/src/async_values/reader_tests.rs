use super::{reader, reader_types::Resources};
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use perry_wit::{Type, World};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use wasmtime::{
    component::{
        Component, Func, FutureAny, FutureReader, Instance, Linker, Resource, ResourceType,
        StreamAny, StreamReader, Val,
    },
    Config, Engine, Store,
};

pub(super) type Signals = Arc<Mutex<BTreeMap<u32, oneshot::Sender<()>>>>;

pub(super) fn world() -> World {
    perry_wit::parse("package test:read; world plugin {}", None).unwrap()
}

pub(super) fn engine() -> Engine {
    let mut config = Config::new();
    config.wasm_component_model_async(true);
    Engine::new(&config).unwrap()
}

pub(super) fn component(
    engine: &Engine,
    world: &World,
    payload: Option<&Type>,
    stream: bool,
    resources: &Resources,
) -> Component {
    let text = reader::generate(world, payload, stream, resources, "control").unwrap();
    let bytes = wat::parse_str(text).unwrap();
    Component::from_binary(engine, &bytes).unwrap()
}

pub(super) fn control(linker: &mut Linker<()>) -> Signals {
    let signals: Signals = Arc::default();
    let pending = signals.clone();
    linker
        .instance("control")
        .unwrap()
        .func_new_concurrent("wait", move |_, _, args, _| {
            let Val::U32(id) = args[0] else { panic!("id") };
            let (send, receive) = oneshot::channel();
            assert!(pending.lock().unwrap().insert(id, send).is_none());
            Box::pin(async move {
                let _ = receive.await;
                Ok(())
            })
        })
        .unwrap();
    signals
}

pub(super) fn ready<F: std::future::Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    for _ in 0..256 {
        if let Some(result) = block_on(poll_once(future.as_mut())) {
            return result;
        }
    }
    panic!("ready helper failed to drain its local tasks")
}

pub(super) fn invoke(store: &mut Store<()>, function: Func, input: &[Val]) -> Val {
    let mut output = [Val::Bool(false)];
    ready(store.run_concurrent(async |accessor| {
        function
            .call_concurrent(accessor, input, &mut output)
            .await?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }))
    .unwrap()
    .unwrap();
    output[0].clone()
}

pub(super) fn field<'a>(packet: &'a Val, name: &str) -> &'a Val {
    let Val::Record(fields) = packet else {
        panic!("packet")
    };
    &fields.iter().find(|(key, _)| key == name).unwrap().1
}

fn functions(store: &mut Store<()>, instance: Instance) -> (Func, Func) {
    (
        instance.get_func(&mut *store, "read").unwrap(),
        instance.get_func(store, "pages").unwrap(),
    )
}

#[test]
fn generic_read_layouts_compile_for_all_supported_payload_shapes() {
    let payloads = vec![
        Type::F32,
        Type::F64,
        Type::Char,
        Type::Flags(vec!["a".into(), "b".into()]),
        Type::Enum(vec!["a".into(), "b".into()]),
        Type::Variant(vec![
            perry_wit::Case {
                name: "empty".into(),
                ty: None,
            },
            perry_wit::Case {
                name: "wide".into(),
                ty: Some(Type::U64),
            },
            perry_wit::Case {
                name: "text".into(),
                ty: Some(Type::String),
            },
        ]),
        Type::Option(Box::new(Type::U64)),
        Type::Result {
            ok: None,
            err: Some(Box::new(Type::String)),
        },
        Type::Future(Some(Box::new(Type::List(Box::new(Type::U8))))),
        Type::Stream(None),
    ];
    let engine = engine();
    for stream in [false, true] {
        component(&engine, &world(), None, stream, &Resources::new());
        for payload in &payloads {
            if stream && matches!(payload, Type::Char) {
                let error = reader::generate(
                    &world(),
                    Some(payload),
                    stream,
                    &Resources::new(),
                    "control",
                )
                .unwrap_err();
                assert!(error.to_string().contains("stream<char> is unsupported"));
                continue;
            }
            component(&engine, &world(), Some(payload), stream, &Resources::new());
        }
    }
}

#[test]
fn compound_future_reads_keep_wide_alignment_and_reuse_nested_allocations() {
    let payload = Type::Tuple(vec![
        Type::U8,
        Type::U64,
        Type::Option(Box::new(Type::String)),
        Type::Result {
            ok: Some(Box::new(Type::List(Box::new(Type::U8)))),
            err: Some(Box::new(Type::String)),
        },
        Type::List(Box::new(Type::Tuple(vec![Type::U8, Type::String]))),
    ]);
    let engine = engine();
    let component = component(&engine, &world(), Some(&payload), false, &Resources::new());
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let (read, pages) = functions(&mut store, instance);
    let mut plateau = None;
    for id in 1..=18 {
        let bytes = (0..3_145_865usize)
            .map(|i| (i.wrapping_mul(31).wrapping_add(7)) as u8)
            .collect::<Vec<_>>();
        let outcome: Result<Vec<u8>, String> = if id % 2 == 0 {
            Err("failure 🌍".into())
        } else {
            Ok(bytes.clone())
        };
        let value = (
            17u8,
            0xFEDC_BA98_7654_3210u64,
            Some("héllo 🌍".to_string()),
            outcome,
            vec![(3u8, "nested".to_string()), (5, "深い".to_string())],
        );
        let future =
            FutureReader::new(&mut store, async move { Ok::<_, wasmtime::Error>(value) }).unwrap();
        let future = FutureAny::try_from_future_reader(&mut store, future).unwrap();
        let packet = invoke(
            &mut store,
            read,
            &[Val::Future(future), Val::U32(1), Val::U32(id)],
        );
        assert_eq!(field(&packet, "status"), &Val::U32(0));
        assert_eq!(field(&packet, "count"), &Val::U32(1));
        assert_eq!(field(&packet, "reader"), &Val::Option(None));
        let Val::List(values) = field(&packet, "values") else {
            panic!("values")
        };
        let Val::Tuple(tuple) = &values[0] else {
            panic!("tuple")
        };
        assert_eq!(tuple[0], Val::U8(17));
        assert_eq!(tuple[1], Val::U64(0xFEDC_BA98_7654_3210));
        assert_eq!(
            tuple[2],
            Val::Option(Some(Box::new(Val::String("héllo 🌍".into()))))
        );
        if id % 2 == 1 {
            let Val::Result(Ok(Some(result))) = &tuple[3] else {
                panic!("result")
            };
            let Val::List(actual) = result.as_ref() else {
                panic!("bytes")
            };
            assert_eq!(actual.len(), bytes.len());
            assert!(actual.iter().zip(&bytes).all(|(a, b)| *a == Val::U8(*b)));
        } else {
            assert_eq!(
                tuple[3],
                Val::Result(Err(Some(Box::new(Val::String("failure 🌍".into())))))
            );
        }
        assert_eq!(
            tuple[4],
            Val::List(vec![
                Val::Tuple(vec![Val::U8(3), Val::String("nested".into())]),
                Val::Tuple(vec![Val::U8(5), Val::String("深い".into())])
            ])
        );
        assert!(signals
            .lock()
            .unwrap()
            .remove(&id)
            .unwrap()
            .send(())
            .is_err());
        let current = invoke(&mut store, pages, &[]);
        if id == 2 {
            plateau = Some(current.clone());
        }
        if id > 2 {
            assert_eq!(
                Some(current),
                plateau,
                "canonical allocations must be recycled"
            );
        }
    }
}

#[test]
fn stream_reads_preserve_partial_final_chunks_and_canonical_continuations() {
    let engine = engine();
    let component = component(&engine, &world(), Some(&Type::U64), true, &Resources::new());
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let (read, _) = functions(&mut store, instance);
    let stream = StreamReader::new(&mut store, vec![9u64, 10, 11, 12, 13]).unwrap();
    let stream = StreamAny::try_from_stream_reader(&mut store, stream).unwrap();
    let mut input = Val::Stream(stream);
    let mut all = Vec::new();
    for id in 1..=5 {
        let packet = invoke(&mut store, read, &[input, Val::U32(2), Val::U32(id)]);
        let Val::List(chunk) = field(&packet, "values") else {
            panic!("chunk")
        };
        assert_eq!(field(&packet, "count"), &Val::U32(chunk.len() as u32));
        all.extend(chunk.iter().cloned());
        assert!(signals
            .lock()
            .unwrap()
            .remove(&id)
            .unwrap()
            .send(())
            .is_err());
        match field(&packet, "reader") {
            Val::Option(Some(reader)) => {
                assert_eq!(field(&packet, "status"), &Val::U32(0));
                input = (**reader).clone();
            }
            Val::Option(None) => {
                assert_eq!(field(&packet, "status"), &Val::U32(1));
                break;
            }
            _ => panic!("continuation"),
        }
    }
    assert_eq!(all, (9..=13).map(Val::U64).collect::<Vec<_>>());
}

#[test]
fn generic_future_payload_transfers_the_original_host_resource_exactly_once() {
    struct Item;
    let world = perry_wit::parse("package test:owned; interface types { resource item; get: func() -> future<tuple<own<item>, string>>; } world plugin { export types; }", None).unwrap();
    let mut result = world.exports[0].functions[0].result.as_ref().unwrap();
    while let Type::Ref(key) = result {
        result = &world.types[key].ty;
    }
    let Type::Future(Some(payload)) = result else {
        panic!("future")
    };
    let resources = world.exports[0]
        .resources
        .values()
        .map(|key| (key.clone(), ("types".into(), "item".into())))
        .collect();
    let engine = engine();
    let component = component(&engine, &world, Some(payload), false, &resources);
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let drops = Arc::new(AtomicUsize::new(0));
    let dropped = drops.clone();
    linker
        .instance("types")
        .unwrap()
        .resource("item", ResourceType::host::<Item>(), move |_, rep| {
            assert_eq!(rep, 77);
            dropped.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let (read, _) = functions(&mut store, instance);
    let future = FutureReader::new(&mut store, async {
        Ok::<_, wasmtime::Error>((Resource::<Item>::new_own(77), "owned 🌍".to_string()))
    })
    .unwrap();
    let future = FutureAny::try_from_future_reader(&mut store, future).unwrap();
    let packet = invoke(
        &mut store,
        read,
        &[Val::Future(future), Val::U32(1), Val::U32(1)],
    );
    let Val::List(values) = field(&packet, "values") else {
        panic!("values")
    };
    let Val::Tuple(tuple) = &values[0] else {
        panic!("tuple")
    };
    let Val::Resource(resource) = &tuple[0] else {
        panic!("resource")
    };
    assert_eq!(resource.ty(), ResourceType::host::<Item>());
    assert_eq!(tuple[1], Val::String("owned 🌍".into()));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    block_on(resource.resource_drop_async(&mut store)).unwrap();
    assert!(block_on(resource.resource_drop_async(&mut store)).is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(signals
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(())
        .is_err());
}

#[test]
fn generated_helpers_cancel_one_of_two_reads_and_keep_the_other_frame_alive() {
    struct Probe(Arc<AtomicUsize>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let engine = engine();
    let component = component(
        &engine,
        &world(),
        Some(&Type::U64),
        false,
        &Resources::new(),
    );
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let (read, _) = functions(&mut store, instance);
    let drops = Arc::new(AtomicUsize::new(0));
    let mut inputs = Vec::new();
    let mut sends = Vec::new();
    for id in 1..=2 {
        let (send, receive) = oneshot::channel::<u64>();
        sends.push(send);
        let guard = Probe(drops.clone());
        let future = FutureReader::new(&mut store, async move {
            let _guard = guard;
            Ok::<_, wasmtime::Error>(receive.await.unwrap())
        })
        .unwrap();
        let future = FutureAny::try_from_future_reader(&mut store, future).unwrap();
        inputs.push([Val::Future(future), Val::U32(1), Val::U32(id)]);
    }
    let mut first = [Val::Bool(false)];
    let mut second = [Val::Bool(false)];
    let mut driver = Box::pin(store.run_concurrent(async |accessor| {
        futures_util::try_join!(
            read.call_concurrent(accessor, &inputs[0], &mut first),
            read.call_concurrent(accessor, &inputs[1], &mut second)
        )?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }));
    assert!(block_on(poll_once(driver.as_mut())).is_none());
    assert_eq!(signals.lock().unwrap().len(), 2);
    signals
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(())
        .unwrap();
    assert!(block_on(poll_once(driver.as_mut())).is_none());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let second_send = sends.pop().unwrap();
    second_send.send(0xFEDC_BA98_7654_3210).unwrap();
    ready(driver).unwrap().unwrap();
    assert_eq!(field(&first[0], "status"), &Val::U32(2));
    assert_eq!(field(&first[0], "count"), &Val::U32(0));
    assert_eq!(field(&first[0], "values"), &Val::List(vec![]));
    assert_eq!(field(&second[0], "status"), &Val::U32(0));
    assert_eq!(
        field(&second[0], "values"),
        &Val::List(vec![Val::U64(0xFEDC_BA98_7654_3210)])
    );
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert!(sends.pop().unwrap().send(1).is_err());
    assert!(signals
        .lock()
        .unwrap()
        .remove(&2)
        .unwrap()
        .send(())
        .is_err());
    let future = FutureReader::new(&mut store, async { Ok::<_, wasmtime::Error>(42u64) }).unwrap();
    let future = FutureAny::try_from_future_reader(&mut store, future).unwrap();
    let packet = invoke(
        &mut store,
        read,
        &[Val::Future(future), Val::U32(1), Val::U32(3)],
    );
    assert_eq!(field(&packet, "values"), &Val::List(vec![Val::U64(42)]));
    assert!(signals
        .lock()
        .unwrap()
        .remove(&3)
        .unwrap()
        .send(())
        .is_err());
}

#[test]
fn absent_stream_payloads_keep_their_element_counts() {
    let engine = engine();
    let component = component(&engine, &world(), None, true, &Resources::new());
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let (read, _) = functions(&mut store, instance);
    let stream = StreamReader::new(&mut store, vec![(); 7]).unwrap();
    let stream = StreamAny::try_from_stream_reader(&mut store, stream).unwrap();
    let packet = invoke(
        &mut store,
        read,
        &[Val::Stream(stream), Val::U32(16), Val::U32(1)],
    );
    assert_eq!(field(&packet, "count"), &Val::U32(7));
    assert_eq!(field(&packet, "status"), &Val::U32(1));
    assert_eq!(field(&packet, "reader"), &Val::Option(None));
    assert_eq!(field(&packet, "values"), &Val::List(vec![]));
    assert!(signals
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(())
        .is_err());
}

#[test]
fn empty_aggregate_types_fail_before_component_encoding() {
    for payload in [Type::Record(vec![]), Type::Tuple(vec![])] {
        let error = reader::generate(
            &world(),
            Some(&payload),
            false,
            &Resources::new(),
            "control",
        )
        .unwrap_err();
        assert!(error.to_string().contains("must have at least one field"));
    }
}

#[test]
fn distinct_resource_definitions_cannot_be_coalesced_in_an_async_helper() {
    let world = perry_wit::parse("package test:distinct; interface types { resource first; resource second; } world plugin { export types; }", None).unwrap();
    let keys = world.exports[0]
        .resources
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let resources = keys
        .iter()
        .map(|key| (key.clone(), ("types".into(), "same".into())))
        .collect();
    let payload = Type::Tuple(keys.iter().cloned().map(Type::Own).collect());
    let error = reader::generate(&world, Some(&payload), false, &resources, "control").unwrap_err();
    assert!(error.to_string().contains("distinct async resources"));
}

#[test]
fn cancellation_preserves_partial_owned_payloads_for_native_disposal() {
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };
    use wasmtime::{
        component::{Destination, StreamProducer, StreamResult},
        StoreContextMut,
    };
    struct Item;
    struct Producer(Option<Resource<Item>>);
    impl StreamProducer<()> for Producer {
        type Item = Resource<Item>;
        type Buffer = Option<Self::Item>;
        fn poll_produce<'a>(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: StoreContextMut<'a, ()>,
            mut destination: Destination<'a, Self::Item, Self::Buffer>,
            finish: bool,
        ) -> Poll<wasmtime::Result<StreamResult>> {
            if finish {
                destination.set_buffer(self.0.take());
                Poll::Ready(Ok(StreamResult::Completed))
            } else {
                Poll::Pending
            }
        }
    }
    let world = perry_wit::parse(
        "package test:partial; interface types { resource item; } world plugin { export types; }",
        None,
    )
    .unwrap();
    let key = world.exports[0].resources["item"].clone();
    let resources = [(key.clone(), ("types".into(), "item".into()))]
        .into_iter()
        .collect();
    let engine = engine();
    let component = component(&engine, &world, Some(&Type::Own(key)), true, &resources);
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let drops = Arc::new(AtomicUsize::new(0));
    let dropped = drops.clone();
    linker
        .instance("types")
        .unwrap()
        .resource("item", ResourceType::host::<Item>(), move |_, rep| {
            assert_eq!(rep, 77);
            dropped.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let (read, _) = functions(&mut store, instance);
    let stream = StreamReader::new(&mut store, Producer(Some(Resource::new_own(77)))).unwrap();
    let stream = StreamAny::try_from_stream_reader(&mut store, stream).unwrap();
    let input = [Val::Stream(stream), Val::U32(4), Val::U32(1)];
    let mut output = [Val::Bool(false)];
    let mut driver = Box::pin(store.run_concurrent(async |accessor| {
        read.call_concurrent(accessor, &input, &mut output).await?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }));
    assert!(block_on(poll_once(driver.as_mut())).is_none());
    signals
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(())
        .unwrap();
    ready(driver).unwrap().unwrap();
    assert_eq!(field(&output[0], "status"), &Val::U32(2));
    assert_eq!(field(&output[0], "count"), &Val::U32(1));
    assert_eq!(field(&output[0], "reader"), &Val::Option(None));
    let Val::List(values) = field(&output[0], "values") else {
        panic!("values")
    };
    let Val::Resource(resource) = values[0] else {
        panic!("resource")
    };
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "memory cleanup must preserve transferred handles"
    );
    let dispose = instance.get_func(&mut store, "dispose").unwrap();
    ready(store.run_concurrent(async |accessor| {
        dispose
            .call_concurrent(accessor, &[Val::List(values.clone())], &mut [])
            .await?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }))
    .unwrap()
    .unwrap();
    assert!(block_on(resource.resource_drop_async(&mut store)).is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn nested_future_payloads_survive_outer_canonical_memory_cleanup() {
    let engine = engine();
    let outer_component = component(
        &engine,
        &world(),
        Some(&Type::Future(Some(Box::new(Type::U64)))),
        false,
        &Resources::new(),
    );
    let inner_component = component(
        &engine,
        &world(),
        Some(&Type::U64),
        false,
        &Resources::new(),
    );
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let mut store = Store::new(&engine, ());
    let outer_instance = block_on(linker.instantiate_async(&mut store, &outer_component)).unwrap();
    let inner_instance = block_on(linker.instantiate_async(&mut store, &inner_component)).unwrap();
    let (read_outer, _) = functions(&mut store, outer_instance);
    let (read_inner, _) = functions(&mut store, inner_instance);
    let inner = FutureReader::new(&mut store, async {
        Ok::<_, wasmtime::Error>(0xFEDC_BA98_7654_3210u64)
    })
    .unwrap();
    let outer =
        FutureReader::new(&mut store, async move { Ok::<_, wasmtime::Error>(inner) }).unwrap();
    let outer = FutureAny::try_from_future_reader(&mut store, outer).unwrap();
    let packet = invoke(
        &mut store,
        read_outer,
        &[Val::Future(outer), Val::U32(1), Val::U32(1)],
    );
    let Val::List(values) = field(&packet, "values") else {
        panic!("values")
    };
    assert!(matches!(values[0], Val::Future(_)));
    let packet = invoke(
        &mut store,
        read_inner,
        &[values[0].clone(), Val::U32(1), Val::U32(2)],
    );
    assert_eq!(
        field(&packet, "values"),
        &Val::List(vec![Val::U64(0xFEDC_BA98_7654_3210)])
    );
    for id in [1, 2] {
        assert!(signals
            .lock()
            .unwrap()
            .remove(&id)
            .unwrap()
            .send(())
            .is_err());
    }
}

#[test]
fn canonical_disposal_consumes_nested_owned_handles_and_recycles_payload_storage() {
    struct Item;
    struct Probe(Arc<AtomicUsize>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let world = perry_wit::parse(
        "package test:dispose; interface types { resource item; } world plugin { export types; }",
        None,
    )
    .unwrap();
    let key = world.exports[0].resources["item"].clone();
    let resources = [(key.clone(), ("types".into(), "item".into()))]
        .into_iter()
        .collect();
    let payload = Type::Tuple(vec![
        Type::U64,
        Type::String,
        Type::List(Box::new(Type::Tuple(vec![
            Type::Option(Box::new(Type::Own(key))),
            Type::String,
        ]))),
        Type::Future(Some(Box::new(Type::U64))),
    ]);
    let engine = engine();
    let component = component(&engine, &world, Some(&payload), false, &resources);
    let mut linker = Linker::new(&engine);
    let signals = control(&mut linker);
    let drops = Arc::new(AtomicUsize::new(0));
    let dropped = drops.clone();
    linker
        .instance("types")
        .unwrap()
        .resource("item", ResourceType::host::<Item>(), move |_, rep| {
            assert!([77, 78].contains(&rep));
            dropped.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    let future_drops = Arc::new(AtomicUsize::new(0));
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let (read, pages) = functions(&mut store, instance);
    let dispose = instance.get_func(&mut store, "dispose").unwrap();
    let mut previous = None;
    for id in 1..=3 {
        let guard = Probe(future_drops.clone());
        let inner = FutureReader::new(&mut store, async move {
            let _guard = guard;
            std::future::pending::<wasmtime::Result<u64>>().await
        })
        .unwrap();
        let value = (
            0xFEDCBA9876543210u64,
            "héllo 🌍".repeat(1024),
            vec![
                (Some(Resource::<Item>::new_own(77)), "nested".to_string()),
                (None, "empty".to_string()),
                (Some(Resource::<Item>::new_own(78)), "深い".to_string()),
            ],
            inner,
        );
        let outer =
            FutureReader::new(&mut store, async move { Ok::<_, wasmtime::Error>(value) }).unwrap();
        let outer = FutureAny::try_from_future_reader(&mut store, outer).unwrap();
        let packet = invoke(
            &mut store,
            read,
            &[Val::Future(outer), Val::U32(1), Val::U32(id)],
        );
        let values = field(&packet, "values").clone();
        assert_eq!(drops.load(Ordering::SeqCst), 2 * (id as usize - 1));
        ready(store.run_concurrent(async |accessor| {
            dispose
                .call_concurrent(accessor, &[values], &mut [])
                .await?;
            std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
            Ok::<_, wasmtime::Error>(())
        }))
        .unwrap()
        .unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 2 * id as usize);
        assert_eq!(future_drops.load(Ordering::SeqCst), id as usize);
        assert!(signals
            .lock()
            .unwrap()
            .remove(&id)
            .unwrap()
            .send(())
            .is_err());
        let current = invoke(&mut store, pages, &[]);
        if let Some(previous) = &previous {
            assert_eq!(&current, previous);
        }
        previous = Some(current);
    }
}
