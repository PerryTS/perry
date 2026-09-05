//! Provider declaration identity and shared-storage regressions (#9791).

/// Model two separately compiled provider copies of one declaration.
/// Merely allocating noncolliding indices is insufficient: both handles
/// must use the same storage and only one initializer/destructor may run.
#[test]
fn provider_copies_share_storage_without_initializing_a_second_value() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static INITIALIZED: AtomicUsize = AtomicUsize::new(0);
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    struct Probe(std::cell::Cell<u64>);
    impl Probe {
        fn new() -> Self {
            INITIALIZED.fetch_add(1, Ordering::SeqCst);
            Self(std::cell::Cell::new(0))
        }
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }
    type Storage = super::HotCell<Probe, 1>;
    thread_local! {
        static FIRST_STORAGE: Storage = Storage::new(Probe::new());
        static SECOND_STORAGE: Storage = Storage::new(Probe::new());
    }
    static FIRST_SLOT: super::SlotId = super::SlotId::named("provider-test::shared");
    static SECOND_SLOT: super::SlotId = super::SlotId::named("provider-test::shared");
    static FIRST: super::HotKey<Probe> = super::HotKey::new(
        &FIRST_SLOT,
        || FIRST_STORAGE.try_with(|c| c.value_addr()),
        |idx| {
            let _ = FIRST_STORAGE.try_with(|c| c.arm_guard(idx));
        },
    );
    static SECOND: super::HotKey<Probe> = super::HotKey::new(
        &SECOND_SLOT,
        || SECOND_STORAGE.try_with(|c| c.value_addr()),
        |idx| {
            let _ = SECOND_STORAGE.try_with(|c| c.arm_guard(idx));
        },
    );
    // Reverse which provider is touched first, and overlap the threads to
    // exercise independent claim atomics and isolate each thread's value.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|i| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let (first, second) = if i % 2 == 0 {
                    (&FIRST, &SECOND)
                } else {
                    (&SECOND, &FIRST)
                };
                first.with(|p| p.0.set(i + 100));
                barrier.wait();
                assert_eq!(second.with(|p| p.0.get()), i + 100);
                assert_eq!(
                    first.with(|p| p as *const Probe),
                    second.with(|p| p as *const Probe)
                );
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("provider probe thread panicked");
    }
    assert_eq!(FIRST.slot_index(), SECOND.slot_index());
    assert!((FIRST.slot_index() as usize) < super::HOT_SLOT_CAPACITY);
    assert_eq!(INITIALIZED.load(Ordering::SeqCst), 8);
    assert_eq!(DROPPED.load(Ordering::SeqCst), 8);
}

/// Function-local declarations can have the same module and identifier;
/// the macro's source coordinates must keep their storage independent.
#[test]
fn same_named_local_declarations_remain_distinct() {
    fn first() -> u32 {
        crate::perry_thread_local! { static LOCAL: std::cell::Cell<u64> = const { std::cell::Cell::new(11) }; }
        assert_eq!(LOCAL.with(|p| p.get()), 11);
        LOCAL.slot_index()
    }
    fn second() -> u32 {
        crate::perry_thread_local! { static LOCAL: std::cell::Cell<u64> = const { std::cell::Cell::new(22) }; }
        assert_eq!(LOCAL.with(|p| p.get()), 22);
        LOCAL.slot_index()
    }
    assert_ne!(first(), second());
}
