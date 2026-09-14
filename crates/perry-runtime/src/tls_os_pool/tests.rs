use super::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
static ORDER: Mutex<Vec<u8>> = Mutex::new(Vec::new());
struct Probe(u8);
static FIRST: LocalKey<Probe> = LocalKey::new(|| Probe(1));
static SECOND: LocalKey<Probe> = LocalKey::new(|| Probe(2));
static THIRD: LocalKey<Probe> = LocalKey::new(|| Probe(3));
static PLAIN: LocalKey<Cell<u8>> = LocalKey::new(|| Cell::new(99));
impl Drop for Probe {
    fn drop(&mut self) {
        PLAIN.with(|v| assert_eq!(v.get(), 99));
        match self.0 {
            2 => {
                assert!(SECOND.try_with(|_| ()).is_err());
                FIRST.with(|v| assert_eq!(v.0, 1));
            }
            1 => {
                assert!(SECOND.try_with(|_| ()).is_err());
                THIRD.with(|v| assert_eq!(v.0, 3));
            }
            3 => assert!(FIRST.try_with(|_| ()).is_err()),
            _ => unreachable!(),
        }
        ORDER.lock().unwrap().push(self.0);
    }
}
#[test]
fn cleanup_supports_other_values_new_initializers_and_destroyed_errors() {
    std::thread::spawn(|| {
        PLAIN.with(|_| ());
        FIRST.with(|_| ());
        SECOND.with(|_| ());
    })
    .join()
    .unwrap();
    assert_eq!(*ORDER.lock().unwrap(), [2, 1, 3]);
}
#[test]
fn initializer_unwind_leaves_the_key_retryable() {
    static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
    static RETRY: LocalKey<u8> = LocalKey::new(|| {
        assert_ne!(
            ATTEMPTS.fetch_add(1, Ordering::SeqCst),
            0,
            "first attempt fails"
        );
        17
    });
    std::thread::spawn(|| {
        assert!(std::panic::catch_unwind(|| RETRY.with(|_| ())).is_err());
        RETRY.with(|value| assert_eq!(*value, 17));
    })
    .join()
    .unwrap();
    assert_eq!(ATTEMPTS.load(Ordering::SeqCst), 2);
}
#[test]
fn recursive_initializer_is_rejected_without_poisoning_other_keys() {
    static RECURSIVE: LocalKey<u8> = LocalKey::new(|| RECURSIVE.with(|_| 1));
    std::thread::spawn(|| {
        assert!(std::panic::catch_unwind(|| RECURSIVE.with(|_| ())).is_err());
        PLAIN.with(|v| assert_eq!(v.get(), 99));
    })
    .join()
    .unwrap();
}
#[test]
fn aligned_values_are_released_at_worker_exit() {
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    #[repr(align(4096))]
    struct Aligned(u8);
    impl Drop for Aligned {
        fn drop(&mut self) {
            assert_eq!(self.0, 41);
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
    static ALIGNED: LocalKey<Aligned> = LocalKey::new(|| Aligned(41));
    for _ in 0..32 {
        std::thread::spawn(|| ALIGNED.with(|v| assert_eq!((v as *const _ as usize) % 4096, 0)))
            .join()
            .unwrap();
    }
    assert_eq!(DROPS.load(Ordering::SeqCst), 32);
}

#[test]
fn hundreds_of_keys_stay_isolated_across_worker_turnover() {
    static KEYS: [LocalKey<Cell<usize>>; 512] = [const { LocalKey::new(|| Cell::new(0)) }; 512];
    KEYS[0].with(|first| {
        first.set(1234);
        for key in KEYS.iter().skip(1) {
            key.with(|_| ());
        }
        assert_eq!(
            first.get(),
            1234,
            "growing the index table moved a borrowed value"
        );
    });
    for (i, key) in KEYS.iter().enumerate() {
        key.with(|v| v.set(i + 1));
    }
    let workers: Vec<_> = (0..8)
        .map(|worker| {
            std::thread::spawn(move || {
                for (i, key) in KEYS.iter().enumerate() {
                    key.with(|v| {
                        assert_eq!(v.get(), 0);
                        v.set(worker + i);
                    });
                }
                for (i, key) in KEYS.iter().enumerate() {
                    key.with(|v| assert_eq!(v.get(), worker + i));
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    for (i, key) in KEYS.iter().enumerate() {
        key.with(|v| assert_eq!(v.get(), i + 1));
    }
}
