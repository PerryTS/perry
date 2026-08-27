use super::*;
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};

pub type NapiThreadsafeFunctionCallJs =
    Option<unsafe extern "C" fn(NapiEnv, NapiValue, *mut c_void, *mut c_void)>;

#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NapiThreadsafeFunctionCallMode {
    Nonblocking = 0,
    Blocking = 1,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NapiThreadsafeFunctionReleaseMode {
    Release = 0,
    Abort = 1,
}

struct TsfnJsState {
    function_bits: Option<u64>,
}

pub(crate) struct ThreadsafeFunctionInner {
    env_address: usize,
    env_serial: u64,
    owner: std::thread::ThreadId,
    js: Mutex<TsfnJsState>,
    queue: Mutex<VecDeque<usize>>,
    capacity: Condvar,
    max_queue_size: usize,
    thread_count: AtomicUsize,
    closing: AtomicBool,
    aborted: AtomicBool,
    referenced: AtomicBool,
    finalized: AtomicBool,
    finalize_data: usize,
    finalize_callback: usize,
    context: usize,
    call_js: usize,
}

struct ThreadsafeFunctionToken {
    _serial: u64,
}

static TSFN_REGISTRY: LazyLock<Mutex<HashMap<usize, Arc<ThreadsafeFunctionInner>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn lookup(handle: NapiThreadsafeFunction) -> Option<Arc<ThreadsafeFunctionInner>> {
    if handle.is_null() {
        return None;
    }
    TSFN_REGISTRY.lock().ok()?.get(&(handle as usize)).cloned()
}

fn owner_env(inner: &ThreadsafeFunctionInner) -> Option<NapiEnv> {
    if inner.owner != std::thread::current().id() {
        return None;
    }
    let env = inner.env_address as NapiEnv;
    (with_env(env, |env| env.serial) == Some(inner.env_serial)).then_some(env)
}

