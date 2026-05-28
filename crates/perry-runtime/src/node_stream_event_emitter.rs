use crate::closure::ClosureHeader;
use crate::value::JSValue;

const STREAM_EVENT_NAMES_KEY: &[u8] = b"__perryStreamEventNames";
const STREAM_LISTENERS_PREFIX: &[u8] = b"__perryStreamListeners:";
const STREAM_ONCE_PREFIX: &[u8] = b"__perryStreamOnce:";

pub(super) extern "C" fn ns_set_max_listeners(closure: *const ClosureHeader, value: f64) -> f64 {
    set_stream_max_listeners(super::this_value(closure), value)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_set_max_listeners(stream_handle: i64, value: f64) -> f64 {
    set_stream_max_listeners(super::stream_value_from_handle(stream_handle), value)
}

fn set_stream_max_listeners(stream: f64, value: f64) -> f64 {
    super::set_hidden_value(stream, super::hidden_max_listeners_key(), value);
    stream
}

pub(super) extern "C" fn ns_get_max_listeners(closure: *const ClosureHeader) -> f64 {
    stream_max_listeners(super::this_value(closure))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_get_max_listeners(stream_handle: i64) -> f64 {
    stream_max_listeners(super::stream_value_from_handle(stream_handle))
}

fn stream_max_listeners(stream: f64) -> f64 {
    super::get_hidden_value(stream, super::hidden_max_listeners_key()).unwrap_or(10.0)
}

pub(super) extern "C" fn ns_on2(closure: *const ClosureHeader, event: f64, cb: f64) -> f64 {
    let stream = super::this_value(closure);
    add_stream_listener_for_event(stream, event, cb);
    stream
}

pub(super) extern "C" fn ns_once2(closure: *const ClosureHeader, event: f64, cb: f64) -> f64 {
    let stream = super::this_value(closure);
    add_stream_listener_for_event_with_options(stream, event, cb, true, false);
    stream
}

pub(super) extern "C" fn ns_prepend_listener2(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure);
    add_stream_listener_for_event_with_options(stream, event, cb, false, true);
    stream
}

pub(super) extern "C" fn ns_prepend_once_listener2(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure);
    add_stream_listener_for_event_with_options(stream, event, cb, true, true);
    stream
}

pub(super) extern "C" fn ns_remove_listener2(
    closure: *const ClosureHeader,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure);
    remove_stream_listener_for_event(stream, event, cb);
    stream
}

pub(super) extern "C" fn ns_off2(closure: *const ClosureHeader, event: f64, cb: f64) -> f64 {
    ns_remove_listener2(closure, event, cb)
}

