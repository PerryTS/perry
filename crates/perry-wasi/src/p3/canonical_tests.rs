use super::tests::{with_store, State};
use crate::WasiCtx;
use futures_lite::future::{block_on, poll_fn, race, zip};
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
use std::time::{Duration, Instant};
use wasmtime::component::{Component, Linker};

#[test]
fn canonical_async_clock_calls_suspend_resume_and_reuse() {
    with_store(|mut store| {
        store.data_mut().ctx = Some(WasiCtx::new(4).unwrap());
        let component =
            Component::new(store.engine(), include_str!("clock.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        super::add_to_linker(&mut linker).unwrap();
        block_on(async {
            let slow = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let fast = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let slow = slow
                .get_typed_func::<(u64,), (u64,)>(&mut store, "run")
                .unwrap();
            let fast = fast
                .get_typed_func::<(u64,), (u64,)>(&mut store, "run")
                .unwrap();
            let finished = AtomicBool::new(false);
            store
                .run_concurrent(async |accessor| {
                    let (slow_result, fast_result) = zip(
                        async {
                            let elapsed = slow
                                .call_concurrent(accessor, (50_000_000,))
                                .await
                                .unwrap()
                                .0;
                            assert!(
                                finished.load(Ordering::SeqCst),
                                "short guest did not run during the long wait"
                            );
                            elapsed
                        },
                        async {
                            let elapsed = fast
                                .call_concurrent(accessor, (1_000_000,))
                                .await
                                .unwrap()
                                .0;
                            finished.store(true, Ordering::SeqCst);
                            elapsed
                        },
                    )
                    .await;
                    assert!(slow_result >= 50_000_000);
                    assert!(fast_result >= 1_000_000);
                    // Exercise both immediate completion and reuse after callback completion.
                    slow.call_concurrent(accessor, (0,)).await.unwrap();
                    fast.call_concurrent(accessor, (1_000_000,)).await.unwrap();
                })
                .await
                .unwrap();
        });
        store.assert_concurrent_state_empty();
        assert_eq!(store.data().ctx.as_ref().unwrap().outstanding_io(), 0);
    });
}

#[test]
fn retiring_the_store_cancels_a_suspended_canonical_call() {
    with_store(|mut store| {
        let component =
            Component::new(store.engine(), include_str!("clock.component.wat")).unwrap();
        let mut linker = Linker::<State>::new(store.engine());
        super::add_to_linker(&mut linker).unwrap();
        let submitter = store.data().ctx.as_ref().unwrap().driver.submitter();
        block_on(async {
            let instance = linker
                .instantiate_async(&mut store, &component)
                .await
                .unwrap();
            let run = instance
                .get_typed_func::<(u64,), (u64,)>(&mut store, "run")
                .unwrap();
            store
                .run_concurrent(async |accessor| {
                    race(
                        async {
                            run.call_concurrent(accessor, (60_000_000_000,))
                                .await
                                .unwrap();
                            panic!("long clock wait unexpectedly completed");
                        },
                        poll_fn(|cx| {
                            if accessor.with(|mut state| {
                                state.get().ctx.as_ref().unwrap().outstanding_io()
                            }) == 1
                            {
                                Poll::Ready(())
                            } else {
                                cx.waker().wake_by_ref();
                                Poll::Pending
                            }
                        }),
                    )
                    .await;
                })
                .await
                .unwrap();
        });
        // Wasmtime retains the guest task after the host call future is dropped.
        // A hard cancellation must retire the Store, not merely that future.
        assert_eq!(store.data().ctx.as_ref().unwrap().outstanding_io(), 1);
        let started = Instant::now();
        drop(store);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            submitter.submit(|_| async {}),
            Err(perry_component_io::Error::Stopped)
        ));
    });
}
