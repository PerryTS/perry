//! The launch ABI must prepare in each actual OS worker before its body runs.
//! Run serially (RUST_TEST_THREADS=1), like the rest of perry-runtime's tests.
//! Map/filter require at least two available CPUs; a caller fallback cannot pass.
use super::*;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::{Duration, Instant};

// The no-argument preparation ABI cannot receive test state. Keep its evidence
// on the executing OS thread; the body transfers it to its captured owner.
crate::perry_thread_local! {
    static IS_LAUNCHER: Cell<bool> = const { Cell::new(false) };
    static PREPARED_AGENT: Cell<Option<u64>> = const { Cell::new(None) };
    static BODY_STARTED: Cell<bool> = const { Cell::new(false) };
    static PREPARE_COUNT: Cell<usize> = const { Cell::new(0) };
    static PREPARE_VIOLATIONS: Cell<usize> = const { Cell::new(0) };
    // Attached by the explicit body capture, retained until worker thread exit.
    // It contains Rust-owned observations only, never a runtime heap pointer.
    static WORKER_OBSERVATIONS: RefCell<Option<Arc<Observations>>> = const { RefCell::new(None) };
}

struct Observations {
    launcher_agent: u64,
    expect_prepared: bool,
    prepare_calls: AtomicUsize,
    body_calls: AtomicUsize,
    violations: AtomicUsize,
}

fn worker_violations() -> usize {
    // A launcher TLS marker proves this is the same OS thread, independently
    // of whether a broken launch path happened to install another agent id.
    usize::from(IS_LAUNCHER.with(Cell::get))
        | if crate::agent::current_agent() == crate::agent::PRIMARY_AGENT {
            2
        } else {
            0
        }
}

extern "C" fn prepare() {
    // Never assert/panic through an extern-C callback; report on the test thread.
    let mut violations = worker_violations();
    if PREPARED_AGENT.with(Cell::get).is_some() {
        violations |= 4;
    }
    if BODY_STARTED.with(Cell::get) {
        violations |= 8;
    }
    PREPARE_VIOLATIONS.with(|slot| slot.set(slot.get() | violations));
    PREPARED_AGENT.with(|slot| slot.set(Some(crate::agent::current_agent())));
    WORKER_OBSERVATIONS.with(|owner| {
        if let Some(observations) = owner.borrow().as_ref() {
            // A callback after a body must remain observable, including in the
            // legacy OFF arm. Its owner was explicitly transferred by that body.
            if crate::agent::current_agent() == observations.launcher_agent {
                violations |= 2;
            }
            if !observations.expect_prepared {
                violations |= 32;
            }
            observations.prepare_calls.fetch_add(1, Ordering::SeqCst);
            observations
                .violations
                .fetch_or(violations, Ordering::SeqCst);
        } else {
            PREPARE_COUNT.with(|slot| slot.set(slot.get() + 1));
        }
    });
}

// One escaped Arc reference keeps the observations alive for the entire launch,
// independent of how many times a broken runtime invokes the body. The caller
// reclaims it only after successful completion; timeout/assertion failures leak
// that reference so an outstanding worker can never observe freed state.
fn capture_observations(
    closure: *mut ClosureHeader,
    observations: &Arc<Observations>,
) -> *const Observations {
    let lease = Arc::into_raw(Arc::clone(observations));
    let address = lease as usize as u64;
    // Two exact numeric u32 captures survive the real serializer. This address
    // names Rust-owned atomics, never a GC object or a process-global lookup.
    closure::js_closure_set_capture_f64(closure, 0, 7.0);
    closure::js_closure_set_capture_f64(closure, 1, (address as u32) as f64);
    closure::js_closure_set_capture_f64(closure, 2, (address >> 32) as f64);
    lease
}

