//! ES cyclic module evaluation. Records live beside the compiled module's
//! globals; dependency visitors name records directly, without a registry.
use crate::closure::body_call::js_bare_body_fn;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader, JsFunctionInfo, JsThis,
};
use crate::promise::{self, Promise};
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

const UNDEFINED: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);
const NEW: u64 = 0;
const EVALUATING: u64 = 1;
const ASYNC: u64 = 2;
const EVALUATED: u64 = 3;
const FAILED: u64 = 4;
const ASYNC_DONE: u64 = u64::MAX;
static ASYNC_ORDER: AtomicU64 = AtomicU64::new(1);

/// Layout mirrored by codegen's sixteen-word module global. Every JSValue
/// field is registered independently as a mutable global GC root.
#[repr(C)]
struct AsyncModule {
    body: usize,
    visit: usize,
    namespace: *mut f64,
    flags: u64,
    status: u64,
    dfs: u64,
    ancestor: u64,
    async_order: u64,
    pending: u64,
    cycle_root: *mut AsyncModule,
    parents: *mut Parent,
    capability: f64,
    error: f64,
    body_promise: f64,
    finish: usize,
    reserved: u64,
}
/// Size of the module record: the layout's owner states it for the census.
pub(crate) const MODULE_RECORD_SIZE: usize = std::mem::size_of::<AsyncModule>();
struct Parent {
    module: *mut AsyncModule,
    next: *mut Parent,
}
struct Walk {
    stack: Vec<*mut AsyncModule>,
    index: u64,
    failed: *mut AsyncModule,
}

unsafe fn store_root(slot: *mut f64, value: f64) {
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    slot.write(value);
}

fn promise_ptr(value: f64) -> *mut Promise {
    js_nanbox_get_pointer(value) as *mut Promise
}

/// # Safety
/// All addresses are emitted by the compiler; record storage lives for the
/// lifetime of this thread's module globals and starts zero-initialized.
#[no_mangle]
pub unsafe extern "C" fn js_async_module_configure(
    addr: i64,
    body: i64,
    visit: i64,
    namespace: i64,
    flags: u64,
    finish: i64,
) {
    let m = addr as *mut AsyncModule;
    if (*m).body != 0 {
        return;
    }
    (*m).body = body as usize;
    (*m).visit = visit as usize;
    (*m).namespace = namespace as *mut f64;
    (*m).flags = flags;
    (*m).finish = finish as usize;
    (*m).capability = UNDEFINED;
    (*m).error = UNDEFINED;
    (*m).body_promise = UNDEFINED;
    for slot in [
        ptr::addr_of_mut!((*m).capability),
        ptr::addr_of_mut!((*m).error),
        ptr::addr_of_mut!((*m).body_promise),
    ] {
        crate::gc::js_gc_register_global_root(slot as i64);
    }
}

unsafe fn fail(m: *mut AsyncModule, error: f64) {
    if (*m).status == FAILED {
        return;
    }
    store_root(ptr::addr_of_mut!((*m).error), error);
    (*m).status = FAILED;
    let mut link = (*m).parents;
    (*m).parents = ptr::null_mut();
    while !link.is_null() {
        let parent = Box::from_raw(link);
        fail(parent.module, (*m).error);
        link = parent.next;
    }
    if (*m).capability.to_bits() != UNDEFINED.to_bits() {
        promise::js_promise_reject(promise_ptr((*m).capability), (*m).error);
    }
}

unsafe fn run_body(m: *mut AsyncModule) -> Result<f64, crate::exception::Caught> {
    let body = js_bare_body_fn!((*m).body as *const u8;);
    // A record evaluation is one census init, as in the guarded path. It
    // closes at the body's synchronous return: a top-level await's
    // continuation is a job row.
    #[cfg(perry_gc_instruments)]
    crate::snap_census::record_enter(m as usize, (*m).body, (*m).reserved as u8);
    let result = crate::exception::catch_thrown(|| body());
    #[cfg(perry_gc_instruments)]
    crate::snap_census::init_exit(result.is_err());
    result
}

unsafe fn handler(
    info: &'static JsFunctionInfo,
    m: *mut AsyncModule,
    result: f64,
) -> *const ClosureHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(result);
    let closure = js_closure_alloc(info, 2);
    js_closure_set_capture_ptr(closure, 0, m as i64);
    js_closure_set_capture_f64(closure, 1, result.get_nanbox_f64());
    closure
}

