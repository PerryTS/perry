use super::*;

pub(super) fn worker_id_from_receiver(receiver: i64) -> Option<u64> {
    if receiver == 0 {
        return None;
    }
    let receiver_value = perry_runtime::value::js_nanbox_pointer(receiver);
    let thread_id = get_object_field_from_value(receiver_value, "threadId");
    if thread_id.is_finite() && thread_id >= 1.0 {
        Some(thread_id as u64)
    } else {
        None
    }
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_post_message(receiver: i64, value: f64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    let message = unsafe { serialize_nanbox_for_thread(value.to_bits()) };
    let sender = WORKERS
        .lock()
        .unwrap()
        .get(&worker_id)
        .map(|worker| worker.sender.clone());
    if let Some(sender) = sender {
        let _ = sender.send(WorkerCommand::Message(message));
    }
    js_undefined()
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_on(receiver: i64, event: f64, callback: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    let callback_value = perry_runtime::value::js_nanbox_pointer(callback);
    worker_add_listener(worker_id, event, callback_value, false)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_once(receiver: i64, event: f64, callback: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    let callback_value = perry_runtime::value::js_nanbox_pointer(callback);
    worker_add_listener(worker_id, event, callback_value, true)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_off(receiver: i64, event: f64, callback: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    let Some(event) = event_name(event) else {
        return js_undefined();
    };
    let callback_bits = perry_runtime::value::js_nanbox_pointer(callback).to_bits();
    let mut workers = WORKERS.lock().unwrap();
    if let Some(worker) = workers.get_mut(&worker_id) {
        if let Some(listeners) = worker.listeners.get_mut(&event) {
            listeners.retain(|listener| listener.callback_bits != callback_bits);
        }
    }
    js_undefined()
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_terminate(receiver: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        let promise = perry_runtime::js_promise_resolved(1.0);
        return perry_runtime::value::js_nanbox_pointer(promise as i64);
    };
    worker_terminate_by_id(worker_id)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_ref(receiver: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    worker_ref_by_id(worker_id)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_unref(receiver: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    worker_unref_by_id(worker_id)
}

pub(super) fn worker_object(
    worker_id: u64,
    options: &WorkerOptions,
) -> *mut perry_runtime::object::ObjectHeader {
    // Perry exposes practical stream-shaped objects here; pipe/fd ownership is
    // not modeled yet, but the options are still parsed as Node-observable state.
    let _ = (options.stdout, options.stderr, options.track_unmanaged_fds);
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(obj, "threadId", worker_id as f64);
    set_object_field(obj, "threadName", string_value(&options.thread_name));
    set_object_field(
        obj,
        "resourceLimits",
        object_value(worker_resource_limits_object(&options.resource_limits)),
    );
    set_object_field(
        obj,
        "stdin",
        if options.stdin {
            worker_writable_stream_object()
        } else {
            js_null()
        },
    );
    set_object_field(obj, "stdout", worker_readable_stream_object());
    set_object_field(obj, "stderr", worker_readable_stream_object());
    set_object_field(
        obj,
        "performance",
        object_value(worker_performance_object()),
    );
    set_object_field(
        obj,
        "getHeapStatistics",
        closure_value_with_worker_id(worker_get_heap_statistics as *const u8, 0, worker_id),
    );
    set_object_field(
        obj,
        "cpuUsage",
        closure_value_with_worker_id(worker_cpu_usage as *const u8, 1, worker_id),
    );
    set_object_field(
        obj,
        "getHeapSnapshot",
        closure_value_with_worker_id(worker_get_heap_snapshot as *const u8, 1, worker_id),
    );
    set_object_field(
        obj,
        "startCpuProfile",
        closure_value_with_worker_id(worker_start_cpu_profile as *const u8, 0, worker_id),
    );
    set_object_field(
        obj,
        "startHeapProfile",
        closure_value_with_worker_id(worker_start_heap_profile as *const u8, 0, worker_id),
    );
    set_object_field(
        obj,
        "postMessage",
        closure_value_with_worker_id(worker_post_message as *const u8, 1, worker_id),
    );
    set_object_field(
        obj,
        "terminate",
        closure_value_with_worker_id(worker_terminate as *const u8, 0, worker_id),
    );
    set_object_field(
        obj,
        "ref",
        closure_value_with_worker_id(worker_ref as *const u8, 0, worker_id),
    );
    set_object_field(
        obj,
        "unref",
        closure_value_with_worker_id(worker_unref as *const u8, 0, worker_id),
    );
    set_object_field(
        obj,
        "on",
        closure_value_with_worker_id(worker_on as *const u8, 2, worker_id),
    );
    set_object_field(
        obj,
        "once",
        closure_value_with_worker_id(worker_once as *const u8, 2, worker_id),
    );
    set_object_field(
        obj,
        "off",
        closure_value_with_worker_id(worker_off as *const u8, 2, worker_id),
    );
    obj
}

pub(super) fn worker_resource_limits_object(
    limits: &WorkerResourceLimits,
) -> *mut perry_runtime::object::ObjectHeader {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    let keys = [
        (
            "maxYoungGenerationSizeMb",
            limits.max_young_generation_size_mb,
        ),
        ("maxOldGenerationSizeMb", limits.max_old_generation_size_mb),
        ("codeRangeSizeMb", limits.code_range_size_mb),
        ("stackSizeMb", limits.stack_size_mb),
    ];
    let mut keys_array = perry_runtime::js_array_alloc(keys.len() as u32);
    for (name, value) in keys {
        set_object_field(obj, name, value);
        let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        keys_array = perry_runtime::js_array_push(keys_array, JSValue::string_ptr(name_ptr));
    }
    perry_runtime::js_object_set_keys(obj, keys_array);
    obj
}

pub(super) fn empty_object() -> *mut perry_runtime::object::ObjectHeader {
    perry_runtime::object::js_object_alloc(0, 0)
}

fn worker_performance_object() -> *mut perry_runtime::object::ObjectHeader {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(
        obj,
        "eventLoopUtilization",
        closure_value(worker_event_loop_utilization as *const u8, 2),
    );
    obj
}

fn stream_listener_key(event: &str) -> String {
    format!("__perryWorkerStreamListener:{event}")
}

fn stream_this() -> f64 {
    perry_runtime::object::js_implicit_this_get()
}

fn stream_register(event: f64, callback: f64) -> f64 {
    let this = stream_this();
    let Some(event) = string_value_to_string(event) else {
        return this;
    };
    let key = stream_listener_key(&event);
    let arr = array_ptr_from_value(get_object_field_from_value(this, &key))
        .unwrap_or_else(|| perry_runtime::array::js_array_alloc(0));
    let arr = perry_runtime::array::js_array_push_f64(arr, callback);
    if let Some(obj) = object_ptr_from_value(this) {
        set_object_field(
            obj,
            &key,
            object_value(arr as *mut perry_runtime::object::ObjectHeader),
        );
    }
    this
}

fn stream_emit_event(event: f64, arg: f64) -> f64 {
    let this = stream_this();
    let Some(event) = string_value_to_string(event) else {
        return js_bool(false);
    };
    let key = stream_listener_key(&event);
    let Some(arr) = array_ptr_from_value(get_object_field_from_value(this, &key)) else {
        return js_bool(false);
    };
    let args = [arg];
    let len = perry_runtime::array::js_array_length(arr);
    for i in 0..len {
        let callback = perry_runtime::array::js_array_get_f64(arr, i);
        let prev_this = perry_runtime::object::js_implicit_this_set(this);
        unsafe {
            let _ =
                perry_runtime::closure::js_native_call_value(callback, args.as_ptr(), args.len());
        }
        perry_runtime::object::js_implicit_this_set(prev_this);
    }
    js_bool(len > 0)
}

fn stream_remove_listener(event: f64, callback: f64) -> f64 {
    let this = stream_this();
    let Some(event) = string_value_to_string(event) else {
        return this;
    };
    let key = stream_listener_key(&event);
    let Some(arr) = array_ptr_from_value(get_object_field_from_value(this, &key)) else {
        return this;
    };
    let len = perry_runtime::array::js_array_length(arr);
    let mut next = perry_runtime::array::js_array_alloc(len);
    for i in 0..len {
        let value = perry_runtime::array::js_array_get_f64(arr, i);
        if value.to_bits() != callback.to_bits() {
            next = perry_runtime::array::js_array_push_f64(next, value);
        }
    }
    if let Some(obj) = object_ptr_from_value(this) {
        set_object_field(
            obj,
            &key,
            object_value(next as *mut perry_runtime::object::ObjectHeader),
        );
    }
    this
}

extern "C" fn stream_on(_closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    stream_register(event, callback)
}

extern "C" fn stream_emit(_closure: *const ClosureHeader, event: f64, arg: f64) -> f64 {
    stream_emit_event(event, arg)
}

extern "C" fn stream_off(_closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    stream_remove_listener(event, callback)
}

extern "C" fn stream_this0(_closure: *const ClosureHeader) -> f64 {
    stream_this()
}

extern "C" fn stream_this1(_closure: *const ClosureHeader, _arg: f64) -> f64 {
    stream_this()
}

extern "C" fn stream_write(_closure: *const ClosureHeader, _chunk: f64, _encoding: f64) -> f64 {
    js_bool(true)
}

extern "C" fn stream_read(_closure: *const ClosureHeader, _size: f64) -> f64 {
    js_null()
}

extern "C" fn stream_pipe(_closure: *const ClosureHeader, dest: f64) -> f64 {
    dest
}

pub(super) fn worker_readable_stream_object() -> f64 {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(obj, "readable", js_bool(true));
    set_object_field(obj, "destroyed", js_bool(false));
    set_object_field(obj, "on", closure_value(stream_on as *const u8, 2));
    set_object_field(obj, "once", closure_value(stream_on as *const u8, 2));
    set_object_field(obj, "addListener", closure_value(stream_on as *const u8, 2));
    set_object_field(
        obj,
        "prependListener",
        closure_value(stream_on as *const u8, 2),
    );
    set_object_field(obj, "off", closure_value(stream_off as *const u8, 2));
    set_object_field(
        obj,
        "removeListener",
        closure_value(stream_off as *const u8, 2),
    );
    set_object_field(obj, "emit", closure_value(stream_emit as *const u8, 2));
    set_object_field(obj, "pause", closure_value(stream_this0 as *const u8, 0));
    set_object_field(obj, "resume", closure_value(stream_this0 as *const u8, 0));
    set_object_field(obj, "destroy", closure_value(stream_this0 as *const u8, 0));
    set_object_field(
        obj,
        "setEncoding",
        closure_value(stream_this1 as *const u8, 1),
    );
    set_object_field(obj, "read", closure_value(stream_read as *const u8, 1));
    set_object_field(obj, "pipe", closure_value(stream_pipe as *const u8, 1));
    object_value(obj)
}

fn worker_writable_stream_object() -> f64 {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(obj, "writable", js_bool(true));
    set_object_field(obj, "destroyed", js_bool(false));
    set_object_field(obj, "on", closure_value(stream_on as *const u8, 2));
    set_object_field(obj, "once", closure_value(stream_on as *const u8, 2));
    set_object_field(obj, "addListener", closure_value(stream_on as *const u8, 2));
    set_object_field(obj, "off", closure_value(stream_off as *const u8, 2));
    set_object_field(
        obj,
        "removeListener",
        closure_value(stream_off as *const u8, 2),
    );
    set_object_field(obj, "emit", closure_value(stream_emit as *const u8, 2));
    set_object_field(obj, "write", closure_value(stream_write as *const u8, 2));
    set_object_field(obj, "end", closure_value(stream_this1 as *const u8, 1));
    set_object_field(obj, "destroy", closure_value(stream_this0 as *const u8, 0));
    set_object_field(obj, "cork", closure_value(stream_this0 as *const u8, 0));
    set_object_field(obj, "uncork", closure_value(stream_this0 as *const u8, 0));
    object_value(obj)
}

extern "C" fn worker_event_loop_utilization(
    _closure: *const ClosureHeader,
    util1: f64,
    util2: f64,
) -> f64 {
    unsafe { js_perf_event_loop_utilization(util1, util2) }
}

fn worker_profile_result(kind: &str) -> f64 {
    string_value(&format!("{{\"perryWorkerProfile\":\"{kind}\"}}"))
}

extern "C" fn worker_profile_stop(closure: *const ClosureHeader) -> f64 {
    let kind_bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    let kind = if kind_bits == 1 { "heap" } else { "cpu" };
    resolved_promise_value(worker_profile_result(kind))
}

pub(super) fn worker_profile_handle(kind_bits: i64) -> f64 {
    perry_runtime::closure::js_register_closure_arity(worker_profile_stop as *const u8, 0);
    let closure = perry_runtime::closure::js_closure_alloc(worker_profile_stop as *const u8, 1);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 0, kind_bits);
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(
        obj,
        "stop",
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );
    object_value(obj)
}
