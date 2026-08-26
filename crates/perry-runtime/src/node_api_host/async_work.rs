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
    std::thread::spawn(move || {
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
            return;
        }
        let execute: unsafe extern "C" fn(NapiEnv, *mut c_void) =
            unsafe { std::mem::transmute(work.execute) };
        unsafe {
            execute(work.env_address as NapiEnv, work.data as *mut c_void);
        }
        work.state.store(WORK_COMPLETING, Ordering::Release);
        enqueue_completion(work);
    });
    ok(env)
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
        unsafe {
            complete(env, status, work.data as *mut c_void);
        }
        if opened {
            unsafe {
                napi_close_handle_scope(env, scope);
            }
        }
        work.state.store(WORK_COMPLETE, Ordering::Release);
        ACTIVE_WORK.fetch_sub(1, Ordering::AcqRel);
        ran = ran.saturating_add(1);
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