fn observe_body(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        // No owner can be recovered; the expected body count will fail.
        return 0.0;
    }
    let low = f64::from_bits(closure::js_closure_get_capture_bits(closure, 1)) as u32;
    let high = f64::from_bits(closure::js_closure_get_capture_bits(closure, 2)) as u32;
    let address = u64::from(low) | (u64::from(high) << 32);
    // SAFETY: the escaped launch reference remains alive until completion.
    // Bodies only borrow it, so even duplicate calls cannot consume its owner.
    let observations_ptr = address as usize as *const Observations;
    let observations = unsafe { &*observations_ptr };
    WORKER_OBSERVATIONS.with(|owner| {
        // SAFETY: clone the live escaped reference without consuming it. The
        // worker's owned clone also keeps any later prepare callback safe.
        let owned = unsafe {
            Arc::increment_strong_count(observations_ptr);
            Arc::from_raw(observations_ptr)
        };
        *owner.borrow_mut() = Some(owned);
    });
    let mut violations = worker_violations() | PREPARE_VIOLATIONS.with(Cell::get);
    if crate::agent::current_agent() == observations.launcher_agent {
        violations |= 2;
    }
    BODY_STARTED.with(|slot| slot.set(true));
    let prepared = PREPARED_AGENT.with(Cell::get);
    if observations.expect_prepared {
        if prepared != Some(crate::agent::current_agent()) {
            violations |= 16;
        }
    } else if prepared.is_some() {
        violations |= 32;
    }
    observations
        .prepare_calls
        .fetch_add(PREPARE_COUNT.with(|slot| slot.replace(0)), Ordering::SeqCst);
    observations.body_calls.fetch_add(1, Ordering::SeqCst);
    let capture = f64::from_bits(closure::js_closure_get_capture_bits(closure, 0));
    if capture != 7.0 {
        violations |= 64;
    }
    observations
        .violations
        .fetch_or(violations, Ordering::SeqCst);
    capture
}

extern "C" fn spawn_body(closure: *const ClosureHeader, _this: closure::JsThis) -> f64 {
    observe_body(closure) + 12.0
}

extern "C" fn map_body(closure: *const ClosureHeader, _this: closure::JsThis, value: f64) -> f64 {
    observe_body(closure) + value
}

extern "C" fn filter_body(
    closure: *const ClosureHeader,
    _this: closure::JsThis,
    value: f64,
) -> f64 {
    let capture = observe_body(closure);
    f64::from_bits(if value == capture + 1.0 {
        TAG_TRUE
    } else {
        TAG_FALSE
    })
}

fn begin(with_literals: bool) -> Arc<Observations> {
    crate::gc::ensure_gc_initialized();
    IS_LAUNCHER.with(|slot| slot.set(true));
    PREPARED_AGENT.with(|slot| slot.set(None));
    BODY_STARTED.with(|slot| slot.set(false));
    PREPARE_COUNT.with(|slot| slot.set(0));
    PREPARE_VIOLATIONS.with(|slot| slot.set(0));
    WORKER_OBSERVATIONS.with(|owner| *owner.borrow_mut() = None);
    Arc::new(Observations {
        launcher_agent: crate::agent::current_agent(),
        expect_prepared: with_literals,
        prepare_calls: AtomicUsize::new(0),
        body_calls: AtomicUsize::new(0),
        violations: AtomicUsize::new(0),
    })
}

fn finish(observations: &Observations, workers: usize) {
    assert_eq!(
        observations.violations.load(Ordering::SeqCst),
        0,
        "worker/order violation bitmask"
    );
    assert_eq!(
        observations.body_calls.load(Ordering::SeqCst),
        workers,
        "body must actually run"
    );
    assert_eq!(
        observations.prepare_calls.load(Ordering::SeqCst),
        if observations.expect_prepared {
            workers
        } else {
            0
        },
        "one preparation per worker; legacy wrappers supply zero callbacks"
    );
    assert_eq!(
        PREPARED_AGENT.with(Cell::get),
        None,
        "caller must not prepare"
    );
    assert_eq!(PREPARE_COUNT.with(Cell::get), 0, "caller must not prepare");
    assert_eq!(
        PREPARE_VIOLATIONS.with(Cell::get),
        0,
        "caller preparation violation"
    );
    assert!(!BODY_STARTED.with(Cell::get), "caller must not run a body");
    IS_LAUNCHER.with(|slot| slot.set(false));
}