pub(super) extern "C" fn ns_remove_all_listeners1(
    closure: *const ClosureHeader,
    event: f64,
) -> f64 {
    let stream = super::this_value(closure);
    remove_all_stream_listeners_for_event(stream, event);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_on(stream_handle: i64, event: f64, cb: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    add_stream_listener_for_event(stream, event, cb);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_once(stream_handle: i64, event: f64, cb: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    add_stream_listener_for_event_with_options(stream, event, cb, true, false);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_prepend_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    add_stream_listener_for_event_with_options(stream, event, cb, false, true);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_prepend_once_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    add_stream_listener_for_event_with_options(stream, event, cb, true, true);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_remove_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    remove_stream_listener_for_event(stream, event, cb);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_off(stream_handle: i64, event: f64, cb: f64) -> f64 {
    js_node_stream_method_remove_listener(stream_handle, event, cb)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_remove_all_listeners(
    stream_handle: i64,
    event: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    remove_all_stream_listeners_for_event(stream, event);
    stream
}

pub(super) extern "C" fn ns_listener_count(closure: *const ClosureHeader, event: f64) -> f64 {
    stream_listener_count_for_event(super::this_value(closure), event) as f64
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_listener_count(stream_handle: i64, event: f64) -> f64 {
    stream_listener_count_for_event(super::stream_value_from_handle(stream_handle), event) as f64
}

pub(super) extern "C" fn ns_event_names(closure: *const ClosureHeader) -> f64 {
    let stream = super::this_value(closure);
    f64::from_bits(JSValue::pointer(stream_event_names_array(stream) as *const u8).bits())
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_event_names(stream_handle: i64) -> i64 {
    stream_event_names_array(super::stream_value_from_handle(stream_handle)) as i64
}

pub(super) extern "C" fn ns_listeners(closure: *const ClosureHeader, event: f64) -> f64 {
    let stream = super::this_value(closure);
    f64::from_bits(
        JSValue::pointer(stream_listeners_array_for_event(stream, event, false) as *const u8)
            .bits(),
    )
}

pub(super) extern "C" fn ns_raw_listeners(closure: *const ClosureHeader, event: f64) -> f64 {
    let stream = super::this_value(closure);
    f64::from_bits(
        JSValue::pointer(stream_listeners_array_for_event(stream, event, true) as *const u8).bits(),
    )
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_listeners(stream_handle: i64, event: f64) -> i64 {
    stream_listeners_array_for_event(super::stream_value_from_handle(stream_handle), event, false)
        as i64
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_raw_listeners(stream_handle: i64, event: f64) -> i64 {
    stream_listeners_array_for_event(super::stream_value_from_handle(stream_handle), event, true)
        as i64
}

pub(super) fn is_callable_value(value: f64) -> bool {
    let raw = super::raw_ptr_from_value(value);
    raw >= 0x10000 && !crate::closure::get_valid_func_ptr(raw as *const ClosureHeader).is_null()
}

pub(super) fn add_stream_listener_for_event(stream: f64, event: f64, cb: f64) {
    add_stream_listener_for_event_with_options(stream, event, cb, false, false);
}

fn add_stream_listener_for_event_with_options(
    stream: f64,
    event: f64,
    cb: f64,
    once: bool,
    prepend: bool,
) {
    if event_identity_bytes(event).is_none() {
        return;
    }
    if !is_callable_value(cb) {
        throw_invalid_listener_type();
    }
    add_stream_listener(stream, event, cb, once, prepend);
    if super::string_value_eq(event, b"data") {
        super::readable_data_listener_added(stream);
    } else if super::string_value_eq(event, b"readable") {
        super::schedule_readable_event(stream);
    }
}

#[cold]
fn throw_invalid_listener_type() -> ! {
    let msg = b"The \"listener\" argument must be of type function";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    let bits = JSValue::pointer(err as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

fn string_bytes(value: f64) -> Option<Vec<u8>> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        Some(std::slice::from_raw_parts(data, len).to_vec())
    }
}

fn event_identity_bytes(event: f64) -> Option<Vec<u8>> {
    if unsafe { crate::symbol::js_is_symbol(event) } != 0 {
        let mut out = b"sym:".to_vec();
        out.extend_from_slice(super::raw_ptr_from_value(event).to_string().as_bytes());
        return Some(out);
    }
    let mut out = b"str:".to_vec();
    out.extend_from_slice(&string_bytes(event)?);
    Some(out)
}

fn event_key(prefix: &[u8], event: f64) -> Option<*mut crate::string::StringHeader> {
    let mut key = prefix.to_vec();
    key.extend_from_slice(&event_identity_bytes(event)?);
    Some(super::hidden_key(&key))
}

fn hidden_event_names_key() -> *mut crate::string::StringHeader {
    super::hidden_key(STREAM_EVENT_NAMES_KEY)
}

fn event_names_value(stream: f64) -> f64 {
    super::get_hidden_value(stream, hidden_event_names_key()).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        let value = super::box_pointer(arr as *const u8);
        super::set_hidden_value(stream, hidden_event_names_key(), value);
        value
    })
}

fn event_names_snapshot(stream: f64) -> Vec<f64> {
    let names = event_names_value(stream);
    if !super::is_array_like_value(names) {
        return Vec::new();
    }
    let arr = super::raw_ptr_from_value(names) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push(crate::array::js_array_get_f64(arr, i));
    }
    out
}

fn event_name_index(stream: f64, event: f64) -> Option<u32> {
    let wanted = event_identity_bytes(event)?;
    let names = event_names_value(stream);
    if !super::is_array_like_value(names) {
        return None;
    }
    let arr = super::raw_ptr_from_value(names) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let existing = crate::array::js_array_get_f64(arr, i);
        if event_identity_bytes(existing).is_some_and(|bytes| bytes == wanted) {
            return Some(i);
        }
    }
    None
}

fn note_event_name(stream: f64, event: f64) {
    if event_name_index(stream, event).is_some() {
        return;
    }
    let names = event_names_value(stream);
    if !super::is_array_like_value(names) {
        return;
    }
    let arr = super::raw_ptr_from_value(names) as *mut crate::array::ArrayHeader;
    let arr = crate::array::js_array_push_f64(arr, event);
    super::set_hidden_value(
        stream,
        hidden_event_names_key(),
        super::box_pointer(arr as *const u8),
    );
}

fn listener_storage(stream: f64, event: f64) -> Option<(f64, f64)> {
    let listeners = super::get_hidden_value(stream, event_key(STREAM_LISTENERS_PREFIX, event)?)?;
    let once = super::get_hidden_value(stream, event_key(STREAM_ONCE_PREFIX, event)?)?;
    Some((listeners, once))
}

fn ensure_listener_storage(stream: f64, event: f64) -> Option<(f64, f64)> {
    let listener_key = event_key(STREAM_LISTENERS_PREFIX, event)?;
    let once_key = event_key(STREAM_ONCE_PREFIX, event)?;
    let listeners = super::get_hidden_value(stream, listener_key).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        let value = super::box_pointer(arr as *const u8);
        super::set_hidden_value(stream, listener_key, value);
        value
    });
    let once = super::get_hidden_value(stream, once_key).unwrap_or_else(|| {
        let arr = crate::array::js_array_alloc(0);
        let value = super::box_pointer(arr as *const u8);
        super::set_hidden_value(stream, once_key, value);
        value
    });
    Some((listeners, once))
}

fn set_listener_storage(stream: f64, event: f64, listeners: f64, once: f64) {
    if let Some(listener_key) = event_key(STREAM_LISTENERS_PREFIX, event) {
        super::set_hidden_value(stream, listener_key, listeners);
    }
    if let Some(once_key) = event_key(STREAM_ONCE_PREFIX, event) {
        super::set_hidden_value(stream, once_key, once);
    }
}

fn add_stream_listener(stream: f64, event: f64, cb: f64, once: bool, prepend: bool) {
    emit_meta_event(stream, b"newListener", &[event, cb]);
    note_event_name(stream, event);
    let Some((listeners, once_flags)) = ensure_listener_storage(stream, event) else {
        return;
    };
    if !super::is_array_like_value(listeners) || !super::is_array_like_value(once_flags) {
        return;
    }
    let listeners_arr = super::raw_ptr_from_value(listeners) as *const crate::array::ArrayHeader;
    let once_arr = super::raw_ptr_from_value(once_flags) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(listeners_arr);
    let mut out_listeners = crate::array::js_array_alloc(len + 1);
    let mut out_once = crate::array::js_array_alloc(len + 1);
    if prepend {
        out_listeners = crate::array::js_array_push_f64(out_listeners, cb);
        out_once = crate::array::js_array_push_f64(out_once, bool_value(once));
    }
    for i in 0..len {
        out_listeners = crate::array::js_array_push_f64(
            out_listeners,
            crate::array::js_array_get_f64(listeners_arr, i),
        );
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_arr, i));
    }
    if !prepend {
        out_listeners = crate::array::js_array_push_f64(out_listeners, cb);
        out_once = crate::array::js_array_push_f64(out_once, bool_value(once));
    }
    set_listener_storage(
        stream,
        event,
        super::box_pointer(out_listeners as *const u8),
        super::box_pointer(out_once as *const u8),
    );
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        super::TAG_TRUE
    } else {
        super::TAG_FALSE
    })
}

