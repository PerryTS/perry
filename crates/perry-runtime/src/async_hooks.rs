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
    js_closure_get_capture_f64, js_closure_get_capture_ptr, js_closure_set_capture_f64,
    js_closure_set_capture_ptr, js_register_closure_rest, ClosureHeader,
};
use crate::object::{js_object_get_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, POINTER_MASK};

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
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
    // #854: async_hooks resource metadata; real createHook lifecycle is #789
    #[allow(dead_code)]
    type_name: String,
    // #854: async_hooks resource metadata; real createHook lifecycle is #789
    #[allow(dead_code)]
    trigger_async_id: u64,
    resource: f64,
    context: crate::async_context::AsyncContextSnapshot,
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
static NEXT_CONTEXT_SNAPSHOT_ID: AtomicUsize = AtomicUsize::new(1);
static CONTEXT_SNAPSHOTS: LazyLock<
    Mutex<HashMap<usize, crate::async_context::AsyncContextSnapshot>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));
static ASYNC_WRAP_PROVIDERS: AtomicU64 = AtomicU64::new(0);

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
    async_id_to_js_number(trigger_async_id_u64())
}

#[no_mangle]
pub extern "C" fn js_async_hooks_execution_async_resource() -> f64 {
    let current_id = execution_async_id_u64();
    if current_id != 0 {
        if let Some(resource) = RESOURCES
            .lock()
            .unwrap()
            .get(&current_id)
            .map(|meta| meta.resource)
        {
            if !JSValue::from_bits(resource.to_bits()).is_undefined() {
                return resource;
            }
        }
    }

    let obj = crate::object::js_object_alloc(0, 0);
    crate::value::js_nanbox_pointer(obj as i64)
}

const ASYNC_WRAP_PROVIDER_CONSTANTS: &[(&str, f64)] = &[
    ("NONE", 0.0),
    ("DIRHANDLE", 1.0),
    ("DNSCHANNEL", 2.0),
    ("ELDHISTOGRAM", 3.0),
    ("FILEHANDLE", 4.0),
    ("FILEHANDLECLOSEREQ", 5.0),
    ("BLOBREADER", 6.0),
    ("FSEVENTWRAP", 7.0),
    ("FSREQCALLBACK", 8.0),
    ("FSREQPROMISE", 9.0),
    ("GETADDRINFOREQWRAP", 10.0),
    ("GETNAMEINFOREQWRAP", 11.0),
    ("HEAPSNAPSHOT", 12.0),
    ("HTTP2SESSION", 13.0),
    ("HTTP2STREAM", 14.0),
    ("HTTP2PING", 15.0),
    ("HTTP2SETTINGS", 16.0),
    ("HTTPINCOMINGMESSAGE", 17.0),
    ("HTTPCLIENTREQUEST", 18.0),
    ("LOCKS", 19.0),
    ("JSSTREAM", 20.0),
    ("JSUDPWRAP", 21.0),
    ("MESSAGEPORT", 22.0),
    ("PIPECONNECTWRAP", 23.0),
    ("PIPESERVERWRAP", 24.0),
    ("PIPEWRAP", 25.0),
    ("PROCESSWRAP", 26.0),
    ("PROMISE", 27.0),
    ("QUERYWRAP", 28.0),
    ("QUIC_ENDPOINT", 29.0),
    ("QUIC_LOGSTREAM", 30.0),
    ("QUIC_PACKET", 31.0),
    ("QUIC_SESSION", 32.0),
    ("QUIC_STREAM", 33.0),
    ("QUIC_UDP", 34.0),
    ("SHUTDOWNWRAP", 35.0),
    ("SIGNALWRAP", 36.0),
    ("STATWATCHER", 37.0),
    ("STREAMPIPE", 38.0),
    ("TCPCONNECTWRAP", 39.0),
    ("TCPSERVERWRAP", 40.0),
    ("TCPWRAP", 41.0),
    ("TTYWRAP", 42.0),
    ("UDPSENDWRAP", 43.0),
    ("UDPWRAP", 44.0),
    ("SIGINTWATCHDOG", 45.0),
    ("WORKER", 46.0),
    ("WORKERCPUPROFILE", 47.0),
    ("WORKERCPUUSAGE", 48.0),
    ("WORKERHEAPPROFILE", 49.0),
    ("WORKERHEAPSNAPSHOT", 50.0),
    ("WORKERHEAPSTATISTICS", 51.0),
    ("WRITEWRAP", 52.0),
    ("ZLIB", 53.0),
    ("CHECKPRIMEREQUEST", 54.0),
    ("PBKDF2REQUEST", 55.0),
    ("KEYPAIRGENREQUEST", 56.0),
    ("KEYGENREQUEST", 57.0),
    ("KEYEXPORTREQUEST", 58.0),
    ("ARGON2REQUEST", 59.0),
    ("CIPHERREQUEST", 60.0),
    ("DERIVEBITSREQUEST", 61.0),
    ("HASHREQUEST", 62.0),
    ("RANDOMBYTESREQUEST", 63.0),
    ("RANDOMPRIMEREQUEST", 64.0),
    ("SCRYPTREQUEST", 65.0),
    ("SIGNREQUEST", 66.0),
    ("TLSWRAP", 67.0),
    ("VERIFYREQUEST", 68.0),
];