fn run_spawn(with_literals: bool) {
    let _lock = crate::gc::global_side_table_test_lock();
    let observations = begin(with_literals);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(closure::js_closure_alloc(
        crate::fn_info!(spawn_body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
        3,
    ));
    let lease = closure.with_mut_ptr(|c| capture_observations(c, &observations));
    let result = closure.with_mut_ptr::<ClosureHeader, _>(|closure| {
        let boxed = crate::value::js_nanbox_pointer(closure as i64);
        if with_literals {
            js_thread_spawn_with_literals(boxed, prepare as *const () as usize as i64)
        } else {
            js_thread_spawn(boxed)
        }
    });
    let promise =
        scope.root_raw_mut_ptr((result.to_bits() & POINTER_MASK) as *mut crate::promise::Promise);
    let deadline = Instant::now() + Duration::from_secs(5);
    while promise.with_mut_ptr(|promise_ptr| crate::promise::js_promise_state(promise_ptr)) == 0 {
        js_thread_process_pending();
        assert!(Instant::now() < deadline, "worker promise did not settle");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        promise.with_mut_ptr(|promise_ptr| crate::promise::js_promise_state(promise_ptr)),
        1
    );
    assert_eq!(
        promise.with_mut_ptr(|promise_ptr| crate::promise::js_promise_value(promise_ptr)),
        19.0
    );
    finish(&observations, 1);
    // SAFETY: spawn queues its result only after the body returns. A settled
    // promise and the checks above establish that observation use has ended;
    // the remaining worker teardown does not access closure captures.
    unsafe { drop(Arc::from_raw(lease)) };
}

fn run_parallel(with_literals: bool, filter: bool) {
    let _lock = crate::gc::global_side_table_test_lock();
    assert!(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            >= 2,
        "requires two available CPUs to exercise actual map/filter OS workers"
    );
    let observations = begin(with_literals);
    let scope = crate::gc::RuntimeHandleScope::new();
    let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(2));
    array.with_mut_ptr(|array_ptr| crate::array::js_array_set_f64(array_ptr, 0, 8.0));
    array.with_mut_ptr(|array_ptr| crate::array::js_array_set_f64(array_ptr, 1, 9.0));
    let info = if filter {
        crate::fn_info!(filter_body, 1; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
    } else {
        crate::fn_info!(map_body, 1; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
    };
    let closure = scope.root_raw_mut_ptr(closure::js_closure_alloc(info, 3));
    let lease = closure.with_mut_ptr(|c| capture_observations(c, &observations));
    // Launch roots the closure before resolving the array and serializing its
    // inputs. Keep raw arguments scoped to the entry point; inspect its rooted result.
    let callback = prepare as *const () as usize as i64;
    let result = array.with_mut_ptr::<crate::array::ArrayHeader, _>(|array| {
        closure.with_mut_ptr::<ClosureHeader, _>(|closure| {
            let array = crate::value::js_nanbox_pointer(array as i64);
            let closure = crate::value::js_nanbox_pointer(closure as i64);
            match (with_literals, filter) {
                (true, false) => js_thread_parallel_map_with_literals(array, closure, callback),
                (true, true) => js_thread_parallel_filter_with_literals(array, closure, callback),
                (false, false) => js_thread_parallel_map(array, closure),
                (false, true) => js_thread_parallel_filter(array, closure),
            }
        })
    });
    let result =
        scope.root_raw_mut_ptr((result.to_bits() & POINTER_MASK) as *mut crate::array::ArrayHeader);
    result.with_mut_ptr::<crate::array::ArrayHeader, _>(|result| {
        assert_eq!(
            crate::array::js_array_get_length(result as i64),
            if filter { 1 } else { 2 }
        );
        assert_eq!(
            crate::array::js_array_get_f64(result, 0),
            if filter { 8.0 } else { 15.0 }
        );
        if !filter {
            assert_eq!(crate::array::js_array_get_f64(result, 1), 16.0);
        }
    });
    finish(&observations, 2);
    // SAFETY: parallel map/filter join every scoped worker before returning.
    unsafe { drop(Arc::from_raw(lease)) };
}

#[test]
fn spawn_prepares_each_os_worker_before_body() {
    run_spawn(true);
}
#[test]
fn map_prepares_each_os_worker_before_body() {
    run_parallel(true, false);
}
#[test]
fn filter_prepares_each_os_worker_before_body() {
    run_parallel(true, true);
}
#[test]
fn legacy_spawn_uses_zero_callback() {
    run_spawn(false);
}
#[test]
fn legacy_map_uses_zero_callback() {
    run_parallel(false, false);
}
#[test]
fn legacy_filter_uses_zero_callback() {
    run_parallel(false, true);
}
