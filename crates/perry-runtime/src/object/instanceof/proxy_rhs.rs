//! InstanceofOperator and OrdinaryHasInstance for a Proxy constructor (#10364).
use super::*;

pub(super) fn proxy_instanceof(value: f64, constructor: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let constructor = scope.root_nanbox_f64(constructor);
    let symbol = crate::symbol::well_known_symbol("hasInstance");
    let key = crate::value::js_nanbox_pointer(symbol as i64);
    let method = scope.root_nanbox_f64(crate::proxy::js_proxy_get(
        constructor.get_nanbox_f64(),
        key,
    ));
    let method_value = crate::JSValue::from_bits(method.get_nanbox_f64().to_bits());
    let result = if method_value.is_null() || method_value.is_undefined() {
        if !crate::proxy::proxy_wraps_callable(constructor.get_nanbox_f64()) {
            throw_type_error(b"Right-hand side of 'instanceof' is not callable");
        }
        ordinary_proxy_has_instance(constructor.get_nanbox_f64(), value.get_nanbox_f64())
    } else {
        if !crate::proxy::is_callable_function(method.get_nanbox_f64()) {
            throw_type_error(b"Symbol(Symbol.hasInstance) is not a function");
        }
        let previous_this =
            scope.root_nanbox_f64(js_implicit_this_set(constructor.get_nanbox_f64()));
        let called = crate::exception::catch_js_throw(|| unsafe {
            let args = [value.get_nanbox_f64()];
            if crate::proxy::js_proxy_is_proxy(method.get_nanbox_f64()) != 0 {
                crate::proxy::call_proxy_value_with_this(
                    method.get_nanbox_f64(),
                    constructor.get_nanbox_f64(),
                    &args,
                )
            } else {
                crate::closure::js_native_call_value(
                    method.get_nanbox_f64(),
                    crate::closure::plain_call_receiver(),
                    args.as_ptr(),
                    1,
                )
            }
        });
        js_implicit_this_set(previous_this.get_nanbox_f64());
        match called {
            Ok(result) => crate::value::js_is_truthy(result) != 0,
            Err(error) => crate::exception::js_throw(error),
        }
    };
    f64::from_bits(if result {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

pub(super) fn ordinary_proxy_has_instance(constructor: f64, value: f64) -> bool {
    if !crate::proxy::proxy_wraps_callable(constructor)
        || instanceof_lhs_is_primitive(value)
        || !crate::proxy::reflect_value_is_object(value)
    {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    let value = scope.root_nanbox_f64(value);
    let key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let prototype = scope.root_nanbox_f64(crate::proxy::js_proxy_get(
        constructor.get_nanbox_f64(),
        crate::value::js_nanbox_string(key as i64),
    ));
    if !rhs_is_object_value(prototype.get_nanbox_f64())
        && class_ref_id(prototype.get_nanbox_f64()).is_none()
        && class_prototype_ref_id(prototype.get_nanbox_f64()).is_none()
    {
        throw_type_error(b"Function has non-object prototype in instanceof check");
    }
    // Both the desired prototype and the current link can move when a proxy
    // getPrototypeOf trap runs. Compare their rewritten values after each Get.
    let current = value;
    for _ in 0..100_000 {
        let next = js_object_get_prototype_of(current.get_nanbox_f64());
        if crate::JSValue::from_bits(next.to_bits()).is_null() {
            return false;
        }
        current.set_nanbox_f64(next);
        if crate::value::js_jsvalue_equals(current.get_nanbox_f64(), prototype.get_nanbox_f64())
            != 0
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_instanceof_dispatches_before_heap_constructor_probes() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let null = f64::from_bits(crate::value::TAG_NULL);
        let handler = scope.root_nanbox_f64(js_object_create(null));
        let object = scope.root_nanbox_f64(js_object_create(null));
        let noncallable = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            object.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        assert!(crate::exception::catch_js_throw(|| {
            js_instanceof_dynamic(object.get_nanbox_f64(), noncallable.get_nanbox_f64())
        })
        .is_err());
        assert!(!ordinary_has_instance(
            noncallable.get_nanbox_f64(),
            object.get_nanbox_f64()
        ));

        let constructor =
            scope.root_nanbox_f64(js_get_global_this_builtin_value(b"Object".as_ptr(), 6));
        let callable = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            constructor.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        let prototype = scope.root_nanbox_f64(unsafe {
            crate::value::js_dynamic_object_get_property(
                constructor.get_nanbox_f64(),
                b"prototype".as_ptr().cast(),
                9,
            )
        });
        let instance = scope.root_nanbox_f64(js_object_create(prototype.get_nanbox_f64()));
        assert_eq!(
            js_instanceof_dynamic(instance.get_nanbox_f64(), callable.get_nanbox_f64()).to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(
            js_instanceof_dynamic(object.get_nanbox_f64(), callable.get_nanbox_f64()).to_bits(),
            crate::value::TAG_FALSE
        );
        crate::proxy::js_proxy_revoke(callable.get_nanbox_f64());
        assert!(crate::exception::catch_js_throw(|| {
            js_instanceof_dynamic(instance.get_nanbox_f64(), callable.get_nanbox_f64())
        })
        .is_err());
        assert!(!ordinary_has_instance(callable.get_nanbox_f64(), 1.0));

        let bound = scope.root_nanbox_f64(unsafe {
            crate::closure::js_function_bind(constructor.get_nanbox_f64(), std::ptr::null(), 0)
        });
        assert!(!crate::object::function_would_have_own_prototype(
            bound.get_nanbox_f64()
        ));
        assert!(
            crate::object::ordinary_function_prototype_value_for_read(bound.get_nanbox_f64())
                .is_none()
        );
        let wrapped_bound = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            bound.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        assert!(crate::exception::catch_js_throw(|| {
            js_instanceof_dynamic(instance.get_nanbox_f64(), wrapped_bound.get_nanbox_f64())
        })
        .is_err());
    }
}
