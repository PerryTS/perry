use crate::p2::bindings::{
    clocks::{monotonic_clock, wall_clock},
    random::{insecure, insecure_seed, random},
};
use crate::{clocks::WasiClocksCtxView, io, WasiCtx, WasiCtxView, WasiView};
use futures_lite::future::block_on;
use io::bindings::wasi::io::poll::{Host as AsyncPoll, HostPollable as AsyncPollable};
use io::sync::wasi::io::poll::{Host as SyncPoll, HostPollable as SyncPollable};
use std::sync::mpsc;
use std::time::Duration;
use wasmtime::component::{Resource, ResourceTable};

fn bounded(test: impl FnOnce() + Send + 'static) {
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
        let _ = send.send(result);
    });
    let result = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("provider watchdog");
    worker.join().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

fn subscribe(
    ctx: &mut WasiCtx,
    table: &mut ResourceTable,
    nanos: u64,
) -> Resource<io::poll::DynPollable> {
    monotonic_clock::Host::subscribe_duration(
        &mut WasiClocksCtxView {
            ctx: &mut ctx.clocks,
            table,
        },
        nanos,
    )
    .unwrap()
}

#[test]
fn timer_is_pending_then_repeatably_ready() {
    bounded(|| {
        let mut ctx = WasiCtx::new(4).unwrap();
        let mut table = ResourceTable::new();
        let distant = subscribe(&mut ctx, &mut table, 5_000_000_000);
        assert!(!SyncPollable::ready(&mut table, Resource::new_borrow(distant.rep())).unwrap());
        SyncPollable::drop(&mut table, distant).unwrap();
        let timer = subscribe(&mut ctx, &mut table, 5_000_000);
        SyncPollable::block(&mut table, Resource::new_borrow(timer.rep())).unwrap();
        assert!(SyncPollable::ready(&mut table, Resource::new_borrow(timer.rep())).unwrap());
        assert_eq!(
            SyncPoll::poll(
                &mut table,
                vec![
                    Resource::new_borrow(timer.rep()),
                    Resource::new_borrow(timer.rep())
                ]
            )
            .unwrap(),
            vec![0, 1]
        );
        SyncPollable::drop(&mut table, timer).unwrap();
        assert_eq!(ctx.outstanding_io(), 0);
    });
}

#[test]
fn native_timer_failure_traps_and_remains_a_failure() {
    bounded(|| {
        let mut ctx = WasiCtx::new(1).unwrap();
        let mut table = ResourceTable::new();
        let timer = subscribe(&mut ctx, &mut table, 5_000_000_000);
        drop(ctx);
        let error = SyncPollable::block(&mut table, Resource::new_borrow(timer.rep())).unwrap_err();
        assert!(error.to_string().contains("clock driver failed"));
        assert!(SyncPollable::ready(&mut table, Resource::new_borrow(timer.rep())).is_err());
        assert!(SyncPoll::poll(&mut table, vec![Resource::new_borrow(timer.rep())]).is_err());
        SyncPollable::drop(&mut table, timer).unwrap();
    });
}

#[test]
fn pollable_drop_cancels_timer_and_releases_capacity() {
    bounded(|| {
        let mut ctx = WasiCtx::new(1).unwrap();
        let mut table = ResourceTable::new();
        let timer = subscribe(&mut ctx, &mut table, 5_000_000_000);
        assert_eq!(ctx.outstanding_io(), 1);
        assert!(monotonic_clock::Host::subscribe_duration(
            &mut WasiClocksCtxView {
                ctx: &mut ctx.clocks,
                table: &mut table
            },
            1_000_000
        )
        .is_err());
        let rep = timer.rep();
        SyncPollable::drop(&mut table, timer).unwrap();
        assert!(table
            .get(&Resource::<io::poll::DynPollable>::new_borrow(rep))
            .is_err());
        // Wait for cancellation acknowledgement using a bounded deadline.
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while ctx.outstanding_io() != 0 {
            assert!(std::time::Instant::now() < until);
            std::thread::yield_now();
        }
        let next = subscribe(&mut ctx, &mut table, 1_000_000);
        SyncPollable::block(&mut table, Resource::new_borrow(next.rep())).unwrap();
        SyncPollable::drop(&mut table, next).unwrap();
    });
}

struct Ready;
#[async_trait::async_trait]
impl io::poll::Pollable for Ready {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        Ok(())
    }
}
struct Failed;
#[async_trait::async_trait]
impl io::poll::Pollable for Failed {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        wasmtime::bail!("native failure")
    }
}

#[test]
fn poll_propagates_errors_even_beside_ready_resources() {
    let mut table = ResourceTable::new();
    let ready = table.push(Ready).unwrap();
    let ready = io::poll::subscribe(&mut table, ready).unwrap();
    let failed = table.push(Failed).unwrap();
    let failed = io::poll::subscribe(&mut table, failed).unwrap();
    let error = block_on(AsyncPoll::poll(&mut table, vec![ready, failed])).unwrap_err();
    assert!(error.to_string().contains("native failure"));
    assert!(block_on(AsyncPoll::poll(&mut table, vec![])).is_err());
}

