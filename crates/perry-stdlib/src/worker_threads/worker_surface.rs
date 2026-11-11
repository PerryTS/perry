use super::*;

fn worker_notification_process_value() -> f64 {
    unsafe {
        js_nm_install_process();
    }
    perry_runtime::object::js_create_native_module_namespace_vt(
        &perry_runtime::object::NM_PROCESS,
        b"process".as_ptr(),
        7,
    )
}

fn call_process_method(method: &[u8], first: f64, second: Option<f64>) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let first = scope.root_nanbox_f64(first);
    let extra = scope.root_nanbox_f64(second.unwrap_or_else(js_undefined));
    let process = scope.root_nanbox_f64(worker_notification_process_value());
    let key = scope.root_raw_const_ptr(
        js_string_from_bytes(method.as_ptr(), method.len() as u32) as *const StringHeader
    );
    let function = scope.root_nanbox_f64(perry_runtime::object::js_object_get_field_by_name_f64(
        JSValue::from_bits(process.get_nanbox_f64().to_bits())
            .as_pointer::<perry_runtime::ObjectHeader>(),
        key.get_raw_const_ptr(),
    ));
    let args = [first.get_nanbox_f64(), extra.get_nanbox_f64()];
    unsafe {
        perry_runtime::closure::js_native_call_value(
            function.get_nanbox_f64(),
            perry_runtime::closure::JsThis::from_f64(process.get_nanbox_f64()),
            args.as_ptr(),
            if second.is_some() { 2 } else { 1 },
        )
    }
}

extern "C" fn emit_process_worker_event(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let worker = scope.root_nanbox_f64(perry_runtime::closure::js_closure_get_capture_f64(
        closure, 0,
    ));
    let event = js_string_from_bytes(b"worker".as_ptr(), 6);
    call_process_method(
        b"emit",
        f64::from_bits(JSValue::string_ptr(event).bits()),
        Some(worker.get_nanbox_f64()),
    )
}

pub(super) fn process_worker_callback(worker: f64) -> f64 {
    let captures = [worker.to_bits()];
    let callback = perry_runtime::closure::js_closure_alloc_init(
        perry_runtime::fn_info!(emit_process_worker_event, 0; with_declared(0), with_flags(perry_runtime::closure::FN_ARROW | perry_runtime::closure::FN_NON_CONSTRUCTOR)),
        1,
        captures.as_ptr(),
    );
    object_value(callback.cast())
}

pub(super) fn queue_process_worker_event(worker: f64) {
    call_process_method(b"nextTick", process_worker_callback(worker), None);
}

