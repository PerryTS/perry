//! Worker error events retain diagnostics even when an Error's fields cannot
//! cross the structured-clone boundary.
use super::*;
use perry_runtime::gc::RuntimeHandleScope;

pub(super) fn worker_error_message(thrown: f64) -> SerializedValue {
    let scope = RuntimeHandleScope::new();
    let thrown = scope.root_nanbox_f64(thrown);
    if let Ok(message) = clone::try_clone_message(thrown.get_nanbox_f64(), js_undefined()) {
        return message;
    }
    use perry_runtime::error::*;
    // Resolve the native family through ancestry, not the user-writable name.
    // Node reconstructs a subclass as its nearest built-in Error family.
    let family = [
        (CLASS_ID_TYPE_ERROR, b"TypeError".as_slice()),
        (CLASS_ID_RANGE_ERROR, b"RangeError".as_slice()),
        (CLASS_ID_REFERENCE_ERROR, b"ReferenceError".as_slice()),
        (CLASS_ID_SYNTAX_ERROR, b"SyntaxError".as_slice()),
        (CLASS_ID_EVAL_ERROR, b"EvalError".as_slice()),
        (CLASS_ID_URI_ERROR, b"URIError".as_slice()),
        (CLASS_ID_ERROR, b"Error".as_slice()),
    ]
    .into_iter()
    .find_map(|(id, name)| {
        (perry_runtime::object::js_instanceof(thrown.get_nanbox_f64(), id).to_bits()
            == JSValue::bool(true).bits())
        .then_some(name)
    });
    let Some(name) = family else {
        return plain_error_message(b"the worker threw a value that could not be cloned");
    };
    let get = |key: &str| unsafe {
        perry_runtime::value::js_dynamic_object_get_property(
            thrown.get_nanbox_f64(),
            key.as_ptr().cast(),
            key.len(),
        )
    };
    // Each read can collect; keep only owned bytes between reads.
    let message = diagnostic_string(get("message"));
    let stack = diagnostic_string(get("stack"));
    let cause = get("cause");
    let cause = if is_undefined(cause) {
        None
    } else {
        clone::try_clone_message(cause, js_undefined())
            .ok()
            .map(Box::new)
    };
    SerializedValue::Error {
        name: name.to_vec(),
        message,
        stack,
        cause,
    }
}

fn diagnostic_string(value: f64) -> Option<Vec<u8>> {
    let ptr = perry_runtime::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        return None;
    }
    // Unified string pointers are flat; copy the WTF-8 bytes without coercing.
    unsafe {
        let data = ptr.cast::<u8>().add(std::mem::size_of::<StringHeader>());
        Some(std::slice::from_raw_parts(data, (*ptr).byte_len as usize).to_vec())
    }
}

pub(super) fn plain_error_message(message: &[u8]) -> SerializedValue {
    SerializedValue::Error {
        name: b"Error".to_vec(),
        message: Some(message.to_vec()),
        stack: None,
        cause: None,
    }
}
