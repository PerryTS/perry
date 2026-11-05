//! Installer-filled async lifecycle operations for ordinary runtime paths.
use crate::closure::ClosureHeader;
use crate::feature_hooks::Hook;
use crate::ObjectHeader;
#[derive(Clone, Copy)]
pub struct AsyncResourceIds {
    pub async_id: u64,
    pub trigger_async_id: u64,
}
#[inline(always)]
pub fn try_enter_resource_scope(ids: AsyncResourceIds) -> Result<(), f64> {
    try_enter_resource_scope_at(crate::exception::frame_mark(), ids)
}
#[inline(always)]
pub fn enter_resource_scope(ids: AsyncResourceIds) {
    if let Err(error) = try_enter_resource_scope(ids) {
        crate::exception::js_throw(error);
    }
}

static PROVIDER_ENTER_AT: Hook<fn(usize, u64)> = Hook::empty();
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_async_hooks_provider_enter(async_id: u64) {
    let mark = crate::exception::frame_mark();
    if let Some(enter) = PROVIDER_ENTER_AT.get() {
        enter(mark, async_id);
    }
}
fn callback_without_hooks(
    receiver: Option<f64>,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    let outcome = {
        let scope = crate::gc::RuntimeHandleScope::new();
        let _receiver = receiver.map(|value| scope.root_nanbox_f64(value));
        crate::exception::js_call_catching(|| unsafe { callback(data) })
    };
    match outcome {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

static ADOPT_ASYNC_RESOURCE_PROTOTYPE: Hook<fn(*mut ObjectHeader) -> *mut ObjectHeader> =
    Hook::empty();
#[inline]
pub fn adopt_async_resource_prototype(proto: *mut ObjectHeader) -> *mut ObjectHeader {
    ADOPT_ASYNC_RESOURCE_PROTOTYPE
        .get()
        .map_or_else(|| proto, |f| f(proto))
}

static AFTER: Hook<fn(u64)> = Hook::empty();
#[inline]
pub fn after(async_id: u64) {
    AFTER.get().map_or_else(|| (), |f| f(async_id))
}

static AFTER_PROMISE: Hook<fn(u64)> = Hook::empty();
#[inline]
pub fn after_promise(async_id: u64) {
    AFTER_PROMISE.get().map_or_else(|| (), |f| f(async_id))
}

static ASYNC_LOCAL_STORAGE_STATIC_SNAPSHOT_VALUE: Hook<fn() -> f64> = Hook::empty();
#[inline]
pub fn async_local_storage_static_snapshot_value() -> f64 {
    ASYNC_LOCAL_STORAGE_STATIC_SNAPSHOT_VALUE
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f())
}

static BEFORE: Hook<fn(u64, u64)> = Hook::empty();
#[inline]
pub fn before(async_id: u64, trigger_async_id: u64) {
    BEFORE
        .get()
        .map_or_else(|| (), |f| f(async_id, trigger_async_id))
}

static BEFORE_PROMISE: Hook<fn(u64, u64)> = Hook::empty();
#[inline]
pub fn before_promise(async_id: u64, trigger_async_id: u64) {
    BEFORE_PROMISE
        .get()
        .map_or_else(|| (), |f| f(async_id, trigger_async_id))
}

static BEFORE_SCHEDULED: Hook<fn(u64, u64, f64)> = Hook::empty();
#[inline]
pub fn before_scheduled(async_id: u64, trigger_async_id: u64, resource: f64) {
    BEFORE_SCHEDULED
        .get()
        .map_or_else(|| (), |f| f(async_id, trigger_async_id, resource))
}

static DEFER_DESTROY_AFTER_CHECK_TURNS: Hook<fn(u64, u32)> = Hook::empty();
#[inline]
pub fn defer_destroy_after_check_turns(async_id: u64, check_turns: u32) {
    DEFER_DESTROY_AFTER_CHECK_TURNS
        .get()
        .map_or_else(|| (), |f| f(async_id, check_turns))
}

static DESTROY: Hook<fn(u64)> = Hook::empty();
#[inline]
pub fn destroy(async_id: u64) {
    DESTROY.get().map_or_else(|| (), |f| f(async_id))
}

static DESTROY_SCHEDULED: Hook<fn(u64)> = Hook::empty();
#[inline]
pub fn destroy_scheduled(async_id: u64) {
    DESTROY_SCHEDULED.get().map_or_else(|| (), |f| f(async_id))
}

static DRAIN_GC_DESTROY_QUEUE: Hook<fn() -> i32> = Hook::empty();
#[inline]
pub fn drain_gc_destroy_queue() -> i32 {
    DRAIN_GC_DESTROY_QUEUE.get().map_or_else(|| 0, |f| f())
}

static ENQUEUE_GC_DESTROY: Hook<fn(u64)> = Hook::empty();
#[inline]
pub fn enqueue_gc_destroy(async_id: u64) {
    ENQUEUE_GC_DESTROY.get().map_or_else(|| (), |f| f(async_id))
}

static EXECUTION_ASYNC_ID_U64: Hook<fn() -> u64> = Hook::empty();
#[inline]
pub fn execution_async_id_u64() -> u64 {
    EXECUTION_ASYNC_ID_U64.get().map_or_else(|| 1, |f| f())
}

static GC_DESTROY_WORK_PENDING: Hook<fn() -> bool> = Hook::empty();
#[inline]
pub fn gc_destroy_work_pending() -> bool {
    GC_DESTROY_WORK_PENDING.get().map_or_else(|| false, |f| f())
}

static INIT_ESM_EVALUATION_PROMISE: Hook<fn()> = Hook::empty();
#[inline]
pub fn init_esm_evaluation_promise() {
    INIT_ESM_EVALUATION_PROMISE
        .get()
        .map_or_else(|| (), |f| f())
}

static INIT_RESOURCE: Hook<fn(&str, f64, bool) -> AsyncResourceIds> = Hook::empty();
#[inline]
pub fn init_resource(type_name: &str, resource: f64, force_allocate: bool) -> AsyncResourceIds {
    INIT_RESOURCE.get().map_or_else(
        || AsyncResourceIds {
            async_id: 0,
            trigger_async_id: 0,
        },
        |f| f(type_name, resource, force_allocate),
    )
}

static INIT_RESOURCE_WITH_TRIGGER: Hook<fn(&str, f64, bool, u64) -> AsyncResourceIds> =
    Hook::empty();
#[inline]
pub fn init_resource_with_trigger(
    type_name: &str,
    resource: f64,
    force_allocate: bool,
    trigger_async_id: u64,
) -> AsyncResourceIds {
    INIT_RESOURCE_WITH_TRIGGER.get().map_or_else(
        || AsyncResourceIds {
            async_id: 0,
            trigger_async_id: trigger_async_id,
        },
        |f| f(type_name, resource, force_allocate, trigger_async_id),
    )
}

static INIT_SCHEDULED_RESOURCE: Hook<fn(&str, f64, Option<u64>) -> AsyncResourceIds> =
    Hook::empty();
#[inline]
pub fn init_scheduled_resource(
    type_name: &str,
    resource: f64,
    trigger_async_id: Option<u64>,
) -> AsyncResourceIds {
    INIT_SCHEDULED_RESOURCE.get().map_or_else(
        || AsyncResourceIds {
            async_id: 0,
            trigger_async_id: 0,
        },
        |f| f(type_name, resource, trigger_async_id),
    )
}

static JS_ASYNC_HOOKS_ASYNC_WRAP_PROVIDERS: Hook<fn() -> f64> = Hook::empty();
#[inline]
pub fn js_async_hooks_async_wrap_providers() -> f64 {
    JS_ASYNC_HOOKS_ASYNC_WRAP_PROVIDERS
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f())
}