unsafe fn execute_async(m: *mut AsyncModule) {
    // ExecuteAsyncModule starts immediately and only its continuation is a
    // promise job. In particular, it never pumps while DFS visits siblings.
    let promise = match run_body(m) {
        Ok(promise) => promise,
        Err(error) => js_nanbox_pointer(promise::js_promise_rejected(error.value()) as i64),
    };
    {
        store_root(ptr::addr_of_mut!((*m).body_promise), promise);
        let scope = crate::gc::RuntimeHandleScope::new();
        let fulfilled =
            scope.root_nanbox_f64(js_nanbox_pointer(handler(&FULFILLED, m, UNDEFINED) as i64));
        let rejected = handler(&REJECTED, m, UNDEFINED);
        promise::then::js_promise_attach_handlers(
            promise_ptr((*m).body_promise),
            js_nanbox_get_pointer(fulfilled.get_nanbox_f64()) as *const ClosureHeader,
            rejected,
        );
    }
}

unsafe fn inner(w: *mut Walk, m: *mut AsyncModule) {
    if !(*w).failed.is_null() {
        return;
    }
    if (*m).status == FAILED {
        (*w).failed = m;
        return;
    }
    if (*m).status != NEW {
        return;
    }
    (*m).status = EVALUATING;
    // The walk that first reaches a record decides its census reach.
    #[cfg(perry_gc_instruments)]
    {
        (*m).reserved = crate::snap_census::walk_origin() as u64;
    }
    (*m).dfs = (*w).index;
    (*m).ancestor = (*w).index;
    (*w).index += 1;
    (*w).stack.push(m);
    let visit: extern "C" fn(i64, i64) = std::mem::transmute((*m).visit);
    visit(w as i64, m as i64);
    if !(*w).failed.is_null() {
        return;
    }
    if (*m).flags & 1 != 0 || (*m).pending != 0 {
        (*m).async_order = ASYNC_ORDER.fetch_add(1, Ordering::Relaxed);
        if (*m).pending == 0 {
            execute_async(m);
        }
    } else if let Err(error) = run_body(m) {
        fail(m, error.value());
        (*w).failed = m;
        return;
    }
    if (*m).ancestor == (*m).dfs {
        loop {
            let member = (*w).stack.pop().expect("module evaluation stack");
            if (*member).status != FAILED {
                (*member).status = if (*member).async_order == 0 {
                    EVALUATED
                } else {
                    ASYNC
                };
                #[cfg(perry_gc_instruments)]
                crate::snap_census::record_settle(member as usize, (*member).status == ASYNC);
            }
            (*member).cycle_root = m;
            if member == m {
                break;
            }
        }
    }
}

/// One [[RequestedModules]] edge, emitted in source order by codegen.
/// # Safety
/// The walk is the live InnerModuleEvaluation stack; both records are emitted.
#[no_mangle]
pub unsafe extern "C" fn js_async_module_visit(w: i64, parent: i64, dependency: i64) {
    let w = w as *mut Walk;
    let parent = parent as *mut AsyncModule;
    let mut dep = dependency as *mut AsyncModule;
    inner(w, dep);
    if !(*w).failed.is_null() {
        return;
    }
    if (*dep).status == EVALUATING {
        (*parent).ancestor = (*parent).ancestor.min((*dep).ancestor);
    } else {
        dep = (*dep).cycle_root;
        if (*dep).status == FAILED {
            (*w).failed = dep;
            return;
        }
    }
    if (*dep).async_order != 0 && (*dep).async_order != ASYNC_DONE {
        (*parent).pending += 1;
        let link = Box::into_raw(Box::new(Parent {
            module: parent,
            next: ptr::null_mut(),
        }));
        // Parent insertion order also determines rejection capability order.
        let mut tail = ptr::addr_of_mut!((*dep).parents);
        while !(*tail).is_null() {
            tail = ptr::addr_of_mut!((**tail).next);
        }
        *tail = link;
    }
}

