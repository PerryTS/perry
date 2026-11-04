use super::*;

pub(crate) unsafe extern "C" fn dispatch(
    method: *const u8,
    method_len: usize,
    args: *const f64,
    args_len: usize,
    result: *mut f64,
) -> bool {
    let method = std::slice::from_raw_parts(method, method_len);
    let arg = |index| {
        if index < args_len && !args.is_null() {
            *args.add(index)
        } else {
            js_undefined()
        }
    };
    *result = match method {
        b"receiveMessageOnPort" => js_worker_threads_receive_message_on_port(arg(0)),
        b"postMessageToThread" => {
            direct_message::js_worker_threads_post_message_to_thread(arg(0), arg(1), arg(2), arg(3))
        }
        _ => return false,
    };
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invoke(method: &[u8], args: &[f64], result: &mut f64) -> bool {
        unsafe {
            dispatch(
                method.as_ptr(),
                method.len(),
                args.as_ptr(),
                args.len(),
                result,
            )
        }
    }

    #[test]
    fn callable_adapter_preserves_values_absence_and_unclaimed_result() {
        std::thread::spawn(|| {
            let agent = perry_runtime::agent::enter_worker_agent();
            perry_runtime::gc::gc_init();
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let channel = scope.root_nanbox_f64(js_worker_threads_message_channel_new());
            let port = scope.root_nanbox_f64(get_object_field_from_value(
                channel.get_nanbox_f64(),
                "port1",
            ));
            let mut result = 37.0;
            assert!(invoke(
                b"receiveMessageOnPort",
                &[port.get_nanbox_f64(), 99.0],
                &mut result
            ));
            assert_eq!(result.to_bits(), js_undefined().to_bits());
            assert!(invoke(b"postMessageToThread", &[], &mut result));
            let rejected = scope.root_nanbox_f64(result);
            let promise = JSValue::from_bits(rejected.get_nanbox_f64().to_bits())
                .as_pointer::<perry_runtime::Promise>()
                .cast_mut();
            assert_eq!(perry_runtime::js_promise_state(promise), 2);
            perry_runtime::promise::js_promise_mark_internally_handled(promise);
            result = 37.0;
            assert!(!invoke(b"moveMessagePortToContext", &[], &mut result));
            assert_eq!(result, 37.0);
            perry_runtime::agent::retire_agent(agent);
        })
        .join()
        .unwrap();
    }
}
