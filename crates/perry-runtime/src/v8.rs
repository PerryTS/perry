//! Real `node:v8` Promise lifecycle hooks (`v8.promiseHooks`) for #3139.
//!
//! Perry does not embed V8 for heap snapshots, cached data, or serializers (the
//! diagnostic/serializer surface lives in `node_v8.rs`), but Node's Promise
//! lifecycle hooks can be represented directly by the native runtime. Hook
//! registrars install callbacks here, and the Promise machinery in
//! `promise/{then,microtasks,async_step}.rs` fires them on allocation, callback
//! entry/exit, and settlement.

use std::cell::Cell;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::closure::{
    js_closure_alloc, js_closure_call1, js_closure_call2, js_closure_get_capture_ptr,
    js_closure_set_capture_ptr, js_register_closure_arity, ClosureHeader,
};
use crate::object::{js_object_get_field_by_name, ObjectHeader};
use crate::promise::Promise;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, POINTER_MASK, POINTER_TAG, TAG_MASK, TAG_UNDEFINED};

const TAG_UNDEFINED_F64: f64 = f64::from_bits(TAG_UNDEFINED);

static PROMISE_HOOKS_ACTIVE: AtomicUsize = AtomicUsize::new(0);
static PROMISE_HOOKS: LazyLock<Mutex<Vec<PromiseHookRecord>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Clone, Copy)]
struct PromiseHookCallbacks {
    init: *const ClosureHeader,
    before: *const ClosureHeader,
    after: *const ClosureHeader,
    settled: *const ClosureHeader,
}

unsafe impl Send for PromiseHookCallbacks {}
unsafe impl Sync for PromiseHookCallbacks {}

impl PromiseHookCallbacks {
    fn empty() -> Self {
        Self {
            init: ptr::null(),
            before: ptr::null(),
            after: ptr::null(),
            settled: ptr::null(),
        }
    }

    fn has_any(&self) -> bool {
        !self.init.is_null()
            || !self.before.is_null()
            || !self.after.is_null()
            || !self.settled.is_null()
    }
}

struct PromiseHookRecord {
    callbacks: PromiseHookCallbacks,
    enabled: bool,
}

thread_local! {
    static IN_PROMISE_HOOK_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

#[inline]
pub fn promise_hooks_active() -> bool {
    PROMISE_HOOKS_ACTIVE.load(Ordering::Relaxed) != 0
}

fn promise_value(promise: *mut Promise) -> f64 {
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

fn string_header_eq(ptr: *const StringHeader, expected: &str) -> bool {
    if ptr.is_null() {
        return false;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len) == expected.as_bytes()
    }
}

fn is_function_value(value: f64) -> bool {
    string_header_eq(crate::builtins::js_value_typeof(value), "function")
}

fn closure_from_function_value(value: f64) -> *const ClosureHeader {
    if !is_function_value(value) {
        return ptr::null();
    }
    let bits = value.to_bits();
    if (bits & TAG_MASK) != POINTER_TAG {
        return ptr::null();
    }
    let ptr = (bits & POINTER_MASK) as usize;
    if crate::closure::is_closure_ptr(ptr) {
        ptr as *const ClosureHeader
    } else {
        ptr::null()
    }
}

fn object_field(obj_value: f64, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_nanbox_f64(obj_value);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32) as *const StringHeader;
    let key_handle = scope.root_string_ptr(key);
    let value = JSValue::from_bits(obj_handle.get_nanbox_f64().to_bits());
    if !value.is_pointer() {
        return TAG_UNDEFINED_F64;
    }
    let obj = value.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return TAG_UNDEFINED_F64;
    }
    f64::from_bits(js_object_get_field_by_name(obj, key_handle.get_raw_const_ptr()).bits())
}

fn throw_invalid_hook(label: &'static str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type function. Received {}",
        label,
        crate::fs::validate::describe_received(value),
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn required_hook(value: f64, label: &'static str) -> *const ClosureHeader {
    let callback = closure_from_function_value(value);
    if callback.is_null() {
        throw_invalid_hook(label, value);
    }
    callback
}

fn optional_hook(options: f64, property: &[u8], label: &'static str) -> *const ClosureHeader {
    let value = object_field(options, property);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return ptr::null();
    }
    required_hook(value, label)
}

fn callbacks_from_options(options: f64) -> PromiseHookCallbacks {
    PromiseHookCallbacks {
        init: optional_hook(options, b"init", "initHook"),
        before: optional_hook(options, b"before", "beforeHook"),
        after: optional_hook(options, b"after", "afterHook"),
        settled: optional_hook(options, b"settled", "settledHook"),
    }
}

fn register_stop_trampoline_once() {
    thread_local! {
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        js_register_closure_arity(promise_hook_stop_trampoline as *const u8, 0);
        registered.set(true);
    });
}

extern "C" fn promise_hook_stop_trampoline(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let index = js_closure_get_capture_ptr(closure, 0) as usize;
    disable_promise_hook(index);
    TAG_UNDEFINED_F64
}

