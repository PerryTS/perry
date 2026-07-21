use super::*;
use std::collections::VecDeque;
use std::ptr::null_mut;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebLockMode {
    Exclusive,
    Shared,
}

impl WebLockMode {
    fn as_str(self) -> &'static str {
        match self {
            WebLockMode::Exclusive => "exclusive",
            WebLockMode::Shared => "shared",
        }
    }
}

pub(crate) struct WebLockHeld {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) mode: WebLockMode,
    pub(crate) client_id: String,
    pub(crate) source_promise: *mut crate::promise::Promise,
    pub(crate) output_promise: *mut crate::promise::Promise,
}

pub(crate) struct WebLockPending {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) mode: WebLockMode,
    pub(crate) client_id: String,
    pub(crate) if_available: bool,
    pub(crate) steal: bool,
    pub(crate) callback_bits: u64,
    pub(crate) output_promise: *mut crate::promise::Promise,
}

#[derive(Default)]
pub(crate) struct WebLocksState {
    pub(crate) next_id: u64,
    pub(crate) held: Vec<WebLockHeld>,
    pub(crate) pending: VecDeque<WebLockPending>,
}

enum WebLocksProcessItem {
    Grant(WebLockPending),
    Unavailable(WebLockPending),
}

fn worker_threads_web_locks_client_id() -> String {
    "node-perry-0".to_string()
}

fn web_locks_string_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn web_locks_object_value<T>(ptr: *mut T) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn web_locks_named_key(name: &str) -> *mut crate::string::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn web_locks_set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    let key = web_locks_named_key(name);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

fn web_locks_get_field(value: f64, name: &str) -> f64 {
    let ptr = crate::value::js_nanbox_get_pointer(value) as *const ObjectHeader;
    if ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = web_locks_named_key(name);
    crate::object::js_object_get_field_by_name_f64(ptr, key)
}

fn web_locks_value_to_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value);
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn web_locks_is_object_like(value: f64) -> bool {
    unsafe { crate::object::object_ops::value_is_object_like(value) }
}

fn web_locks_is_callable(value: f64) -> bool {
    let ptr = crate::value::js_nanbox_get_pointer(value) as usize;
    ptr >= 0x1000 && crate::closure::is_closure_ptr(ptr)
}

fn web_locks_undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn web_locks_null() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

fn web_locks_is_undefined(value: f64) -> bool {
    value.to_bits() == crate::value::TAG_UNDEFINED
}

fn web_locks_is_nullish(value: f64) -> bool {
    let bits = value.to_bits();
    bits == crate::value::TAG_UNDEFINED || bits == crate::value::TAG_NULL
}

fn web_locks_type_error_value(message: &str, code: &'static str) -> f64 {
    crate::fs::validate::build_type_error_with_code_value(message, code)
}

fn web_locks_dom_not_supported_value(message: &str) -> f64 {
    let msg = web_locks_string_value(message);
    let name = web_locks_string_value("NotSupportedError");
    let err = crate::event_target::js_dom_exception_new(msg, name);
    crate::value::js_nanbox_pointer(err as i64)
}

