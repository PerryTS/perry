//! Message-method transfer admission, before entry validation or detachment.
use crate::gc::RuntimeHandleScope;
use crate::value::{js_nanbox_get_pointer, JSValue};

fn optional_sequence(value: f64) -> Option<f64> {
    if !crate::builtins::arithmetic::js_value_is_object(value) {
        return None;
    }
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let symbol = crate::symbol::well_known_symbol("iterator");
    let method = unsafe {
        crate::symbol::js_object_get_symbol_property(
            value.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(symbol as i64),
        )
    };
    if !crate::proxy::is_callable_function(method) {
        return None;
    }
    let method = scope.root_nanbox_f64(method);
    let iterator = unsafe {
        crate::closure::native_call_value_this(
            method.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(value.get_nanbox_f64()),
            std::ptr::null(),
            0,
        )
    };
    if !crate::builtins::arithmetic::js_value_is_object(iterator) {
        return None;
    }
    let iterator = scope.root_nanbox_f64(iterator);
    let next = unsafe { crate::array::js_iterator_next_method(iterator.get_nanbox_f64()) };
    if !crate::proxy::is_callable_function(next) {
        return None;
    }
    Some(unsafe {
        crate::array::js_iterator_step_rest_to_array(
            iterator.get_nanbox_f64(),
            next,
            f64::from_bits(JSValue::bool(false).bits()),
        )
    })
}

#[no_mangle]
pub extern "C-unwind" fn js_worker_transfer_sequence(value: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let js = JSValue::from_bits(value.get_nanbox_u64());
    if js.is_undefined() || js.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    if !crate::builtins::arithmetic::js_value_is_object(value.get_nanbox_f64()) {
        crate::validators::received::throw_type_error_with_code(
            "Optional transferList argument must be an iterable",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    if let Some(sequence) = optional_sequence(value.get_nanbox_f64()) {
        return sequence;
    }
    // A malformed direct record falls back only after its factory/next reads.
    let key = crate::string::intern_ascii_literal(b"transfer");
    let transfer = crate::object::js_object_get_field_by_name_f64(
        js_nanbox_get_pointer(value.get_nanbox_f64()) as *const crate::object::ObjectHeader,
        key,
    );
    if JSValue::from_bits(transfer.to_bits()).is_undefined() {
        return transfer;
    }
    if let Some(sequence) = optional_sequence(transfer) {
        return sequence;
    }
    crate::validators::received::throw_type_error_with_code(
        "Optional options.transfer argument must be an iterable",
        "ERR_INVALID_ARG_TYPE",
    )
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_WORKER_TRANSFER_SEQUENCE: extern "C-unwind" fn(f64) -> f64 =
    js_worker_transfer_sequence;

#[cfg(test)]
mod tests {
    use super::*;

    fn field(object: f64, name: &[u8], value: f64) {
        let scope = RuntimeHandleScope::new();
        let object = scope.root_nanbox_f64(object);
        let value = scope.root_nanbox_f64(value);
        let key = crate::string::intern_ascii_literal(name);
        crate::object::js_object_set_field_by_name(
            js_nanbox_get_pointer(object.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
            key,
            value.get_nanbox_f64(),
        );
    }

    extern "C" fn captured(
        closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        crate::closure::js_closure_get_capture_f64(closure, 0)
    }

    #[test]
    fn worker_transfer_sequence_materializes_and_falls_back() {
        let scope = RuntimeHandleScope::new();
        let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc(1));
        array.with_mut_ptr(|p| crate::array::js_array_push_f64(p, 7.0));
        let input = crate::value::js_nanbox_pointer(
            array.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
        );
        let result = scope.root_nanbox_f64(js_worker_transfer_sequence(input));
        let output =
            js_nanbox_get_pointer(result.get_nanbox_f64()) as *const crate::array::ArrayHeader;
        assert_eq!(crate::array::js_array_length(output), 1);
        assert_eq!(crate::array::js_array_get_f64(output, 0), 7.0);
        for primitive in [true, false] {
            let malformed = if primitive {
                42.0
            } else {
                crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64)
            };
            let malformed = scope.root_nanbox_f64(malformed);
            let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            let object_value = || {
                crate::value::js_nanbox_pointer(
                    object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64,
                )
            };
            field(
                object_value(),
                b"transfer",
                crate::value::js_nanbox_pointer(
                    array.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
                ),
            );
            let factory = crate::closure::js_closure_alloc(crate::fn_info!(captured, 0), 1);
            crate::closure::js_closure_set_capture_f64(factory, 0, malformed.get_nanbox_f64());
            let factory = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(factory as i64));
            let symbol = crate::symbol::js_symbol_well_known_iterator();
            unsafe {
                crate::symbol::js_object_set_symbol_property(
                    object_value(),
                    symbol,
                    factory.get_nanbox_f64(),
                );
            }
            let result = js_worker_transfer_sequence(object_value());
            assert_eq!(
                crate::array::js_array_length(
                    js_nanbox_get_pointer(result) as *const crate::array::ArrayHeader
                ),
                1
            );
        }
    }

    #[test]
    fn worker_transfer_sequence_rejects_primitives_and_nested_noniterables() {
        assert!(JSValue::from_bits(
            js_worker_transfer_sequence(f64::from_bits(JSValue::undefined().bits())).to_bits()
        )
        .is_undefined());
        assert!(crate::exception::catch_js_throw(|| js_worker_transfer_sequence(1.0)).is_err());
        let scope = RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let value = crate::value::js_nanbox_pointer(
            object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64,
        );
        field(value, b"transfer", 1.0);
        assert!(
            crate::exception::catch_js_throw(|| js_worker_transfer_sequence(
                crate::value::js_nanbox_pointer(
                    object.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
                )
            ))
            .is_err()
        );
    }
}