pub(super) fn worker_id_from_receiver(receiver: i64) -> Option<u64> {
    if receiver == 0 {
        return None;
    }
    perry_runtime::messaging::worker_id(perry_runtime::value::js_nanbox_pointer(receiver))
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_post_message(
    receiver: i64,
    value: f64,
    transfer: f64,
) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    worker_post_message_by_id(worker_id, value, transfer)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_on(receiver: i64, event: f64, callback: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    let callback_value = f64::from_bits(callback as u64);
    worker_add_listener(worker_id, event, callback_value, false, false)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_once(receiver: i64, event: f64, callback: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    let callback_value = f64::from_bits(callback as u64);
    worker_add_listener(worker_id, event, callback_value, true, false)
}

/// `worker.addEventListener(type, listener)` on the main-thread Worker handle.
/// Web-style: the listener receives a `MessageEvent` for "message".
#[no_mangle]
pub extern "C" fn js_worker_threads_worker_add_event_listener(
    receiver: i64,
    event: f64,
    callback: i64,
) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    let callback_value = f64::from_bits(callback as u64);
    worker_add_listener(worker_id, event, callback_value, false, true)
}

/// `worker.removeEventListener(type, listener)`.
#[no_mangle]
pub extern "C" fn js_worker_threads_worker_remove_event_listener(
    receiver: i64,
    event: f64,
    callback: i64,
) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    worker_remove_listener(worker_id, event, f64::from_bits(callback as u64), true)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_off(receiver: i64, event: f64, callback: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    worker_remove_listener(worker_id, event, f64::from_bits(callback as u64), false)
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
pub extern "C" fn js_worker_threads_worker_reload(receiver: i64) -> f64 {
    let Some(worker_id) = worker_id_from_receiver(receiver) else {
        return js_undefined();
    };
    worker_reload_by_id(worker_id)
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

pub(super) extern "C" fn worker_metadata(
    id: u64,
    field: perry_runtime::messaging::WorkerMetadataField,
) -> f64 {
    use perry_runtime::messaging::WorkerMetadataField;
    // Copy native state only: the owner heap must never allocate under WORKERS.
    let state = {
        let workers = WORKERS.lock().unwrap();
        workers
            .get(&id)
            .filter(|worker| worker.owner_agent == perry_runtime::agent::current_agent())
            .map(|worker| {
                (
                    worker.alive,
                    matches!(field, WorkerMetadataField::ThreadName)
                        .then(|| worker.thread_name.clone()),
                    worker.resource_limits,
                )
            })
    };
    let Some((alive, name, limits)) = state else {
        return js_undefined();
    };
    match field {
        WorkerMetadataField::ThreadId => {
            if alive {
                id as f64
            } else {
                -1.0
            }
        }
        WorkerMetadataField::ThreadName => {
            if alive {
                string_value(&name.unwrap())
            } else {
                js_null()
            }
        }
        WorkerMetadataField::ResourceLimits => object_value(if alive {
            worker_resource_limits_object(&limits)
        } else {
            empty_object()
        }),
    }
}

pub(super) fn worker_object(
    worker_id: u64,
    options: &WorkerOptions,
) -> *mut perry_runtime::object::ObjectHeader {
    // Perry exposes practical stream-shaped objects here; pipe/fd ownership is
    // not modeled yet, but the options are still parsed as Node-observable state.
    let _ = (options.stdout, options.stderr, options.track_unmanaged_fds);
    perry_runtime::messaging::register_worker_metadata_provider(worker_metadata);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(perry_runtime::messaging::worker_object_with_id(worker_id));
    let set = |name: &str, value: f64| {
        let value = scope.root_nanbox_f64(value);
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        perry_runtime::object::js_object_set_field_by_name(
            obj.get_raw_mut_ptr(),
            key,
            value.get_nanbox_f64(),
        );
    };
    set(
        "stdin",
        if options.stdin {
            worker_writable_stream_object()
        } else {
            js_null()
        },
    );
    set("stdout", worker_readable_stream_object());
    set("stderr", worker_readable_stream_object());
    set("performance", object_value(worker_performance_object()));
    set(
        "getHeapStatistics",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_get_heap_statistics, 0; with_declared(0)),
            worker_id,
        ),
    );
    set(
        "cpuUsage",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_cpu_usage, 1; with_declared(1)),
            worker_id,
        ),
    );
    set(
        "getHeapSnapshot",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_get_heap_snapshot, 1; with_declared(1)),
            worker_id,
        ),
    );
    set(
        "startCpuProfile",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_start_cpu_profile, 0; with_declared(0)),
            worker_id,
        ),
    );
    set(
        "startHeapProfile",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_start_heap_profile, 0; with_declared(0)),
            worker_id,
        ),
    );
    set(
        "postMessage",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(native_args worker_post_message, 1),
            worker_id,
        ),
    );
    set(
        "terminate",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_terminate, 0; with_declared(0)),
            worker_id,
        ),
    );
    set(
        "reload",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_reload, 0; with_declared(0)),
            worker_id,
        ),
    );
    set(
        "ref",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_ref, 0; with_declared(0)),
            worker_id,
        ),
    );
    set(
        "unref",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_unref, 0; with_declared(0)),
            worker_id,
        ),
    );
    let on = scope.root_nanbox_f64(closure_value_with_worker_id(
        perry_runtime::fn_info!(worker_on, 2; with_declared(2)),
        worker_id,
    ));
    set("on", on.get_nanbox_f64());
    set("addListener", on.get_nanbox_f64());
    set(
        "once",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_once, 2; with_declared(2)),
            worker_id,
        ),
    );
    let off = scope.root_nanbox_f64(closure_value_with_worker_id(
        perry_runtime::fn_info!(worker_off, 2; with_declared(2)),
        worker_id,
    ));
    set("off", off.get_nanbox_f64());
    set("removeListener", off.get_nanbox_f64());
    set(
        "addEventListener",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_add_event_listener, 2; with_declared(2)),
            worker_id,
        ),
    );
    set(
        "removeEventListener",
        closure_value_with_worker_id(
            perry_runtime::fn_info!(worker_remove_event_listener, 2; with_declared(2)),
            worker_id,
        ),
    );
    set("onmessage", js_null());
    set("onmessageerror", js_null());
    set("onerror", js_null());
    obj.get_raw_mut_ptr()
}