static JS_ASYNC_HOOKS_CREATE_HOOK: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_hooks_create_hook(options: f64) -> f64 {
    JS_ASYNC_HOOKS_CREATE_HOOK.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(options),
    )
}

static JS_ASYNC_HOOKS_EXECUTION_ASYNC_ID: Hook<extern "C" fn() -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_hooks_execution_async_id() -> f64 {
    JS_ASYNC_HOOKS_EXECUTION_ASYNC_ID
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f())
}

static JS_ASYNC_HOOKS_EXECUTION_ASYNC_RESOURCE: Hook<extern "C" fn() -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_hooks_execution_async_resource() -> f64 {
    JS_ASYNC_HOOKS_EXECUTION_ASYNC_RESOURCE
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f())
}

static JS_ASYNC_HOOKS_OWNED_PROVIDER_DESTROY: Hook<extern "C" fn(f64)> = Hook::empty();
#[no_mangle]
pub extern "C" fn js_async_hooks_owned_provider_destroy(resource: f64) {
    JS_ASYNC_HOOKS_OWNED_PROVIDER_DESTROY
        .get()
        .map_or_else(|| (), |f| f(resource))
}

static JS_ASYNC_HOOKS_OWNED_PROVIDER_EMIT_INIT: Hook<extern "C" fn(f64)> = Hook::empty();
#[no_mangle]
pub extern "C" fn js_async_hooks_owned_provider_emit_init(resource: f64) {
    JS_ASYNC_HOOKS_OWNED_PROVIDER_EMIT_INIT
        .get()
        .map_or_else(|| (), |f| f(resource))
}

