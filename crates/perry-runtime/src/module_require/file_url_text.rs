//! Module path identities and the common file-URL text encoder.
use crate::object::url_layout::is_url_object_shape;
use crate::value::boxed_text::*;

pub(crate) fn is_js_string_value(value: f64) -> bool {
    crate::value::JSValue::from_bits(value.to_bits()).is_any_string()
}

pub(crate) fn string_from_js_value(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *mut crate::StringHeader;
    string_from_header(ptr)
}

pub(crate) fn resolve_path_to_file_url_posix(path: &str) -> String {
    let preserve_trailing_slash = !path.is_empty() && path.ends_with('/');
    let mut resolved = crate::path::resolve_posix_str(path);
    if preserve_trailing_slash && !resolved.ends_with('/') {
        resolved.push('/');
    }
    resolved
}

/// Percent-encode a file-URL path component (after separator normalization),
/// keeping the WHATWG path-safe set plus `/` and `:` (drive-letter colon).
pub(crate) fn encode_file_url_path(path: &str) -> String {
    let mut encoded = String::new();
    for b in path.bytes() {
        match b {
            b'/' | b':' => encoded.push(b as char),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char)
            }
            _ => encoded.push_str(&format!("%{b:02X}")),
        }
    }
    encoded
}

pub(crate) fn path_to_file_url_string(path: &str, windows: bool) -> String {
    if windows {
        // Win32 (#2975). UNC paths (`\\host\share\...`) become
        // `file://host/share/...`; everything else is a (drive-letter) path
        // with `\` separators rewritten to `/`.
        if let Some(unc) = path.strip_prefix("\\\\") {
            // First segment after `\\` is the host; the remainder is the path.
            let (host, rest) = match unc.find('\\') {
                Some(idx) => (&unc[..idx], &unc[idx..]),
                None => (unc, ""),
            };
            let rest_fwd = rest.replace('\\', "/");
            format!("file://{}{}", host, encode_file_url_path(&rest_fwd))
        } else {
            // Node's win32 `pathToFileURL` resolves the input against the
            // cwd first (`path.win32.resolve(filepath)`), then preserves a
            // trailing separator — mirroring the posix arm below. Absolute
            // inputs pass through resolution unchanged (modulo
            // normalization).
            let preserve_trailing_sep = path.ends_with('\\') || path.ends_with('/');
            let mut resolved = crate::path::resolve_win32_str(&path);
            if preserve_trailing_sep && !resolved.ends_with('\\') {
                resolved.push('\\');
            }
            let fwd = resolved.replace('\\', "/");
            let encoded = encode_file_url_path(&fwd);
            if encoded.starts_with('/') {
                format!("file://{}", encoded)
            } else {
                format!("file:///{}", encoded)
            }
        }
    } else {
        let resolved = resolve_path_to_file_url_posix(&path);
        let encoded = encode_file_url_path(&resolved);
        if encoded.starts_with('/') {
            format!("file://{}", encoded)
        } else {
            format!("file:///{}", encoded)
        }
    }
}

/// Resolve a `node:module` "base" argument (file URL object/string or a
/// bare path string) to a filesystem path string. URL-shaped values and
/// `file:`-scheme strings go through the file-URL decoder; any other string
/// is treated as a path and returned verbatim. Used by
/// `module.findPackageJSON` (#3120). Returns `None` for non-string,
/// non-URL-object values so the caller can raise `ERR_INVALID_ARG_TYPE`.
pub(crate) fn module_base_to_path(base_f64: f64) -> Option<String> {
    if is_js_string_value(base_f64) {
        let s = string_from_js_value(base_f64);
        if s.starts_with("file:") {
            return Some(
                String::from_utf8_lossy(&crate::url_hooks::file_url_to_path_bytes(base_f64, false))
                    .into_owned(),
            );
        }
        return Some(s);
    }
    if let Some(obj) = object_from_f64(base_f64) {
        if is_url_object_shape(obj) {
            return Some(
                String::from_utf8_lossy(&crate::url_hooks::file_url_to_path_bytes(base_f64, false))
                    .into_owned(),
            );
        }
    }
    None
}