/// Install the browser/Bun worker-scope globals on this worker thread's own
/// global object. This happens before the compiled worker entry runs, so strict
/// assignments such as `onmessage = fn` target pre-existing properties.
pub(super) fn install_web_worker_globals() {
    let global_value = perry_runtime::object::js_get_global_this();
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let global_h = scope.root_nanbox_f64(global_value);
    let global = || {
        perry_runtime::value::js_nanbox_get_pointer(global_h.get_nanbox_f64())
            as *mut perry_runtime::object::ObjectHeader
    };
    if global().is_null() {
        return;
    }
    set_object_field(global(), "self", global_h.get_nanbox_f64());
    let post_message_h = scope.root_nanbox_f64(closure_value(
        perry_runtime::fn_info!(web_worker_post_message, 2; with_declared(2)),
    ));
    set_object_field(global(), "postMessage", post_message_h.get_nanbox_f64());
    let add_event_listener_h = scope.root_nanbox_f64(closure_value(
        perry_runtime::fn_info!(web_worker_add_event_listener, 3; with_declared(3)),
    ));
    set_object_field(
        global(),
        "addEventListener",
        add_event_listener_h.get_nanbox_f64(),
    );
    let remove_event_listener_h = scope.root_nanbox_f64(closure_value(
        perry_runtime::fn_info!(web_worker_remove_event_listener, 2; with_declared(2)),
    ));
    set_object_field(
        global(),
        "removeEventListener",
        remove_event_listener_h.get_nanbox_f64(),
    );
    let close_h = scope.root_nanbox_f64(closure_value(
        perry_runtime::fn_info!(web_worker_close, 0; with_declared(0)),
    ));
    set_object_field(global(), "close", close_h.get_nanbox_f64());
    set_object_field(global(), "onmessage", js_null());
    set_object_field(global(), "onmessageerror", js_null());
    set_object_field(global(), "onerror", js_null());
}

pub(super) fn web_worker_global_handler(name: &str) -> Option<u64> {
    let global_value = perry_runtime::object::js_get_global_this();
    object_event_handler(global_value.to_bits(), name)
}

extern "C" fn web_worker_post_message(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    value: f64,
    transfer: f64,
) -> f64 {
    js_worker_threads_post_message(value, transfer)
}

extern "C" fn web_worker_add_event_listener(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
    options: f64,
) -> f64 {
    parent_port::worker_parent_port_object();
    message_port::add_event_listener(parent_port::id(), event, callback, options, true)
}

extern "C" fn web_worker_remove_event_listener(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
) -> f64 {
    message_port::remove_event_listener(parent_port::id(), event, callback)
}

extern "C" fn web_worker_close(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    CURRENT_WORKER_CLOSE_REQUESTED.with(|closed| closed.set(true));
    js_undefined()
}

pub(super) fn worker_resource_limits_object(
    limits: &WorkerResourceLimits,
) -> *mut perry_runtime::object::ObjectHeader {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
    let keys = [
        (
            "maxYoungGenerationSizeMb",
            limits.max_young_generation_size_mb,
        ),
        ("maxOldGenerationSizeMb", limits.max_old_generation_size_mb),
        ("codeRangeSizeMb", limits.code_range_size_mb),
        ("stackSizeMb", limits.stack_size_mb),
    ];
    let keys_array = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(keys.len() as u32));
    for (name, value) in keys {
        let name_ptr =
            scope.root_raw_mut_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
        perry_runtime::object::js_object_set_field_by_name(
            obj.get_raw_mut_ptr(),
            name_ptr.get_raw_mut_ptr(),
            value,
        );
        let array = perry_runtime::js_array_push(
            keys_array.get_raw_mut_ptr(),
            JSValue::string_ptr(name_ptr.get_raw_mut_ptr()),
        );
        keys_array.set_raw_mut_ptr(array);
    }
    perry_runtime::js_object_set_keys(obj.get_raw_mut_ptr(), keys_array.get_raw_mut_ptr());
    obj.get_raw_mut_ptr()
}

pub(super) fn empty_object() -> *mut perry_runtime::object::ObjectHeader {
    perry_runtime::realm_decl! { birth perry_runtime::object::js_object_alloc(0, 0); realm { "node:worker_threads.resourceLimits", kind = namespace, members = { }; "node:worker_threads.default.resourceLimits", kind = namespace, members = { }; } }
}

fn worker_performance_object() -> *mut perry_runtime::object::ObjectHeader {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(
        obj,
        "eventLoopUtilization",
        closure_value(perry_runtime::fn_info!(worker_event_loop_utilization, 2; with_declared(2))),
    );
    obj
}

