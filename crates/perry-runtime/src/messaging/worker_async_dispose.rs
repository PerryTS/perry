//! Worker disposal delegates to the receiver's current terminate operation.
use super::*;
use crate::closure::JsThis;
use crate::promise::{self, Promise};

fn promise_ptr(value: f64) -> *mut Promise {
    crate::value::js_nanbox_get_pointer(value) as *mut Promise
}

extern "C" fn fulfilled(closure: *const ClosureHeader, _this: JsThis, _value: f64) -> f64 {
    let outer = crate::closure::js_closure_get_capture_f64(closure, 0);
    promise::js_promise_resolve(promise_ptr(outer), js_undefined());
    js_undefined()
}

extern "C" fn rejected(closure: *const ClosureHeader, _this: JsThis, reason: f64) -> f64 {
    let outer = crate::closure::js_closure_get_capture_f64(closure, 0);
    promise::js_promise_reject(promise_ptr(outer), reason);
    js_undefined()
}

fn type_error(message: &str) -> f64 {
    crate::exception::throw_core(crate::error::CoreError::type_error(key(message)))
}

extern "C" fn async_dispose(_closure: *const ClosureHeader, this: JsThis) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(this.as_f64());
    let outer = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        promise::js_promise_new() as i64
    ));
    // Native FN_ASYNC describes the callable, but does not catch body throws
    // or create its result promise. Both are owned here, before any user code.
    let outcome = crate::exception::catch_js_throw(|| {
        match receiver.get_nanbox_u64() {
            crate::value::TAG_NULL => {
                type_error("Cannot read properties of null (reading 'terminate')");
            }
            crate::value::TAG_UNDEFINED => {
                type_error("Cannot read properties of undefined (reading 'terminate')");
            }
            _ => {}
        }
        let method = scope.root_nanbox_f64(unsafe {
            crate::value::js_dynamic_object_get_property(
                receiver.get_nanbox_f64(),
                b"terminate".as_ptr().cast(),
                9,
            )
        });
        if !promise::spec_combinators::is_callable_value(method.get_nanbox_f64()) {
            type_error("this.terminate is not a function");
        }
        let value = scope.root_nanbox_f64(unsafe {
            crate::closure::call_value(
                method.get_nanbox_f64(),
                JsThis::from_f64(receiver.get_nanbox_f64()),
                &[],
            )
        });
        let intrinsic = scope.root_nanbox_f64(object::builtin_constructor_value("Promise"));
        let native = promise::js_value_is_promise(value.get_nanbox_f64()) != 0;
        let constructor_key =
            scope.root_nanbox_f64(crate::value::js_nanbox_string(key("constructor") as i64));
        // The generic dynamic getter only sees a Promise's own expandos;
        // the public property getter also resolves its inherited constructor.
        let same_constructor = native
            && unsafe {
                object::js_object_get_property_key(
                    value.get_nanbox_f64(),
                    constructor_key.get_nanbox_f64(),
                )
            }
            .to_bits()
                == intrinsic.get_nanbox_u64();
        // Await's intrinsic PromiseResolve bypasses `then` and @@species for
        // native promises with the intrinsic constructor, including overrides.
        let awaited = scope.root_nanbox_f64(if same_constructor {
            value.get_nanbox_f64()
        } else {
            crate::value::js_nanbox_pointer(promise::js_promise_new() as i64)
        });
        if !same_constructor {
            promise::assimilate::promise_resolve_assimilating(
                promise_ptr(awaited.get_nanbox_f64()),
                value.get_nanbox_f64(),
            );
        }
        let capture = [outer.get_nanbox_u64()];
        let fulfill = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc_init(
            crate::fn_info!(fulfilled, 1; with_declared(1), with_flags(crate::closure::FN_NON_CONSTRUCTOR)),
            1, capture.as_ptr(),
        ));
        let capture = [outer.get_nanbox_u64()];
        let reject = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc_init(
            crate::fn_info!(rejected, 1; with_declared(1), with_flags(crate::closure::FN_NON_CONSTRUCTOR)),
            1, capture.as_ptr(),
        ));
        promise::js_promise_attach_handlers(
            promise_ptr(awaited.get_nanbox_f64()),
            fulfill.get_raw_mut_ptr(),
            reject.get_raw_mut_ptr(),
        );
    });
    if let Err(reason) = outcome {
        promise::js_promise_reject(promise_ptr(outer.get_nanbox_f64()), reason);
    }
    outer.get_nanbox_f64()
}

pub(crate) fn install(prototype: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let prototype = scope.root_nanbox_f64(prototype);
    let method = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(async_dispose, 0; with_declared(0), with_flags(crate::closure::FN_ASYNC | crate::closure::FN_NON_CONSTRUCTOR)),
        0,
    ));
    object::set_bound_native_closure_name(method.get_raw_mut_ptr(), "[Symbol.asyncDispose]");
    let symbol = crate::symbol::well_known_symbol("asyncDispose");
    unsafe {
        crate::symbol::define_symbol_data_property(
            prototype.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(symbol as i64),
            crate::value::js_nanbox_pointer(method.get_raw_mut_ptr::<ClosureHeader>() as i64),
        );
    }
    crate::symbol::set_symbol_property_attrs(
        crate::value::js_nanbox_get_pointer(prototype.get_nanbox_f64()) as usize,
        symbol as usize,
        object::PropertyAttrs::new(true, false, true),
    );
}
