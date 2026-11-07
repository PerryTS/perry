use super::*;
use crate::closure::{ClosureHeader, JsThis};

fn channel() -> f64 {
    crate::event_target_hook_slots::install();
    crate::object::js_get_global_this();
    boxed_ptr(crate::messaging::broadcast_channel_object_with_id(
        0,
        string_value(b"events"),
    ))
}

fn call(target: f64, method: &str, args: &[f64]) -> f64 {
    unsafe {
        crate::object::js_native_call_method(
            target,
            method.as_ptr().cast(),
            method.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

fn listeners(target: f64, event: &[u8]) -> u32 {
    unsafe {
        js_array_length(js_event_target_get_event_listeners(
            value_as_ptr(target).unwrap(),
            key(event),
        ))
    }
}

extern "C" fn record(closure: *const ClosureHeader, this: JsThis, event: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_nanbox_f64(event);
    let trace = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
    let code = crate::closure::js_closure_get_capture_f64(closure, 1);
    let event = value_as_ptr::<ObjectHeader>(event.get_nanbox_f64()).unwrap();
    assert_eq!(
        state::get(event, state::TARGET).to_bits(),
        this.as_f64().to_bits()
    );
    assert!(event_bool_field(event, state::DISPATCHED));
    assert_eq!(js_array_push_f64(trace, code), trace);
    undefined_value()
}

fn recorder(trace: *mut ArrayHeader, code: f64) -> f64 {
    let closure = crate::closure::js_closure_alloc(crate::fn_info!(record, 1; with_declared(1)), 2);
    crate::closure::js_closure_set_capture_ptr(closure, 0, trace as i64);
    crate::closure::js_closure_set_capture_f64(closure, 1, code);
    boxed_ptr(closure)
}

extern "C" fn clear_constructor(_: *const ClosureHeader, this: JsThis, _: f64) -> f64 {
    js_object_set_field_by_name(
        value_as_ptr(this.as_f64()).unwrap(),
        key(b"constructor"),
        null_value(),
    );
    undefined_value()
}

#[test]
fn once_removal_uses_validated_record_after_constructor_mutation() {
    let _gc = crate::gc::GcSuppressScope::new();
    let target = channel();
    let ptr = value_as_ptr(target).unwrap();
    let constructor = js_object_get_field_by_name_f64(ptr, key(b"constructor"));
    let trace = js_array_alloc(8);
    let name = string_value(b"message");
    let mutator = crate::closure::js_closure_alloc(crate::fn_info!(clear_constructor, 1), 0);
    call(target, "addEventListener", &[name, boxed_ptr(mutator)]);
    let options = js_object_alloc(0, 0);
    js_object_set_field_by_name(options, key(b"once"), bool_value(true));
    call(
        target,
        "addEventListener",
        &[name, recorder(trace, 1.0), boxed_ptr(options)],
    );
    for _ in 0..2 {
        call(
            target,
            "dispatchEvent",
            &[boxed_ptr(js_event_new(name, undefined_value(), 1))],
        );
        js_object_set_field_by_name(ptr, key(b"constructor"), constructor);
    }
    assert_eq!(
        js_array_length(trace),
        1,
        "once removal must not revalidate the mutable constructor"
    );
    assert_eq!(listeners(target, b"message"), 1);
}

#[test]
fn callable_proxy_listener_uses_the_target_receiver() {
    let _gc = crate::gc::GcSuppressScope::new();
    let target = channel();
    let trace = js_array_alloc(8);
    let proxy = crate::proxy::js_proxy_new(recorder(trace, 7.0), boxed_ptr(js_object_alloc(0, 0)));
    let name = string_value(b"message");
    call(target, "addEventListener", &[name, proxy]);
    call(
        target,
        "dispatchEvent",
        &[boxed_ptr(js_event_new(name, undefined_value(), 1))],
    );
    assert_eq!(js_array_length(trace), 1);
    assert_eq!(js_array_get(trace, 0).as_number(), 7.0);
}

#[test]
fn broadcast_handlers_share_ordered_listeners_and_survive_close() {
    let _gc = crate::gc::GcSuppressScope::new();
    let target = channel();
    let ptr = value_as_ptr::<ObjectHeader>(target).unwrap();
    let trace = js_array_alloc(32);
    let name = string_value(b"message");
    js_object_set_field_by_name(ptr, key(b"onmessage"), null_value());
    assert_eq!(listeners(target, b"message"), 1);
    call(target, "addEventListener", &[name, recorder(trace, 1.0)]);
    js_object_set_field_by_name(ptr, key(b"onmessage"), recorder(trace, 2.0));
    call(target, "addEventListener", &[name, recorder(trace, 3.0)]);
    for value in [None, Some(4.0), Some(0.0), Some(5.0)] {
        if let Some(value) = value {
            let handler = if value == 0.0 {
                null_value()
            } else {
                recorder(trace, value)
            };
            js_object_set_field_by_name(ptr, key(b"onmessage"), handler);
        }
        call(
            target,
            "dispatchEvent",
            &[boxed_ptr(js_event_new(name, undefined_value(), 1))],
        );
        assert_eq!(listeners(target, b"message"), 3);
    }
    let actual: Vec<_> = (0..js_array_length(trace))
        .map(|i| js_array_get(trace, i).as_number())
        .collect();
    assert_eq!(
        actual,
        [2.0, 1.0, 3.0, 4.0, 1.0, 3.0, 1.0, 3.0, 5.0, 1.0, 3.0]
    );
    call(target, "close", &[]);
    call(
        target,
        "dispatchEvent",
        &[boxed_ptr(js_event_new(name, undefined_value(), 1))],
    );
    assert_eq!(js_array_length(trace), 14);
    assert_eq!(listeners(target, b"message"), 3);
}

extern "C" fn handle_event(_: *const ClosureHeader, this: JsThis, event: f64) -> f64 {
    let this = value_as_ptr::<ObjectHeader>(this.as_f64()).unwrap();
    js_object_set_field_by_name(this, key(b"seen"), event);
    undefined_value()
}

#[test]
fn broadcast_listener_objects_are_called_with_the_object_receiver() {
    let _gc = crate::gc::GcSuppressScope::new();
    let target = channel();
    let listener = js_object_alloc(0, 0);
    let callback = crate::closure::js_closure_alloc(crate::fn_info!(handle_event, 1), 0);
    js_object_set_field_by_name(listener, key(b"handleEvent"), boxed_ptr(callback));
    call(
        target,
        "addEventListener",
        &[string_value(b"object"), boxed_ptr(listener)],
    );
    let event = boxed_ptr(js_event_new(string_value(b"object"), undefined_value(), 1));
    call(target, "dispatchEvent", &[event]);
    assert_eq!(
        js_object_get_field_by_name_f64(listener, key(b"seen")).to_bits(),
        event.to_bits()
    );
    assert!(!event_bool_field(
        value_as_ptr(event).unwrap(),
        state::DISPATCHED
    ));
}

extern "C" fn recurse(_: *const ClosureHeader, this: JsThis, event: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_nanbox_f64(event);
    let target = scope.root_nanbox_f64(this.as_f64());
    let error = crate::exception::catch_js_throw(|| {
        call(
            target.get_nanbox_f64(),
            "dispatchEvent",
            &[event.get_nanbox_f64()],
        )
    })
    .unwrap_err();
    let error = scope.root_nanbox_f64(error);
    let ptr = value_as_ptr(event.get_nanbox_f64()).unwrap();
    assert!(event_bool_field(ptr, state::DISPATCHED));
    assert_eq!(
        state::get(ptr, state::TARGET).to_bits(),
        target.get_nanbox_u64()
    );
    let error_key = key(b"recursiveError");
    js_object_set_field_by_name(
        value_as_ptr(target.get_nanbox_f64()).unwrap(),
        error_key,
        error.get_nanbox_f64(),
    );
    undefined_value()
}

#[test]
fn shared_dispatch_rejects_recursion_and_restores_state_after_throw() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(channel());
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(recurse, 1),
        0,
    ));
    let options = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
    let once = key(b"once");
    options.with_mut_ptr(|ptr| js_object_set_field_by_name(ptr, once, bool_value(true)));
    call(
        target.get_nanbox_f64(),
        "addEventListener",
        &[
            string_value(b"recursive"),
            boxed_ptr(callback.get_raw_mut_ptr::<ClosureHeader>()),
            boxed_ptr(options.get_raw_mut_ptr::<ObjectHeader>()),
        ],
    );
    let event = scope.root_nanbox_f64(boxed_ptr(js_event_new(
        string_value(b"recursive"),
        undefined_value(),
        1,
    )));
    assert_eq!(
        call(
            target.get_nanbox_f64(),
            "dispatchEvent",
            &[event.get_nanbox_f64()]
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
    let error_key = key(b"recursiveError");
    let error =
        js_object_get_field_by_name_f64(value_as_ptr(target.get_nanbox_f64()).unwrap(), error_key);
    let error = scope.root_nanbox_f64(error);
    for (name, expected) in [
        (b"name".as_slice(), "Error"),
        (b"code", "ERR_EVENT_RECURSION"),
        (
            b"message",
            "The event \"recursive\" is already being dispatched",
        ),
    ] {
        let name = key(name);
        let value =
            js_object_get_field_by_name_f64(value_as_ptr(error.get_nanbox_f64()).unwrap(), name);
        assert_eq!(
            crate::value::boxed_text::get_string_content(value),
            expected
        );
    }
    assert!(!event_bool_field(
        value_as_ptr(event.get_nanbox_f64()).unwrap(),
        state::DISPATCHED
    ));
    assert_eq!(listeners(target.get_nanbox_f64(), b"recursive"), 0);
    assert_eq!(
        call(
            target.get_nanbox_f64(),
            "dispatchEvent",
            &[event.get_nanbox_f64()]
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
}