pub fn js_async_hooks_async_wrap_providers() -> f64 {
    let cached = ASYNC_WRAP_PROVIDERS.load(Ordering::Acquire);
    if cached != 0 {
        return f64::from_bits(cached);
    }

    let obj = crate::object::js_object_alloc(0, ASYNC_WRAP_PROVIDER_CONSTANTS.len() as u32);
    for (name, value) in ASYNC_WRAP_PROVIDER_CONSTANTS {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, *value);
    }
    let value = crate::value::js_nanbox_pointer(obj as i64);
    let value = crate::object::js_object_freeze(value);
    ASYNC_WRAP_PROVIDERS.store(value.to_bits(), Ordering::Release);
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    value
}

// #854: pointer-boxing helper retained for async_hooks resource tracking (#789)
#[allow(dead_code)]
#[inline]
fn box_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK))
}

/// NaN-box a `StringHeader` pointer with `STRING_TAG` so JS sees a real
/// string (#789): the `init` hook's `type` argument is a string like
/// `"PROMISE"` — boxing it as a generic `POINTER_TAG` made the callback
/// observe `[object Object]` instead.
#[inline]
fn box_string(ptr: *const u8) -> f64 {
    f64::from_bits(STRING_TAG | (ptr as u64 & POINTER_MASK))
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

fn object_field(obj_value: f64, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_nanbox_f64(obj_value);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32) as *const StringHeader;
    let key_handle = scope.root_string_ptr(key);
    let obj = ptr_from_nanboxed(obj_handle.get_nanbox_f64()) as *const ObjectHeader;
    if obj.is_null() {
        return TAG_UNDEFINED_F64;
    }
    f64::from_bits(js_object_get_field_by_name(obj, key_handle.get_raw_const_ptr()).bits())
}

/// #3089 — `createHook(options)` destructures `options` immediately, so a
/// nullish top-level value throws a plain `TypeError` (no error code) with
/// Node's "Cannot destructure property 'init' of …" message *before* any
/// callback is read. Non-nullish primitives (e.g. `0`) are accepted because
/// destructuring them simply yields no callback fields.
fn validate_create_hook_options(options: f64) {
    let jv = JSValue::from_bits(options.to_bits());
    let received = if jv.is_undefined() {
        "'undefined' as it is undefined"
    } else if jv.is_null() {
        "'object null' as it is null"
    } else {
        return;
    };
    let message = format!("Cannot destructure property 'init' of {}.", received);
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// #3089 — a *present* (non-`undefined`) hook member must be callable, matching
/// Node's `validateFunction(value, 'hook.<name>')` which throws
/// `TypeError [ERR_ASYNC_CALLBACK]` "hook.<name> must be a function". A missing
/// or `undefined` member is allowed (left as a null callback).
fn validate_hook_member(value: f64, member: &str) -> *const ClosureHeader {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return ptr::null();
    }
    if is_callable_value(value) {
        return closure_from_value(value);
    }
    let message = format!("hook.{} must be a function", member);
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_ASYNC_CALLBACK")
}