unsafe fn evaluate(mut m: *mut AsyncModule) -> *mut AsyncModule {
    if (*m).status != NEW && (*m).status != EVALUATING && !(*m).cycle_root.is_null() {
        m = (*m).cycle_root;
    }
    if (*m).capability.to_bits() == UNDEFINED.to_bits() {
        store_root(
            ptr::addr_of_mut!((*m).capability),
            js_nanbox_pointer(promise::js_promise_new() as i64),
        );
        // The host owns this hidden evaluation capability. Only import()'s
        // returned promise can be an unhandled user rejection.
        let rejected = handler(&REJECTED, m, UNDEFINED);
        promise::then::js_promise_attach_handlers(
            promise_ptr((*m).capability),
            ptr::null(),
            rejected,
        );
    }
    if (*m).status == NEW {
        let mut walk = Walk {
            stack: Vec::new(),
            index: 0,
            failed: ptr::null_mut(),
        };
        inner(&mut walk, m);
        if !walk.failed.is_null() {
            for member in walk.stack.drain(..) {
                fail(member, (*walk.failed).error);
            }
            fail(m, (*walk.failed).error);
        }
    }
    if (*m).status == EVALUATED {
        promise::js_promise_resolve(promise_ptr((*m).capability), UNDEFINED);
    } else if (*m).status == FAILED {
        promise::js_promise_reject(promise_ptr((*m).capability), (*m).error);
    }
    m
}

unsafe fn gather(m: *mut AsyncModule, available: &mut Vec<*mut AsyncModule>) {
    let mut link = (*m).parents;
    while !link.is_null() {
        let a = (*link).module;
        link = (*link).next;
        let root = (*a).cycle_root;
        if (*a).status == FAILED
            || (!root.is_null() && (*root).status == FAILED)
            || available.contains(&a)
        {
            continue;
        }
        (*a).pending -= 1;
        if (*a).pending == 0 {
            available.push(a);
            if (*a).flags & 1 == 0 {
                gather(a, available);
            }
        }
    }
}

extern "C" fn fulfilled(c: *const ClosureHeader, _this: JsThis, _value: f64) -> f64 {
    unsafe {
        let m = js_closure_get_capture_ptr(c, 0) as *mut AsyncModule;
        if (*m).status == FAILED {
            return UNDEFINED;
        }
        if (*m).finish != 0 {
            let finish = js_bare_body_fn!((*m).finish as *const u8;);
            if let Err(error) = crate::exception::catch_thrown(|| finish()) {
                fail(m, error.value());
                return UNDEFINED;
            }
        }
        complete(m);
        let mut available = Vec::new();
        gather(m, &mut available);
        available.sort_by_key(|a| (**a).async_order);
        for a in available {
            if (*a).status == FAILED {
                continue;
            }
            if (*a).flags & 1 != 0 {
                execute_async(a);
            } else {
                match run_body(a) {
                    Ok(_) => complete(a),
                    Err(error) => fail(a, error.value()),
                }
            }
        }
    }
    UNDEFINED
}
unsafe fn complete(m: *mut AsyncModule) {
    (*m).async_order = ASYNC_DONE;
    (*m).status = EVALUATED;
    if (*m).capability.to_bits() != UNDEFINED.to_bits() {
        promise::js_promise_resolve(promise_ptr((*m).capability), UNDEFINED);
    }
}
extern "C" fn rejected(c: *const ClosureHeader, _this: JsThis, value: f64) -> f64 {
    unsafe {
        fail(js_closure_get_capture_ptr(c, 0) as *mut AsyncModule, value);
    }
    UNDEFINED
}

