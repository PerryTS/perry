use super::*;
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once, yield_now};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use wasmtime::{
    component::{Component, GuestTaskId, Linker, Val},
    AsContextMut, Config, Engine, Store,
};

#[derive(Default)]
struct Contexts {
    roots: BTreeMap<GuestTaskId, u64>,
}

#[test]
fn concurrent_imports_capture_their_call_origin_before_yielding_to_the_embedding_stack() {
    let component = include_str!("task_context.component.wat").replace(
        ";; HEAP",
        &HEAP.replacen("(module", "(core module $heap", 1),
    );
    let mut config = Config::new();
    config.wasm_component_model_async(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_binary(&engine, &wat::parse_str(component).unwrap()).unwrap();
    let mut linker = Linker::<Contexts>::new(&engine);
    let pending: Arc<Mutex<BTreeMap<u64, oneshot::Sender<()>>>> = Arc::default();
    let sends = pending.clone();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let observed = observations.clone();
    linker
        .instance("clock")
        .unwrap()
        .func_new_concurrent("wait-for", move |accessor, _, args, _| {
            let sends = sends.clone();
            let observed = observed.clone();
            Box::pin(async move {
                let (root, expected) = accessor.with(|mut access| {
                    let root = access.as_context_mut().async_call_stack()?.last().unwrap();
                    let expected = access.data_mut().roots[&root];
                    Ok::<_, wasmtime::Error>((root, expected))
                })?;
                assert_eq!(args, [Val::U64(expected)]);
                yield_now().await;
                accessor.with(|mut access| {
                    // Ambient guest ancestry can be empty on later host-task
                    // polls. Capture its native identity on the initial poll,
                    // before the dispatcher yields off the Wasm fiber.
                    assert_eq!(access.data_mut().roots[&root], expected);
                });
                let (send, receive) = oneshot::channel();
                sends.lock().unwrap().insert(expected, send);
                receive.await.unwrap();
                observed.lock().unwrap().push(expected);
                Ok(())
            })
        })
        .unwrap();
    let mut store = Store::new(&engine, Contexts::default());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let run = instance.get_func(&mut store, "run").unwrap();
    let completed = Arc::new(Mutex::new(Vec::new()));
    let finished = completed.clone();
    let mut driver = Box::pin(store.run_concurrent(async |accessor| {
        let args1 = [Val::U64(1)];
        let args2 = [Val::U64(2)];
        let mut out1 = [Val::Bool(false)];
        let mut out2 = [Val::Bool(false)];
        let first = accessor.with(|mut access| {
            let call = run.start_call_concurrent(access.as_context_mut(), &args1, &mut out1)?;
            access.data_mut().roots.insert(call.task(), 1);
            Ok::<_, wasmtime::Error>(call)
        })?;
        let second = accessor.with(|mut access| {
            let call = run.start_call_concurrent(access.as_context_mut(), &args2, &mut out2)?;
            access.data_mut().roots.insert(call.task(), 2);
            Ok::<_, wasmtime::Error>(call)
        })?;
        let finish1 = async {
            run.finish_call_concurrent(accessor, first).await?;
            finished.lock().unwrap().push(1);
            Ok::<_, wasmtime::Error>(())
        };
        let finish2 = async {
            run.finish_call_concurrent(accessor, second).await?;
            finished.lock().unwrap().push(2);
            Ok::<_, wasmtime::Error>(())
        };
        futures_util::try_join!(finish1, finish2)?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        assert_eq!(out1, [Val::U64(1)]);
        assert_eq!(out2, [Val::U64(2)]);
        Ok::<_, wasmtime::Error>(())
    }));
    assert!(block_on(poll_once(driver.as_mut())).is_none());
    assert_eq!(
        pending.lock().unwrap().len(),
        2,
        "both calls must suspend in the same component instance"
    );
    pending
        .lock()
        .unwrap()
        .remove(&2)
        .unwrap()
        .send(())
        .unwrap();
    assert!(block_on(poll_once(driver.as_mut())).is_none());
    assert_eq!(*completed.lock().unwrap(), vec![2]);
    pending
        .lock()
        .unwrap()
        .remove(&1)
        .unwrap()
        .send(())
        .unwrap();
    block_on(driver).unwrap().unwrap();
    assert_eq!(*completed.lock().unwrap(), vec![2, 1]);
    assert_eq!(*observations.lock().unwrap(), vec![2, 1]);
}
