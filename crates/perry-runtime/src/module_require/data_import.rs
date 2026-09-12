//! Runtime data-file loaders for import attributes (#10104).

use super::{js_nanbox_pointer, js_string_from_bytes, set_field_rooted, string_value, undefined};
use crate::gc::RuntimeHandleScope;
use crate::value::JSValue;
use std::path::Path;

fn string_bytes(value: f64) -> Option<String> {
    let value = JSValue::from_bits(value.to_bits());
    let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
    // SAFETY: the helper validates the value's string representation.
    unsafe { crate::string::js_string_key_bytes(value, &mut sso) }
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

fn property(value: f64, key: &[u8]) -> Result<f64, f64> {
    // Catch accessors here so errors reject import() and the caller's root
    // scopes are dropped normally, even with the longjmp exception transport.
    crate::exception::catch_js_throw(|| unsafe {
        crate::value::js_get_property(value, key.as_ptr() as i64, key.len() as i64)
    })
}

fn io_error(path: &str, error: std::io::Error) -> f64 {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        _ => "EIO",
    };
    let message = format!("{code}: cannot import '{path}': {error}");
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(message, code);
    js_nanbox_pointer(crate::error::js_error_new_with_message(message) as i64)
}

/// `None` leaves builtin/code-module resolution to the existing fallback.
pub(super) fn load(specifier: &str, options: f64) -> Result<Option<f64>, f64> {
    if JSValue::from_bits(options.to_bits()).is_undefined() {
        return Ok(None);
    }
    let scope = RuntimeHandleScope::new();
    let attributes = scope.root_nanbox_f64(property(options, b"with")?);
    if JSValue::from_bits(attributes.get_nanbox_f64().to_bits()).is_undefined() {
        return Ok(None);
    }
    let loader = string_bytes(property(attributes.get_nanbox_f64(), b"type")?);
    let Some(loader @ ("toml" | "json" | "text" | "file")) = loader.as_deref() else {
        return Ok(None);
    };
    let path = if specifier.starts_with("file://") {
        let url = scope.root_nanbox_f64(string_value(specifier));
        let decoded = crate::exception::catch_js_throw(|| {
            crate::url::js_url_file_url_to_path(url.get_nanbox_f64(), undefined())
        })?;
        string_bytes(decoded).expect("fileURLToPath returns a string")
    } else if Path::new(specifier).is_absolute() {
        specifier.to_owned()
    } else {
        return Ok(None);
    };

    let value = if loader == "file" {
        let metadata = std::fs::metadata(&path).map_err(|error| io_error(&path, error))?;
        if metadata.is_dir() {
            return Err(io_error(&path, std::io::ErrorKind::IsADirectory.into()));
        }
        string_value(&path)
    } else {
        let bytes = std::fs::read(&path).map_err(|error| io_error(&path, error))?;
        let source = String::from_utf8_lossy(&bytes);
        match loader {
            "text" => string_value(&source),
            "json" => {
                let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
                let source = js_string_from_bytes(source.as_ptr(), source.len() as u32);
                // SAFETY: source is a live runtime string; parse_result returns
                // a SyntaxError value instead of throwing on invalid JSON.
                unsafe { crate::json::js_json_parse_result(source) }
                    .map(|value| f64::from_bits(value.bits()))?
            }
            #[cfg(feature = "bun-cli-utils")]
            "toml" => crate::bun_compat::toml_parse_result(&source)?,
            // Optimized builds retain bun-cli-utils for sites with options.
            // A deliberately minimal runtime still uses the deferred error.
            _ => return Ok(None),
        }
    };
    let value = scope.root_nanbox_f64(value);
    let namespace = scope.root_raw_mut_ptr(crate::object::js_object_alloc_null_proto(0, 1));
    set_field_rooted(&namespace, "default", value.get_nanbox_f64());
    Ok(Some(namespace.with_mut_ptr(
        |object: *mut crate::object::ObjectHeader| js_nanbox_pointer(object as i64),
    )))
}

#[cfg(test)]
mod tests;