extern "C" fn import_job(c: *const ClosureHeader, _this: JsThis, _value: f64) -> f64 {
    unsafe {
        let m = js_closure_get_capture_ptr(c, 0) as *mut AsyncModule;
        let scope = crate::gc::RuntimeHandleScope::new();
        let result = scope.root_nanbox_f64(js_closure_get_capture_f64(c, 1));
        #[cfg(perry_gc_instruments)]
        let _walk = crate::snap_census::walk_scope(crate::snap_census::WALK_IMPORT, m as usize);
        let root = evaluate(m);
        let fulfilled = scope.root_nanbox_f64(js_nanbox_pointer(handler(
            &IMPORT_FULFILLED,
            m,
            result.get_nanbox_f64(),
        ) as i64));
        let rejected = handler(&IMPORT_REJECTED, m, result.get_nanbox_f64());
        promise::then::js_promise_attach_handlers(
            promise_ptr((*root).capability),
            js_nanbox_get_pointer(fulfilled.get_nanbox_f64()) as *const ClosureHeader,
            rejected,
        );
    }
    UNDEFINED
}
extern "C" fn import_fulfilled(c: *const ClosureHeader, _this: JsThis, _value: f64) -> f64 {
    unsafe {
        let m = js_closure_get_capture_ptr(c, 0) as *mut AsyncModule;
        let result = js_closure_get_capture_f64(c, 1);
        let namespace = if (*m).namespace.is_null() {
            UNDEFINED
        } else {
            *(*m).namespace
        };
        promise::js_promise_resolve(promise_ptr(result), namespace);
    }
    UNDEFINED
}
extern "C" fn import_rejected(c: *const ClosureHeader, _this: JsThis, value: f64) -> f64 {
    let result = js_closure_get_capture_f64(c, 1);
    promise::js_promise_reject(promise_ptr(result), value);
    UNDEFINED
}
macro_rules! callback_info {
    ($name:ident, $body:ident) => {
        static $name: JsFunctionInfo =
            JsFunctionInfo::of($body as crate::codegen_abi::JsBody1<ClosureHeader>)
                .with_declared(1)
                .with_flags(crate::closure::FN_NON_CONSTRUCTOR);
    };
}
callback_info!(FULFILLED, fulfilled);
callback_info!(REJECTED, rejected);
callback_info!(IMPORT_JOB, import_job);
callback_info!(IMPORT_FULFILLED, import_fulfilled);
callback_info!(IMPORT_REJECTED, import_rejected);

/// # Safety
/// `addr` is the target module's emitted record, returned by its accessor.
#[no_mangle]
pub unsafe extern "C" fn js_async_module_import(addr: i64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(js_nanbox_pointer(promise::js_promise_new() as i64));
    let job = handler(
        &IMPORT_JOB,
        addr as *mut AsyncModule,
        result.get_nanbox_f64(),
    );
    crate::builtins::js_queue_microtask(job as i64);
    result.get_nanbox_f64()
}

unsafe fn settled(m: *mut AsyncModule) -> bool {
    let status = (*m).status;
    status == EVALUATED || status == FAILED
}

/// # Safety
/// `addr` is the compiled entry module's record.
#[no_mangle]
pub unsafe extern "C" fn js_async_module_entry(addr: i64) {
    #[cfg(perry_gc_instruments)]
    let walk = {
        crate::snap_census::entry_record(addr as usize);
        crate::snap_census::walk_scope(crate::snap_census::WALK_ENTRY, addr as usize)
    };
    let m = evaluate(addr as *mut AsyncModule);
    #[cfg(perry_gc_instruments)]
    drop(walk);
    // Entry evaluation owns the host's startup await. Dependency traversal
    // has finished before the first checkpoint can resume a suspended body.
    // Each turn is the one the synchronous `await` loop runs (fs_await.rs):
    // microtasks, the stdlib pump, then the timers, poll and check phases. A
    // callback-style native completion (`fs.readdir(path, cb)`) is delivered
    // only by the poll phase, so a turn without it never settles the body.
    // The record's status changes only inside the callbacks this loop runs.
    while !settled(m) {
        crate::event_pump::perry_poll();
        crate::timer::js_await_loop_tick_timers();
        if settled(m) {
            break;
        }
        // `perry_has_work` sees timer deadlines but not the check and poll
        // queues; a queued completion callback is live work, as it is for the
        // synchronous `await` loop's liveness test.
        if crate::event_pump::perry_has_work() == 0
            && crate::timer::js_callback_timer_has_pending() == 0
        {
            crate::event_pump::js_unsettled_top_level_await_exit();
        }
        crate::event_pump::js_wait_for_event();
    }
    if (*m).status == FAILED {
        crate::exception::throw_open((*m).error);
    }
}

/// # Safety
/// `addr` is the required compiled module's record.
#[no_mangle]
pub unsafe extern "C" fn js_async_module_require(addr: i64) {
    let m = addr as *mut AsyncModule;
    if (*m).flags & 2 != 0 {
        crate::fs::validate::throw_error_with_code(
            "require() cannot load an ESM graph with top-level await",
            "ERR_REQUIRE_ASYNC_MODULE",
        );
    }
    #[cfg(perry_gc_instruments)]
    let _walk = crate::snap_census::walk_scope(crate::snap_census::WALK_REQUIRE, m as usize);
    let m = evaluate(m);
    if (*m).status == FAILED {
        crate::exception::throw_open((*m).error);
    }
}

#[cfg(test)]
mod tests;
