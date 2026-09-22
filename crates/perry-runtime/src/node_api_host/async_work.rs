use super::*;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

pub type NapiAsyncExecuteCallback = Option<unsafe extern "C" fn(NapiEnv, *mut c_void)>;
pub type NapiAsyncCompleteCallback = Option<unsafe extern "C" fn(NapiEnv, NapiStatus, *mut c_void)>;

const WORK_CREATED: u8 = 0;
const WORK_QUEUED: u8 = 1;
const WORK_RUNNING: u8 = 2;
const WORK_COMPLETING: u8 = 3;
const WORK_CANCELLED: u8 = 4;
const WORK_COMPLETE: u8 = 5;

pub(crate) struct AsyncWorkInner {
    env_address: usize,
    env_serial: u64,
    owner: std::thread::ThreadId,
    execute: usize,
    complete: usize,
    data: usize,
    module: Option<u32>,
    state: AtomicU8,
    deleted: AtomicBool,
}

pub(crate) struct AsyncWorkRecord {
    pub env_serial: u64,
    pub inner: Arc<AsyncWorkInner>,
}

static COMPLETIONS: LazyLock<Mutex<VecDeque<Arc<AsyncWorkInner>>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
static ACTIVE_WORK: AtomicUsize = AtomicUsize::new(0);

fn work(env: NapiEnv, handle: NapiAsyncWork) -> Option<Arc<AsyncWorkInner>> {
    if handle.is_null() {
        return None;
    }
    with_env(env, |env| {
        let index = *env.async_work_lookup.get(&(handle as usize))?;
        env.async_works
            .get(index)
            .filter(|record| record.env_serial == env.serial)
            .map(|record| Arc::clone(&record.inner))
    })
    .flatten()
}

