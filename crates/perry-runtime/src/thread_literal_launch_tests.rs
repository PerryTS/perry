//! The launch ABI must prepare in each actual OS worker before its body runs.
//! Run serially (RUST_TEST_THREADS=1), like the rest of perry-runtime's tests.
//! Map/filter require at least two available CPUs; a caller fallback cannot pass.
use super::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::{Duration, Instant};

// Primitive observations only: no heap pointers or runtime registry.
crate::perry_thread_local! {
    static IS_LAUNCHER: Cell<bool> = const { Cell::new(false) };
    static PREPARED_AGENT: Cell<Option<u64>> = const { Cell::new(None) };
    static BODY_STARTED: Cell<bool> = const { Cell::new(false) };
}
static LAUNCHER_AGENT: AtomicU64 = AtomicU64::new(0);
static EXPECT_PREPARED: AtomicBool = AtomicBool::new(false);
static PREPARE_CALLS: AtomicUsize = AtomicUsize::new(0);
static BODY_CALLS: AtomicUsize = AtomicUsize::new(0);
static VIOLATIONS: AtomicUsize = AtomicUsize::new(0);

fn note_worker() {
    // The launcher's marker is thread-local: seeing it means the same OS thread.
    if IS_LAUNCHER.with(Cell::get) {
        VIOLATIONS.fetch_or(1, Ordering::SeqCst);
    }
    let agent = crate::agent::current_agent();
    if agent == crate::agent::PRIMARY_AGENT || agent == LAUNCHER_AGENT.load(Ordering::SeqCst) {
        VIOLATIONS.fetch_or(2, Ordering::SeqCst);
    }
}

extern "C" fn prepare() {
    // Never assert/panic through an extern-C callback; report on the test thread.
    note_worker();
    if PREPARED_AGENT.with(Cell::get).is_some() {
        VIOLATIONS.fetch_or(4, Ordering::SeqCst);
    }
    if BODY_STARTED.with(Cell::get) {
        VIOLATIONS.fetch_or(8, Ordering::SeqCst);
    }
    PREPARED_AGENT.with(|slot| slot.set(Some(crate::agent::current_agent())));
    PREPARE_CALLS.fetch_add(1, Ordering::SeqCst);
}

fn observe_body(closure: *const ClosureHeader) -> f64 {
    note_worker();
    BODY_STARTED.with(|slot| slot.set(true));
    let prepared = PREPARED_AGENT.with(Cell::get);
    if EXPECT_PREPARED.load(Ordering::SeqCst) {
        if prepared != Some(crate::agent::current_agent()) {
            VIOLATIONS.fetch_or(16, Ordering::SeqCst);
        }
    } else if prepared.is_some() {
        VIOLATIONS.fetch_or(32, Ordering::SeqCst);
    }
    BODY_CALLS.fetch_add(1, Ordering::SeqCst);
    if closure.is_null() {
        VIOLATIONS.fetch_or(64, Ordering::SeqCst);
        return 0.0;
    }
    let capture = f64::from_bits(closure::js_closure_get_capture_bits(closure, 0));
    if capture != 7.0 {
        VIOLATIONS.fetch_or(64, Ordering::SeqCst);
    }
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

fn begin(with_literals: bool) {
    crate::gc::ensure_gc_initialized();
    IS_LAUNCHER.with(|slot| slot.set(true));
    PREPARED_AGENT.with(|slot| slot.set(None));
    BODY_STARTED.with(|slot| slot.set(false));
    LAUNCHER_AGENT.store(crate::agent::current_agent(), Ordering::SeqCst);
    EXPECT_PREPARED.store(with_literals, Ordering::SeqCst);
    PREPARE_CALLS.store(0, Ordering::SeqCst);
    BODY_CALLS.store(0, Ordering::SeqCst);
    VIOLATIONS.store(0, Ordering::SeqCst);
}

fn finish(with_literals: bool, workers: usize) {
    assert_eq!(
        VIOLATIONS.load(Ordering::SeqCst),
        0,
        "worker/order violation bitmask"
    );
    assert_eq!(
        BODY_CALLS.load(Ordering::SeqCst),
        workers,
        "body must actually run"
    );
    assert_eq!(
        PREPARE_CALLS.load(Ordering::SeqCst),
        if with_literals { workers } else { 0 },
        "one preparation per worker; legacy wrappers supply zero callbacks"
    );
    assert_eq!(
        PREPARED_AGENT.with(Cell::get),
        None,
        "caller must not prepare"
    );
    IS_LAUNCHER.with(|slot| slot.set(false));
}

fn run_spawn(with_literals: bool) {
    let _lock = crate::gc::global_side_table_test_lock();
    begin(with_literals);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(closure::js_closure_alloc(
        crate::fn_info!(spawn_body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE)),
        1,
    ));
    closure::js_closure_set_capture_f64(closure.get_raw_mut_ptr(), 0, 7.0);
    let boxed = crate::value::js_nanbox_pointer(closure.get_raw_mut_ptr::<ClosureHeader>() as i64);
    let result = if with_literals {
        js_thread_spawn_with_literals(boxed, prepare as *const () as usize as i64)
    } else {
        js_thread_spawn(boxed)
    };
    let promise =
        scope.root_raw_mut_ptr((result.to_bits() & POINTER_MASK) as *mut crate::promise::Promise);
    let deadline = Instant::now() + Duration::from_secs(5);
    while crate::promise::js_promise_state(promise.get_raw_mut_ptr()) == 0 {
        js_thread_process_pending();
        assert!(Instant::now() < deadline, "worker promise did not settle");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        crate::promise::js_promise_state(promise.get_raw_mut_ptr()),
        1
    );
    assert_eq!(
        crate::promise::js_promise_value(promise.get_raw_mut_ptr()),
        19.0
    );
    finish(with_literals, 1);
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
    begin(with_literals);
    let scope = crate::gc::RuntimeHandleScope::new();
    let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(2));
    crate::array::js_array_set_f64(array.get_raw_mut_ptr(), 0, 8.0);
    crate::array::js_array_set_f64(array.get_raw_mut_ptr(), 1, 9.0);
    let info = if filter {
        crate::fn_info!(filter_body, 1; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
    } else {
        crate::fn_info!(map_body, 1; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE))
    };
    let closure = scope.root_raw_mut_ptr(closure::js_closure_alloc(info, 1));
    closure::js_closure_set_capture_f64(closure.get_raw_mut_ptr(), 0, 7.0);
    let array = crate::value::js_nanbox_pointer(
        array.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
    );
    let closure =
        crate::value::js_nanbox_pointer(closure.get_raw_mut_ptr::<ClosureHeader>() as i64);
    let callback = prepare as *const () as usize as i64;
    let result = match (with_literals, filter) {
        (true, false) => js_thread_parallel_map_with_literals(array, closure, callback),
        (true, true) => js_thread_parallel_filter_with_literals(array, closure, callback),
        (false, false) => js_thread_parallel_map(array, closure),
        (false, true) => js_thread_parallel_filter(array, closure),
    };
    let result =
        scope.root_raw_mut_ptr((result.to_bits() & POINTER_MASK) as *mut crate::array::ArrayHeader);
    let result = result.get_raw_mut_ptr::<crate::array::ArrayHeader>();
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
    finish(with_literals, 2);
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
