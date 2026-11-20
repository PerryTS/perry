use super::js_string_from_bytes;

pub(super) unsafe fn stream_type_error(msg: &str) -> perry_runtime::error::CoreError {
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = perry_runtime::error::CoreError::type_error(s);
    err
}

pub(super) unsafe fn stream_type_error_with_code(
    message: &str,
    code: &'static str,
) -> perry_runtime::error::CoreError {
    let s = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(s, code);
    let err = perry_runtime::error::CoreError::type_error(s);
    err
}

pub(super) unsafe fn stream_range_error_with_code(
    message: &str,
    code: &'static str,
) -> perry_runtime::error::CoreError {
    let s = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(s, code);
    let err = perry_runtime::error::CoreError::range_error(s);
    err
}