fn callbacks_from_options(options: f64) -> HookCallbacks {
    let scope = crate::gc::RuntimeHandleScope::new();
    let options_handle = scope.root_nanbox_f64(options);
    let mut callbacks = HookCallbacks::empty();
    let init = scope.root_nanbox_f64(object_field(options_handle.get_nanbox_f64(), b"init"));
    let before = scope.root_nanbox_f64(object_field(options_handle.get_nanbox_f64(), b"before"));
    let after = scope.root_nanbox_f64(object_field(options_handle.get_nanbox_f64(), b"after"));
    let destroy = scope.root_nanbox_f64(object_field(options_handle.get_nanbox_f64(), b"destroy"));
    let promise_resolve = scope.root_nanbox_f64(object_field(
        options_handle.get_nanbox_f64(),
        b"promiseResolve",
    ));
    callbacks.init = validate_hook_member(init.get_nanbox_f64(), "init");
    callbacks.before = validate_hook_member(before.get_nanbox_f64(), "before");
    callbacks.after = validate_hook_member(after.get_nanbox_f64(), "after");
    callbacks.destroy = validate_hook_member(destroy.get_nanbox_f64(), "destroy");
    callbacks.promise_resolve =
        validate_hook_member(promise_resolve.get_nanbox_f64(), "promiseResolve");
    callbacks
}

