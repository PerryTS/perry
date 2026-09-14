//! #10219: more live Perry declarations than Android's entire pthread key
//! budget. The same binary checks thread isolation and real value destruction.
#![cfg(unix)]
#![recursion_limit = "512"]

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
static DROPS: AtomicUsize = AtomicUsize::new(0);
struct Probe(usize);
impl Drop for Probe {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}
macro_rules! declarations {
    ($($key:ident),+ $(,)?) => {
        perry_runtime::perry_thread_local! {
            $(static $key: RefCell<Vec<Probe>> = RefCell::new(Vec::new());)+
        }
        fn visit(mut f: impl FnMut(usize, &'static perry_runtime::tls_hot::HotKey<RefCell<Vec<Probe>>>)) {
            for (i, key) in [$(&$key),+].into_iter().enumerate() { f(i, key); }
        }
    };
}
declarations!(
    KEY_000, KEY_001, KEY_002, KEY_003, KEY_004, KEY_005, KEY_006, KEY_007, KEY_008, KEY_009,
    KEY_010, KEY_011, KEY_012, KEY_013, KEY_014, KEY_015, KEY_016, KEY_017, KEY_018, KEY_019,
    KEY_020, KEY_021, KEY_022, KEY_023, KEY_024, KEY_025, KEY_026, KEY_027, KEY_028, KEY_029,
    KEY_030, KEY_031, KEY_032, KEY_033, KEY_034, KEY_035, KEY_036, KEY_037, KEY_038, KEY_039,
    KEY_040, KEY_041, KEY_042, KEY_043, KEY_044, KEY_045, KEY_046, KEY_047, KEY_048, KEY_049,
    KEY_050, KEY_051, KEY_052, KEY_053, KEY_054, KEY_055, KEY_056, KEY_057, KEY_058, KEY_059,
    KEY_060, KEY_061, KEY_062, KEY_063, KEY_064, KEY_065, KEY_066, KEY_067, KEY_068, KEY_069,
    KEY_070, KEY_071, KEY_072, KEY_073, KEY_074, KEY_075, KEY_076, KEY_077, KEY_078, KEY_079,
    KEY_080, KEY_081, KEY_082, KEY_083, KEY_084, KEY_085, KEY_086, KEY_087, KEY_088, KEY_089,
    KEY_090, KEY_091, KEY_092, KEY_093, KEY_094, KEY_095, KEY_096, KEY_097, KEY_098, KEY_099,
    KEY_100, KEY_101, KEY_102, KEY_103, KEY_104, KEY_105, KEY_106, KEY_107, KEY_108, KEY_109,
    KEY_110, KEY_111, KEY_112, KEY_113, KEY_114, KEY_115, KEY_116, KEY_117, KEY_118, KEY_119,
    KEY_120, KEY_121, KEY_122, KEY_123, KEY_124, KEY_125, KEY_126, KEY_127, KEY_128, KEY_129,
    KEY_130, KEY_131, KEY_132, KEY_133, KEY_134, KEY_135, KEY_136, KEY_137, KEY_138, KEY_139,
    KEY_140, KEY_141, KEY_142, KEY_143, KEY_144, KEY_145, KEY_146, KEY_147, KEY_148, KEY_149,
    KEY_150, KEY_151, KEY_152, KEY_153, KEY_154, KEY_155, KEY_156, KEY_157, KEY_158, KEY_159,
    KEY_160, KEY_161, KEY_162, KEY_163, KEY_164, KEY_165, KEY_166, KEY_167, KEY_168, KEY_169,
    KEY_170, KEY_171, KEY_172, KEY_173, KEY_174, KEY_175, KEY_176, KEY_177, KEY_178, KEY_179,
    KEY_180, KEY_181, KEY_182, KEY_183, KEY_184, KEY_185, KEY_186, KEY_187, KEY_188, KEY_189,
    KEY_190, KEY_191,
);

#[test]
fn hundreds_of_perry_declarations_initialize_and_drop_on_multiple_threads() {
    visit(|i, key| key.with(|value| value.borrow_mut().push(Probe(1000 + i))));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|worker| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                visit(|i, key| {
                    key.with(|value| {
                        assert!(
                            value.borrow().is_empty(),
                            "a worker inherited another thread's storage"
                        );
                        value.borrow_mut().push(Probe(worker * 1000 + i));
                    })
                });
                barrier.wait();
                visit(|i, key| {
                    key.with(|value| assert_eq!(value.borrow()[0].0, worker * 1000 + i))
                });
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        DROPS.load(Ordering::SeqCst),
        192 * 8,
        "worker values must be destroyed exactly once"
    );
    visit(|i, key| key.with(|value| assert_eq!(value.borrow()[0].0, 1000 + i)));
}

// A destructor owned by another library can run after Perry's entire pool.
// Keep try_with fallible then, including across repeated POSIX destructor passes.
#[cfg(target_os = "android")]
#[test]
fn later_pthread_destructors_cannot_resurrect_the_hot_cache() {
    use std::sync::atomic::AtomicU8;
    static OBSERVED: AtomicU8 = AtomicU8::new(0);
    perry_runtime::perry_thread_local! {
        static LATE_PROBE: RefCell<Vec<u8>> = RefCell::new(Vec::new());
    }
    struct Callback {
        key: libc::pthread_key_t,
        round: u8,
    }
    unsafe extern "C" fn callback(raw: *mut libc::c_void) {
        let state = &mut *raw.cast::<Callback>();
        state.round += 1;
        if state.round < 3 {
            assert_eq!(libc::pthread_setspecific(state.key, raw), 0);
            return;
        }
        let observed = match std::panic::catch_unwind(|| LATE_PROBE.try_with(|_| ())) {
            Ok(Err(_)) => 1,
            Ok(Ok(_)) => 2,
            Err(_) => 3,
        };
        OBSERVED.store(observed, Ordering::SeqCst);
        drop(Box::from_raw(raw.cast::<Callback>()));
    }
    // By the third callback pass the pool has been destroyed, regardless of
    // which key the platform visits first within each pass.
    let key = std::thread::spawn(|| {
        LATE_PROBE.with(|value| value.borrow_mut().push(1));
        let mut key = 0;
        assert_eq!(
            unsafe { libc::pthread_key_create(&mut key, Some(callback)) },
            0
        );
        let state = Box::into_raw(Box::new(Callback { key, round: 0 }));
        assert_eq!(unsafe { libc::pthread_setspecific(key, state.cast()) }, 0);
        key
    })
    .join()
    .unwrap();
    unsafe {
        libc::pthread_key_delete(key);
    }
    assert_eq!(
        OBSERVED.load(Ordering::SeqCst),
        1,
        "1=AccessError, 2=resurrected value, 3=panic, 0=callback never ran"
    );
}
