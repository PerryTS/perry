//! Synchronous `net.Socket` EventEmitter dispatch used by the HTTP Agent.

use super::*;

extern "C" {
    fn js_closure_call_array(closure: i64, args: *const f64, args_len: i64) -> f64;
}

type HttpAgentSocketEventHook = extern "C" fn(i64, *const u8, usize);

fn http_agent_socket_event_hook() -> &'static Mutex<Option<HttpAgentSocketEventHook>> {
    static HOOK: std::sync::OnceLock<Mutex<Option<HttpAgentSocketEventHook>>> =
        std::sync::OnceLock::new();
    HOOK.get_or_init(|| Mutex::new(None))
}

/// Mark an alloc-only net.Socket handle as the public facade for an HTTP
/// Agent pool slot. `active != 0` selects the in-use listener shape; zero is
/// the free-pool shape.
#[no_mangle]
pub extern "C" fn js_ext_net_set_http_agent_phase(handle: i64, active: i32) {
    if is_net_socket_handle(handle) {
        statics::http_agent_phases()
            .lock()
            .unwrap()
            .insert(handle, active != 0);
    }
}

/// Register the HTTP-side lifecycle hook used when user code manually emits
/// an error on an idle Agent socket.
#[no_mangle]
pub extern "C" fn js_ext_net_register_http_agent_socket_event_hook(hook: HttpAgentSocketEventHook) {
    *http_agent_socket_event_hook().lock().unwrap() = Some(hook);
}

/// Synchronous EventEmitter `socket.emit(event, ...args)` surface. Network
/// generated events still travel through the pending-event pump; this entry
/// point covers explicit user emission and preserves `once()` removal.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_emit(
    handle: i64,
    event_ptr: i64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let Some(event) = lifecycle::event_name_from_ptr(event_ptr) else {
        return f64::from_bits(0x7FFC_0000_0000_0003);
    };
    let args = if args_ptr.is_null() || args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    let arg_bits = args.iter().map(|arg| arg.to_bits()).collect::<Vec<_>>();
    let mut frame = dispatch_custody::DispatchFrame::park(listeners_for(handle, &event));
    frame.set_payloads(&arg_bits);
    for index in 0..frame.len() {
        let callback = frame.cb(index);
        if callback == 0 {
            continue;
        }
        let closure = JsClosure::from_raw(callback as *const RawClosureHeader);
        match arg_bits.len() {
            0 => {
                let _ = closure.call0();
            }
            1 => {
                let _ = closure.call1(f64::from_bits(frame.payload_bits()));
            }
            2 => {
                let _ = closure.call2(
                    f64::from_bits(frame.payload_bits_at(0)),
                    f64::from_bits(frame.payload_bits_at(1)),
                );
            }
            _ => {
                let args = (0..arg_bits.len())
                    .map(|index| f64::from_bits(frame.payload_bits_at(index)))
                    .collect::<Vec<_>>();
                let _ = js_closure_call_array(callback, args.as_ptr(), args.len() as i64);
            }
        }
    }
    let emitted = frame.len() != 0;
    drop(frame);
    lifecycle::drain_once_listeners(handle, &event);
    if statics::http_agent_phases()
        .lock()
        .unwrap()
        .contains_key(&handle)
    {
        if event == "error" {
            js_ext_net_destroy_socket(handle);
        }
        let hook = *http_agent_socket_event_hook().lock().unwrap();
        if let Some(hook) = hook {
            hook(handle, event.as_ptr(), event.len());
        }
    }
    f64::from_bits(if emitted {
        0x7FFC_0000_0000_0004
    } else {
        0x7FFC_0000_0000_0003
    })
}

/// Queue a Node AbortError on a socket. Used by Agent.createConnection's
/// AbortSignal integration so an already-aborted signal still emits after the
/// caller has had a chance to attach `once(socket, 'error')`.
#[no_mangle]
pub extern "C" fn js_ext_net_socket_emit_abort_error(handle: i64) {
    js_ext_net_destroy_socket(handle);
    push_event(PendingNetEvent::AbortError(handle));
}
