//! Native async_hooks lifecycle support.
//!
//! This module owns the process-wide hook list, async resource ids, and the
//! thread-local execution/trigger id stack used by the compiled runtime.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::array::{js_array_length, ArrayHeader};
use crate::closure::{
    js_closure_alloc, js_closure_call1, js_closure_call4, js_closure_call_array,
    js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_register_closure_rest,
    ClosureHeader,
};
use crate::object::{js_object_get_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::POINTER_MASK;

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const INT32_MASK: u64 = 0x0000_0000_FFFF_FFFF;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const TAG_UNDEFINED_F64: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);

static NEXT_ASYNC_ID: AtomicU64 = AtomicU64::new(1);
pub static HOOKS_ACTIVE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
pub struct AsyncResourceIds {
    pub async_id: u64,
    pub trigger_async_id: u64,
}

#[derive(Clone)]
struct ResourceMeta {
    type_name: String,
    trigger_async_id: u64,
    resource: f64,
    destroyed: bool,
}

#[derive(Clone, Copy)]
struct HookCallbacks {
    init: *const ClosureHeader,
    before: *const ClosureHeader,
    after: *const ClosureHeader,
    destroy: *const ClosureHeader,
    promise_resolve: *const ClosureHeader,
}

unsafe impl Send for HookCallbacks {}
unsafe impl Sync for HookCallbacks {}

impl HookCallbacks {
    fn empty() -> Self {
        Self {
            init: ptr::null(),
            before: ptr::null(),
            after: ptr::null(),
            destroy: ptr::null(),
            promise_resolve: ptr::null(),
        }
    }

    fn has_any(&self) -> bool {
        !self.init.is_null()
            || !self.before.is_null()
            || !self.after.is_null()
            || !self.destroy.is_null()
            || !self.promise_resolve.is_null()
    }
}

struct HookRecord {
    callbacks: HookCallbacks,
    enabled: bool,
}

static HOOKS: LazyLock<Mutex<Vec<HookRecord>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static RESOURCES: LazyLock<Mutex<HashMap<u64, ResourceMeta>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static GC_DESTROY_QUEUE: LazyLock<Mutex<VecDeque<u64>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

thread_local! {
    static EXECUTION_STACK: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
    static CURRENT_EXECUTION_ID: Cell<u64> = const { Cell::new(0) };
    static CURRENT_TRIGGER_ID: Cell<u64> = const { Cell::new(0) };
    static IN_HOOK_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

pub struct AsyncHookHandle {
    index: usize,
}

pub struct AsyncResourceHandle {
    ids: AsyncResourceIds,
}

#[inline(always)]
pub fn hooks_active() -> bool {
    HOOKS_ACTIVE.load(Ordering::Relaxed) != 0
}

#[inline]
pub fn execution_async_id_u64() -> u64 {
    CURRENT_EXECUTION_ID.with(Cell::get)
}

#[inline]
pub fn trigger_async_id_u64() -> u64 {
    CURRENT_TRIGGER_ID.with(Cell::get)
}

#[no_mangle]
pub extern "C" fn js_async_hooks_execution_async_id() -> f64 {
    execution_async_id_u64() as f64
}

#[no_mangle]
pub extern "C" fn js_async_hooks_trigger_async_id() -> f64 {
    trigger_async_id_u64() as f64
}

#[inline]
fn box_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK))
}

fn ptr_from_nanboxed(value: f64) -> *const u8 {
    let bits = value.to_bits();
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG && tag != STRING_TAG {
        return ptr::null();
    }
    (bits & POINTER_MASK) as *const u8
}

fn closure_from_value(value: f64) -> *const ClosureHeader {
    ptr_from_nanboxed(value) as *const ClosureHeader
}

fn string_from_value(value: f64, default: &str) -> String {
    let bits = value.to_bits();
    let tag = bits & TAG_MASK;
    if tag != STRING_TAG && tag != POINTER_TAG {
        return default.to_string();
    }
    let ptr = (bits & POINTER_MASK) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return default.to_string();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len))
            .unwrap_or(default)
            .to_string()
    }
}

fn object_field(obj_value: f64, name: &[u8]) -> f64 {
    let obj = ptr_from_nanboxed(obj_value) as *const ObjectHeader;
    if obj.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32) as *const StringHeader;
    unsafe { f64::from_bits(js_object_get_field_by_name(obj, key).bits()) }
}

fn callbacks_from_options(options: f64) -> HookCallbacks {
    let mut callbacks = HookCallbacks::empty();
    callbacks.init = closure_from_value(object_field(options, b"init"));
    callbacks.before = closure_from_value(object_field(options, b"before"));
    callbacks.after = closure_from_value(object_field(options, b"after"));
    callbacks.destroy = closure_from_value(object_field(options, b"destroy"));
    callbacks.promise_resolve = closure_from_value(object_field(options, b"promiseResolve"));
    callbacks
}

