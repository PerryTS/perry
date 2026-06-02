//! Minimal `node:cluster` primary lifecycle support.
//!
//! This intentionally builds on the existing `child_process.fork()` IPC
//! reactor. It tracks primary-side settings and Worker handles, but does not
//! implement socket/listening handle distribution.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

use crate::array::ArrayHeader;
use crate::closure::{js_closure_get_capture_f64, ClosureHeader};
use crate::object::{
    js_implicit_this_set, js_object_alloc, js_object_delete_field, js_object_get_field_by_name_f64,
    js_object_keys, js_object_set_field_by_name, ObjectHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

const TAG_UNDEFINED_F64: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);
const TAG_TRUE_F64: f64 = f64::from_bits(crate::value::TAG_TRUE);
const TAG_FALSE_F64: f64 = f64::from_bits(crate::value::TAG_FALSE);
const CLUSTER_SHAPE_ID: u32 = 0x7FFF_FC80;

#[derive(Default)]
struct ClusterState {
    setup_called: bool,
    next_worker_id: u32,
    settings_bits: u64,
    workers_bits: u64,
    self_worker_bits: u64,
    worker_bits_by_id: Vec<(u32, u64)>,
    disconnect_callbacks: Vec<u64>,
}

impl ClusterState {
    fn new() -> Self {
        Self {
            next_worker_id: 1,
            ..Self::default()
        }
    }
}

thread_local! {
    static CLUSTER_STATE: RefCell<ClusterState> = RefCell::new(ClusterState::new());
}

static CLUSTER_INIT: Once = Once::new();