fn web_locks_callback_type_error(callback: f64) -> f64 {
    let received = if web_locks_is_undefined(callback) {
        "undefined".to_string()
    } else {
        format!("type {}", web_locks_value_to_string(callback))
    };
    let message =
        format!("The \"callback\" argument must be of type function. Received {received}");
    web_locks_type_error_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn web_locks_parse_mode(options: f64) -> Result<WebLockMode, f64> {
    if web_locks_is_nullish(options) {
        return Ok(WebLockMode::Exclusive);
    }
    if !web_locks_is_object_like(options) {
        return Err(web_locks_type_error_value(
            "Value cannot be converted to a dictionary",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let mode_value = web_locks_get_field(options, "mode");
    if web_locks_is_undefined(mode_value) {
        return Ok(WebLockMode::Exclusive);
    }
    let mode = web_locks_value_to_string(mode_value);
    match mode.as_str() {
        "exclusive" => Ok(WebLockMode::Exclusive),
        "shared" => Ok(WebLockMode::Shared),
        _ => {
            let message =
                format!("mode value '{mode}' is not a valid enum value of type LockMode.");
            Err(web_locks_type_error_value(
                &message,
                "ERR_INVALID_ARG_VALUE",
            ))
        }
    }
}

fn web_locks_parse_bool_option(options: f64, name: &str) -> bool {
    if web_locks_is_nullish(options) || !web_locks_is_object_like(options) {
        return false;
    }
    let value = web_locks_get_field(options, name);
    if web_locks_is_undefined(value) {
        return false;
    }
    crate::value::js_is_truthy(value) != 0
}

fn web_locks_signal_rejection(options: f64) -> Result<Option<f64>, f64> {
    if web_locks_is_nullish(options) || !web_locks_is_object_like(options) {
        return Ok(None);
    }
    let signal = web_locks_get_field(options, "signal");
    if web_locks_is_nullish(signal) {
        return Ok(None);
    }
    if !web_locks_is_object_like(signal) {
        return Err(web_locks_type_error_value(
            "Value is not an object",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    let aborted = web_locks_get_field(signal, "aborted");
    if web_locks_is_undefined(aborted) {
        return Err(web_locks_type_error_value(
            "The \"options.signal\" property must be an instance of AbortSignal. Received an instance of Object",
            "ERR_INVALID_ARG_TYPE",
        ));
    }
    if crate::value::js_is_truthy(aborted) != 0 {
        let reason = web_locks_get_field(signal, "reason");
        if web_locks_is_undefined(reason) {
            Ok(Some(crate::event_target::abort_dom_exception_value()))
        } else {
            Ok(Some(reason))
        }
    } else {
        Ok(None)
    }
}

fn web_locks_make_function(
    name: &str,
    func_ptr: *const u8,
    call_arity: u32,
    exposed_length: u32,
) -> f64 {
    crate::closure::js_register_closure_arity(func_ptr, call_arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    set_bound_native_closure_name(closure, name);
    set_builtin_closure_length(closure as usize, exposed_length);
    crate::value::js_nanbox_pointer(closure as i64)
}

extern "C" fn worker_threads_lock_manager_to_string_tag(_this: f64) -> f64 {
    web_locks_string_value("LockManager")
}

extern "C" fn worker_threads_lock_to_string_tag(_this: f64) -> f64 {
    web_locks_string_value("Lock")
}

fn worker_threads_locks_proto_value() -> f64 {
    let proto = crate::object::js_object_alloc(0, 0);
    let request =
        web_locks_make_function("request", worker_threads_locks_request as *const u8, 3, 2);
    crate::object::class_prototype_method_root_store(
        WORKER_THREADS_LOCK_MANAGER_CLASS_ID,
        "request".to_string(),
        request.to_bits(),
    );
    web_locks_set_field(proto, "request", request);
    let query = web_locks_make_function("query", worker_threads_locks_query as *const u8, 0, 0);
    crate::object::class_prototype_method_root_store(
        WORKER_THREADS_LOCK_MANAGER_CLASS_ID,
        "query".to_string(),
        query.to_bits(),
    );
    web_locks_set_field(proto, "query", query);
    web_locks_object_value(proto)
}

pub(crate) fn worker_threads_locks_value() -> f64 {
    if let Some(bits) = WORKER_THREADS_LOCKS_VALUE.with(|slot| {
        let bits = slot.get();
        (bits != 0).then_some(bits)
    }) {
        return f64::from_bits(bits);
    }
    let name = "LockManager";
    unsafe {
        js_register_class_id(WORKER_THREADS_LOCK_MANAGER_CLASS_ID);
        js_register_class_name(
            WORKER_THREADS_LOCK_MANAGER_CLASS_ID,
            name.as_ptr(),
            name.len() as u32,
        );
        crate::object::js_register_class_to_string_tag(
            WORKER_THREADS_LOCK_MANAGER_CLASS_ID,
            worker_threads_lock_manager_to_string_tag as *const u8 as i64,
        );
    }
    let lock_name = "Lock";
    unsafe {
        js_register_class_id(WORKER_THREADS_LOCK_CLASS_ID);
        js_register_class_name(
            WORKER_THREADS_LOCK_CLASS_ID,
            lock_name.as_ptr(),
            lock_name.len() as u32,
        );
        crate::object::js_register_class_to_string_tag(
            WORKER_THREADS_LOCK_CLASS_ID,
            worker_threads_lock_to_string_tag as *const u8 as i64,
        );
    }
    let obj = js_object_alloc(WORKER_THREADS_LOCK_MANAGER_CLASS_ID, 0);
    let obj_value = crate::value::js_nanbox_pointer(obj as i64);
    crate::object::js_object_set_prototype_of(obj_value, worker_threads_locks_proto_value());
    WORKER_THREADS_LOCKS_VALUE.with(|slot| slot.set(obj_value.to_bits()));
    obj_value
}

fn web_locks_new_id(state: &mut WebLocksState) -> u64 {
    state.next_id = state.next_id.saturating_add(1);
    state.next_id
}

fn web_locks_is_grantable(state: &WebLocksState, name: &str, mode: WebLockMode) -> bool {
    let mut has_same_name = false;
    for held in &state.held {
        if held.name != name {
            continue;
        }
        has_same_name = true;
        if mode == WebLockMode::Exclusive || held.mode == WebLockMode::Exclusive {
            return false;
        }
    }
    !has_same_name || mode == WebLockMode::Shared
}

fn web_locks_has_pending_same_name(state: &WebLocksState, name: &str) -> bool {
    state.pending.iter().any(|pending| pending.name == name)
}

fn web_locks_lock_info_object(name: &str, mode: WebLockMode, client_id: &str) -> f64 {
    let obj = crate::object::js_object_alloc(0, 0);
    web_locks_set_field(obj, "name", web_locks_string_value(name));
    web_locks_set_field(obj, "mode", web_locks_string_value(mode.as_str()));
    web_locks_set_field(obj, "clientId", web_locks_string_value(client_id));
    web_locks_object_value(obj)
}

fn web_locks_lock_object(name: &str, mode: WebLockMode) -> f64 {
    let obj = crate::object::js_object_alloc(WORKER_THREADS_LOCK_CLASS_ID, 0);
    web_locks_set_field(obj, "name", web_locks_string_value(name));
    web_locks_set_field(obj, "mode", web_locks_string_value(mode.as_str()));
    web_locks_object_value(obj)
}

fn web_locks_snapshot_array<'a>(
    items: impl Iterator<Item = (&'a String, WebLockMode, &'a String)>,
) -> *mut crate::array::ArrayHeader {
    let mut array = crate::array::js_array_alloc(0);
    for (name, mode, client_id) in items {
        array = crate::array::js_array_push_f64(
            array,
            web_locks_lock_info_object(name, mode, client_id),
        );
    }
    array
}

fn web_locks_query_snapshot() -> f64 {
    let (held, pending) = WORKER_THREADS_WEB_LOCKS.with(|state| {
        let state = state.borrow();
        let held = web_locks_snapshot_array(
            state
                .held
                .iter()
                .map(|item| (&item.name, item.mode, &item.client_id)),
        );
        let pending = web_locks_snapshot_array(
            state
                .pending
                .iter()
                .map(|item| (&item.name, item.mode, &item.client_id)),
        );
        (held, pending)
    });
    let snapshot = crate::object::js_object_alloc(0, 0);
    web_locks_set_field(snapshot, "held", web_locks_object_value(held));
    web_locks_set_field(snapshot, "pending", web_locks_object_value(pending));
    web_locks_object_value(snapshot)
}

fn web_locks_reject_promise(reason: f64) -> *mut crate::promise::Promise {
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_reject(promise, reason);
    promise
}

fn web_locks_rejected_error(error: f64) -> f64 {
    web_locks_object_value(web_locks_reject_promise(error))
}

fn web_locks_request_args(callback: f64, arg: f64) -> *mut crate::array::ArrayHeader {
    let _ = callback;
    let mut args = crate::array::js_array_alloc(1);
    args = crate::array::js_array_push_f64(args, arg);
    args
}

fn web_locks_release_callback_value(
    id: u64,
    output_promise: *mut crate::promise::Promise,
    reject: bool,
) -> *const crate::closure::ClosureHeader {
    let func_ptr = if reject {
        worker_threads_locks_release_reject as *const u8
    } else {
        worker_threads_locks_release_fulfill as *const u8
    };
    crate::closure::js_register_closure_arity(func_ptr, 1);
    let closure = crate::closure::js_closure_alloc(func_ptr, 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, id as i64);
    crate::closure::js_closure_set_capture_ptr(closure, 1, output_promise as i64);
    closure
}

fn web_locks_call_callback(
    id: u64,
    callback_bits: u64,
    arg: f64,
    output_promise: *mut crate::promise::Promise,
) -> *mut crate::promise::Promise {
    let callback = f64::from_bits(callback_bits);
    let args = web_locks_request_args(callback, arg);
    let source = crate::promise::js_promise_try(callback, args as *const crate::array::ArrayHeader);
    let on_fulfilled = web_locks_release_callback_value(id, output_promise, false);
    let on_rejected = web_locks_release_callback_value(id, output_promise, true);
    crate::promise::js_promise_then(source, on_fulfilled, on_rejected);
    source
}

fn web_locks_grant_request(request: WebLockPending) {
    let lock_arg = web_locks_lock_object(&request.name, request.mode);
    WORKER_THREADS_WEB_LOCKS.with(|state| {
        let mut state = state.borrow_mut();
        state.held.push(WebLockHeld {
            id: request.id,
            name: request.name.clone(),
            mode: request.mode,
            client_id: request.client_id.clone(),
            source_promise: null_mut(),
            output_promise: request.output_promise,
        });
    });
    let source = web_locks_call_callback(
        request.id,
        request.callback_bits,
        lock_arg,
        request.output_promise,
    );
    WORKER_THREADS_WEB_LOCKS.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(held) = state.held.iter_mut().find(|held| held.id == request.id) {
            held.source_promise = source;
        }
    });
}

fn web_locks_run_unavailable_request(request: WebLockPending) {
    web_locks_call_callback(
        0,
        request.callback_bits,
        web_locks_null(),
        request.output_promise,
    );
}

fn web_locks_steal_locked(
    state: &mut WebLocksState,
    name: &str,
) -> Vec<*mut crate::promise::Promise> {
    let mut rejected = Vec::new();
    let mut i = 0;
    while i < state.held.len() {
        if state.held[i].name == name {
            let held = state.held.remove(i);
            rejected.push(held.output_promise);
        } else {
            i += 1;
        }
    }
    rejected
}

fn web_locks_steal_reason() -> f64 {
    let msg = web_locks_string_value("The lock request was stolen");
    let name = web_locks_string_value("AbortError");
    let err = crate::event_target::js_dom_exception_new(msg, name);
    crate::value::js_nanbox_pointer(err as i64)
}

fn web_locks_reject_stolen(promises: Vec<*mut crate::promise::Promise>) {
    if promises.is_empty() {
        return;
    }
    let reason = web_locks_steal_reason();
    for promise in promises {
        crate::promise::js_promise_reject(promise, reason);
    }
}

fn web_locks_take_next_process_item(
) -> Option<(WebLocksProcessItem, Vec<*mut crate::promise::Promise>)> {
    WORKER_THREADS_WEB_LOCKS.with(|state| {
        let mut state = state.borrow_mut();
        for index in 0..state.pending.len() {
            let name = state.pending[index].name.clone();
            if state
                .pending
                .iter()
                .take(index)
                .any(|pending| pending.name == name)
            {
                continue;
            }
            if state.pending[index].steal {
                let request = state.pending.remove(index)?;
                let rejected = web_locks_steal_locked(&mut state, &request.name);
                return Some((WebLocksProcessItem::Grant(request), rejected));
            }
            if web_locks_is_grantable(&state, &name, state.pending[index].mode) {
                let request = state.pending.remove(index)?;
                return Some((WebLocksProcessItem::Grant(request), Vec::new()));
            }
            if state.pending[index].if_available {
                let request = state.pending.remove(index)?;
                return Some((WebLocksProcessItem::Unavailable(request), Vec::new()));
            }
        }
        None
    })
}

fn web_locks_process_queue() {
    while let Some((item, stolen)) = web_locks_take_next_process_item() {
        web_locks_reject_stolen(stolen);
        match item {
            WebLocksProcessItem::Grant(request) => web_locks_grant_request(request),
            WebLocksProcessItem::Unavailable(request) => web_locks_run_unavailable_request(request),
        }
    }
}

fn web_locks_release(id: u64) {
    if id == 0 {
        return;
    }
    WORKER_THREADS_WEB_LOCKS.with(|state| {
        let mut state = state.borrow_mut();
        state.held.retain(|held| held.id != id);
    });
    web_locks_process_queue();
}

extern "C" fn worker_threads_locks_release_fulfill(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let id = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    let output =
        crate::closure::js_closure_get_capture_ptr(closure, 1) as *mut crate::promise::Promise;
    web_locks_release(id);
    crate::promise::js_promise_resolve(output, value);
    web_locks_undefined()
}

extern "C" fn worker_threads_locks_release_reject(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let id = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    let output =
        crate::closure::js_closure_get_capture_ptr(closure, 1) as *mut crate::promise::Promise;
    web_locks_release(id);
    crate::promise::js_promise_reject(output, reason);
    web_locks_undefined()
}

extern "C" fn worker_threads_locks_request(
    _closure: *const crate::closure::ClosureHeader,
    name_value: f64,
    options_or_callback: f64,
    maybe_callback: f64,
) -> f64 {
    let has_options = !web_locks_is_undefined(maybe_callback);
    let callback = if has_options {
        maybe_callback
    } else {
        options_or_callback
    };
    if !web_locks_is_callable(callback) {
        return web_locks_rejected_error(web_locks_callback_type_error(callback));
    }

    let options = if has_options {
        options_or_callback
    } else {
        web_locks_undefined()
    };
    let name = web_locks_value_to_string(name_value);
    let mode = match web_locks_parse_mode(options) {
        Ok(mode) => mode,
        Err(error) => return web_locks_rejected_error(error),
    };
    let if_available = web_locks_parse_bool_option(options, "ifAvailable");
    let steal = web_locks_parse_bool_option(options, "steal");
    if if_available && steal {
        return web_locks_rejected_error(web_locks_dom_not_supported_value(
            "ifAvailable and steal are mutually exclusive",
        ));
    }

    match web_locks_signal_rejection(options) {
        Ok(Some(reason)) => return web_locks_object_value(web_locks_reject_promise(reason)),
        Ok(None) => {}
        Err(error) => return web_locks_rejected_error(error),
    }

    let output_promise = crate::promise::js_promise_new();
    let client_id = worker_threads_web_locks_client_id();
    let callback_bits = callback.to_bits();

    let immediate = WORKER_THREADS_WEB_LOCKS.with(|state| {
        let mut state = state.borrow_mut();
        let id = web_locks_new_id(&mut state);
        let request = WebLockPending {
            id,
            name,
            mode,
            client_id,
            if_available,
            steal,
            callback_bits,
            output_promise,
        };
        if request.steal {
            let rejected = web_locks_steal_locked(&mut state, &request.name);
            return (Some(WebLocksProcessItem::Grant(request)), rejected);
        }
        if !web_locks_has_pending_same_name(&state, &request.name)
            && web_locks_is_grantable(&state, &request.name, request.mode)
        {
            return (Some(WebLocksProcessItem::Grant(request)), Vec::new());
        }
        if request.if_available {
            return (Some(WebLocksProcessItem::Unavailable(request)), Vec::new());
        }
        state.pending.push_back(request);
        (None, Vec::new())
    });

    web_locks_reject_stolen(immediate.1);
    if let Some(item) = immediate.0 {
        match item {
            WebLocksProcessItem::Grant(request) => web_locks_grant_request(request),
            WebLocksProcessItem::Unavailable(request) => web_locks_run_unavailable_request(request),
        }
        web_locks_process_queue();
    }

    web_locks_object_value(output_promise)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_locks_request(
    name_value: f64,
    options_or_callback: f64,
    maybe_callback: f64,
) -> f64 {
    worker_threads_locks_request(
        std::ptr::null(),
        name_value,
        options_or_callback,
        maybe_callback,
    )
}

extern "C" fn worker_threads_locks_query(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let snapshot = web_locks_query_snapshot();
    web_locks_object_value(crate::promise::js_promise_resolved(snapshot))
}

#[no_mangle]
pub extern "C" fn js_worker_threads_locks_query() -> f64 {
    worker_threads_locks_query(std::ptr::null())
}