fn stream_listener_key(event: &str) -> String {
    format!("__perryWorkerStreamListener:{event}")
}

fn stream_register(receiver: perry_runtime::closure::JsThis, event: f64, callback: f64) -> f64 {
    let this = receiver.as_f64();
    let Some(event) = string_value_to_string(event) else {
        return this;
    };
    let key = stream_listener_key(&event);
    // The listener array allocations can collect: root the receiver (and the
    // callback being stored) across them and re-read both.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let this_h = scope.root_nanbox_f64(this);
    let callback_h = scope.root_nanbox_f64(callback);
    let arr = array_ptr_from_value(get_object_field_from_value(this, &key))
        .unwrap_or_else(|| perry_runtime::array::js_array_alloc(0));
    let arr = perry_runtime::array::js_array_push_f64(arr, callback_h.get_nanbox_f64());
    let this = this_h.get_nanbox_f64();
    if let Some(obj) = object_ptr_from_value(this) {
        set_object_field(
            obj,
            &key,
            object_value(arr as *mut perry_runtime::object::ObjectHeader),
        );
    }
    this
}

fn stream_emit_event(receiver: perry_runtime::closure::JsThis, event: f64, arg: f64) -> f64 {
    let this = receiver.as_f64();
    let Some(event) = string_value_to_string(event) else {
        return js_bool(false);
    };
    let key = stream_listener_key(&event);
    let Some(arr) = array_ptr_from_value(get_object_field_from_value(this, &key)) else {
        return js_bool(false);
    };
    // #10600: `this`, `arr` and `arg` are plain Rust locals, not GC roots.
    // `arr` is the listener array's own raw pointer, so a stale copy after a
    // listener allocates enough to trigger a moving minor collection
    // corrupts every read for the rest of this loop, not just one listener.
    // Root all three through one handle scope and re-read the current bits
    // before every dispatch. Each listener runs with the stream as `this`.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let this_h = scope.root_nanbox_f64(this);
    let arr_h = scope.root_raw_mut_ptr(arr);
    let arg_h = scope.root_nanbox_f64(arg);
    let len = perry_runtime::array::js_array_length(arr_h.get_raw_mut_ptr());
    for i in 0..len {
        let callback = perry_runtime::array::js_array_get_f64(arr_h.get_raw_mut_ptr(), i);
        unsafe {
            let args = [arg_h.get_nanbox_f64()];
            let _ = perry_runtime::closure::js_native_call_value(
                callback,
                perry_runtime::closure::JsThis::from_f64(this_h.get_nanbox_f64()),
                args.as_ptr(),
                args.len(),
            );
        }
    }
    js_bool(len > 0)
}

fn stream_remove_listener(
    receiver: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
) -> f64 {
    let this = receiver.as_f64();
    let Some(event) = string_value_to_string(event) else {
        return this;
    };
    let key = stream_listener_key(&event);
    let Some(arr) = array_ptr_from_value(get_object_field_from_value(this, &key)) else {
        return this;
    };
    // The filtered copy allocates: root the receiver, the old array and the
    // callback compared against across it and re-read them.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let this_h = scope.root_nanbox_f64(this);
    let arr_h = scope.root_raw_mut_ptr(arr);
    let callback_h = scope.root_nanbox_f64(callback);
    let len = perry_runtime::array::js_array_length(arr_h.get_raw_mut_ptr());
    let mut next = perry_runtime::array::js_array_alloc(len);
    for i in 0..len {
        let value = perry_runtime::array::js_array_get_f64(arr_h.get_raw_mut_ptr(), i);
        if value.to_bits() != callback_h.get_nanbox_f64().to_bits() {
            next = perry_runtime::array::js_array_push_f64(next, value);
        }
    }
    let this = this_h.get_nanbox_f64();
    if let Some(obj) = object_ptr_from_value(this) {
        set_object_field(
            obj,
            &key,
            object_value(next as *mut perry_runtime::object::ObjectHeader),
        );
    }
    this
}

extern "C" fn stream_on(
    _closure: *const ClosureHeader,
    this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
) -> f64 {
    stream_register(this, event, callback)
}

extern "C" fn stream_emit(
    _closure: *const ClosureHeader,
    this: perry_runtime::closure::JsThis,
    event: f64,
    arg: f64,
) -> f64 {
    stream_emit_event(this, event, arg)
}

extern "C" fn stream_off(
    _closure: *const ClosureHeader,
    this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
) -> f64 {
    stream_remove_listener(this, event, callback)
}

extern "C" fn stream_this0(
    _closure: *const ClosureHeader,
    this: perry_runtime::closure::JsThis,
) -> f64 {
    this.as_f64()
}