static JS_ASYNC_HOOKS_OWNED_PROVIDER_NEW: Hook<unsafe extern "C" fn(*const u8, usize, u64) -> f64> =
    Hook::empty();
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_owned_provider_new(
    type_ptr: *const u8,
    type_len: usize,
    trigger_async_id: u64,
) -> f64 {
    JS_ASYNC_HOOKS_OWNED_PROVIDER_NEW.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| unsafe { f(type_ptr, type_len, trigger_async_id) },
    )
}

static JS_ASYNC_HOOKS_PROVIDER_DEFER_DESTROY: Hook<extern "C" fn(u64, u32)> = Hook::empty();
#[no_mangle]
pub extern "C" fn js_async_hooks_provider_defer_destroy(async_id: u64, check_turns: u32) {
    JS_ASYNC_HOOKS_PROVIDER_DEFER_DESTROY
        .get()
        .map_or_else(|| (), |f| f(async_id, check_turns))
}

static JS_ASYNC_HOOKS_PROVIDER_DESTROY: Hook<extern "C" fn(u64)> = Hook::empty();
#[no_mangle]
pub extern "C" fn js_async_hooks_provider_destroy(async_id: u64) {
    JS_ASYNC_HOOKS_PROVIDER_DESTROY
        .get()
        .map_or_else(|| (), |f| f(async_id))
}

static JS_ASYNC_HOOKS_PROVIDER_INIT: Hook<unsafe extern "C" fn(*const u8, usize) -> u64> =
    Hook::empty();
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_init(type_ptr: *const u8, type_len: usize) -> u64 {
    JS_ASYNC_HOOKS_PROVIDER_INIT
        .get()
        .map_or_else(|| 0, |f| unsafe { f(type_ptr, type_len) })
}

static JS_ASYNC_HOOKS_PROVIDER_INIT_WITH_TRIGGER: Hook<
    unsafe extern "C" fn(*const u8, usize, u64) -> u64,
> = Hook::empty();
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_init_with_trigger(
    type_ptr: *const u8,
    type_len: usize,
    trigger_async_id: u64,
) -> u64 {
    JS_ASYNC_HOOKS_PROVIDER_INIT_WITH_TRIGGER
        .get()
        .map_or_else(|| 0, |f| unsafe { f(type_ptr, type_len, trigger_async_id) })
}

static JS_ASYNC_HOOKS_PROVIDER_LEAVE: Hook<extern "C" fn(u64)> = Hook::empty();
#[no_mangle]
pub extern "C" fn js_async_hooks_provider_leave(async_id: u64) {
    JS_ASYNC_HOOKS_PROVIDER_LEAVE
        .get()
        .map_or_else(|| (), |f| f(async_id))
}

static JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING: Hook<
    unsafe extern "C" fn(
        u64,
        unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
        *mut std::ffi::c_void,
    ) -> f64,
> = Hook::empty();
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching(
    async_id: u64,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING.get().map_or_else(
        || callback_without_hooks(None, callback, data),
        |f| unsafe { f(async_id, callback, data) },
    )
}

static JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_DEFERRED_DESTROY: Hook<
    unsafe extern "C" fn(
        u64,
        u32,
        unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
        *mut std::ffi::c_void,
    ) -> f64,
> = Hook::empty();
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching_deferred_destroy(
    async_id: u64,
    check_turns: u32,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_DEFERRED_DESTROY
        .get()
        .map_or_else(
            || callback_without_hooks(None, callback, data),
            |f| unsafe { f(async_id, check_turns, callback, data) },
        )
}

static JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_DEFERRED_DESTROY_ON_ERROR: Hook<
    unsafe extern "C" fn(
        u64,
        u32,
        unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
        *mut std::ffi::c_void,
    ) -> f64,
> = Hook::empty();
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching_deferred_destroy_on_error(
    async_id: u64,
    check_turns: u32,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_DEFERRED_DESTROY_ON_ERROR
        .get()
        .map_or_else(
            || callback_without_hooks(None, callback, data),
            |f| unsafe { f(async_id, check_turns, callback, data) },
        )
}

static JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_WITH_THIS: Hook<
    unsafe extern "C" fn(
        u64,
        f64,
        i32,
        unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
        *mut std::ffi::c_void,
    ) -> f64,
> = Hook::empty();
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching_with_this(
    async_id: u64,
    this_value: f64,
    destroy_after: i32,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_WITH_THIS
        .get()
        .map_or_else(
            || callback_without_hooks(Some(this_value), callback, data),
            |f| unsafe { f(async_id, this_value, destroy_after, callback, data) },
        )
}

static JS_ASYNC_HOOKS_TRIGGER_ASYNC_ID: Hook<extern "C" fn() -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_hooks_trigger_async_id() -> f64 {
    JS_ASYNC_HOOKS_TRIGGER_ASYNC_ID
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f())
}

static JS_ASYNC_LOCAL_STORAGE_STATIC_BIND_METHOD: Hook<
    extern "C" fn(*const ClosureHeader, crate::closure::JsThis, f64, f64) -> f64,
> = Hook::empty();
#[inline]
pub extern "C" fn js_async_local_storage_static_bind_method(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    callback_value: f64,
    _rest: f64,
) -> f64 {
    JS_ASYNC_LOCAL_STORAGE_STATIC_BIND_METHOD.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(_closure, _this, callback_value, _rest),
    )
}

static JS_ASYNC_LOCAL_STORAGE_STATIC_SNAPSHOT_METHOD: Hook<
    extern "C" fn(*const ClosureHeader, crate::closure::JsThis, f64) -> f64,
> = Hook::empty();
#[inline]
pub extern "C" fn js_async_local_storage_static_snapshot_method(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _rest: f64,
) -> f64 {
    JS_ASYNC_LOCAL_STORAGE_STATIC_SNAPSHOT_METHOD
        .get()
        .map_or_else(
            || f64::from_bits(crate::value::TAG_UNDEFINED),
            |f| f(_closure, _this, _rest),
        )
}

static JS_ASYNC_RESOURCE_ASYNC_ID: Hook<extern "C" fn(i64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_resource_async_id(handle: i64) -> f64 {
    JS_ASYNC_RESOURCE_ASYNC_ID.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(handle),
    )
}

static JS_ASYNC_RESOURCE_EMIT_DESTROY: Hook<extern "C" fn(i64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_resource_emit_destroy(handle: i64) -> f64 {
    JS_ASYNC_RESOURCE_EMIT_DESTROY.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(handle),
    )
}

static JS_ASYNC_RESOURCE_NEW: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_resource_new(type_value: f64, options: f64) -> f64 {
    JS_ASYNC_RESOURCE_NEW.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(type_value, options),
    )
}

static JS_ASYNC_RESOURCE_RUNTIME_ASYNC_ID: Hook<extern "C" fn(i64) -> u64> = Hook::empty();
#[no_mangle]
pub extern "C" fn js_async_resource_runtime_async_id(handle: i64) -> u64 {
    JS_ASYNC_RESOURCE_RUNTIME_ASYNC_ID
        .get()
        .map_or_else(|| 0, |f| f(handle))
}

static JS_ASYNC_RESOURCE_SET_EVENT_EMITTER: Hook<extern "C" fn(i64, i64)> = Hook::empty();
#[inline]
pub extern "C" fn js_async_resource_set_event_emitter(handle: i64, event_emitter: i64) {
    JS_ASYNC_RESOURCE_SET_EVENT_EMITTER
        .get()
        .map_or_else(|| (), |f| f(handle, event_emitter))
}

static JS_ASYNC_RESOURCE_STATIC_BIND_METHOD: Hook<
    extern "C" fn(*const ClosureHeader, crate::closure::JsThis, f64, f64, f64, f64) -> f64,
> = Hook::empty();
#[inline]
pub extern "C" fn js_async_resource_static_bind_method(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    callback_value: f64,
    type_value: f64,
    this_arg: f64,
    _rest: f64,
) -> f64 {
    JS_ASYNC_RESOURCE_STATIC_BIND_METHOD.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(_closure, _this, callback_value, type_value, this_arg, _rest),
    )
}

static JS_ASYNC_RESOURCE_SUBCLASS_INIT: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_resource_subclass_init(
    this_value: f64,
    type_value: f64,
    options: f64,
) -> f64 {
    JS_ASYNC_RESOURCE_SUBCLASS_INIT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(this_value, type_value, options),
    )
}