#[no_mangle]
pub extern "C" fn js_async_hooks_create_hook(options: f64) -> i64 {
    let callbacks = callbacks_from_options(options);
    let mut hooks = HOOKS.lock().unwrap();
    let index = hooks.len();
    hooks.push(HookRecord {
        callbacks,
        enabled: false,
    });
    Box::into_raw(Box::new(AsyncHookHandle { index })) as i64
}

#[no_mangle]
pub extern "C" fn js_async_hook_enable(handle: i64) -> i64 {
    if handle == 0 {
        return handle;
    }
    let hook = unsafe { &*(handle as *const AsyncHookHandle) };
    let mut hooks = HOOKS.lock().unwrap();
    if let Some(record) = hooks.get_mut(hook.index) {
        if !record.enabled && record.callbacks.has_any() {
            HOOKS_ACTIVE.fetch_add(1, Ordering::Relaxed);
        }
        record.enabled = true;
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_async_hook_disable(handle: i64) -> i64 {
    if handle == 0 {
        return handle;
    }
    let hook = unsafe { &*(handle as *const AsyncHookHandle) };
    let mut hooks = HOOKS.lock().unwrap();
    if let Some(record) = hooks.get_mut(hook.index) {
        if record.enabled && record.callbacks.has_any() {
            HOOKS_ACTIVE.fetch_sub(1, Ordering::Relaxed);
        }
        record.enabled = false;
    }
    handle
}

fn enabled_callbacks() -> Vec<HookCallbacks> {
    if !hooks_active() {
        return Vec::new();
    }
    HOOKS
        .lock()
        .unwrap()
        .iter()
        .filter(|hook| hook.enabled)
        .map(|hook| hook.callbacks)
        .collect()
}

fn with_hook_callbacks(mut f: impl FnMut(HookCallbacks)) {
    if !hooks_active() {
        return;
    }
    IN_HOOK_CALLBACK.with(|guard| {
        if guard.get() {
            return;
        }
        guard.set(true);
        for callbacks in enabled_callbacks() {
            f(callbacks);
        }
        guard.set(false);
    });
}

pub fn init_resource(type_name: &str, resource: f64, force_allocate: bool) -> AsyncResourceIds {
    init_resource_with_trigger(
        type_name,
        resource,
        force_allocate,
        execution_async_id_u64(),
    )
}

pub fn init_resource_with_trigger(
    type_name: &str,
    resource: f64,
    force_allocate: bool,
    trigger_async_id: u64,
) -> AsyncResourceIds {
    if !force_allocate && !hooks_active() {
        return AsyncResourceIds {
            async_id: 0,
            trigger_async_id,
        };
    }

    let async_id = NEXT_ASYNC_ID.fetch_add(1, Ordering::Relaxed);
    RESOURCES.lock().unwrap().insert(
        async_id,
        ResourceMeta {
            type_name: type_name.to_string(),
            trigger_async_id,
            resource,
            destroyed: false,
        },
    );

    emit_init(async_id, type_name, trigger_async_id, resource);
    AsyncResourceIds {
        async_id,
        trigger_async_id,
    }
}

fn emit_init(async_id: u64, type_name: &str, trigger_async_id: u64, resource: f64) {
    let type_ptr = js_string_from_bytes(type_name.as_ptr(), type_name.len() as u32);
    let type_value = box_ptr(type_ptr as *const u8);
    with_hook_callbacks(|callbacks| {
        if !callbacks.init.is_null() {
            unsafe {
                js_closure_call4(
                    callbacks.init,
                    async_id as f64,
                    type_value,
                    trigger_async_id as f64,
                    resource,
                );
            }
        }
    });
}

pub fn before(async_id: u64, trigger_async_id: u64) {
    if async_id == 0 {
        return;
    }
    EXECUTION_STACK.with(|stack| {
        stack
            .borrow_mut()
            .push((execution_async_id_u64(), trigger_async_id_u64()));
    });
    CURRENT_EXECUTION_ID.with(|c| c.set(async_id));
    CURRENT_TRIGGER_ID.with(|c| c.set(trigger_async_id));
    with_hook_callbacks(|callbacks| {
        if !callbacks.before.is_null() {
            unsafe { js_closure_call1(callbacks.before, async_id as f64) };
        }
    });
}

pub fn after(async_id: u64) {
    if async_id == 0 {
        return;
    }
    with_hook_callbacks(|callbacks| {
        if !callbacks.after.is_null() {
            unsafe { js_closure_call1(callbacks.after, async_id as f64) };
        }
    });
    let prev = EXECUTION_STACK
        .with(|stack| stack.borrow_mut().pop())
        .unwrap_or((0, 0));
    CURRENT_EXECUTION_ID.with(|c| c.set(prev.0));
    CURRENT_TRIGGER_ID.with(|c| c.set(prev.1));
}

pub fn promise_resolve(async_id: u64) {
    if async_id == 0 {
        return;
    }
    with_hook_callbacks(|callbacks| {
        if !callbacks.promise_resolve.is_null() {
            unsafe { js_closure_call1(callbacks.promise_resolve, async_id as f64) };
        }
    });
}

pub fn destroy(async_id: u64) {
    if async_id == 0 {
        return;
    }
    let should_emit = {
        let mut resources = RESOURCES.lock().unwrap();
        match resources.get_mut(&async_id) {
            Some(meta) if !meta.destroyed => {
                meta.destroyed = true;
                true
            }
            Some(_) => false,
            None => true,
        }
    };
    if !should_emit {
        return;
    }
    with_hook_callbacks(|callbacks| {
        if !callbacks.destroy.is_null() {
            unsafe { js_closure_call1(callbacks.destroy, async_id as f64) };
        }
    });
    RESOURCES.lock().unwrap().remove(&async_id);
}

pub fn enqueue_gc_destroy(async_id: u64) {
    if async_id != 0 {
        GC_DESTROY_QUEUE.lock().unwrap().push_back(async_id);
    }
}

pub fn drain_gc_destroy_queue() -> i32 {
    let ids: Vec<u64> = {
        let mut q = GC_DESTROY_QUEUE.lock().unwrap();
        q.drain(..).collect()
    };
    let count = ids.len() as i32;
    for id in ids {
        destroy(id);
    }
    count
}

fn value_to_u64(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    if (bits & !INT32_MASK) == INT32_TAG {
        return Some((bits & INT32_MASK) as i32 as u64);
    }
    if (bits >> 48) < 0x7FFC {
        let n = f64::from_bits(bits);
        if n.is_finite() && n >= 0.0 {
            return Some(n as u64);
        }
    }
    None
}

fn trigger_id_from_options(options: f64) -> u64 {
    value_to_u64(object_field(options, b"triggerAsyncId")).unwrap_or_else(execution_async_id_u64)
}

#[no_mangle]
pub extern "C" fn js_async_resource_new(type_value: f64, options: f64) -> i64 {
    let type_name = string_from_value(type_value, "AsyncResource");
    let trigger_async_id = trigger_id_from_options(options);
    let ids = init_resource_with_trigger(&type_name, TAG_UNDEFINED_F64, true, trigger_async_id);
    Box::into_raw(Box::new(AsyncResourceHandle { ids })) as i64
}

#[no_mangle]
pub extern "C" fn js_async_resource_async_id(handle: i64) -> f64 {
    if handle == 0 {
        return 0.0;
    }
    let resource = unsafe { &*(handle as *const AsyncResourceHandle) };
    resource.ids.async_id as f64
}

#[no_mangle]
pub extern "C" fn js_async_resource_trigger_async_id(handle: i64) -> f64 {
    if handle == 0 {
        return 0.0;
    }
    let resource = unsafe { &*(handle as *const AsyncResourceHandle) };
    resource.ids.trigger_async_id as f64
}

#[no_mangle]
pub extern "C" fn js_async_resource_emit_destroy(handle: i64) -> i64 {
    if handle != 0 {
        let resource = unsafe { &*(handle as *const AsyncResourceHandle) };
        destroy(resource.ids.async_id);
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_async_resource_run_in_async_scope(
    handle: i64,
    callback: i64,
    this_arg: f64,
    args_array: i64,
) -> f64 {
    let _ = this_arg;
    if handle == 0 || callback == 0 {
        return TAG_UNDEFINED_F64;
    }
    let resource = unsafe { &*(handle as *const AsyncResourceHandle) };
    before(resource.ids.async_id, resource.ids.trigger_async_id);
    let result = if args_array == 0 {
        unsafe { js_closure_call_array(callback, ptr::null(), 0) }
    } else {
        let arr = args_array as *const ArrayHeader;
        let len = js_array_length(arr) as i64;
        let data = if arr.is_null() {
            ptr::null()
        } else {
            unsafe { (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64 }
        };
        unsafe { js_closure_call_array(callback, data, len) }
    };
    after(resource.ids.async_id);
    result
}

/// Trampoline body for `AsyncResource#bind`. Stored as the `func_ptr` of the
/// synthesized closure; receives the rest array of forwarded args and replays
/// the call through `runInAsyncScope` so init/before/after/destroy fire with
/// the bound resource's async id active.
extern "C" fn async_resource_bind_trampoline(closure: *const ClosureHeader, rest: f64) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let handle = js_closure_get_capture_ptr(closure, 0);
    let callback = js_closure_get_capture_ptr(closure, 1);
    if handle == 0 || callback == 0 {
        return TAG_UNDEFINED_F64;
    }
    let args_array_ptr = ptr_from_nanboxed(rest) as i64;
    js_async_resource_run_in_async_scope(handle, callback, TAG_UNDEFINED_F64, args_array_ptr)
}

fn register_bind_trampoline_once() {
    thread_local! {
        // CLOSURE_REST_REGISTRY is thread-local, so each thread that
        // synthesizes a bind() trampoline must register the func_ptr once.
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    REGISTERED.with(|flag| {
        if !flag.get() {
            // fixed_arity=0 → dispatch_rest_bundled calls
            // `f(closure, rest_array)` regardless of forwarded arity.
            js_register_closure_rest(async_resource_bind_trampoline as *const u8, 0);
            flag.set(true);
        }
    });
}

#[no_mangle]
pub extern "C" fn js_async_resource_bind(handle: i64, callback: i64) -> i64 {
    if handle == 0 || callback == 0 {
        return callback;
    }
    register_bind_trampoline_once();
    let closure = js_closure_alloc(async_resource_bind_trampoline as *const u8, 2);
    if closure.is_null() {
        return callback;
    }
    js_closure_set_capture_ptr(closure, 0, handle);
    js_closure_set_capture_ptr(closure, 1, callback);
    closure as i64
}

#[no_mangle]
pub extern "C" fn js_async_resource_static_bind(callback: i64, type_value: f64) -> i64 {
    let handle = js_async_resource_new(type_value, TAG_UNDEFINED_F64);
    js_async_resource_bind(handle, callback)
}

pub fn scan_async_hooks_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_async_hooks_roots_mut(&mut visitor);
}

pub fn scan_async_hooks_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut hooks = HOOKS.lock().unwrap();
    for hook in hooks.iter_mut() {
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.init);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.before);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.after);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.destroy);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.promise_resolve);
    }
    drop(hooks);
    let mut resources = RESOURCES.lock().unwrap();
    for meta in resources.values_mut() {
        visitor.visit_nanbox_f64_slot(&mut meta.resource);
    }
}