fn enqueue_completion(work: Arc<AsyncWorkInner>) {
    if let Ok(mut queue) = COMPLETIONS.lock() {
        queue.push_back(work);
    }
    crate::event_pump::js_notify_main_thread();
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_async_work(
    env: NapiEnv,
    async_resource: NapiValue,
    async_resource_name: NapiValue,
    execute: NapiAsyncExecuteCallback,
    complete: NapiAsyncCompleteCallback,
    data: *mut c_void,
    result: *mut NapiAsyncWork,
) -> NapiStatus {
    if result.is_null() || execute.is_none() || complete.is_none() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "execute, complete, and result must not be null",
        );
    }
    if (!async_resource.is_null() && value_bits(env, async_resource).is_err())
        || value_bits(env, async_resource_name).is_err()
    {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "async resource handles are invalid",
        );
    }
    let env_serial = match with_env(env, |env| env.serial) {
        Some(serial) => serial,
        None => return NapiStatus::InvalidArg,
    };
    let inner = Arc::new(AsyncWorkInner {
        env_address: env as usize,
        env_serial,
        owner: std::thread::current().id(),
        execute: execute.unwrap() as usize,
        complete: complete.unwrap() as usize,
        data: data as usize,
        module: active_module(env),
        state: AtomicU8::new(WORK_CREATED),
        deleted: AtomicBool::new(false),
    });
    let handle = with_env_mut(env, |env| {
        let mut record = Box::new(AsyncWorkRecord {
            env_serial: env.serial,
            inner,
        });
        let pointer = (&mut *record) as *mut AsyncWorkRecord as NapiAsyncWork;
        env.async_work_lookup
            .insert(pointer as usize, env.async_works.len());
        env.async_works.push(record);
        pointer
    });
    let Some(handle) = handle else {
        return NapiStatus::InvalidArg;
    };
    *result = handle;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_queue_async_work(env: NapiEnv, handle: NapiAsyncWork) -> NapiStatus {
    let Some(work) = work(env, handle) else {
        return set_status(env, NapiStatus::InvalidArg, "async work is unknown");
    };
    if work.deleted.load(Ordering::Acquire)
        || work
            .state
            .compare_exchange(
                WORK_CREATED,
                WORK_QUEUED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
    {
        return set_status(
            env,
            NapiStatus::GenericFailure,
            "async work cannot be queued",
        );
    }
    ACTIVE_WORK.fetch_add(1, Ordering::AcqRel);
    // turnloop P4: an addon's async work is exactly what the shared blocking
    // pool is for (DESIGN D8). It used to get **one fresh OS thread per queued
    // work item** — an addon that queues a work item per request paid a thread
    // creation per request, and nothing bounded how many ran at once.
    //
    // `execute` is the addon's own C function and only ever touches the data
    // pointer it was given; the `complete` half, which runs JS, still runs on
    // the owning thread out of `COMPLETIONS`, unchanged. So the only thing
    // that moves is which thread runs `execute`.
    //
    // A thread with no loop keeps the old transport rather than running the
    // addon's `execute` inline: an addon that queues async work expects it
    // *not* to run on the JS thread (that is the whole point of the API), so
    // `submit_or_run_inline` would be the wrong fallback here.
    #[cfg(not(target_arch = "wasm32"))]
    let queued = {
        let pooled = std::sync::Arc::clone(&work);
        crate::turnloop_pool::submit(
            move || run_async_work(pooled),
            |delivery| {
                if let crate::turnloop_pool::Delivery::Done(work) = delivery {
                    // On the owning thread already; push into the same queue the
                    // thread pushed into, so `drain_async_completions` and its
                    // handle-scope handling are untouched.
                    finish_async_work(work);
                }
            },
        )
        .is_ok()
    };
    #[cfg(target_arch = "wasm32")]
    let queued = false;
    if !queued {
        std::thread::spawn(move || {
            if let Some(work) = run_async_work(work) {
                finish_async_work(Some(work));
            }
        });
    }
    ok(env)
}

/// Run one queued work item's `execute` callback, wherever this is called.
///
/// Returns the record when it really ran, and `None` when the state machine
/// had already moved it out of `WORK_QUEUED` (a cancel won). The `Arc` is
/// carried through rather than captured so the same body serves the pool path
/// and the no-loop thread fallback.
fn run_async_work(work: Arc<AsyncWorkInner>) -> Option<Arc<AsyncWorkInner>> {
    if work
        .state
        .compare_exchange(
            WORK_QUEUED,
            WORK_RUNNING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return None;
    }
    let execute: unsafe extern "C" fn(NapiEnv, *mut c_void) =
        unsafe { std::mem::transmute(work.execute) };
    unsafe {
        execute(work.env_address as NapiEnv, work.data as *mut c_void);
    }
    work.state.store(WORK_COMPLETING, Ordering::Release);
    Some(work)
}

/// Hand a finished work item to the owning thread's completion queue.
fn finish_async_work(work: Option<Arc<AsyncWorkInner>>) {
    if let Some(work) = work {
        enqueue_completion(work);
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_cancel_async_work(env: NapiEnv, handle: NapiAsyncWork) -> NapiStatus {
    let Some(work) = work(env, handle) else {
        return set_status(env, NapiStatus::InvalidArg, "async work is unknown");
    };
    if work
        .state
        .compare_exchange(
            WORK_QUEUED,
            WORK_CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return set_status(
            env,
            NapiStatus::GenericFailure,
            "async work is not waiting in the queue",
        );
    }
    enqueue_completion(work);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_delete_async_work(env: NapiEnv, handle: NapiAsyncWork) -> NapiStatus {
    let Some(work) = work(env, handle) else {
        return set_status(env, NapiStatus::InvalidArg, "async work is unknown");
    };
    if work.deleted.swap(true, Ordering::AcqRel) {
        return set_status(env, NapiStatus::InvalidArg, "async work is already deleted");
    }
    ok(env)
}

pub(crate) fn drain_async_completions() -> i32 {
    // turnloop P4, and the rule P2 established: a pump has to turn the loop
    // before it drains its queue. A thread-backed work item had already pushed
    // its completion by the time anything looked; a pool-backed one exists only
    // once the loop has been turned, so a caller that drives this pump without
    // parking — an addon's own poll loop, and this module's unit tests — would
    // otherwise spin against a queue nothing can fill.
    //
    // Costs a thread-local read and no syscall when this process has queued no
    // pool job.
    #[cfg(not(target_arch = "wasm32"))]
    if crate::turnloop_pool::has_pending_jobs() {
        crate::event_pump::js_loop_turn_bounded(0);
    }
    let current = std::thread::current().id();
    let ready = {
        let Ok(mut queue) = COMPLETIONS.lock() else {
            return 0;
        };
        let mut ready = Vec::new();
        let mut retained = VecDeque::new();
        while let Some(work) = queue.pop_front() {
            if work.owner == current {
                ready.push(work);
            } else {
                retained.push_back(work);
            }
        }
        *queue = retained;
        ready
    };
    let mut ran = 0i32;
    for work in ready {
        let env = work.env_address as NapiEnv;
        if with_env(env, |env| env.serial) != Some(work.env_serial) {
            ACTIVE_WORK.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        let status = if work.state.load(Ordering::Acquire) == WORK_CANCELLED {
            NapiStatus::Cancelled
        } else {
            NapiStatus::Ok
        };
        let mut scope = std::ptr::null_mut();
        let opened = unsafe { napi_open_handle_scope(env, &mut scope) } == NapiStatus::Ok;
        let complete: unsafe extern "C" fn(NapiEnv, NapiStatus, *mut c_void) =
            unsafe { std::mem::transmute(work.complete) };
        with_active_module(env, work.module, || unsafe {
            complete(env, status, work.data as *mut c_void);
        });
        if opened {
            unsafe {
                napi_close_handle_scope(env, scope);
            }
        }
        work.state.store(WORK_COMPLETE, Ordering::Release);
        ACTIVE_WORK.fetch_sub(1, Ordering::AcqRel);
        ran = ran.saturating_add(1);
        // Node completes async work with the uncaught-exception policy
        // enforced for every module version.
        settle_callback_exception(env, work.module, true);
    }
    ran
}

pub(crate) fn has_active_async_work() -> bool {
    ACTIVE_WORK.load(Ordering::Acquire) != 0
}

pub(crate) fn cancel_env_async_work(env: NapiEnv) {
    let work = with_env(env, |env| {
        env.async_works
            .iter()
            .map(|record| Arc::clone(&record.inner))
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for work in work {
        if work
            .state
            .compare_exchange(
                WORK_QUEUED,
                WORK_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            enqueue_completion(work);
        }
    }
}
