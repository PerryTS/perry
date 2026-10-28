//! JavaScript AsyncResource method receiver semantics.
use super::*;

fn resource_receiver(receiver: f64) -> i64 {
    let value = JSValue::from_bits(receiver.to_bits());
    if value.is_null() || value.is_undefined() {
        let message = if value.is_null() {
            b"Cannot read properties of null".as_slice()
        } else {
            b"Cannot read properties of undefined".as_slice()
        };
        let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let error = crate::error::js_typeerror_new(message);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64));
    }
    crate::value::js_nanbox_get_pointer(receiver)
}

pub(super) extern "C" fn async_resource_id_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    js_async_resource_async_id(resource_receiver(this.as_f64()))
}

pub(super) extern "C" fn async_resource_trigger_id_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    js_async_resource_trigger_async_id(resource_receiver(this.as_f64()))
}

pub(super) extern "C" fn async_resource_destroy_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    js_async_resource_emit_destroy(resource_receiver(this.as_f64()))
}

pub(super) extern "C" fn async_resource_run_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    callback: f64,
    this_arg: f64,
    rest: f64,
) -> f64 {
    js_async_resource_run_in_async_scope(
        resource_receiver(this.as_f64()),
        callback,
        this_arg,
        crate::value::js_nanbox_get_pointer(rest),
    )
}

pub(super) extern "C" fn async_resource_bind_thunk(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    callback: f64,
    this_arg: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(this.as_f64());
    let callback = scope.root_nanbox_f64(callback);
    let this_arg = scope.root_nanbox_f64(this_arg);
    validate_bind_callback(callback.get_nanbox_f64());
    // Native callers keep `js_async_resource_bind`'s silent-invalid-receiver
    // contract; the JavaScript seam always dispatches through public Get, so
    // an own `runInAsyncScope` on a real resource is honored as in Node.
    let bound = bind_deferred_receiver(
        receiver.get_nanbox_f64(),
        callback.get_nanbox_f64(),
        this_arg.get_nanbox_f64(),
    );
    if bound == 0 {
        TAG_UNDEFINED_F64
    } else {
        crate::value::js_nanbox_pointer(bound)
    }
}

fn scope_method(receiver: f64) -> f64 {
    resource_receiver(receiver);
    unsafe { crate::value::js_get_property(receiver, b"runInAsyncScope".as_ptr() as i64, 15) }
}

fn bind_deferred_receiver(receiver: f64, callback: f64, this_arg: f64) -> i64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let callback = scope.root_nanbox_f64(callback);
    let this_arg = scope.root_nanbox_f64(this_arg);
    // Node's implicit-this wrapper looks up the method at invocation; an
    // explicit thisArg instead captures and validates it at bind time.
    let method = if JSValue::from_bits(this_arg.get_nanbox_f64().to_bits()).is_undefined() {
        TAG_UNDEFINED_F64
    } else {
        let method = scope_method(receiver.get_nanbox_f64());
        if !is_callable_value(method) {
            throw_apply_not_function(method);
        }
        method
    };
    let method = scope.root_nanbox_f64(method);
    let closure = js_closure_alloc(
        crate::fn_info!(deferred_bind_trampoline, 1; with_rest(0)),
        4,
    );
    if closure.is_null() {
        return 0;
    }
    let closure = scope.root_raw_mut_ptr(closure);
    for (index, value) in [
        receiver.get_nanbox_f64(),
        callback.get_nanbox_f64(),
        this_arg.get_nanbox_f64(),
        method.get_nanbox_f64(),
    ]
    .into_iter()
    .enumerate()
    {
        js_closure_set_capture_f64(closure.get_raw_mut_ptr(), index as u32, value);
    }
    // An explicit thisArg is FunctionPrototypeBind of the method in Node, so
    // the name follows the method's public `name`; implicit stays anonymous.
    let mut name = String::from("bound");
    if !JSValue::from_bits(method.get_nanbox_f64().to_bits()).is_undefined() {
        let method_name = unsafe {
            crate::value::js_get_property(method.get_nanbox_f64(), b"name".as_ptr() as i64, 4)
        };
        name.push(' ');
        if JSValue::from_bits(method_name.to_bits()).is_string() {
            name.push_str(&js_string_value_to_string(method_name));
        }
    }
    crate::object::set_bound_native_closure_name(closure.get_raw_mut_ptr::<ClosureHeader>(), &name);
    // Node copies `fn.length` through ordinary Get, whatever its type.
    let length = unsafe {
        crate::value::js_get_property(callback.get_nanbox_f64(), b"length".as_ptr() as i64, 6)
    };
    let length = scope.root_nanbox_f64(length);
    crate::closure::closure_define_data_with_attrs(
        closure.get_raw_mut_ptr::<ClosureHeader>() as usize,
        "length",
        length.get_nanbox_f64(),
        crate::object::PropertyAttrs::new(false, false, true),
    );
    closure.get_raw_mut_ptr::<ClosureHeader>() as i64
}

extern "C" fn deferred_bind_trampoline(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let callback = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let bound_this = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 2));
    let captured_method = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 3));
    let call_this = scope.root_nanbox_f64(this.as_f64());
    let rest = scope.root_nanbox_f64(rest);
    let dynamic = JSValue::from_bits(bound_this.get_nanbox_f64().to_bits()).is_undefined();
    let method = if dynamic {
        scope_method(receiver.get_nanbox_f64())
    } else {
        captured_method.get_nanbox_f64()
    };
    let method = scope.root_nanbox_f64(method);
    let mut args = vec![
        callback.get_nanbox_f64(),
        if dynamic {
            call_this.get_nanbox_f64()
        } else {
            bound_this.get_nanbox_f64()
        },
    ];
    let array = crate::value::js_nanbox_get_pointer(rest.get_nanbox_f64()) as *const ArrayHeader;
    for index in 0..js_array_length(array) {
        args.push(crate::array::js_array_get_f64(array, index));
    }
    let args = scope.root_nanbox_f64_slice(&args);
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    unsafe {
        crate::object::method_call::js_method_call_value(
            method.get_nanbox_f64(),
            receiver.get_nanbox_f64(),
            args.as_ptr(),
            args.len(),
        )
    }
}

crate::realm_decl!("resource_methods.realm.json#*");
