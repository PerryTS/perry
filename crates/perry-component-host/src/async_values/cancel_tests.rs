use super::*;
use futures_channel::oneshot;
use futures_lite::future::{block_on, poll_once};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use wasmtime::{
    component::{Component, FutureAny, FutureReader, Linker, Val},
    Config, Engine, Store,
};

struct Probe(Arc<AtomicUsize>);
impl Drop for Probe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

// Once signalled, these tests have only local Wasm tasks and ready channels.
// Bound scheduler turns so a lost cancellation wakeup fails instead of hanging.
fn finish_ready<F: std::future::Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    for _ in 0..128 {
        if let Some(result) = block_on(poll_once(future.as_mut())) {
            return result;
        }
    }
    panic!("canonical read did not reach quiescence after its signal");
}

#[test]
fn an_owned_cancel_signal_wakes_a_pending_canonical_read_without_retiring_the_store() {
    let wat = include_str!("cancel.component.wat").replace(
        ";; HEAP",
        &HEAP.replacen("(module", "(core module $heap", 1),
    );
    let mut config = Config::new();
    config.wasm_component_model_async(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_binary(&engine, &wat::parse_str(wat).unwrap()).unwrap();
    let gates: Arc<Mutex<BTreeMap<u32, oneshot::Sender<()>>>> = Arc::default();
    let waiters = Arc::new(AtomicUsize::new(0));
    let producers = Arc::new(AtomicUsize::new(0));
    let pending = gates.clone();
    let waiter_drops = waiters.clone();
    let mut linker = Linker::<()>::new(&engine);
    linker
        .instance("control")
        .unwrap()
        .func_new_concurrent("wait", move |_, _, args, _| {
            let Val::U32(id) = args[0] else { panic!("id") };
            let (send, receive) = oneshot::channel();
            pending.lock().unwrap().insert(id, send);
            let guard = Probe(waiter_drops.clone());
            Box::pin(async move {
                let _guard = guard;
                let _ = receive.await;
                Ok(())
            })
        })
        .unwrap();
    let mut store = Store::new(&engine, ());
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let read = instance.get_func(&mut store, "read").unwrap();
    let guard = Probe(producers.clone());
    let reader = FutureReader::new(&mut store, async move {
        let _guard = guard;
        std::future::pending::<wasmtime::Result<u64>>().await
    })
    .unwrap();
    let input = [
        Val::Future(FutureAny::try_from_future_reader(&mut store, reader).unwrap()),
        Val::U32(1),
    ];
    let mut output = [Val::Bool(false)];
    let mut driver = Box::pin(store.run_concurrent(async |accessor| {
        read.call_concurrent(accessor, &input, &mut output).await?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }));
    assert!(block_on(poll_once(driver.as_mut())).is_none());
    gates.lock().unwrap().remove(&1).unwrap().send(()).unwrap();
    finish_ready(driver).unwrap().unwrap();
    assert_eq!(output, [Val::Result(Err(None))]);
    assert_eq!(waiters.load(Ordering::SeqCst), 1);
    assert_eq!(producers.load(Ordering::SeqCst), 1);
    // The same component instance can read another value; success cancels its
    // unused signal waiter instead of leaving an interesting task behind.
    let guard = Probe(producers.clone());
    let reader = FutureReader::new(&mut store, async move {
        let _guard = guard;
        Ok::<_, wasmtime::Error>(21u64)
    })
    .unwrap();
    let input = [
        Val::Future(FutureAny::try_from_future_reader(&mut store, reader).unwrap()),
        Val::U32(2),
    ];
    finish_ready(store.run_concurrent(async |accessor| {
        read.call_concurrent(accessor, &input, &mut output).await?;
        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
        Ok::<_, wasmtime::Error>(())
    }))
    .unwrap()
    .unwrap();
    assert_eq!(output, [Val::Result(Ok(Some(Box::new(Val::U64(21)))))]);
    assert_eq!(waiters.load(Ordering::SeqCst), 2);
    assert_eq!(producers.load(Ordering::SeqCst), 2);
    assert!(
        gates.lock().unwrap().remove(&2).unwrap().send(()).is_err(),
        "the unused native signal receiver must have been cancelled"
    );
}