static JS_ASYNC_RESOURCE_TRIGGER_ASYNC_ID: Hook<extern "C" fn(i64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn js_async_resource_trigger_async_id(handle: i64) -> f64 {
    JS_ASYNC_RESOURCE_TRIGGER_ASYNC_ID.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(handle),
    )
}

static LEAVE_RESOURCE_SCOPE: Hook<fn(u64)> = Hook::empty();
#[inline]
pub fn leave_resource_scope(async_id: u64) {
    LEAVE_RESOURCE_SCOPE
        .get()
        .map_or_else(|| (), |f| f(async_id))
}

static PROMISE_HOOKS_ACTIVE: Hook<fn() -> bool> = Hook::empty();
#[inline]
pub fn promise_hooks_active() -> bool {
    PROMISE_HOOKS_ACTIVE.get().map_or_else(|| false, |f| f())
}

static PROMISE_RESOLVE: Hook<fn(u64)> = Hook::empty();
#[inline]
pub fn promise_resolve(async_id: u64) {
    PROMISE_RESOLVE.get().map_or_else(|| (), |f| f(async_id))
}

static RELEASE_ASYNC_HOOKS_IN_FREED_RANGES: Hook<fn(&crate::arena::thread_exit::FreedRanges)> =
    Hook::empty();
#[inline]
pub fn release_async_hooks_in_freed_ranges(freed: &crate::arena::thread_exit::FreedRanges) {
    RELEASE_ASYNC_HOOKS_IN_FREED_RANGES
        .get()
        .map_or_else(|| (), |f| f(freed))
}

static RESERVE_RESOURCE_IDS: Hook<fn(u64) -> AsyncResourceIds> = Hook::empty();
#[inline]
pub fn reserve_resource_ids(trigger_async_id: u64) -> AsyncResourceIds {
    RESERVE_RESOURCE_IDS.get().map_or_else(
        || AsyncResourceIds {
            async_id: 0,
            trigger_async_id: trigger_async_id,
        },
        |f| f(trigger_async_id),
    )
}

static RESOLVE_ASYNC_RESOURCE_HANDLE: Hook<fn(i64) -> Option<i64>> = Hook::empty();
#[inline]
pub fn resolve_async_resource_handle(receiver: i64) -> Option<i64> {
    RESOLVE_ASYNC_RESOURCE_HANDLE
        .get()
        .map_or_else(|| None, |f| f(receiver))
}

static SCAN_ASYNC_HOOKS_ROOTS: Hook<fn(&mut dyn FnMut(f64))> = Hook::empty();
#[inline]
pub fn scan_async_hooks_roots(mark: &mut dyn FnMut(f64)) {
    SCAN_ASYNC_HOOKS_ROOTS.get().map_or_else(|| (), |f| f(mark))
}

static SCAN_ASYNC_HOOKS_ROOTS_MUT: Hook<fn(&mut crate::gc::RuntimeRootVisitor<'_>)> = Hook::empty();
#[inline]
pub fn scan_async_hooks_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SCAN_ASYNC_HOOKS_ROOTS_MUT
        .get()
        .map_or_else(|| (), |f| f(visitor))
}

static TRY_ENTER_RESOURCE_SCOPE_AT: Hook<fn(usize, AsyncResourceIds) -> Result<(), f64>> =
    Hook::empty();
#[inline]
pub fn try_enter_resource_scope_at(mark: usize, ids: AsyncResourceIds) -> Result<(), f64> {
    TRY_ENTER_RESOURCE_SCOPE_AT
        .get()
        .map_or_else(|| Ok(()), |f| f(mark, ids))
}

static TRY_LEAVE_RESOURCE_SCOPE: Hook<fn(u64) -> Result<(), f64>> = Hook::empty();
#[inline]
pub fn try_leave_resource_scope(async_id: u64) -> Result<(), f64> {
    TRY_LEAVE_RESOURCE_SCOPE
        .get()
        .map_or_else(|| Ok(()), |f| f(async_id))
}

static UNWIND_EXECUTION_SCOPE: Hook<fn()> = Hook::empty();
#[inline]
pub fn unwind_execution_scope() {
    UNWIND_EXECUTION_SCOPE.get().map_or_else(|| (), |f| f())
}

type Callback = unsafe fn(*mut ()) -> f64;
unsafe fn invoke_once<F: FnOnce() -> f64>(context: *mut ()) -> f64 {
    (&mut *context.cast::<Option<F>>())
        .take()
        .expect("async callback invoked once")()
}
fn with_callback<F: FnOnce() -> f64, R>(
    callback: F,
    run: impl FnOnce(*mut (), Callback) -> R,
) -> R {
    let mut callback = Some(callback);
    run((&mut callback as *mut Option<F>).cast(), invoke_once::<F>)
}