extern "C" fn stream_this1(
    _closure: *const ClosureHeader,
    this: perry_runtime::closure::JsThis,
    _arg: f64,
) -> f64 {
    this.as_f64()
}

extern "C" fn stream_write(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _chunk: f64,
    _encoding: f64,
) -> f64 {
    js_bool(true)
}

extern "C" fn stream_read(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _size: f64,
) -> f64 {
    js_null()
}

extern "C" fn stream_pipe(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    dest: f64,
) -> f64 {
    dest
}

pub(super) fn worker_readable_stream_object() -> f64 {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(obj, "readable", js_bool(true));
    set_object_field(obj, "destroyed", js_bool(false));
    set_object_field(
        obj,
        "on",
        closure_value(perry_runtime::fn_info!(stream_on, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "once",
        closure_value(perry_runtime::fn_info!(stream_on, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "addListener",
        closure_value(perry_runtime::fn_info!(stream_on, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "prependListener",
        closure_value(perry_runtime::fn_info!(stream_on, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "off",
        closure_value(perry_runtime::fn_info!(stream_off, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "removeListener",
        closure_value(perry_runtime::fn_info!(stream_off, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "emit",
        closure_value(perry_runtime::fn_info!(stream_emit, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "pause",
        closure_value(perry_runtime::fn_info!(stream_this0, 0; with_declared(0))),
    );
    set_object_field(
        obj,
        "resume",
        closure_value(perry_runtime::fn_info!(stream_this0, 0; with_declared(0))),
    );
    set_object_field(
        obj,
        "destroy",
        closure_value(perry_runtime::fn_info!(stream_this0, 0; with_declared(0))),
    );
    set_object_field(
        obj,
        "setEncoding",
        closure_value(perry_runtime::fn_info!(stream_this1, 1; with_declared(1))),
    );
    set_object_field(
        obj,
        "read",
        closure_value(perry_runtime::fn_info!(stream_read, 1; with_declared(1))),
    );
    set_object_field(
        obj,
        "pipe",
        closure_value(perry_runtime::fn_info!(stream_pipe, 1; with_declared(1))),
    );
    object_value(obj)
}

fn worker_writable_stream_object() -> f64 {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(obj, "writable", js_bool(true));
    set_object_field(obj, "destroyed", js_bool(false));
    set_object_field(
        obj,
        "on",
        closure_value(perry_runtime::fn_info!(stream_on, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "once",
        closure_value(perry_runtime::fn_info!(stream_on, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "addListener",
        closure_value(perry_runtime::fn_info!(stream_on, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "off",
        closure_value(perry_runtime::fn_info!(stream_off, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "removeListener",
        closure_value(perry_runtime::fn_info!(stream_off, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "emit",
        closure_value(perry_runtime::fn_info!(stream_emit, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "write",
        closure_value(perry_runtime::fn_info!(stream_write, 2; with_declared(2))),
    );
    set_object_field(
        obj,
        "end",
        closure_value(perry_runtime::fn_info!(stream_this1, 1; with_declared(1))),
    );
    set_object_field(
        obj,
        "destroy",
        closure_value(perry_runtime::fn_info!(stream_this0, 0; with_declared(0))),
    );
    set_object_field(
        obj,
        "cork",
        closure_value(perry_runtime::fn_info!(stream_this0, 0; with_declared(0))),
    );
    set_object_field(
        obj,
        "uncork",
        closure_value(perry_runtime::fn_info!(stream_this0, 0; with_declared(0))),
    );
    object_value(obj)
}

extern "C" fn worker_event_loop_utilization(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    util1: f64,
    util2: f64,
) -> f64 {
    unsafe { js_perf_event_loop_utilization(util1, util2) }
}

fn worker_profile_result(kind: &str) -> f64 {
    string_value(&format!("{{\"perryWorkerProfile\":\"{kind}\"}}"))
}

extern "C" fn worker_profile_stop(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let kind_bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    let kind = if kind_bits == 1 { "heap" } else { "cpu" };
    resolved_promise_value(worker_profile_result(kind))
}

pub(super) fn worker_profile_handle(kind_bits: i64) -> f64 {
    let closure = perry_runtime::closure::js_closure_alloc(
        perry_runtime::fn_info!(worker_profile_stop, 0; with_declared(0)),
        1,
    );
    perry_runtime::closure::closure_set_capture_word(closure, 0, kind_bits);
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(
        obj,
        "stop",
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );
    object_value(obj)
}