#[test]
fn owned_subscriptions_drop_the_parent_borrowed_ones_preserve_it() {
    for owned in [true, false] {
        let mut table = ResourceTable::new();
        let resource = table.push(Ready).unwrap();
        let rep = resource.rep();
        let subscription = io::poll::subscribe(
            &mut table,
            if owned {
                resource
            } else {
                Resource::new_borrow(rep)
            },
        )
        .unwrap();
        assert!(table.delete(Resource::<Ready>::new_own(rep)).is_err());
        AsyncPollable::drop(&mut table, subscription).unwrap();
        assert_eq!(
            table.get(&Resource::<Ready>::new_borrow(rep)).is_ok(),
            !owned
        );
        if !owned {
            table.delete(Resource::<Ready>::new_own(rep)).unwrap();
        }
    }
}

#[test]
fn entropy_limits_and_stable_seed_are_preserved() {
    let mut ctx = crate::random::WasiRandomCtx::default();
    assert_eq!(
        random::Host::get_random_bytes(&mut ctx, 0).unwrap(),
        Vec::<u8>::new()
    );
    assert_eq!(
        random::Host::get_random_bytes(&mut ctx, 32).unwrap().len(),
        32
    );
    assert_eq!(
        insecure::Host::get_insecure_random_bytes(&mut ctx, 32)
            .unwrap()
            .len(),
        32
    );
    let first = insecure_seed::Host::insecure_seed(&mut ctx).unwrap();
    assert_eq!(first, insecure_seed::Host::insecure_seed(&mut ctx).unwrap());
    assert!(random::Host::get_random_bytes(&mut ctx, u64::MAX).is_err());
    assert!(insecure::Host::get_insecure_random_bytes(&mut ctx, u64::MAX).is_err());
}

struct State {
    ctx: WasiCtx,
    table: ResourceTable,
}
impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

#[test]
fn component_uses_versioned_clock_provider() {
    bounded(|| {
        let engine = wasmtime::Engine::default();
        let component = wasmtime::component::Component::new(
            &engine,
            r#"(component
          (import "wasi:clocks/monotonic-clock@0.2.0" (instance $clock
            (export "now" (func (result u64)))))
          (alias export $clock "now" (func $now))
          (core func $lower (canon lower (func $now)))
          (core module $code
            (import "env" "now" (func $now (result i64)))
            (func (export "now") (result i64) (call $now)))
          (core instance $env (export "now" (func $lower)))
          (core instance $code (instantiate $code (with "env" (instance $env))))
          (alias core export $code "now" (core func $forward))
          (func (export "now") (result u64) (canon lift (core func $forward))))"#,
        )
        .unwrap();
        let mut linker = wasmtime::component::Linker::new(&engine);
        crate::p2::add_to_linker_sync(&mut linker).unwrap();
        let mut store = wasmtime::Store::new(
            &engine,
            State {
                ctx: WasiCtx::new(4).unwrap(),
                table: ResourceTable::new(),
            },
        );
        let instance = linker.instantiate(&mut store, &component).unwrap();
        let now = instance
            .get_typed_func::<(), (u64,)>(&mut store, "now")
            .unwrap();
        let first = now.call(&mut store, ()).unwrap().0;
        let second = now.call(&mut store, ()).unwrap().0;
        assert!(second >= first);
        let state = store.data_mut();
        let mut view = WasiClocksCtxView {
            ctx: &mut state.ctx.clocks,
            table: &mut state.table,
        };
        let wall = wall_clock::Host::now(&mut view).unwrap();
        assert!(wall.seconds > 0 && wall.nanoseconds < 1_000_000_000);
        assert!(monotonic_clock::Host::resolution(&mut view).unwrap() > 0);
    });
}

#[test]
fn component_subscribes_blocks_and_drops_a_native_timer() {
    bounded(|| {
        let engine = wasmtime::Engine::default();
        let component =
            wasmtime::component::Component::new(&engine, include_str!("timer.component.wat"))
                .unwrap();
        let mut linker = wasmtime::component::Linker::new(&engine);
        crate::p2::add_to_linker_sync(&mut linker).unwrap();
        let mut store = wasmtime::Store::new(
            &engine,
            State {
                ctx: WasiCtx::new(4).unwrap(),
                table: ResourceTable::new(),
            },
        );
        let instance = linker.instantiate(&mut store, &component).unwrap();
        let run = instance
            .get_typed_func::<(u64,), ()>(&mut store, "wait")
            .unwrap();
        run.call(&mut store, (1_000_000,)).unwrap();
        run.call(&mut store, (0,)).unwrap();
        assert_eq!(store.data().ctx.outstanding_io(), 0);
    });
}