fn listener_snapshot(stream: f64, event: f64) -> Vec<(f64, bool)> {
    let Some((listeners, once_flags)) = listener_storage(stream, event) else {
        return Vec::new();
    };
    if !super::is_array_like_value(listeners) || !super::is_array_like_value(once_flags) {
        return Vec::new();
    }
    let listeners_arr = super::raw_ptr_from_value(listeners) as *const crate::array::ArrayHeader;
    let once_arr = super::raw_ptr_from_value(once_flags) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(listeners_arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push((
            crate::array::js_array_get_f64(listeners_arr, i),
            crate::value::js_is_truthy(crate::array::js_array_get_f64(once_arr, i)) != 0,
        ));
    }
    out
}

fn remove_event_name(stream: f64, event: f64) {
    let Some(remove_idx) = event_name_index(stream, event) else {
        return;
    };
    let names = event_names_value(stream);
    if !super::is_array_like_value(names) {
        return;
    }
    let arr = super::raw_ptr_from_value(names) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut out = crate::array::js_array_alloc(len.saturating_sub(1));
    for i in 0..len {
        if i != remove_idx {
            out = crate::array::js_array_push_f64(out, crate::array::js_array_get_f64(arr, i));
        }
    }
    super::set_hidden_value(
        stream,
        hidden_event_names_key(),
        super::box_pointer(out as *const u8),
    );
}