fn empty_object_value() -> f64 {
    let obj = js_object_alloc(0, 0);
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

pub fn cluster_property(property: &str) -> Option<f64> {
    ensure_cluster_runtime();

    let is_worker = std::env::var("NODE_UNIQUE_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some();

    if is_worker {
        match property {
            "isPrimary" | "isMaster" => return Some(TAG_FALSE_F64),
            "isWorker" => return Some(TAG_TRUE_F64),
            "worker" => return Some(self_worker_value()),
            "workers" | "settings" => return Some(TAG_UNDEFINED_F64),
            _ => {}
        }
    }

    match property {
        "isPrimary" | "isMaster" => Some(TAG_TRUE_F64),
        "isWorker" => Some(TAG_FALSE_F64),
        "worker" => Some(TAG_UNDEFINED_F64),
        "workers" => Some(workers_value()),
        "settings" => Some(settings_value()),
        "schedulingPolicy" | "SCHED_RR" => Some(2.0),
        "SCHED_NONE" => Some(1.0),
        "_events" => Some(empty_object_value()),
        "_eventsCount" => Some(0.0),
        "_maxListeners" => Some(TAG_UNDEFINED_F64),
        "on" | "addListener" => Some(TAG_UNDEFINED_F64),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// node:cluster default-import EventEmitter (#3687)
//
// In Node, `node:cluster` is a singleton EventEmitter, so the *default* import
// (`import cluster from "node:cluster"`) exposes `on`/`once`/`emit`/… while the
// *namespace* import (`import * as cluster`) does not (those live on
// EventEmitter.prototype, not as named module exports). Perry models the
// default import as a distinct `cluster.default` native-module namespace whose
// EventEmitter method reads resolve here; the namespace import keeps the
// `undefined` shape via `cluster_property`. Real worker-lifecycle events are
// still deferred (closed umbrella #3605) — this is module-level listener
// bookkeeping plus a synchronous `fork` emit so feature-detection and manual
// `emit()` round-trips match Node.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct ClusterListener {
    callback_bits: u64,
    once: bool,
}

#[derive(Default)]
struct ClusterEmitter {
    events: HashMap<String, Vec<ClusterListener>>,
    order: Vec<String>,
}

thread_local! {
    static CLUSTER_EMITTER: RefCell<ClusterEmitter> = RefCell::new(ClusterEmitter::default());
}

/// The `cluster.default` namespace object — the value bound by
/// `import cluster from "node:cluster"`. Cached (see
/// `should_cache_native_module_namespace`), so EventEmitter methods can return
/// it for `cluster.on(...) === cluster` chaining.
fn cluster_default_value() -> f64 {
    crate::object::js_create_native_module_namespace(
        b"cluster.default".as_ptr(),
        "cluster.default".len(),
    )
}

fn cluster_emitter_event_name(event: f64) -> Option<String> {
    let jv = JSValue::from_bits(event.to_bits());
    if jv.is_string() || jv.is_short_string() {
        return value_to_string(event);
    }
    // Non-string event names follow EventEmitter ToString semantics.
    let coerced = crate::value::js_jsvalue_to_string(event);
    if coerced.is_null() {
        return None;
    }
    value_to_string(f64::from_bits(JSValue::string_ptr(coerced).bits()))
}

fn cluster_register_listener(event: f64, listener: f64, once: bool, prepend: bool) -> f64 {
    if !is_closure_value(listener) {
        return cluster_default_value();
    }
    if let Some(name) = cluster_emitter_event_name(event) {
        CLUSTER_EMITTER.with(|emitter| {
            let mut emitter = emitter.borrow_mut();
            if !emitter.order.iter().any(|n| n == &name) {
                emitter.order.push(name.clone());
            }
            let entry = ClusterListener {
                callback_bits: listener.to_bits(),
                once,
            };
            let listeners = emitter.events.entry(name).or_default();
            if prepend {
                listeners.insert(0, entry);
            } else {
                listeners.push(entry);
            }
        });
    }
    cluster_default_value()
}

pub(crate) fn cluster_emit_event(event: &str, args: &[f64]) -> bool {
    let listeners = CLUSTER_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        let Some(listeners) = emitter.events.get_mut(event) else {
            return Vec::new();
        };
        let snapshot = listeners.clone();
        if snapshot.iter().any(|l| l.once) {
            listeners.retain(|l| !l.once);
            if listeners.is_empty() {
                emitter.events.remove(event);
                emitter.order.retain(|n| n != event);
            }
        }
        snapshot
    });
    if listeners.is_empty() {
        return false;
    }
    for listener in listeners {
        let cb = f64::from_bits(listener.callback_bits);
        let prev = js_implicit_this_set(cluster_default_value());
        unsafe {
            let _ = crate::closure::js_native_call_value(cb, args.as_ptr(), args.len());
        }
        js_implicit_this_set(prev);
    }
    true
}

fn cluster_emitter_scan(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CLUSTER_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        for listeners in emitter.events.values_mut() {
            for listener in listeners {
                visitor.visit_nanbox_u64_slot(&mut listener.callback_bits);
            }
        }
    });
}

#[no_mangle]
pub extern "C" fn js_cluster_on(event: f64, listener: f64) -> f64 {
    ensure_cluster_runtime();
    cluster_register_listener(event, listener, false, false)
}

#[no_mangle]
pub extern "C" fn js_cluster_once(event: f64, listener: f64) -> f64 {
    ensure_cluster_runtime();
    cluster_register_listener(event, listener, true, false)
}

#[no_mangle]
pub extern "C" fn js_cluster_prepend_listener(event: f64, listener: f64) -> f64 {
    ensure_cluster_runtime();
    cluster_register_listener(event, listener, false, true)
}

#[no_mangle]
pub extern "C" fn js_cluster_prepend_once_listener(event: f64, listener: f64) -> f64 {
    ensure_cluster_runtime();
    cluster_register_listener(event, listener, true, true)
}