#[no_mangle]
pub extern "C" fn js_async_hooks_create_hook(options: f64) -> i64 {
    validate_create_hook_options(options);
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
            context: crate::async_context::capture_context(),
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource_handle = scope.root_nanbox_f64(resource);
    let type_ptr = js_string_from_bytes(type_name.as_ptr(), type_name.len() as u32);
    let type_value_handle = scope.root_nanbox_f64(box_string(type_ptr as *const u8));
    with_hook_callbacks(|callbacks| {
        if !callbacks.init.is_null() {
            let callback_handle = scope.root_raw_const_ptr(callbacks.init);
            js_closure_call4(
                callback_handle.get_raw_const_ptr(),
                async_id as f64,
                type_value_handle.get_nanbox_f64(),
                async_id_to_js_number(trigger_async_id),
                resource_handle.get_nanbox_f64(),
            );
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
    let scope = crate::gc::RuntimeHandleScope::new();
    with_hook_callbacks(|callbacks| {
        if !callbacks.before.is_null() {
            let callback_handle = scope.root_raw_const_ptr(callbacks.before);
            js_closure_call1(callback_handle.get_raw_const_ptr(), async_id as f64);
        }
    });
}

pub fn after(async_id: u64) {
    if async_id == 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    with_hook_callbacks(|callbacks| {
        if !callbacks.after.is_null() {
            let callback_handle = scope.root_raw_const_ptr(callbacks.after);
            js_closure_call1(callback_handle.get_raw_const_ptr(), async_id as f64);
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
    let scope = crate::gc::RuntimeHandleScope::new();
    with_hook_callbacks(|callbacks| {
        if !callbacks.promise_resolve.is_null() {
            let callback_handle = scope.root_raw_const_ptr(callbacks.promise_resolve);
            js_closure_call1(callback_handle.get_raw_const_ptr(), async_id as f64);
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
    let scope = crate::gc::RuntimeHandleScope::new();
    with_hook_callbacks(|callbacks| {
        if !callbacks.destroy.is_null() {
            let callback_handle = scope.root_raw_const_ptr(callbacks.destroy);
            js_closure_call1(callback_handle.get_raw_const_ptr(), async_id as f64);
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

#[inline]
fn async_id_to_js_number(id: u64) -> f64 {
    if id == u64::MAX {
        -1.0
    } else {
        id as f64
    }
}

fn string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn js_string_value_to_string(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    string_header_to_string(ptr)
}

fn symbol_to_string(value: f64) -> String {
    if unsafe { crate::symbol::js_is_symbol(value) == 0 } {
        return "Symbol()".to_string();
    }
    let ptr = unsafe { crate::symbol::js_symbol_to_string(value) } as *const StringHeader;
    string_header_to_string(ptr)
}

fn value_is_array(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let gc_header = &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
        gc_header.obj_type == crate::gc::GC_TYPE_ARRAY
    }
}

fn is_callable_value(value: f64) -> bool {
    !crate::fs::extract_closure_ptr(value).is_null()
}

fn describe_received_async_hooks(value: f64) -> String {
    if is_callable_value(value) {
        return "function ".to_string();
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return format!("type symbol ({})", symbol_to_string(value));
    }
    crate::fs::validate::describe_received(value)
}

fn require_string_arg(arg_name: &str, value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        let message = format!(
            "The \"{}\" argument must be of type string. Received {}",
            arg_name,
            describe_received_async_hooks(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    js_string_value_to_string(value)
}

fn format_js_number_for_error(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f64::INFINITY {
        "Infinity".to_string()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn trigger_async_id_value(value: f64) -> Option<u64> {
    let jv = JSValue::from_bits(value.to_bits());
    let id = if jv.is_int32() {
        jv.as_int32() as f64
    } else if jv.is_number() {
        jv.as_number()
    } else {
        return None;
    };

    if !id.is_finite() || id.fract() != 0.0 || id < -1.0 || id > u64::MAX as f64 {
        return None;
    }
    if id == -1.0 {
        Some(u64::MAX)
    } else {
        Some(id as u64)
    }
}

fn render_invalid_trigger_async_id(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return jv.as_bool().to_string();
    }
    if jv.is_any_string() {
        return js_string_value_to_string(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return symbol_to_string(value);
    }
    if jv.is_int32() {
        return jv.as_int32().to_string();
    }
    if jv.is_number() {
        return format_js_number_for_error(jv.as_number());
    }
    if value_is_array(value) {
        return "[]".to_string();
    }
    if jv.is_pointer() {
        return "{}".to_string();
    }
    "undefined".to_string()
}

fn trigger_id_from_options(options: f64) -> u64 {
    let trigger_value = object_field(options, b"triggerAsyncId");
    let jv = JSValue::from_bits(trigger_value.to_bits());
    if jv.is_undefined() {
        return execution_async_id_u64();
    }
    if let Some(id) = trigger_async_id_value(trigger_value) {
        return id;
    }
    let message = format!(
        "Invalid triggerAsyncId value: {}",
        render_invalid_trigger_async_id(trigger_value)
    );
    crate::fs::validate::throw_range_error_named(&message, "ERR_INVALID_ASYNC_ID")
}

fn render_apply_value(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return jv.as_bool().to_string();
    }
    if jv.is_any_string() {
        return js_string_value_to_string(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return symbol_to_string(value);
    }
    if jv.is_int32() {
        return jv.as_int32().to_string();
    }
    if jv.is_number() {
        return format_js_number_for_error(jv.as_number());
    }
    if value_is_array(value) {
        return "[object Array]".to_string();
    }
    if jv.is_pointer() {
        return "#<Object>".to_string();
    }
    "undefined".to_string()
}

fn describe_apply_type(value: f64) -> &'static str {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        "a undefined"
    } else if jv.is_null() {
        "null"
    } else if jv.is_bool() {
        "a boolean"
    } else if jv.is_any_string() {
        "a string"
    } else if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        "a symbol"
    } else if jv.is_int32() || jv.is_number() {
        "a number"
    } else {
        "an object"
    }
}

fn throw_apply_not_function(value: f64) -> ! {
    let message = format!(
        "Function.prototype.apply was called on {}, which is {} and not a function",
        render_apply_value(value),
        describe_apply_type(value)
    );
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn validate_bind_callback(value: f64) {
    if is_callable_value(value) {
        return;
    }
    let message = format!(
        "The \"fn\" argument must be of type function. Received {}",
        describe_received_async_hooks(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

#[no_mangle]
pub extern "C" fn js_async_resource_new(type_value: f64, options: f64) -> i64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let type_handle = scope.root_nanbox_f64(type_value);
    let options_handle = scope.root_nanbox_f64(options);
    let type_name = require_string_arg("type", type_handle.get_nanbox_f64());
    let trigger_async_id = trigger_id_from_options(options_handle.get_nanbox_f64());
    let ids = init_resource_with_trigger(&type_name, TAG_UNDEFINED_F64, true, trigger_async_id);
    let handle = Box::into_raw(Box::new(AsyncResourceHandle { ids })) as i64;
    let resource_value = crate::value::js_nanbox_pointer(handle);
    if let Some(meta) = RESOURCES.lock().unwrap().get_mut(&ids.async_id) {
        meta.resource = resource_value;
    }
    handle
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
    async_id_to_js_number(resource.ids.trigger_async_id)
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
    callback_value: f64,
    this_arg: f64,
    args_array: i64,
) -> f64 {
    if handle == 0 {
        return TAG_UNDEFINED_F64;
    }
    if !is_callable_value(callback_value) {
        throw_apply_not_function(callback_value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let rebound_bits = crate::closure::clone_closure_rebind_this(
        callback_handle.get_nanbox_f64().to_bits(),
        this_arg_handle.get_nanbox_f64(),
    );
    let rebound_handle = scope.root_nanbox_f64(f64::from_bits(rebound_bits));
    let callback = crate::fs::extract_closure_ptr(rebound_handle.get_nanbox_f64());
    if callback.is_null() {
        throw_apply_not_function(callback_handle.get_nanbox_f64());
    }
    let args_array_handle = scope.root_raw_const_ptr(args_array as *const ArrayHeader);
    let resource = unsafe { &*(handle as *const AsyncResourceHandle) };
    let resource_context = RESOURCES
        .lock()
        .unwrap()
        .get(&resource.ids.async_id)
        .map(|meta| meta.context.clone())
        .unwrap_or_default();
    let mut resource_context = resource_context;
    let resource_context_roots = crate::async_context::root_snapshot(&scope, &resource_context);
    let previous = crate::async_context::enter_context(&resource_context);
    let mut previous = previous;
    let previous_roots = crate::async_context::root_snapshot(&scope, &previous);
    before(resource.ids.async_id, resource.ids.trigger_async_id);
    let prev_this = crate::object::js_implicit_this_set(this_arg_handle.get_nanbox_f64());
    let result = if args_array == 0 {
        unsafe { js_closure_call_array(callback as i64, ptr::null(), 0) }
    } else {
        let arr = args_array_handle.get_raw_const_ptr::<ArrayHeader>();
        let len = js_array_length(arr) as i64;
        let data = if arr.is_null() {
            ptr::null()
        } else {
            unsafe { (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64 }
        };
        unsafe { js_closure_call_array(callback as i64, data, len) }
    };
    crate::object::js_implicit_this_set(prev_this);
    let result_handle = scope.root_nanbox_f64(result);
    after(resource.ids.async_id);
    crate::async_context::refresh_snapshot_from_roots(
        &mut resource_context,
        &resource_context_roots,
    );
    crate::async_context::refresh_snapshot_from_roots(&mut previous, &previous_roots);
    crate::async_context::restore_context(previous);
    result_handle.get_nanbox_f64()
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
    let callback = js_closure_get_capture_f64(closure, 1);
    let mut this_arg = js_closure_get_capture_f64(closure, 2);
    if handle == 0 {
        return TAG_UNDEFINED_F64;
    }
    if JSValue::from_bits(this_arg.to_bits()).is_undefined() {
        this_arg = crate::object::js_implicit_this_get();
    }
    let args_array_ptr = ptr_from_nanboxed(rest) as i64;
    js_async_resource_run_in_async_scope(handle, callback, this_arg, args_array_ptr)
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
pub extern "C" fn js_async_resource_bind(handle: i64, callback_value: f64, this_arg: f64) -> i64 {
    validate_bind_callback(callback_value);
    if handle == 0 {
        return 0;
    }
    register_bind_trampoline_once();
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let closure = js_closure_alloc(async_resource_bind_trampoline as *const u8, 3);
    if closure.is_null() {
        return 0;
    }
    let closure_handle = scope.root_raw_mut_ptr(closure);
    js_closure_set_capture_ptr(closure_handle.get_raw_mut_ptr(), 0, handle);
    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        1,
        callback_handle.get_nanbox_f64(),
    );
    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        2,
        this_arg_handle.get_nanbox_f64(),
    );
    if let Some(length) =
        crate::closure::closure_length(crate::fs::extract_closure_ptr(callback_value))
    {
        crate::object::set_builtin_closure_length(
            closure_handle.get_raw_mut_ptr::<ClosureHeader>() as usize,
            length,
        );
    }
    crate::object::set_bound_native_closure_name(
        closure_handle.get_raw_mut_ptr::<ClosureHeader>(),
        "bound",
    );
    closure_handle.get_raw_mut_ptr::<ClosureHeader>() as i64
}

#[no_mangle]
pub extern "C" fn js_async_resource_static_bind(callback: i64, type_value: f64) -> i64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_raw_const_ptr(callback as *const ClosureHeader);
    let callback_value = if callback_handle
        .get_raw_const_ptr::<ClosureHeader>()
        .is_null()
    {
        TAG_UNDEFINED_F64
    } else {
        box_ptr(callback_handle.get_raw_const_ptr::<ClosureHeader>() as *const u8)
    };
    let callback_value_handle = scope.root_nanbox_f64(callback_value);
    let type_handle = scope.root_nanbox_f64(type_value);
    let bound = js_async_resource_static_bind_value(
        callback_value_handle.get_nanbox_f64(),
        type_handle.get_nanbox_f64(),
        TAG_UNDEFINED_F64,
    );
    ptr_from_nanboxed(bound) as i64
}

pub extern "C" fn js_async_resource_static_bind_value(
    callback_value: f64,
    type_value: f64,
    this_arg: f64,
) -> f64 {
    validate_bind_callback(callback_value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let type_value = if JSValue::from_bits(type_value.to_bits()).is_undefined() {
        let default_type = b"AsyncResource";
        box_string(
            js_string_from_bytes(default_type.as_ptr(), default_type.len() as u32) as *const u8,
        )
    } else {
        type_value
    };
    let type_handle = scope.root_nanbox_f64(type_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let handle = js_async_resource_new(type_handle.get_nanbox_f64(), TAG_UNDEFINED_F64);
    let bound = js_async_resource_bind(
        handle,
        callback_handle.get_nanbox_f64(),
        this_arg_handle.get_nanbox_f64(),
    );
    if bound == 0 {
        TAG_UNDEFINED_F64
    } else {
        crate::value::js_nanbox_pointer(bound)
    }
}

#[no_mangle]
pub extern "C" fn js_async_resource_static_bind_direct(
    callback_value: f64,
    type_value: f64,
    this_arg: f64,
    _rest: i64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, type_value, this_arg)
}

pub extern "C" fn js_async_resource_static_bind_method(
    _closure: *const ClosureHeader,
    callback_value: f64,
    type_value: f64,
    this_arg: f64,
    _rest: f64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, type_value, this_arg)
}

pub extern "C" fn js_async_local_storage_static_bind_method(
    _closure: *const ClosureHeader,
    callback_value: f64,
    _rest: f64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, TAG_UNDEFINED_F64, TAG_UNDEFINED_F64)
}

#[no_mangle]
pub extern "C" fn js_async_local_storage_static_bind_direct(
    callback_value: f64,
    _rest: i64,
) -> f64 {
    js_async_resource_static_bind_value(callback_value, TAG_UNDEFINED_F64, TAG_UNDEFINED_F64)
}

fn register_context_snapshot(snapshot: crate::async_context::AsyncContextSnapshot) -> usize {
    let id = NEXT_CONTEXT_SNAPSHOT_ID.fetch_add(1, Ordering::Relaxed);
    CONTEXT_SNAPSHOTS.lock().unwrap().insert(id, snapshot);
    id
}

fn run_with_context_snapshot(snapshot_id: usize, f: impl FnOnce() -> f64) -> f64 {
    let snapshot = CONTEXT_SNAPSHOTS
        .lock()
        .unwrap()
        .get(&snapshot_id)
        .cloned()
        .unwrap_or_default();
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut snapshot = snapshot;
    let snapshot_roots = crate::async_context::root_snapshot(&scope, &snapshot);
    let previous = crate::async_context::enter_context(&snapshot);
    let mut previous = previous;
    let previous_roots = crate::async_context::root_snapshot(&scope, &previous);
    let result = f();
    let result_handle = scope.root_nanbox_f64(result);
    crate::async_context::refresh_snapshot_from_roots(&mut snapshot, &snapshot_roots);
    crate::async_context::refresh_snapshot_from_roots(&mut previous, &previous_roots);
    crate::async_context::restore_context(previous);
    result_handle.get_nanbox_f64()
}

fn call_callback_with_rest(callback_value: f64, this_arg: f64, rest: f64) -> f64 {
    if !is_callable_value(callback_value) {
        throw_apply_not_function(callback_value);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle = scope.root_nanbox_f64(callback_value);
    let this_arg_handle = scope.root_nanbox_f64(this_arg);
    let rebound_bits = crate::closure::clone_closure_rebind_this(
        callback_handle.get_nanbox_f64().to_bits(),
        this_arg_handle.get_nanbox_f64(),
    );
    let rebound_handle = scope.root_nanbox_f64(f64::from_bits(rebound_bits));
    let callback = crate::fs::extract_closure_ptr(rebound_handle.get_nanbox_f64());
    if callback.is_null() {
        throw_apply_not_function(callback_handle.get_nanbox_f64());
    }
    let args_array = ptr_from_nanboxed(rest) as *const ArrayHeader;
    let args_array_handle = scope.root_raw_const_ptr(args_array);
    let prev_this = crate::object::js_implicit_this_set(this_arg_handle.get_nanbox_f64());
    let result = if args_array.is_null() {
        unsafe { js_closure_call_array(callback as i64, ptr::null(), 0) }
    } else {
        let arr = args_array_handle.get_raw_const_ptr::<ArrayHeader>();
        let len = js_array_length(arr) as i64;
        let data = if arr.is_null() || len == 0 {
            ptr::null()
        } else {
            unsafe { (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64 }
        };
        unsafe { js_closure_call_array(callback as i64, data, len) }
    };
    crate::object::js_implicit_this_set(prev_this);
    result
}

extern "C" fn async_local_storage_snapshot_trampoline(
    closure: *const ClosureHeader,
    callback_value: f64,
    rest: f64,
) -> f64 {
    validate_bind_callback(callback_value);
    let snapshot_id = js_closure_get_capture_ptr(closure, 0) as usize;
    let this_arg = crate::object::js_implicit_this_get();
    run_with_context_snapshot(snapshot_id, || {
        call_callback_with_rest(callback_value, this_arg, rest)
    })
}

fn register_snapshot_trampoline_once() {
    thread_local! {
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    REGISTERED.with(|flag| {
        if !flag.get() {
            js_register_closure_rest(async_local_storage_snapshot_trampoline as *const u8, 1);
            flag.set(true);
        }
    });
}

fn async_local_storage_static_snapshot_value() -> f64 {
    register_snapshot_trampoline_once();
    let snapshot_id = register_context_snapshot(crate::async_context::capture_context());
    let closure = js_closure_alloc(async_local_storage_snapshot_trampoline as *const u8, 1);
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    js_closure_set_capture_ptr(closure, 0, snapshot_id as i64);
    crate::object::set_builtin_closure_length(closure as usize, 1);
    crate::object::set_bound_native_closure_name(closure, "bound");
    crate::value::js_nanbox_pointer(closure as i64)
}

pub extern "C" fn js_async_local_storage_static_snapshot_method(
    _closure: *const ClosureHeader,
    _rest: f64,
) -> f64 {
    async_local_storage_static_snapshot_value()
}

#[no_mangle]
pub extern "C" fn js_async_local_storage_static_snapshot_direct(_rest: i64) -> f64 {
    async_local_storage_static_snapshot_value()
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
        crate::async_context::scan_snapshot_roots_mut(&mut meta.context, visitor);
    }
    drop(resources);

    let mut snapshots = CONTEXT_SNAPSHOTS.lock().unwrap();
    for snapshot in snapshots.values_mut() {
        crate::async_context::scan_snapshot_roots_mut(snapshot, visitor);
    }
    drop(snapshots);

    let mut providers_bits = ASYNC_WRAP_PROVIDERS.load(Ordering::Relaxed);
    if providers_bits != 0 {
        visitor.visit_nanbox_u64_slot(&mut providers_bits);
        ASYNC_WRAP_PROVIDERS.store(providers_bits, Ordering::Relaxed);
    }
}

#[cfg(test)]
pub fn reset_for_tests() {
    HOOKS.lock().unwrap().clear();
    RESOURCES.lock().unwrap().clear();
    GC_DESTROY_QUEUE.lock().unwrap().clear();
    CONTEXT_SNAPSHOTS.lock().unwrap().clear();
    ASYNC_WRAP_PROVIDERS.store(0, Ordering::Relaxed);
    HOOKS_ACTIVE.store(0, Ordering::Relaxed);
    NEXT_ASYNC_ID.store(1, Ordering::Relaxed);
    NEXT_CONTEXT_SNAPSHOT_ID.store(1, Ordering::Relaxed);
    CURRENT_EXECUTION_ID.with(|c| c.set(0));
    CURRENT_TRIGGER_ID.with(|c| c.set(0));
    IN_HOOK_CALLBACK.with(|c| c.set(false));
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
            context: crate::async_context::AsyncContextSnapshot::default(),
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
