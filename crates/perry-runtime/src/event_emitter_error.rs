//! Shared Node EventEmitter unhandled-error construction.
use crate::string::{js_string_from_bytes, StringHeader};

pub(crate) fn read_event_name(event_ptr: *const StringHeader) -> Option<String> {
    if event_ptr.is_null() {
        return None;
    }
    unsafe {
        let len = (*event_ptr).byte_len as usize;
        let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).ok().map(|s| s.to_string())
    }
}

/// Read the `util.inspect` result (a NaN-boxed string value) back into a Rust
/// `String` for embedding in the unhandled-error message.
unsafe fn read_inspected(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    read_event_name(ptr).unwrap_or_default()
}

/// Emit Node's special unhandled-`error`-event semantics (#3052).
///
/// `EventEmitter` (and therefore the `process` global) treats `emit("error", …)`
/// with no registered `error` listener specially: if the first argument is an
/// `Error` instance (or a subclass) it is re-thrown *as-is* — the same object,
/// preserving its `code`, prototype, and identity. Otherwise Node constructs a
/// fresh `Error` with `code: "ERR_UNHANDLED_ERROR"` and a message of
/// `Unhandled error. (<util.inspect(arg)>)`, where a missing argument inspects
/// to `undefined`. This replaces the previous behaviour of throwing the raw
/// first argument (so `emit("error", "boom")` now throws an `ERR_UNHANDLED_ERROR`
/// `Error` rather than the bare `"boom"` string).
pub(crate) fn throw_unhandled_error_event(args: &[f64]) -> ! {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let first = args.first().copied().unwrap_or(undefined);

    // `arg instanceof Error` → rethrow the original value untouched.
    let is_error =
        crate::value::js_is_truthy(crate::object::js_util_types_is_native_error(first)) != 0;
    if is_error {
        crate::exception::js_throw(first);
    }

    // Otherwise: `Unhandled error. (<inspected>)` with ERR_UNHANDLED_ERROR.
    let inspected = unsafe { read_inspected(crate::builtins::js_util_inspect(first, undefined)) };
    let message = format!("Unhandled error. ({inspected})");
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_UNHANDLED_ERROR");
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
}
