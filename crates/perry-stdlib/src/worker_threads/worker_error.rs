//! Worker error events retain diagnostics even when an Error's fields cannot
//! cross the structured-clone boundary.
use super::*;
use perry_runtime::gc::RuntimeHandleScope;

mod properties;
use properties::Properties;

fn error_family(thrown: f64) -> Option<&'static [u8]> {
    use perry_runtime::error::*;
    // Resolve the native family through ancestry, not the user-writable name.
    // Node reconstructs a subclass as its nearest built-in Error family.
    [
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
        (perry_runtime::object::js_instanceof(thrown, id).to_bits() == JSValue::bool(true).bits())
            .then_some(name)
    })
}

fn cloned_diagnostic(value: f64) -> Option<SerializedValue> {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    perry_runtime::exception::catch_js_throw(|| {
        clone::try_clone_message(value.get_nanbox_f64(), js_undefined())
    })
    .ok()
    .and_then(Result::ok)
}

fn cloned_error(thrown: f64) -> SerializedValue {
    let scope = RuntimeHandleScope::new();
    let thrown = scope.root_nanbox_f64(thrown);
    if let Some(message) = cloned_diagnostic(thrown.get_nanbox_f64()) {
        return message;
    }
    let family = error_family(thrown.get_nanbox_f64());
    let Some(name) = family else {
        return plain_error_message(b"the worker threw a value that could not be cloned").value;
    };
    let get = |key: &str| {
        perry_runtime::exception::catch_js_throw(|| unsafe {
            perry_runtime::value::js_dynamic_object_get_property(
                thrown.get_nanbox_f64(),
                key.as_ptr().cast(),
                key.len(),
            )
        })
        .unwrap_or_else(|_| js_undefined())
    };
    // Each read can collect; keep only owned bytes between reads.
    let message = diagnostic_string(get("message"));
    let stack = diagnostic_string(get("stack"));
    let cause = get("cause");
    let cause = if is_undefined(cause) {
        None
    } else {
        cloned_diagnostic(cause).map(Box::new)
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

pub(super) fn plain_error_message(message: &[u8]) -> WorkerErrorMessage {
    WorkerErrorMessage {
        properties: Properties::default(),
        value: SerializedValue::Error {
            name: b"Error".to_vec(),
            message: Some(message.to_vec()),
            stack: None,
            cause: None,
        },
    }
}

/// Worker exceptions preserve their code; ordinary postMessage structured
/// cloning deliberately does not copy custom Error properties.
pub(super) struct WorkerErrorMessage {
    pub(super) value: SerializedValue,
    properties: Properties,
}

pub(super) fn worker_error_message(thrown: f64) -> WorkerErrorMessage {
    let scope = RuntimeHandleScope::new();
    let thrown = scope.root_nanbox_f64(thrown);
    perry_runtime::exception::catch_js_throw(|| {
        properties::project(&thrown, &scope, &mut Vec::new())
    })
    .ok()
    .and_then(Result::ok)
    .unwrap_or_else(|| WorkerErrorMessage {
        value: cloned_error(thrown.get_nanbox_f64()),
        properties: Properties::default(),
    })
}

pub(super) fn run_entry(entry: WorkerEntry, is_eval: bool) {
    if !is_eval {
        entry();
        return;
    }
    if let Err(caught) = perry_runtime::exception::run_worker_body_caught(|| entry()) {
        let scope = RuntimeHandleScope::new();
        let caught = caught.root(&scope);
        // Eval's VM display-errors phase observes stack before diagnostics;
        // a failing formatter must not replace the original thrown value.
        let _ = perry_runtime::exception::catch_js_throw(|| unsafe {
            perry_runtime::value::js_dynamic_object_get_property(
                caught.value(),
                b"stack".as_ptr().cast(),
                5,
            )
        });
        perry_runtime::exception::rethrow_caught(caught.caught());
    }
}

impl WorkerErrorMessage {
    pub(super) fn deserialize(&self) -> f64 {
        let scope = RuntimeHandleScope::new();
        let error = scope.root_nanbox_f64(f64::from_bits(unsafe {
            deserialize_nanbox_on_current_thread(&self.value)
        }));
        self.properties.restore(error.get_nanbox_f64());
        error.get_nanbox_f64()
    }
}
