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

fn type_error(message: &str) -> f64 {
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    js_nanbox_pointer(crate::error::js_typeerror_new(message) as i64)
}

/// EvaluateImportCall step 10 (import attributes): validate `import()`'s
/// second argument before any module is loaded. `Err` carries the value the
/// returned promise rejects with: a TypeError for a non-object `options` or
/// `options.with`, or a non-string attribute value, and whatever a getter
/// threw (IfAbruptRejectPromise).
pub(super) fn check_options(options: f64) -> Result<(), f64> {
    if JSValue::from_bits(options.to_bits()).is_undefined() {
        return Ok(());
    }
    if !crate::builtins::arithmetic::js_value_is_object(options) {
        return Err(type_error(
            "The second argument of import() must be an object",
        ));
    }
    let scope = RuntimeHandleScope::new();
    let attributes = scope.root_nanbox_f64(property(options, b"with")?);
    let attributes = attributes.get_nanbox_f64();
    if JSValue::from_bits(attributes.to_bits()).is_undefined() {
        return Ok(());
    }
    if !crate::builtins::arithmetic::js_value_is_object(attributes) {
        return Err(type_error(
            "The 'with' option of import() must be an object",
        ));
    }
    let object =
        JSValue::from_bits(attributes.to_bits()).as_pointer::<crate::object::ObjectHeader>();
    let entries = crate::exception::catch_js_throw(|| crate::object::js_object_entries(object))?;
    let entries = scope.root_raw_mut_ptr(entries);
    let all_strings = entries.with_mut_ptr(|entries: *mut crate::array::ArrayHeader| {
        (0..crate::array::js_array_length(entries)).all(|index| {
            let entry = crate::array::js_array_get_f64(entries, index);
            let entry =
                JSValue::from_bits(entry.to_bits()).as_pointer::<crate::array::ArrayHeader>();
            JSValue::from_bits(crate::array::js_array_get_f64(entry, 1).to_bits()).is_any_string()
        })
    });
    if all_strings {
        Ok(())
    } else {
        Err(type_error(
            "Every import attribute value of import() must be a string",
        ))
    }
}

/// Codegen entry for a statically resolved `import()` site: `0` when the
/// options are valid, else the rejected promise (a raw pointer) the call
/// evaluates to.
#[no_mangle]
pub extern "C" fn js_module_dynamic_import_check_options(options: f64) -> i64 {
    match check_options(options) {
        Ok(()) => 0,
        Err(reason) => {
            let scope = RuntimeHandleScope::new();
            let reason = scope.root_nanbox_f64(reason);
            crate::promise::js_promise_rejected(reason.get_nanbox_f64()) as i64
        }
    }
}

/// Keepalive anchor (same pattern as the dynamic-import fallback anchors).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_MODULE_DYNAMIC_IMPORT_CHECK_OPTIONS: extern "C" fn(f64) -> i64 =
    js_module_dynamic_import_check_options;

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
            crate::url_hooks::url_file_url_to_path(url.get_nanbox_f64(), undefined())
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
            // TOML is parsed only when `bun-cli-utils` is installed (see
            // `bun_compat::cli_utils_hooks`); otherwise this takes the
            // deferred-error path below, as a runtime without it always has.
            "toml" => match crate::bun_compat::toml_parse_result(&source) {
                Some(parsed) => parsed?,
                None => return Ok(None),
            },
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