fn prune_event_if_empty(stream: f64, event: f64) {
    if stream_listener_count_for_event(stream, event) == 0 {
        remove_event_name(stream, event);
    }
}

fn remove_stream_listener_for_event(stream: f64, event: f64, cb: f64) -> bool {
    let Some((listeners, once_flags)) = listener_storage(stream, event) else {
        return false;
    };
    if !super::is_array_like_value(listeners) || !super::is_array_like_value(once_flags) {
        return false;
    }
    let listeners_arr = super::raw_ptr_from_value(listeners) as *const crate::array::ArrayHeader;
    let once_arr = super::raw_ptr_from_value(once_flags) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(listeners_arr);
    let mut remove_idx = None;
    for i in (0..len).rev() {
        let listener = crate::array::js_array_get_f64(listeners_arr, i);
        if listener.to_bits() == cb.to_bits() {
            remove_idx = Some(i);
            break;
        }
    }
    let Some(remove_idx) = remove_idx else {
        return false;
    };
    let mut out_listeners = crate::array::js_array_alloc(len);
    let mut out_once = crate::array::js_array_alloc(len);
    for i in 0..len {
        let listener = crate::array::js_array_get_f64(listeners_arr, i);
        if i == remove_idx {
            continue;
        }
        out_listeners = crate::array::js_array_push_f64(out_listeners, listener);
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_arr, i));
    }
    set_listener_storage(
        stream,
        event,
        super::box_pointer(out_listeners as *const u8),
        super::box_pointer(out_once as *const u8),
    );
    prune_event_if_empty(stream, event);
    emit_meta_event(stream, b"removeListener", &[event, cb]);
    true
}

fn remove_all_stream_listeners_for_event(stream: f64, event: f64) {
    if event_identity_bytes(event).is_none() {
        for name in event_names_snapshot(stream) {
            remove_all_stream_listeners_for_event(stream, name);
        }
        super::set_hidden_value(
            stream,
            hidden_event_names_key(),
            super::box_pointer(crate::array::js_array_alloc(0) as *const u8),
        );
        return;
    }
    let removed = listener_snapshot(stream, event);
    let empty_listeners = super::box_pointer(crate::array::js_array_alloc(0) as *const u8);
    let empty_once = super::box_pointer(crate::array::js_array_alloc(0) as *const u8);
    set_listener_storage(stream, event, empty_listeners, empty_once);
    remove_event_name(stream, event);
    for (listener, _) in removed {
        emit_meta_event(stream, b"removeListener", &[event, listener]);
    }
}

fn remove_once_listeners(stream: f64, event: f64) {
    let Some((listeners, once_flags)) = listener_storage(stream, event) else {
        return;
    };
    if !super::is_array_like_value(listeners) || !super::is_array_like_value(once_flags) {
        return;
    }
    let listeners_arr = super::raw_ptr_from_value(listeners) as *const crate::array::ArrayHeader;
    let once_arr = super::raw_ptr_from_value(once_flags) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(listeners_arr);
    let mut out_listeners = crate::array::js_array_alloc(len);
    let mut out_once = crate::array::js_array_alloc(len);
    let mut removed = Vec::new();
    for i in 0..len {
        let listener = crate::array::js_array_get_f64(listeners_arr, i);
        if crate::value::js_is_truthy(crate::array::js_array_get_f64(once_arr, i)) == 0 {
            out_listeners = crate::array::js_array_push_f64(out_listeners, listener);
            out_once = crate::array::js_array_push_f64(
                out_once,
                crate::array::js_array_get_f64(once_arr, i),
            );
        } else {
            removed.push(listener);
        }
    }
    set_listener_storage(
        stream,
        event,
        super::box_pointer(out_listeners as *const u8),
        super::box_pointer(out_once as *const u8),
    );
    prune_event_if_empty(stream, event);
    for listener in removed {
        emit_meta_event(stream, b"removeListener", &[event, listener]);
    }
}