#[no_mangle]
pub extern "C" fn js_cluster_emit(event: f64, args: *const ArrayHeader) -> f64 {
    ensure_cluster_runtime();
    let Some(name) = cluster_emitter_event_name(event) else {
        return TAG_FALSE_F64;
    };
    let values = if args.is_null() {
        Vec::new()
    } else {
        let len = crate::array::js_array_length(args);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(crate::array::js_array_get_f64(args, i));
        }
        out
    };
    if cluster_emit_event(&name, &values) {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

#[no_mangle]
pub extern "C" fn js_cluster_event_names() -> f64 {
    ensure_cluster_runtime();
    let names = CLUSTER_EMITTER.with(|emitter| {
        let emitter = emitter.borrow();
        emitter
            .order
            .iter()
            .filter(|n| emitter.events.get(*n).is_some_and(|l| !l.is_empty()))
            .cloned()
            .collect::<Vec<_>>()
    });
    let mut arr = crate::array::js_array_alloc(names.len() as u32);
    for name in names {
        arr = crate::array::js_array_push(arr, JSValue::string_ptr(str_key(name.as_bytes())));
    }
    box_ptr(arr as *const u8)
}

#[no_mangle]
pub extern "C" fn js_cluster_listener_count(event: f64) -> f64 {
    ensure_cluster_runtime();
    let Some(name) = cluster_emitter_event_name(event) else {
        return 0.0;
    };
    CLUSTER_EMITTER.with(|emitter| {
        emitter
            .borrow()
            .events
            .get(&name)
            .map(|l| l.len() as f64)
            .unwrap_or(0.0)
    })
}

#[no_mangle]
pub extern "C" fn js_cluster_remove_listener(event: f64, listener: f64) -> f64 {
    ensure_cluster_runtime();
    if let Some(name) = cluster_emitter_event_name(event) {
        let bits = listener.to_bits();
        CLUSTER_EMITTER.with(|emitter| {
            let mut emitter = emitter.borrow_mut();
            if let Some(listeners) = emitter.events.get_mut(&name) {
                if let Some(pos) = listeners.iter().rposition(|l| l.callback_bits == bits) {
                    listeners.remove(pos);
                }
                if listeners.is_empty() {
                    emitter.events.remove(&name);
                    emitter.order.retain(|n| n != &name);
                }
            }
        });
    }
    cluster_default_value()
}

#[no_mangle]
pub extern "C" fn js_cluster_remove_all_listeners(event: f64) -> f64 {
    ensure_cluster_runtime();
    let jv = JSValue::from_bits(event.to_bits());
    let target = if jv.is_undefined() || jv.is_null() {
        None
    } else {
        cluster_emitter_event_name(event)
    };
    CLUSTER_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        match target {
            Some(name) => {
                emitter.events.remove(&name);
                emitter.order.retain(|n| n != &name);
            }
            None => {
                emitter.events.clear();
                emitter.order.clear();
            }
        }
    });
    cluster_default_value()
}

#[no_mangle]
pub extern "C" fn js_cluster_setup_primary(settings: f64) -> f64 {
    ensure_cluster_runtime();
    apply_setup_primary(settings);
    TAG_UNDEFINED_F64
}

#[no_mangle]
pub extern "C" fn js_cluster_fork(env: f64) -> f64 {
    ensure_cluster_runtime();

    let needs_setup = CLUSTER_STATE.with(|state| !state.borrow().setup_called);
    if needs_setup {
        apply_setup_primary(TAG_UNDEFINED_F64);
    }

    let settings = settings_value();
    let module = get_field(settings, b"exec");
    let module_ptr = crate::string::js_string_materialize_to_heap(module) as i64;
    if module_ptr == 0 {
        return TAG_UNDEFINED_F64;
    }

    let args = get_field(settings, b"args");
    let args_ptr = array_ptr(args).map(|p| p as i64).unwrap_or(0);
    let opts = build_fork_options(settings, env);
    let worker =
        crate::child_process::fork::js_child_process_fork(module_ptr, args_ptr, opts as i64);
    if object_ptr(worker).is_none() {
        return worker;
    }

    let id = CLUSTER_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let id = state.next_worker_id;
        state.next_worker_id = state.next_worker_id.saturating_add(1).max(1);
        id
    });

    decorate_worker(worker, id);
    register_worker(id, worker);
    // Node fires the cluster-level `fork` event synchronously when the worker
    // object is created (`online`/`exit`/etc. remain deferred — #3605).
    cluster_emit_event("fork", &[worker]);
    worker
}