pub(crate) fn scan_tsfn_function_root(
    tsfn: &Arc<ThreadsafeFunctionInner>,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    let Ok(mut js) = tsfn.js.lock() else {
        return;
    };
    if let Some(function) = js.function_bits.as_mut() {
        visitor.visit_nanbox_u64_slot(function);
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_threadsafe_function(
    env: NapiEnv,
    function: NapiValue,
    async_resource: NapiValue,
    async_resource_name: NapiValue,
    max_queue_size: usize,
    initial_thread_count: usize,
    thread_finalize_data: *mut c_void,
    thread_finalize_cb: NapiFinalize,
    context: *mut c_void,
    call_js_cb: NapiThreadsafeFunctionCallJs,
    result: *mut NapiThreadsafeFunction,
) -> NapiStatus {
    if result.is_null() || initial_thread_count == 0 || (function.is_null() && call_js_cb.is_none())
    {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "TSFN result, callback, and thread count must be valid",
        );
    }
    let function_bits = if function.is_null() {
        None
    } else {
        match value_bits(env, function) {
            Ok(bits) => Some(bits),
            Err(status) => return set_status(env, status, "TSFN function is not a live handle"),
        }
    };
    if (!async_resource.is_null() && value_bits(env, async_resource).is_err())
        || value_bits(env, async_resource_name).is_err()
    {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "TSFN resource handles are invalid",
        );
    }
    let env_serial = match with_env(env, |env| env.serial) {
        Some(serial) => serial,
        None => return NapiStatus::InvalidArg,
    };
    let inner = Arc::new(ThreadsafeFunctionInner {
        env_address: env as usize,
        env_serial,
        owner: std::thread::current().id(),
        js: Mutex::new(TsfnJsState { function_bits }),
        queue: Mutex::new(VecDeque::new()),
        capacity: Condvar::new(),
        max_queue_size,
        thread_count: AtomicUsize::new(initial_thread_count),
        closing: AtomicBool::new(false),
        aborted: AtomicBool::new(false),
        referenced: AtomicBool::new(true),
        finalized: AtomicBool::new(false),
        finalize_data: thread_finalize_data as usize,
        finalize_callback: thread_finalize_cb.map_or(0, |callback| callback as usize),
        context: context as usize,
        call_js: call_js_cb.map_or(0, |callback| callback as usize),
    });
    // Tokens are permanent tombstones. Their tiny allocation is intentionally
    // not reused, so a stale addon handle can never alias a later TSFN.
    let token = Box::into_raw(Box::new(ThreadsafeFunctionToken {
        _serial: env_serial,
    })) as NapiThreadsafeFunction;
    if let Ok(mut registry) = TSFN_REGISTRY.lock() {
        registry.insert(token as usize, Arc::clone(&inner));
    } else {
        return set_status(
            env,
            NapiStatus::GenericFailure,
            "TSFN registry is unavailable",
        );
    }
    if with_env_mut(env, |env| env.tsfns.push(inner)).is_none() {
        return NapiStatus::InvalidArg;
    }
    *result = token;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_threadsafe_function_context(
    function: NapiThreadsafeFunction,
    result: *mut *mut c_void,
) -> NapiStatus {
    if result.is_null() {
        return NapiStatus::InvalidArg;
    }
    let Some(inner) = lookup(function) else {
        return NapiStatus::InvalidArg;
    };
    if owner_env(&inner).is_none() {
        return NapiStatus::GenericFailure;
    }
    *result = inner.context as *mut c_void;
    NapiStatus::Ok
}

#[no_mangle]
pub unsafe extern "C" fn napi_call_threadsafe_function(
    function: NapiThreadsafeFunction,
    data: *mut c_void,
    mode: NapiThreadsafeFunctionCallMode,
) -> NapiStatus {
    let Some(inner) = lookup(function) else {
        return NapiStatus::InvalidArg;
    };
    let Ok(mut queue) = inner.queue.lock() else {
        return NapiStatus::GenericFailure;
    };
    loop {
        if inner.closing.load(Ordering::Acquire) {
            return NapiStatus::Closing;
        }
        if inner.max_queue_size == 0 || queue.len() < inner.max_queue_size {
            break;
        }
        if mode == NapiThreadsafeFunctionCallMode::Nonblocking {
            return NapiStatus::QueueFull;
        }
        if inner.owner == std::thread::current().id() {
            return NapiStatus::WouldDeadlock;
        }
        let Ok(next) = inner.capacity.wait(queue) else {
            return NapiStatus::GenericFailure;
        };
        queue = next;
    }
    queue.push_back(data as usize);
    drop(queue);
    crate::event_pump::js_notify_main_thread();
    NapiStatus::Ok
}

#[no_mangle]
pub unsafe extern "C" fn napi_acquire_threadsafe_function(
    function: NapiThreadsafeFunction,
) -> NapiStatus {
    let Some(inner) = lookup(function) else {
        return NapiStatus::InvalidArg;
    };
    if inner.closing.load(Ordering::Acquire) {
        return NapiStatus::Closing;
    }
    let mut current = inner.thread_count.load(Ordering::Acquire);
    loop {
        let Some(next) = current.checked_add(1) else {
            return NapiStatus::GenericFailure;
        };
        match inner.thread_count.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return NapiStatus::Ok,
            Err(actual) => current = actual,
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_release_threadsafe_function(
    function: NapiThreadsafeFunction,
    mode: NapiThreadsafeFunctionReleaseMode,
) -> NapiStatus {
    let Some(inner) = lookup(function) else {
        return NapiStatus::InvalidArg;
    };
    if mode == NapiThreadsafeFunctionReleaseMode::Abort {
        inner.aborted.store(true, Ordering::Release);
        inner.closing.store(true, Ordering::Release);
    }
    let mut current = inner.thread_count.load(Ordering::Acquire);
    loop {
        if current == 0 {
            return NapiStatus::InvalidArg;
        }
        match inner.thread_count.compare_exchange_weak(
            current,
            current - 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                if current == 1 {
                    inner.closing.store(true, Ordering::Release);
                }
                inner.capacity.notify_all();
                crate::event_pump::js_notify_main_thread();
                return NapiStatus::Ok;
            }
            Err(actual) => current = actual,
        }
    }
}

fn owner_ref_change(
    env: NapiEnv,
    function: NapiThreadsafeFunction,
    referenced: bool,
) -> NapiStatus {
    let Some(inner) = lookup(function) else {
        return set_status(env, NapiStatus::InvalidArg, "TSFN is unknown");
    };
    if inner.env_address != env as usize || owner_env(&inner) != Some(env) {
        return set_status(
            env,
            NapiStatus::GenericFailure,
            "TSFN belongs to another agent",
        );
    }
    inner.referenced.store(referenced, Ordering::Release);
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_unref_threadsafe_function(
    env: NapiEnv,
    function: NapiThreadsafeFunction,
) -> NapiStatus {
    owner_ref_change(env, function, false)
}

#[no_mangle]
pub unsafe extern "C" fn napi_ref_threadsafe_function(
    env: NapiEnv,
    function: NapiThreadsafeFunction,
) -> NapiStatus {
    owner_ref_change(env, function, true)
}

fn invoke_item(inner: &Arc<ThreadsafeFunctionInner>, env: NapiEnv, data: usize, aborted: bool) {
    let mut scope = std::ptr::null_mut();
    let opened = unsafe { napi_open_handle_scope(env, &mut scope) } == NapiStatus::Ok;
    let function_bits = inner.js.lock().ok().and_then(|js| js.function_bits);
    let function = if aborted {
        std::ptr::null_mut()
    } else {
        function_bits
            .and_then(|bits| add_handle(env, bits).ok())
            .unwrap_or(std::ptr::null_mut())
    };
    if inner.call_js != 0 {
        let callback: unsafe extern "C" fn(NapiEnv, NapiValue, *mut c_void, *mut c_void) =
            unsafe { std::mem::transmute(inner.call_js) };
        unsafe {
            callback(
                if aborted { std::ptr::null_mut() } else { env },
                function,
                inner.context as *mut c_void,
                data as *mut c_void,
            );
        }
    } else if !aborted && !function.is_null() {
        let mut global = std::ptr::null_mut();
        if unsafe { napi_get_global(env, &mut global) } == NapiStatus::Ok {
            unsafe {
                napi_call_function(
                    env,
                    global,
                    function,
                    0,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                );
            }
        }
    }
    if opened {
        unsafe {
            napi_close_handle_scope(env, scope);
        }
    }
}

fn maybe_finalize(handle: usize, inner: &Arc<ThreadsafeFunctionInner>, env: NapiEnv) -> bool {
    let empty = inner.queue.lock().is_ok_and(|queue| queue.is_empty());
    if !empty
        || !inner.closing.load(Ordering::Acquire)
        || inner.thread_count.load(Ordering::Acquire) != 0
        || inner.finalized.swap(true, Ordering::AcqRel)
    {
        return false;
    }
    if inner.finalize_callback != 0 {
        let callback: unsafe extern "C" fn(NapiEnv, *mut c_void, *mut c_void) =
            unsafe { std::mem::transmute(inner.finalize_callback) };
        unsafe {
            callback(
                env,
                inner.finalize_data as *mut c_void,
                inner.context as *mut c_void,
            );
        }
    }
    if let Ok(mut js) = inner.js.lock() {
        js.function_bits = None;
    }
    if let Ok(mut registry) = TSFN_REGISTRY.lock() {
        registry.remove(&handle);
    }
    true
}

pub(crate) fn drain_threadsafe_functions() -> i32 {
    let current = std::thread::current().id();
    let snapshot = TSFN_REGISTRY
        .lock()
        .map(|registry| {
            registry
                .iter()
                .filter(|(_, inner)| inner.owner == current)
                .map(|(handle, inner)| (*handle, Arc::clone(inner)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut ran = 0i32;
    for (handle, inner) in snapshot {
        let Some(env) = owner_env(&inner) else {
            continue;
        };
        let aborted = inner.aborted.load(Ordering::Acquire);
        loop {
            let item = inner
                .queue
                .lock()
                .ok()
                .and_then(|mut queue| queue.pop_front());
            let Some(item) = item else {
                break;
            };
            inner.capacity.notify_one();
            invoke_item(&inner, env, item, aborted);
            ran = ran.saturating_add(1);
        }
        if maybe_finalize(handle, &inner, env) {
            ran = ran.saturating_add(1);
        }
    }
    ran
}

pub(crate) fn has_active_threadsafe_functions() -> bool {
    NODE_API_ENV.with(|cell| {
        cell.borrow().as_deref().is_some_and(|env| {
            env.tsfns.iter().any(|inner| {
                inner.referenced.load(Ordering::Acquire) && !inner.finalized.load(Ordering::Acquire)
            })
        })
    })
}

pub(crate) fn shutdown_threadsafe_functions(env: NapiEnv) {
    let functions = with_env(env, |env| env.tsfns.clone()).unwrap_or_default();
    for inner in functions {
        inner.aborted.store(true, Ordering::Release);
        inner.closing.store(true, Ordering::Release);
        inner.thread_count.store(0, Ordering::Release);
        inner.capacity.notify_all();
    }
}
