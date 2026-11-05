use super::bindings::{
    clocks::monotonic_clock,
    random::{insecure, insecure_seed, random},
};
use crate::clocks::{WasiClocks, WasiClocksCtxView};
use crate::{WasiCtx, WasiCtxView, WasiView};
use futures_lite::future::{block_on, poll_once};
use std::time::{Duration, Instant};
use wasmtime::component::{Accessor, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};

pub(super) struct State {
    pub(super) ctx: Option<WasiCtx>,
    pub(super) table: ResourceTable,
}
impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: self.ctx.as_mut().unwrap(),
            table: &mut self.table,
        }
    }
}
fn clock_accessor(accessor: &Accessor<State>) -> Accessor<State, WasiClocks> {
    accessor.with_getter(|state| WasiClocksCtxView {
        ctx: &mut state.ctx.as_mut().unwrap().clocks,
        table: &mut state.table,
    })
}
fn setup() -> Store<State> {
    let mut config = Config::new();
    config.wasm_component_model_async(true);
    let engine = Engine::new(&config).unwrap();
    let mut linker = Linker::<State>::new(&engine);
    // The versions must coexist without aliasing or duplicate definitions.
    crate::p2::add_to_linker_sync(&mut linker).unwrap();
    super::add_to_linker(&mut linker).unwrap();
    Store::new(
        &engine,
        State {
            ctx: Some(WasiCtx::new(1).unwrap()),
            table: ResourceTable::new(),
        },
    )
}

pub(super) fn with_store(test: impl FnOnce(Store<State>) + Send + 'static) {
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(setup())));
        let _ = send.send(result);
    });
    let result = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("P3 provider watchdog");
    worker.join().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn async_wait_cancellation_releases_capacity_and_preserves_the_store() {
    with_store(|mut store| {
        block_on(store.run_concurrent(async |accessor| {
            let clock = clock_accessor(accessor);
            let mut wait = Box::pin(
                <WasiClocks as monotonic_clock::HostWithStore<State>>::wait_for(
                    &clock,
                    5_000_000_000,
                ),
            );
            assert!(poll_once(wait.as_mut()).await.is_none());
            assert_eq!(
                accessor.with(|mut state| state.get().ctx.as_ref().unwrap().outstanding_io()),
                1
            );
            drop(wait);
            let deadline = Instant::now() + Duration::from_secs(2);
            while accessor.with(|mut state| state.get().ctx.as_ref().unwrap().outstanding_io()) != 0
            {
                assert!(
                    Instant::now() < deadline,
                    "cancelled timer retained driver capacity"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let started = Instant::now();
            <WasiClocks as monotonic_clock::HostWithStore<State>>::wait_for(&clock, 5_000_000)
                .await
                .unwrap();
            assert!(started.elapsed() >= Duration::from_millis(5));
            let mut zero = Box::pin(
                <WasiClocks as monotonic_clock::HostWithStore<State>>::wait_for(&clock, 0),
            );
            assert!(poll_once(zero.as_mut()).await.unwrap().is_ok());
            let mut past = Box::pin(
                <WasiClocks as monotonic_clock::HostWithStore<State>>::wait_until(&clock, 0),
            );
            assert!(poll_once(past.as_mut()).await.unwrap().is_ok());
        }))
        .unwrap();
        store.assert_concurrent_state_empty();
    });
}

#[test]
fn closing_the_context_fails_a_pending_async_wait() {
    with_store(|mut store| {
        block_on(store.run_concurrent(async |accessor| {
            let clock = clock_accessor(accessor);
            let mut wait = Box::pin(
                <WasiClocks as monotonic_clock::HostWithStore<State>>::wait_for(
                    &clock,
                    5_000_000_000,
                ),
            );
            assert!(poll_once(wait.as_mut()).await.is_none());
            drop(accessor.with(|mut state| state.get().ctx.take()));
            assert!(
                wait.await.is_err(),
                "driver loss must not look like timer completion"
            );
        }))
        .unwrap();
        store.assert_concurrent_state_empty();
    });
}

#[test]
fn random_short_reads_are_bounded_and_the_insecure_seed_is_stable() {
    let mut random_ctx = crate::random::WasiRandomCtx {
        max_size: 3,
        insecure_random: Box::new(crate::random::Deterministic::new(vec![7])),
        ..Default::default()
    };
    assert!(random::Host::get_random_bytes(&mut random_ctx, 0)
        .unwrap()
        .is_empty());
    assert_eq!(
        random::Host::get_random_bytes(&mut random_ctx, u64::MAX)
            .unwrap()
            .len(),
        3
    );
    assert!(
        insecure::Host::get_insecure_random_bytes(&mut random_ctx, 0)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        insecure::Host::get_insecure_random_bytes(&mut random_ctx, u64::MAX).unwrap(),
        [7, 7, 7]
    );
    assert_eq!(
        insecure::Host::get_insecure_random_bytes(&mut random_ctx, 2).unwrap(),
        [7, 7]
    );
    random_ctx.insecure_random_seed = (12u128 << 64) | 7;
    assert_eq!(
        insecure_seed::Host::get_insecure_seed(&mut random_ctx).unwrap(),
        (7, 12)
    );
    assert_eq!(
        insecure_seed::Host::get_insecure_seed(&mut random_ctx).unwrap(),
        (7, 12)
    );
    random_ctx.max_size = 0;
    assert!(random::Host::get_random_bytes(&mut random_ctx, 0)
        .unwrap()
        .is_empty());
    assert!(random::Host::get_random_bytes(&mut random_ctx, 1).is_err());
    assert!(insecure::Host::get_insecure_random_bytes(&mut random_ctx, 1).is_err());
    random_ctx.max_size = u64::MAX;
    assert_eq!(
        random::Host::get_random_bytes(&mut random_ctx, u64::MAX)
            .unwrap()
            .len(),
        crate::MAX_READ_SIZE_ALLOC
    );
}
