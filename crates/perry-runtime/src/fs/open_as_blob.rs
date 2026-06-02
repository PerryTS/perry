//! `fs.openAsBlob(path[, options])`.

use super::*;

#[no_mangle]
pub extern "C" fn js_fs_open_as_blob(path_value: f64, options_value: f64) -> f64 {
    unsafe {
        validate::validate_object_options("options", options_value);
        let content_type = open_as_blob_content_type(options_value);

        validate::validate_path("path", path_value);
        let path = match decode_path_value(path_value) {
            Some(path) => path,
            None => validate::throw_invalid_path_arg("path", path_value),
        };
        let metadata = fs::metadata(&path).unwrap_or_else(|_| throw_open_as_blob_error());
        if metadata.is_file() && fs::File::open(&path).is_err() {
            throw_open_as_blob_error();
        }

        let blob =
            crate::node_submodules::blob::blob_value_from_file_path(&path, &metadata, content_type);
        let promise = crate::promise::js_promise_resolved(blob);
        f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
    }
}

unsafe fn open_as_blob_content_type(options_value: f64) -> String {
    let Some(value) = options_field_value(options_value, b"type") else {
        return String::new();
    };
    let type_value = f64::from_bits(value.bits());
    if let Some(content_type) = crate::node_submodules::blob::string_from_value(type_value) {
        return content_type;
    }
    if crate::value::js_is_truthy(type_value) == 0 {
        return String::new();
    }
    let message = format!(
        "The \"options.type\" property must be of type string. Received {}",
        validate::describe_received(type_value)
    );
    validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn throw_open_as_blob_error() -> ! {
    crate::exception::js_throw(validate::build_type_error_with_code_value(
        "Unable to open file as blob",
        "ERR_INVALID_ARG_VALUE",
    ))
}
