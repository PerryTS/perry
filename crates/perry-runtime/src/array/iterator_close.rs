//! Shared synchronous IteratorClose and yield* abrupt protocol operations.
use super::{is_object_like_value, throw_iterator_result_not_object};

#[inline(never)]
fn call_protocol(iter_value: f64, arg: Option<f64>, name: &[u8]) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter = scope.root_nanbox_f64(iter_value);
    let arg = arg.map(|value| scope.root_nanbox_f64(value));
    let key = crate::string::js_string_from_bytes_longlived(name.as_ptr(), name.len() as u32);
    let ret = crate::object::js_object_get_field_by_name_f64(
        crate::value::js_nanbox_get_pointer(iter.get_nanbox_f64())
            as *const crate::object::ObjectHeader,
        key,
    );
    if matches!(
        ret.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        return None;
    }
    if !crate::proxy::is_callable_function(ret) {
        crate::closure::throw_not_callable();
    }
    let method = scope.root_nanbox_f64(ret);
    let args = [arg.as_ref().map_or(0.0, |arg| arg.get_nanbox_f64())];
    let result = unsafe {
        crate::closure::native_call_value_this(
            method.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(iter.get_nanbox_f64()),
            if arg.is_some() {
                args.as_ptr()
            } else {
                std::ptr::null()
            },
            usize::from(arg.is_some()),
        )
    };
    if !is_object_like_value(result) {
        throw_iterator_result_not_object();
    }
    Some(result)
}

fn iterator_close_if_not_done(iter: f64, done: f64) -> f64 {
    if crate::value::js_is_truthy(done) == 0 {
        call_protocol(iter, None, b"return");
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cold]
fn iterator_close_on_throw(iter: f64, done: f64, error: f64) -> f64 {
    // Original throw completion wins over GetMethod/Call/result failures.
    // Both pending operands survive nested user code and catch restoration.
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter = scope.root_nanbox_f64(iter);
    let error = scope.root_nanbox_f64(error);
    if crate::value::js_is_truthy(done) == 0 {
        let _ = crate::exception::catch_js_throw(|| {
            iterator_close_if_not_done(iter.get_nanbox_f64(), done);
        });
    }
    error.get_nanbox_f64()
}

#[cold]
fn iterator_delegate_return(iter: f64, value: f64) -> f64 {
    // Undefined is an absence sentinel; a present method returning undefined
    // instead throws in call_protocol. Unfinished results retain identity.
    call_protocol(iter, Some(value), b"return")
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[cold]
fn iterator_delegate_throw(iter: f64, error: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter = scope.root_nanbox_f64(iter);
    let error = scope.root_nanbox_f64(error);
    if let Some(result) = call_protocol(
        iter.get_nanbox_f64(),
        Some(error.get_nanbox_f64()),
        b"throw",
    ) {
        return result;
    }
    iterator_close_if_not_done(
        iter.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_FALSE),
    );
    let msg = b"The iterator does not provide a 'throw' method";
    let msg = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_iterator_close_if_not_done(iter: f64, done: f64) -> f64 {
    iterator_close_if_not_done(iter, done)
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_iterator_close_if_not_done(iter: f64, done: f64) -> f64 {
    iterator_close_if_not_done(iter, done)
}

#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_iterator_close_on_throw(iter: f64, done: f64, error: f64) -> f64 {
    iterator_close_on_throw(iter, done, error)
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_iterator_close_on_throw(iter: f64, done: f64, error: f64) -> f64 {
    iterator_close_on_throw(iter, done, error)
}

#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_iterator_delegate_return(iter: f64, value: f64) -> f64 {
    iterator_delegate_return(iter, value)
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_iterator_delegate_return(iter: f64, value: f64) -> f64 {
    iterator_delegate_return(iter, value)
}

#[cfg(panic = "abort")]
#[no_mangle]
pub extern "C" fn js_iterator_delegate_throw(iter: f64, error: f64) -> f64 {
    iterator_delegate_throw(iter, error)
}

#[cfg(not(panic = "abort"))]
#[no_mangle]
pub extern "C-unwind" fn js_iterator_delegate_throw(iter: f64, error: f64) -> f64 {
    iterator_delegate_throw(iter, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{closure, object, string, value};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn returns_object(
        _closure: *const closure::ClosureHeader,
        _this: closure::JsThis,
        value: f64,
    ) -> f64 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        let obj = object::js_object_alloc(0, 1);
        let key = string::js_string_from_bytes(b"value".as_ptr(), 5);
        object::js_object_set_field_by_name(obj, key, value);
        value::js_nanbox_pointer(obj as i64)
    }
    fn iterator(method: f64) -> f64 {
        let obj = object::js_object_alloc(0, 1);
        let key = string::js_string_from_bytes(b"return".as_ptr(), 6);
        object::js_object_set_field_by_name(obj, key, method);
        value::js_nanbox_pointer(obj as i64)
    }
    #[test]
    fn iterator_close_calls_only_before_exhaustion() {
        CALLS.store(0, Ordering::Relaxed);
        let method = closure::js_closure_alloc(crate::fn_info!(returns_object, 1), 0);
        let iter = iterator(value::js_nanbox_pointer(method as i64));
        iterator_close_if_not_done(iter, f64::from_bits(value::TAG_TRUE));
        assert_eq!(CALLS.load(Ordering::Relaxed), 0);
        iterator_close_if_not_done(iter, f64::from_bits(value::TAG_FALSE));
        assert_eq!(CALLS.load(Ordering::Relaxed), 1);
        let result = iterator_delegate_return(iter, 42.0);
        let key = string::js_string_from_bytes(b"value".as_ptr(), 5);
        let got = object::js_object_get_field_by_name_f64(
            value::js_nanbox_get_pointer(result) as *const object::ObjectHeader,
            key,
        );
        assert_eq!(got, 42.0);
        assert_eq!(CALLS.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn iterator_close_preserves_throw_and_checks_callability() {
        let iter = iterator(17.0);
        assert!(crate::exception::catch_js_throw(|| {
            iterator_close_if_not_done(iter, f64::from_bits(value::TAG_FALSE));
        })
        .is_err());
        assert_eq!(
            iterator_close_on_throw(iter, f64::from_bits(value::TAG_FALSE), 91.0),
            91.0
        );
        let absent = iterator(f64::from_bits(value::TAG_UNDEFINED));
        assert_eq!(
            iterator_delegate_return(absent, 1.0).to_bits(),
            value::TAG_UNDEFINED
        );
        assert!(crate::exception::catch_js_throw(|| {
            iterator_delegate_throw(absent, 1.0);
        })
        .is_err());
    }
}