static OWNED_PROVIDER_SCOPE: Hook<unsafe fn(f64, *mut (), Callback) -> Result<f64, f64>> =
    Hook::empty();
pub fn owned_provider_scope(resource: f64, callback: impl FnOnce() -> f64) -> Result<f64, f64> {
    let Some(run) = OWNED_PROVIDER_SCOPE.get() else {
        return crate::exception::js_call_catching(callback);
    };

    with_callback(callback, |context, callback| unsafe {
        run(resource, context, callback)
    })
}

static OWNED_PROVIDER_SCOPE_ROOTED: Hook<
    unsafe fn(
        &crate::gc::RuntimeHandleScope,
        &crate::gc::RuntimeHandle<'_>,
        *mut (),
        Callback,
    ) -> Result<f64, f64>,
> = Hook::empty();
pub fn owned_provider_scope_rooted(
    scope: &crate::gc::RuntimeHandleScope,
    owner: &crate::gc::RuntimeHandle<'_>,
    callback: impl FnOnce() -> f64,
) -> Result<f64, f64> {
    let Some(run) = OWNED_PROVIDER_SCOPE_ROOTED.get() else {
        return crate::exception::js_call_catching(callback);
    };

    with_callback(callback, |context, callback| unsafe {
        run(scope, owner, context, callback)
    })
}

