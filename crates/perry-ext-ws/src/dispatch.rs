//! Runtime dispatch for WebSocket receivers whose static type was erased.
use super::*;

extern "C" {
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    fn js_class_method_bind(receiver: f64, name: *const u8, len: usize) -> f64;
}

pub(super) unsafe fn register_method_dispatch() {
    js_register_handle_method_dispatch_extension(method);
}

fn knows(handle: i64, name: &str) -> bool {
    if get_handle_mut::<WsServerHandle>(handle).is_some() {
        matches!(
            name,
            "clients" | "address" | "handleUpgrade" | "emit" | "on" | "addListener" | "close"
        )
    } else if get_handle_mut::<WsClientHandle>(handle).is_some() {
        matches!(name, "send" | "close" | "on" | "addListener" | "readyState")
    } else {
        false
    }
}

pub(super) unsafe fn property(handle: i64, ptr: *const u8, len: usize, out: *mut f64) -> i32 {
    if ptr.is_null() {
        return 0;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) else {
        return 0;
    };
    if !knows(handle, name) {
        return 0;
    }
    let value = match name {
        "clients" => js_ws_server_clients(handle),
        "readyState" => js_ws_ready_state(handle),
        _ => js_class_method_bind(f64::from_bits(POINTER_TAG | handle as u64), ptr, len),
    };
    if !out.is_null() {
        *out = value;
    }
    1
}

unsafe extern "C" fn method(
    handle: i64,
    ptr: *const u8,
    len: usize,
    args: *const f64,
    argc: usize,
    out: *mut f64,
) -> i32 {
    if ptr.is_null() {
        return 0;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) else {
        return 0;
    };
    if !knows(handle, name) || matches!(name, "clients" | "readyState") {
        return 0;
    }
    let args = if args.is_null() {
        &[][..]
    } else {
        std::slice::from_raw_parts(args, argc)
    };
    let scope = perry_ffi::TransientRootScope::enter();
    let args: Vec<_> = args.iter().map(|value| scope.root_nanbox(*value)).collect();
    let arg = |i| {
        args.get(i)
            .map(|value: &perry_ffi::TransientRootedNanbox| value.get())
            .unwrap_or_else(undefined)
    };
    let value = match name {
        "address" => js_ws_server_address(handle),
        "handleUpgrade" => {
            js_ws_handle_upgrade(
                handle,
                arg(0),
                arg(1),
                arg(2),
                (arg(3).to_bits() & POINTER_MASK) as i64,
            );
            undefined()
        }
        "emit" => {
            let event = string_arg(arg(0));
            f64::from_bits(
                JsValue::from_bool(js_ws_server_emit(handle, event, arg(1), arg(2)) != 0).bits(),
            )
        }
        "on" | "addListener" => {
            let event = string_arg(arg(0));
            js_ws_on(handle, event, (arg(1).to_bits() & POINTER_MASK) as i64);
            f64::from_bits(POINTER_TAG | handle as u64)
        }
        "send" => {
            js_ws_send(handle, string_arg(arg(0)));
            undefined()
        }
        "close" => {
            js_ws_close(handle);
            undefined()
        }
        _ => return 0,
    };
    if !out.is_null() {
        *out = value;
    }
    1
}

fn string_arg(value: f64) -> *const StringHeader {
    let value = JsValue::from_bits(value.to_bits());
    if value.is_short_string() {
        value_string(value)
            .map(|s| alloc_string(&s).as_raw() as *const StringHeader)
            .unwrap_or(std::ptr::null())
    } else {
        value.as_string_ptr()
    }
}