#[no_mangle]
pub extern "C" fn js_cluster_disconnect(callback: f64) -> f64 {
    ensure_cluster_runtime();

    if is_closure_value(callback) {
        CLUSTER_STATE.with(|state| {
            state
                .borrow_mut()
                .disconnect_callbacks
                .push(callback.to_bits());
        });
    }

    let workers = CLUSTER_STATE.with(|state| state.borrow().worker_bits_by_id.clone());
    if workers.is_empty() {
        drain_disconnect_callbacks_if_idle();
        return TAG_UNDEFINED_F64;
    }

    for (_, bits) in workers {
        let worker = f64::from_bits(bits);
        call_worker_disconnect(worker);
    }

    drain_disconnect_callbacks_if_idle();
    TAG_UNDEFINED_F64
}

fn ensure_cluster_runtime() {
    CLUSTER_INIT.call_once(|| {
        crate::gc::gc_register_mutable_root_scanner_named("node_cluster", cluster_root_scanner);
        register_cluster_arities();
    });
}

fn cluster_root_scanner(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CLUSTER_STATE.with(|state| {
        let mut state = state.borrow_mut();
        visitor.visit_nanbox_u64_slot(&mut state.settings_bits);
        visitor.visit_nanbox_u64_slot(&mut state.workers_bits);
        visitor.visit_nanbox_u64_slot(&mut state.self_worker_bits);
        for (_, bits) in &mut state.worker_bits_by_id {
            visitor.visit_nanbox_u64_slot(bits);
        }
        for bits in &mut state.disconnect_callbacks {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
    cluster_emitter_scan(visitor);
}

fn register_cluster_arities() {
    let arities: [(*const u8, u32); 6] = [
        (worker_is_connected as *const u8, 0),
        (worker_is_dead as *const u8, 0),
        (worker_disconnect as *const u8, 0),
        (worker_destroy as *const u8, 0),
        (cluster_internal_online as *const u8, 0),
        (cluster_internal_exit as *const u8, 2),
    ];
    for (func, arity) in arities {
        crate::closure::js_register_closure_arity(func, arity);
    }
}

fn settings_value() -> f64 {
    CLUSTER_STATE.with(|state| {
        let bits = state.borrow().settings_bits;
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let settings = alloc_object_value(0);
        state.borrow_mut().settings_bits = settings.to_bits();
        settings
    })
}

fn workers_value() -> f64 {
    CLUSTER_STATE.with(|state| {
        let bits = state.borrow().workers_bits;
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let workers = alloc_object_value(0);
        state.borrow_mut().workers_bits = workers.to_bits();
        workers
    })
}

fn self_worker_value() -> f64 {
    CLUSTER_STATE.with(|state| {
        let bits = state.borrow().self_worker_bits;
        if bits != 0 {
            return f64::from_bits(bits);
        }
        let worker = alloc_object_value(3);
        let id = std::env::var("NODE_UNIQUE_ID")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        set_field(worker, b"id", id);
        set_field(worker, b"process", TAG_UNDEFINED_F64);
        set_field(worker, b"exitedAfterDisconnect", TAG_FALSE_F64);
        state.borrow_mut().self_worker_bits = worker.to_bits();
        worker
    })
}

fn apply_setup_primary(settings_arg: f64) {
    let previous = settings_value();
    let next = alloc_default_settings();
    copy_object_fields(previous, next);
    copy_object_fields(settings_arg, next);

    CLUSTER_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.setup_called = true;
        state.settings_bits = next.to_bits();
    });
}

fn alloc_default_settings() -> f64 {
    let settings = alloc_object_value(8);
    set_field(settings, b"args", default_args_array_value());
    set_field(settings, b"exec", box_string(&default_exec_path()));
    set_field(settings, b"execArgv", alloc_array_value(0));
    set_field(settings, b"silent", TAG_FALSE_F64);
    settings
}

fn default_args_array_value() -> f64 {
    let argv = crate::os::js_process_argv();
    let len = crate::array::js_array_length(argv);
    let mut args = crate::array::js_array_alloc(len.saturating_sub(2));
    for i in 2..len {
        args = crate::array::js_array_push_f64(args, crate::array::js_array_get_f64(argv, i));
    }
    box_ptr(args as *const u8)
}

fn default_exec_path() -> String {
    std::env::args()
        .nth(1)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .or_else(|| std::env::args().next())
        .unwrap_or_default()
}

fn copy_object_fields(src: f64, dst: f64) {
    let Some(src_obj) = object_ptr(src) else {
        return;
    };
    let keys = js_object_keys(src_obj);
    if keys.is_null() {
        return;
    }
    let n = crate::array::js_array_length(keys);
    for i in 0..n {
        let key_val = crate::array::js_array_get_f64(keys, i);
        let Some(name) = value_to_string(key_val) else {
            continue;
        };
        let value = get_field(src, name.as_bytes());
        set_field(dst, name.as_bytes(), value);
    }
}

fn build_fork_options(settings: f64, env_arg: f64) -> *mut ObjectHeader {
    let opts = js_object_alloc(0, 10);
    let opts_val = box_ptr(opts as *const u8);

    copy_setting_to_option(settings, opts_val, b"cwd");
    copy_setting_to_option(settings, opts_val, b"execArgv");
    copy_setting_to_option(settings, opts_val, b"execPath");
    copy_setting_to_option(settings, opts_val, b"serialization");
    copy_setting_to_option(settings, opts_val, b"silent");
    copy_setting_to_option(settings, opts_val, b"stdio");
    copy_setting_to_option(settings, opts_val, b"uid");
    copy_setting_to_option(settings, opts_val, b"gid");
    copy_setting_to_option(settings, opts_val, b"windowsHide");

    let worker_id = CLUSTER_STATE.with(|state| state.borrow().next_worker_id);
    let env = build_worker_env(env_arg, worker_id);
    set_field(opts_val, b"env", env);
    opts
}

fn copy_setting_to_option(settings: f64, opts: f64, name: &[u8]) {
    let value = get_field(settings, name);
    if !JSValue::from_bits(value.to_bits()).is_undefined() {
        set_field(opts, name, value);
    }
}

fn build_worker_env(env_arg: f64, worker_id: u32) -> f64 {
    let env = alloc_object_value(16);
    copy_object_fields(crate::process::js_process_env(), env);
    copy_object_fields(env_arg, env);
    set_field(env, b"NODE_UNIQUE_ID", box_string(&worker_id.to_string()));
    set_field(env, b"NODE_CLUSTER_SCHED_POLICY", box_string("rr"));
    env
}

fn decorate_worker(worker: f64, id: u32) {
    set_field(worker, b"id", id as f64);
    set_field(worker, b"process", worker);
    set_field(worker, b"exitedAfterDisconnect", TAG_FALSE_F64);
    set_field(worker, b"__clusterWorker", TAG_TRUE_F64);
    set_field(worker, b"__clusterState", box_string("online"));

    let original_disconnect = get_field(worker, b"disconnect");
    set_field(worker, b"__clusterDisconnect", original_disconnect);
    set_field(
        worker,
        b"isConnected",
        closure_value(worker_is_connected as *const u8, worker),
    );
    set_field(
        worker,
        b"isDead",
        closure_value(worker_is_dead as *const u8, worker),
    );
    set_field(
        worker,
        b"disconnect",
        closure_value(worker_disconnect as *const u8, worker),
    );
    set_field(
        worker,
        b"destroy",
        closure_value(worker_destroy as *const u8, worker),
    );

    register_listener(
        worker,
        "spawn",
        closure_value(cluster_internal_online as *const u8, worker),
    );
    register_listener(
        worker,
        "exit",
        closure_value(cluster_internal_exit as *const u8, worker),
    );
}

pub(crate) fn consume_internal_message(worker: f64, message: f64) -> bool {
    let marker = get_field(worker, b"__clusterWorker");
    if marker.to_bits() != TAG_TRUE_F64.to_bits() {
        return false;
    }

    let Some(cmd) = value_to_string(get_field(message, b"cmd")) else {
        return false;
    };
    if cmd != "NODE_CLUSTER" {
        return false;
    }

    if let Some(act) = value_to_string(get_field(message, b"act")) {
        if act == "online" {
            set_field(worker, b"__clusterState", box_string("online"));
        }
    }
    true
}

fn register_worker(id: u32, worker: f64) {
    let workers = workers_value();
    set_field(workers, id.to_string().as_bytes(), worker);
    CLUSTER_STATE.with(|state| {
        state
            .borrow_mut()
            .worker_bits_by_id
            .push((id, worker.to_bits()));
    });
}

fn remove_worker(worker: f64) {
    let mut removed_ids = Vec::new();
    CLUSTER_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let bits = worker.to_bits();
        let mut kept = Vec::with_capacity(state.worker_bits_by_id.len());
        for (id, worker_bits) in state.worker_bits_by_id.drain(..) {
            if worker_bits == bits {
                removed_ids.push(id);
            } else {
                kept.push((id, worker_bits));
            }
        }
        state.worker_bits_by_id = kept;
    });

    if !removed_ids.is_empty() {
        let workers = workers_value();
        if let Some(obj) = object_ptr(workers) {
            for id in removed_ids {
                let key_name = id.to_string();
                let key = js_string_from_bytes(key_name.as_ptr(), key_name.len() as u32);
                js_object_delete_field(obj, key);
            }
        }
    }
}

