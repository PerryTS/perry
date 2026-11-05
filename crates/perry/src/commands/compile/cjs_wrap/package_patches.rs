//! Source rewrites for specific npm packages whose dynamic code generation
//! (`new Function`) or private-binding access the native pipeline cannot run.

use std::path::Path;

pub(super) fn is_depd_index_path(source_path: &Path) -> bool {
    source_path
        .file_name()
        .map(|name| name == "index.js")
        .unwrap_or(false)
        && source_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "depd")
}

pub(super) fn is_function_bind_implementation_path(source_path: &Path) -> bool {
    source_path
        .file_name()
        .map(|name| name == "implementation.js")
        .unwrap_or(false)
        && source_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "function-bind")
}

pub(super) fn is_safer_buffer_path(source_path: &Path) -> bool {
    source_path
        .file_name()
        .map(|name| name == "safer.js")
        .unwrap_or(false)
        && source_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "safer-buffer")
}

pub(super) fn is_safe_buffer_path(source_path: &Path) -> bool {
    source_path
        .file_name()
        .map(|name| name == "index.js")
        .unwrap_or(false)
        && source_path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "safe-buffer")
}

pub(super) fn rewrite_depd_dynamic_wrapper(source: &str) -> Option<String> {
    let needle = r#"  // eslint-disable-next-line no-new-func
  var deprecatedfn = new Function('fn', 'log', 'deprecate', 'message', 'site',
    '"use strict"\n' +
    'return function (' + args + ') {' +
    'log.call(deprecate, message, site)\n' +
    'return fn.apply(this, arguments)\n' +
    '}')(fn, log, this, message, site)"#;

    let replacement = r#"  var deprecatedfn = (function (fn, log, deprecate, message, site) {
    "use strict"
    return function () {
      log.call(deprecate, message, site)
      return fn.apply(this, arguments)
    }
  })(fn, log, this, message, site)"#;

    if source.contains(needle) {
        Some(source.replace(needle, replacement))
    } else {
        None
    }
}

pub(super) fn rewrite_function_bind_dynamic_wrapper(source: &str) -> Option<String> {
    let needle = r#"    bound = Function('binder', 'return function (' + joiny(boundArgs, ',') + '){ return binder.apply(this,arguments); }')(binder);"#;
    let replacement = r#"    bound = function () {
        return binder.apply(this, arguments);
    };"#;

    if source.contains(needle) {
        Some(source.replace(needle, replacement))
    } else {
        None
    }
}

pub(super) fn rewrite_safer_buffer_private_binding(source: &str) -> Option<String> {
    let needle = r#"if (!safer.kStringMaxLength) {
  try {
    safer.kStringMaxLength = process.binding('buffer').kStringMaxLength
  } catch (e) {
    // we can't determine kStringMaxLength in environments where process.binding
    // is unsupported, so let's not set it
  }
}"#;

    let replacement = r#"if (!safer.kStringMaxLength) {
  safer.kStringMaxLength = 536870888
}"#;

    if source.contains(needle) {
        Some(source.replace(needle, replacement))
    } else {
        None
    }
}

pub(super) fn rewrite_safe_buffer_slow_buffer_fallback(source: &str) -> Option<String> {
    let needle = "return buffer.SlowBuffer(size)";
    let replacement = "return Buffer.allocUnsafeSlow(size)";

    if source.contains(needle) {
        Some(source.replace(needle, replacement))
    } else {
        None
    }
}