#[cfg(test)]
pub fn reset_for_tests() {
    HOOKS.lock().unwrap().clear();
    RESOURCES.lock().unwrap().clear();
    GC_DESTROY_QUEUE.lock().unwrap().clear();
    HOOKS_ACTIVE.store(0, Ordering::Relaxed);
    NEXT_ASYNC_ID.store(1, Ordering::Relaxed);
    CURRENT_EXECUTION_ID.with(|c| c.set(0));
    CURRENT_TRIGGER_ID.with(|c| c.set(0));
    EXECUTION_STACK.with(|s| s.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn test_seed_async_hooks_scanner_roots(callback: *const ClosureHeader, resource: f64) {
    reset_for_tests();
    HOOKS.lock().unwrap().push(HookRecord {
        callbacks: HookCallbacks {
            init: callback,
            before: callback,
            after: callback,
            destroy: callback,
            promise_resolve: callback,
        },
        enabled: true,
    });
    HOOKS_ACTIVE.store(1, Ordering::Relaxed);
    RESOURCES.lock().unwrap().insert(
        1,
        ResourceMeta {
            type_name: "test".to_string(),
            trigger_async_id: 0,
            resource,
            destroyed: false,
        },
    );
}

#[cfg(test)]
pub(crate) fn test_async_hooks_scanner_snapshot() -> (usize, u64) {
    let callback = HOOKS
        .lock()
        .unwrap()
        .first()
        .map(|hook| hook.callbacks.init as usize)
        .unwrap_or(0);
    let resource_bits = RESOURCES
        .lock()
        .unwrap()
        .get(&1)
        .map(|meta| meta.resource.to_bits())
        .unwrap_or(0);
    (callback, resource_bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn resource_ids_are_monotonic_even_without_hooks() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_for_tests();
        let a = init_resource("A", TAG_UNDEFINED_F64, true);
        let b = init_resource("B", TAG_UNDEFINED_F64, true);
        assert_eq!(a.async_id, 1);
        assert_eq!(b.async_id, 2);
    }

    #[test]
    fn before_after_restore_execution_ids() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_for_tests();
        let ids = init_resource("A", TAG_UNDEFINED_F64, true);
        before(ids.async_id, ids.trigger_async_id);
        assert_eq!(execution_async_id_u64(), ids.async_id);
        after(ids.async_id);
        assert_eq!(execution_async_id_u64(), 0);
    }
}