static RUN_PROVIDER_COMPLETION: Hook<unsafe fn(&'static str, *mut (), Callback) -> f64> =
    Hook::empty();
pub fn run_provider_completion(type_name: &'static str, callback: impl FnOnce() -> f64) -> f64 {
    let Some(run) = RUN_PROVIDER_COMPLETION.get() else {
        return callback();
    };

    with_callback(callback, |context, callback| unsafe {
        run(type_name, context, callback)
    })
}

static RUN_RESOURCE_SCOPE: Hook<unsafe fn(AsyncResourceIds, *mut (), Callback) -> ()> =
    Hook::empty();
pub fn run_resource_scope(ids: AsyncResourceIds, callback: impl FnOnce()) -> () {
    let Some(run) = RUN_RESOURCE_SCOPE.get() else {
        return callback();
    };
    let callback = || {
        callback();
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    with_callback(callback, |context, callback| unsafe {
        run(ids, context, callback)
    })
}

static RUN_RESOURCE_SCOPE_CATCHING: Hook<unsafe fn(AsyncResourceIds, *mut (), Callback) -> f64> =
    Hook::empty();
pub fn run_resource_scope_catching(ids: AsyncResourceIds, callback: impl FnOnce() -> f64) -> f64 {
    let Some(run) = RUN_RESOURCE_SCOPE_CATCHING.get() else {
        return callback();
    };

    with_callback(callback, |context, callback| unsafe {
        run(ids, context, callback)
    })
}

static RUN_WITH_CONTEXT_SNAPSHOT: Hook<unsafe fn(*const ClosureHeader, *mut (), Callback) -> f64> =
    Hook::empty();
pub fn run_with_context_snapshot(
    closure: *const ClosureHeader,
    callback: impl FnOnce() -> f64,
) -> f64 {
    let Some(run) = RUN_WITH_CONTEXT_SNAPSHOT.get() else {
        return callback();
    };

    with_callback(callback, |context, callback| unsafe {
        run(closure, context, callback)
    })
}

static REGISTER_ROOTS: Hook<fn()> = Hook::empty();
pub(crate) fn register_installed_roots() {
    if let Some(register) = REGISTER_ROOTS.peek() {
        register();
    }
}
pub(crate) fn install() {
    crate::perry_install_once!({
        ADOPT_ASYNC_RESOURCE_PROTOTYPE.set(crate::async_hooks::adopt_async_resource_prototype);
        AFTER.set(crate::async_hooks::after);
        AFTER_PROMISE.set(crate::async_hooks::after_promise);
        ASYNC_LOCAL_STORAGE_STATIC_SNAPSHOT_VALUE
            .set(crate::async_hooks::async_local_storage_static_snapshot_value);
        BEFORE.set(crate::async_hooks::before);
        BEFORE_PROMISE.set(crate::async_hooks::before_promise);
        BEFORE_SCHEDULED.set(crate::async_hooks::before_scheduled);
        DEFER_DESTROY_AFTER_CHECK_TURNS
            .set(crate::async_hooks::provider_ffi::defer_destroy_after_check_turns);
        DESTROY.set(crate::async_hooks::destroy);
        DESTROY_SCHEDULED.set(crate::async_hooks::destroy_scheduled);
        DRAIN_GC_DESTROY_QUEUE.set(crate::async_hooks::drain_gc_destroy_queue);
        ENQUEUE_GC_DESTROY.set(crate::async_hooks::enqueue_gc_destroy);
        EXECUTION_ASYNC_ID_U64.set(crate::async_hooks::execution_async_id_u64);
        GC_DESTROY_WORK_PENDING.set(crate::async_hooks::gc_destroy_work_pending);
        INIT_ESM_EVALUATION_PROMISE.set(crate::async_hooks::init_esm_evaluation_promise);
        INIT_RESOURCE.set(crate::async_hooks::init_resource);
        INIT_RESOURCE_WITH_TRIGGER.set(crate::async_hooks::init_resource_with_trigger);
        INIT_SCHEDULED_RESOURCE.set(crate::async_hooks::init_scheduled_resource);
        JS_ASYNC_HOOKS_ASYNC_WRAP_PROVIDERS
            .set(crate::async_hooks::js_async_hooks_async_wrap_providers);
        JS_ASYNC_HOOKS_CREATE_HOOK.set(crate::async_hooks::js_async_hooks_create_hook);
        JS_ASYNC_HOOKS_EXECUTION_ASYNC_ID
            .set(crate::async_hooks::js_async_hooks_execution_async_id);
        JS_ASYNC_HOOKS_EXECUTION_ASYNC_RESOURCE
            .set(crate::async_hooks::js_async_hooks_execution_async_resource);
        JS_ASYNC_HOOKS_OWNED_PROVIDER_DESTROY
            .set(crate::async_hooks::provider_ffi::js_async_hooks_owned_provider_destroy);
        JS_ASYNC_HOOKS_OWNED_PROVIDER_EMIT_INIT
            .set(crate::async_hooks::provider_ffi::js_async_hooks_owned_provider_emit_init);
        JS_ASYNC_HOOKS_OWNED_PROVIDER_NEW
            .set(crate::async_hooks::provider_ffi::js_async_hooks_owned_provider_new);
        JS_ASYNC_HOOKS_PROVIDER_DEFER_DESTROY
            .set(crate::async_hooks::provider_ffi::js_async_hooks_provider_defer_destroy);
        JS_ASYNC_HOOKS_PROVIDER_DESTROY
            .set(crate::async_hooks::provider_ffi::js_async_hooks_provider_destroy);
        JS_ASYNC_HOOKS_PROVIDER_INIT
            .set(crate::async_hooks::provider_ffi::js_async_hooks_provider_init);
        JS_ASYNC_HOOKS_PROVIDER_INIT_WITH_TRIGGER
            .set(crate::async_hooks::provider_ffi::js_async_hooks_provider_init_with_trigger);
        JS_ASYNC_HOOKS_PROVIDER_LEAVE
            .set(crate::async_hooks::provider_ffi::js_async_hooks_provider_leave);
        JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING
            .set(crate::async_hooks::provider_ffi::js_async_hooks_provider_run_catching);
        JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_DEFERRED_DESTROY.set(
            crate::async_hooks::provider_ffi::js_async_hooks_provider_run_catching_deferred_destroy,
        );
        JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_DEFERRED_DESTROY_ON_ERROR.set(crate::async_hooks::provider_ffi::js_async_hooks_provider_run_catching_deferred_destroy_on_error);
        JS_ASYNC_HOOKS_PROVIDER_RUN_CATCHING_WITH_THIS
            .set(crate::async_hooks::provider_ffi::js_async_hooks_provider_run_catching_with_this);
        JS_ASYNC_HOOKS_TRIGGER_ASYNC_ID.set(crate::async_hooks::js_async_hooks_trigger_async_id);
        JS_ASYNC_LOCAL_STORAGE_STATIC_BIND_METHOD
            .set(crate::async_hooks::js_async_local_storage_static_bind_method);
        JS_ASYNC_LOCAL_STORAGE_STATIC_SNAPSHOT_METHOD
            .set(crate::async_hooks::js_async_local_storage_static_snapshot_method);
        JS_ASYNC_RESOURCE_ASYNC_ID.set(crate::async_hooks::js_async_resource_async_id);
        JS_ASYNC_RESOURCE_EMIT_DESTROY.set(crate::async_hooks::js_async_resource_emit_destroy);
        JS_ASYNC_RESOURCE_NEW.set(crate::async_hooks::js_async_resource_new);
        JS_ASYNC_RESOURCE_RUNTIME_ASYNC_ID
            .set(crate::async_hooks::js_async_resource_runtime_async_id);
        JS_ASYNC_RESOURCE_SET_EVENT_EMITTER
            .set(crate::async_hooks::js_async_resource_set_event_emitter);
        JS_ASYNC_RESOURCE_STATIC_BIND_METHOD
            .set(crate::async_hooks::js_async_resource_static_bind_method);
        JS_ASYNC_RESOURCE_SUBCLASS_INIT.set(crate::async_hooks::js_async_resource_subclass_init);
        JS_ASYNC_RESOURCE_TRIGGER_ASYNC_ID
            .set(crate::async_hooks::js_async_resource_trigger_async_id);
        LEAVE_RESOURCE_SCOPE.set(crate::async_hooks::leave_resource_scope);
        PROMISE_HOOKS_ACTIVE.set(crate::async_hooks::promise_hooks_active);
        PROMISE_RESOLVE.set(crate::async_hooks::promise_resolve);
        RELEASE_ASYNC_HOOKS_IN_FREED_RANGES
            .set(crate::async_hooks::release_async_hooks_in_freed_ranges);
        RESERVE_RESOURCE_IDS.set(crate::async_hooks::reserve_resource_ids);
        RESOLVE_ASYNC_RESOURCE_HANDLE.set(crate::async_hooks::resolve_async_resource_handle);
        SCAN_ASYNC_HOOKS_ROOTS.set(crate::async_hooks::scan_async_hooks_roots);
        SCAN_ASYNC_HOOKS_ROOTS_MUT.set(crate::async_hooks::scan_async_hooks_roots_mut);
        TRY_ENTER_RESOURCE_SCOPE_AT.set(crate::async_hooks::scopes::try_enter_resource_scope_at);
        TRY_LEAVE_RESOURCE_SCOPE.set(crate::async_hooks::try_leave_resource_scope);
        UNWIND_EXECUTION_SCOPE.set(crate::async_hooks::unwind_execution_scope);
        OWNED_PROVIDER_SCOPE.set(crate::async_hooks::slots_bridge::owned_provider_scope);
        OWNED_PROVIDER_SCOPE_ROOTED
            .set(crate::async_hooks::slots_bridge::owned_provider_scope_rooted);
        RUN_PROVIDER_COMPLETION.set(crate::async_hooks::slots_bridge::run_provider_completion);
        RUN_RESOURCE_SCOPE.set(crate::async_hooks::slots_bridge::run_resource_scope);
        RUN_RESOURCE_SCOPE_CATCHING
            .set(crate::async_hooks::slots_bridge::run_resource_scope_catching);
        RUN_WITH_CONTEXT_SNAPSHOT.set(crate::async_hooks::slots_bridge::run_with_context_snapshot);
        PROVIDER_ENTER_AT.set(crate::async_hooks::provider_ffi::provider_enter_at);
        resource_state::install();
        REGISTER_ROOTS.set(crate::async_hooks::register_roots);
    });
    register_installed_roots();
}
pub(crate) mod resource_state {
    use super::*;
    pub(crate) use crate::async_hooks::resource_state::ResourceState;

    static FINALIZE: Hook<unsafe fn(*mut ResourceState, bool)> = Hook::empty();
    #[inline]
    pub unsafe fn finalize(state: *mut ResourceState, notify: bool) {
        FINALIZE
            .get()
            .map_or_else(|| (), |f| unsafe { f(state, notify) })
    }

    static GET: Hook<fn(u64) -> Option<*mut ResourceState>> = Hook::empty();
    #[inline]
    pub fn get(id: u64) -> Option<*mut ResourceState> {
        GET.get().map_or_else(|| None, |f| f(id))
    }

    static SCAN_TOKEN: Hook<fn(&mut u64, &mut crate::gc::RuntimeRootVisitor<'_>)> = Hook::empty();
    #[inline]
    pub fn scan_token(id: &mut u64, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
        SCAN_TOKEN.get().map_or_else(|| (), |f| f(id, visitor))
    }

    pub(super) fn install() {
        FINALIZE.set(crate::async_hooks::resource_state::finalize);
        GET.set(crate::async_hooks::resource_state::get);
        SCAN_TOKEN.set(crate::async_hooks::resource_state::scan_token);
    }
}