fn drain_disconnect_callbacks_if_idle() {
    let callbacks = CLUSTER_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.worker_bits_by_id.is_empty() {
            return Vec::new();
        }
        std::mem::take(&mut state.disconnect_callbacks)
    });

    for bits in callbacks {
        let cb = f64::from_bits(bits);
        unsafe {
            let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
        }
    }
}

extern "C" fn worker_is_connected(closure: *const ClosureHeader) -> f64 {
    let worker = closure_this(closure);
    let connected = get_field(worker, b"connected");
    if JSValue::from_bits(connected.to_bits()).is_bool()
        && connected.to_bits() == crate::value::TAG_TRUE
    {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

extern "C" fn worker_is_dead(closure: *const ClosureHeader) -> f64 {
    if is_worker_dead(closure_this(closure)) {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

extern "C" fn worker_disconnect(closure: *const ClosureHeader) -> f64 {
    let worker = closure_this(closure);
    set_field(worker, b"exitedAfterDisconnect", TAG_TRUE_F64);
    call_original_disconnect(worker);
    worker
}

extern "C" fn worker_destroy(closure: *const ClosureHeader) -> f64 {
    let worker = closure_this(closure);
    set_field(worker, b"exitedAfterDisconnect", TAG_TRUE_F64);
    let kill = get_field(worker, b"kill");
    if is_closure_value(kill) {
        unsafe {
            let _ = crate::closure::js_native_call_value(kill, std::ptr::null(), 0);
        }
    }
    worker
}

extern "C" fn cluster_internal_online(closure: *const ClosureHeader) -> f64 {
    let worker = closure_this(closure);
    set_field(worker, b"__clusterState", box_string("online"));
    emit(worker, "online", &[]);
    TAG_UNDEFINED_F64
}

extern "C" fn cluster_internal_exit(
    closure: *const ClosureHeader,
    _code: f64,
    _signal: f64,
) -> f64 {
    let worker = closure_this(closure);
    set_field(worker, b"__clusterState", box_string("dead"));
    remove_worker(worker);
    drain_disconnect_callbacks_if_idle();
    TAG_UNDEFINED_F64
}

fn call_worker_disconnect(worker: f64) {
    let disconnect = get_field(worker, b"disconnect");
    if is_closure_value(disconnect) {
        unsafe {
            let _ = crate::closure::js_native_call_value(disconnect, std::ptr::null(), 0);
        }
    }
}

fn call_original_disconnect(worker: f64) {
    let original = get_field(worker, b"__clusterDisconnect");
    if !is_closure_value(original) {
        return;
    }
    unsafe {
        let _ = crate::closure::js_native_call_value(original, std::ptr::null(), 0);
    }
}

fn is_worker_dead(worker: f64) -> bool {
    let exit_code = get_field(worker, b"exitCode");
    let signal_code = get_field(worker, b"signalCode");
    !JSValue::from_bits(exit_code.to_bits()).is_null()
        || !JSValue::from_bits(signal_code.to_bits()).is_null()
}

fn closure_this(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        TAG_UNDEFINED_F64
    } else {
        js_closure_get_capture_f64(closure, 0)
    }
}

fn closure_value(func: *const u8, captured: f64) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, captured);
    box_ptr(closure as *const u8)
}

fn register_listener(target: f64, event: &str, cb: f64) {
    let key = listener_key(event);
    let arr = match array_ptr(get_field(target, &key)) {
        Some(a) => a,
        None => crate::array::js_array_alloc(2),
    };
    let arr = crate::array::js_array_push_f64(arr, cb);
    set_field(target, &key, box_ptr(arr as *const u8));
}

fn emit(target: f64, event: &str, args: &[f64]) -> bool {
    let key = listener_key(event);
    let mut i = 0;
    let mut fired = false;
    loop {
        let Some(arr) = array_ptr(get_field(target, &key)) else {
            break;
        };
        if i >= crate::array::js_array_length(arr) {
            break;
        }
        let cb = crate::array::js_array_get_f64(arr, i);
        let prev = js_implicit_this_set(target);
        unsafe {
            let _ = crate::closure::js_native_call_value(cb, args.as_ptr(), args.len());
        }
        js_implicit_this_set(prev);
        fired = true;
        i += 1;
    }
    fired
}

fn listener_key(event: &str) -> Vec<u8> {
    let mut key = b"__cpL_".to_vec();
    key.extend_from_slice(event.as_bytes());
    key
}

fn alloc_object_value(field_count: u32) -> f64 {
    box_ptr(js_object_alloc(CLUSTER_SHAPE_ID, field_count) as *const u8)
}

fn alloc_array_value(capacity: u32) -> f64 {
    box_ptr(crate::array::js_array_alloc(capacity) as *const u8)
}

fn box_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn box_string(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn str_key(bytes: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn get_field(value: f64, name: &[u8]) -> f64 {
    match object_ptr(value) {
        Some(obj) => js_object_get_field_by_name_f64(obj, str_key(name)),
        None => TAG_UNDEFINED_F64,
    }
}

fn set_field(value: f64, name: &[u8], field_value: f64) {
    if let Some(obj) = object_ptr(value) {
        js_object_set_field_by_name(obj, str_key(name), field_value);
    }
}

fn object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        let header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

fn array_ptr(value: f64) -> Option<*mut ArrayHeader> {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw < 0x10000 {
        return None;
    }
    unsafe {
        let header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        match (*header).obj_type {
            crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_LAZY_ARRAY => {
                Some(raw as *mut ArrayHeader)
            }
            _ => None,
        }
    }
}

fn is_closure_value(value: f64) -> bool {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let raw = if (0x7FF8..=0x7FFF).contains(&top16) {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return false;
    };
    raw >= 0x1000 && crate::closure::is_closure_ptr(raw)
}

fn value_to_string(value: f64) -> Option<String> {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len))
            .ok()
            .map(|s| s.to_string())
    }
}