fn make_stop_function(index: usize) -> f64 {
    register_stop_trampoline_once();
    let closure = js_closure_alloc(promise_hook_stop_trampoline as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, index as i64);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn install_promise_hook(callbacks: PromiseHookCallbacks) -> f64 {
    let mut hooks = PROMISE_HOOKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let index = hooks.len();
    let enabled = callbacks.has_any();
    hooks.push(PromiseHookRecord { callbacks, enabled });
    if enabled {
        PROMISE_HOOKS_ACTIVE.fetch_add(1, Ordering::Relaxed);
    }
    make_stop_function(index)
}

fn disable_promise_hook(index: usize) {
    let mut hooks = PROMISE_HOOKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(record) = hooks.get_mut(index) else {
        return;
    };
    if record.enabled && record.callbacks.has_any() {
        PROMISE_HOOKS_ACTIVE.fetch_sub(1, Ordering::Relaxed);
    }
    record.enabled = false;
    record.callbacks = PromiseHookCallbacks::empty();
}

fn enabled_callbacks() -> Vec<PromiseHookCallbacks> {
    if !promise_hooks_active() {
        return Vec::new();
    }
    PROMISE_HOOKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter(|hook| hook.enabled)
        .map(|hook| hook.callbacks)
        .collect()
}

fn with_promise_hook_callbacks(mut f: impl FnMut(PromiseHookCallbacks)) {
    if !promise_hooks_active() {
        return;
    }
    IN_PROMISE_HOOK_CALLBACK.with(|guard| {
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

#[no_mangle]
pub extern "C" fn js_v8_promise_hooks_on_init(callback: f64) -> f64 {
    install_promise_hook(PromiseHookCallbacks {
        init: required_hook(callback, "initHook"),
        ..PromiseHookCallbacks::empty()
    })
}

#[no_mangle]
pub extern "C" fn js_v8_promise_hooks_on_before(callback: f64) -> f64 {
    install_promise_hook(PromiseHookCallbacks {
        before: required_hook(callback, "beforeHook"),
        ..PromiseHookCallbacks::empty()
    })
}

#[no_mangle]
pub extern "C" fn js_v8_promise_hooks_on_after(callback: f64) -> f64 {
    install_promise_hook(PromiseHookCallbacks {
        after: required_hook(callback, "afterHook"),
        ..PromiseHookCallbacks::empty()
    })
}

#[no_mangle]
pub extern "C" fn js_v8_promise_hooks_on_settled(callback: f64) -> f64 {
    install_promise_hook(PromiseHookCallbacks {
        settled: required_hook(callback, "settledHook"),
        ..PromiseHookCallbacks::empty()
    })
}

#[no_mangle]
pub extern "C" fn js_v8_promise_hooks_create_hook(options: f64) -> f64 {
    install_promise_hook(callbacks_from_options(options))
}

pub(crate) fn promise_hook_init(promise: *mut Promise, parent: *mut Promise) {
    if promise.is_null() || !promise_hooks_active() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let parent_handle = scope.root_raw_mut_ptr(parent);
    with_promise_hook_callbacks(|callbacks| {
        if callbacks.init.is_null() {
            return;
        }
        let callback_handle = scope.root_raw_const_ptr(callbacks.init);
        let promise_arg = promise_value(promise_handle.get_raw_mut_ptr::<Promise>());
        let parent_ptr = parent_handle.get_raw_mut_ptr::<Promise>();
        let parent_arg = if parent_ptr.is_null() {
            TAG_UNDEFINED_F64
        } else {
            promise_value(parent_ptr)
        };
        js_closure_call2(callback_handle.get_raw_const_ptr(), promise_arg, parent_arg);
    });
}

pub(crate) fn promise_hook_before(promise: *mut Promise) {
    if promise.is_null() || !promise_hooks_active() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    with_promise_hook_callbacks(|callbacks| {
        if callbacks.before.is_null() {
            return;
        }
        let callback_handle = scope.root_raw_const_ptr(callbacks.before);
        js_closure_call1(
            callback_handle.get_raw_const_ptr(),
            promise_value(promise_handle.get_raw_mut_ptr::<Promise>()),
        );
    });
}

pub(crate) fn promise_hook_after(promise: *mut Promise) {
    if promise.is_null() || !promise_hooks_active() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    with_promise_hook_callbacks(|callbacks| {
        if callbacks.after.is_null() {
            return;
        }
        let callback_handle = scope.root_raw_const_ptr(callbacks.after);
        js_closure_call1(
            callback_handle.get_raw_const_ptr(),
            promise_value(promise_handle.get_raw_mut_ptr::<Promise>()),
        );
    });
}

pub(crate) fn promise_hook_settled(promise: *mut Promise) {
    if promise.is_null() || !promise_hooks_active() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    with_promise_hook_callbacks(|callbacks| {
        if callbacks.settled.is_null() {
            return;
        }
        let callback_handle = scope.root_raw_const_ptr(callbacks.settled);
        js_closure_call1(
            callback_handle.get_raw_const_ptr(),
            promise_value(promise_handle.get_raw_mut_ptr::<Promise>()),
        );
    });
}

pub fn scan_v8_promise_hook_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut hooks = PROMISE_HOOKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for hook in hooks.iter_mut() {
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.init);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.before);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.after);
        visitor.visit_raw_const_ptr_slot(&mut hook.callbacks.settled);
    }
}

#[cfg(test)]
pub fn reset_for_tests() {
    PROMISE_HOOKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    PROMISE_HOOKS_ACTIVE.store(0, Ordering::Relaxed);
    IN_PROMISE_HOOK_CALLBACK.with(|c| c.set(false));
}