pub(super) fn stream_listener_count_for_event(stream: f64, event: f64) -> usize {
    listener_snapshot(stream, event).len()
}

fn stream_event_names_array(stream: f64) -> *mut crate::array::ArrayHeader {
    let mut out = crate::array::js_array_alloc(0);
    for name in event_names_snapshot(stream) {
        if stream_listener_count_for_event(stream, name) > 0 {
            out = crate::array::js_array_push_f64(out, name);
        }
    }
    out
}

fn stream_listeners_array_for_event(
    stream: f64,
    event: f64,
    raw: bool,
) -> *mut crate::array::ArrayHeader {
    let snapshot = listener_snapshot(stream, event);
    let mut out = crate::array::js_array_alloc(snapshot.len() as u32);
    for (listener, once) in snapshot {
        if raw && once {
            let obj = crate::object::js_object_alloc(0, 1);
            crate::object::js_object_set_field_by_name(
                obj,
                super::hidden_key(b"listener"),
                listener,
            );
            out = crate::array::js_array_push_f64(out, super::box_pointer(obj as *const u8));
        } else {
            out = crate::array::js_array_push_f64(out, listener);
        }
    }
    out
}

pub(super) fn call_listener_args(stream: f64, listener: f64, args: &[f64]) {
    if !is_callable_value(listener) {
        return;
    }
    let prev = crate::object::js_implicit_this_set(stream);
    unsafe {
        let _ = crate::closure::js_native_call_value(listener, args.as_ptr(), args.len());
    }
    crate::object::js_implicit_this_set(prev);
}

pub(super) fn emit_stream_event_from_array(
    stream: f64,
    event: f64,
    args_arr: *const crate::array::ArrayHeader,
) -> f64 {
    let len = if args_arr.is_null() {
        0
    } else {
        crate::array::js_array_length(args_arr)
    };
    let mut args = Vec::with_capacity(len as usize);
    for i in 0..len {
        args.push(crate::array::js_array_get_f64(args_arr, i));
    }
    emit_stream_event(stream, event, &args)
}

pub(super) fn emit_stream_event(stream: f64, event: f64, args: &[f64]) -> f64 {
    if event_identity_bytes(event).is_none() {
        return f64::from_bits(super::TAG_FALSE);
    }
    if super::string_value_eq(event, b"error") {
        if let Some(first) = args.first() {
            super::set_hidden_value(stream, super::hidden_error_key(), *first);
            super::refresh_readable_aborted_flag(stream);
        }
        let monitor_event = error_monitor_event();
        let monitor_snapshot = listener_snapshot(stream, monitor_event);
        if monitor_snapshot.iter().any(|(_, once)| *once) {
            remove_once_listeners(stream, monitor_event);
        }
        for (listener, _) in monitor_snapshot {
            call_listener_args(stream, listener, args);
        }
    }

    let snapshot = listener_snapshot(stream, event);
    if snapshot.is_empty() {
        if super::string_value_eq(event, b"error") {
            let err = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(super::TAG_UNDEFINED));
            crate::exception::js_throw(err);
        }
        return f64::from_bits(super::TAG_FALSE);
    }
    if snapshot.iter().any(|(_, once)| *once) {
        remove_once_listeners(stream, event);
    }
    for (listener, _) in snapshot {
        call_listener_args(stream, listener, args);
    }
    f64::from_bits(super::TAG_TRUE)
}

fn error_monitor_event() -> f64 {
    unsafe { crate::symbol::js_symbol_for(super::string_value(b"events.errorMonitor")) }
}

fn emit_meta_event(stream: f64, name: &[u8], args: &[f64]) {
    let event = super::string_value(name);
    if stream_listener_count_for_event(stream, event) > 0 {
        let _ = emit_stream_event(stream, event, args);
    }
}
