//! Node.js URL-module compatibility helpers — `fileURLToPath`,
//! `pathToFileURL`, `domainToASCII`, `urlToHttpOptions`, legacy
//! `url.format` / `url.parse` / `url.resolve`.

use super::*;

use super::parse::{create_url_object, is_valid_absolute_url, parse_url, resolve_url};
use super::search_params::url_decode;

const QUERYSTRING_ESCAPE_HEX: &[u8; 16] = b"0123456789ABCDEF";

fn legacy_querystring_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(QUERYSTRING_ESCAPE_HEX[(b >> 4) as usize] as char);
                out.push(QUERYSTRING_ESCAPE_HEX[(b & 0x0F) as usize] as char);
            }
        }
    }
    out
}

fn throw_url_format_invalid_arg() -> ! {
    let msg = b"The \"urlObject\" argument must be of type object or string.";
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_url_type_error_with_code(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn is_js_string_value(value: f64) -> bool {
    crate::value::JSValue::from_bits(value.to_bits()).is_any_string()
}

fn string_from_js_value(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *mut crate::StringHeader;
    string_from_header(ptr)
}

fn url_received(value: f64) -> String {
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return "an instance of Buffer".to_string();
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        let ptr = unsafe { crate::symbol::js_symbol_to_string(value) } as *const StringHeader;
        return format!(
            "type symbol ({})",
            string_from_header(ptr as *mut StringHeader)
        );
    }
    crate::fs::validate::describe_received(value)
}

fn throw_invalid_url_arg(value: f64, url_instance_allowed: bool) -> ! {
    let expected = if url_instance_allowed {
        "string or an instance of URL"
    } else {
        "string"
    };
    let message = format!(
        "The \"path\" argument must be of type {expected}. Received {}",
        url_received(value)
    );
    throw_url_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_legacy_url_arg(value: f64) -> ! {
    let message = format!(
        "The \"url\" argument must be of type string. Received {}",
        url_received(value)
    );
    throw_url_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn resolve_path_to_file_url_posix(path: &str) -> String {
    let preserve_trailing_slash = !path.is_empty() && path.ends_with('/');
    let mut resolved = crate::path::resolve_posix_str(path);
    if preserve_trailing_slash && !resolved.ends_with('/') {
        resolved.push('/');
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::resolve_path_to_file_url_posix;

    fn cwd() -> String {
        std::env::current_dir()
            .expect("current dir")
            .to_string_lossy()
            .trim_end_matches('/')
            .to_string()
    }

    /// Unix-only: the `../` case needs a `/`-separated cwd for the posix
    /// resolver to pop a segment — on a Windows host the backslashed cwd is
    /// a single opaque segment to the pinned posix machinery.
    #[cfg(not(windows))]
    #[test]
    fn path_to_file_url_posix_preserves_relative_trailing_slash() {
        let cwd = cwd();
        let parent = std::env::current_dir()
            .expect("current dir")
            .parent()
            .unwrap_or_else(|| std::path::Path::new("/"))
            .to_string_lossy()
            .trim_end_matches('/')
            .to_string();

        assert_eq!(
            resolve_path_to_file_url_posix("relative/"),
            format!("{cwd}/relative/")
        );
        assert_eq!(
            resolve_path_to_file_url_posix("relative//"),
            format!("{cwd}/relative/")
        );
        assert_eq!(resolve_path_to_file_url_posix("./"), format!("{cwd}/"));
        assert_eq!(resolve_path_to_file_url_posix("../"), format!("{parent}/"));
    }

    #[test]
    fn path_to_file_url_posix_does_not_add_slash_without_input_slash() {
        let cwd = cwd();

        assert_eq!(
            resolve_path_to_file_url_posix("relative/."),
            format!("{cwd}/relative")
        );
        assert_eq!(resolve_path_to_file_url_posix(""), cwd);
    }

    fn str_f64(text: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }

    #[test]
    fn windows_flag_defaults_to_platform() {
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        assert_eq!(super::options_windows_flag(undefined), cfg!(windows));
        // Non-object options behave like a missing options argument.
        assert_eq!(super::options_windows_flag(3.0), cfg!(windows));
    }

    #[test]
    fn explicit_windows_option_wins_over_platform() {
        let obj = crate::object::js_object_alloc(0, 1);
        let key = crate::string::js_string_from_bytes("windows".as_ptr(), 7);
        let obj_f64 = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());

        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(crate::value::TAG_FALSE),
        );
        assert!(!super::options_windows_flag(obj_f64));

        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(crate::value::TAG_TRUE),
        );
        assert!(super::options_windows_flag(obj_f64));

        // `{ windows: undefined }` / `{ windows: null }` fall back to the
        // platform, matching Node's `options?.windows ?? isWindows`.
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(super::options_windows_flag(obj_f64), cfg!(windows));
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(crate::value::TAG_NULL),
        );
        assert_eq!(super::options_windows_flag(obj_f64), cfg!(windows));
    }

    #[cfg(windows)]
    #[test]
    fn file_url_conversions_default_to_win32_on_windows() {
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

        let path = super::js_url_file_url_to_path(str_f64("file:///C:/tmp/x.txt"), undefined);
        assert_eq!(super::string_from_js_value(path), "C:\\tmp\\x.txt");

        let out = super::js_url_path_to_file_url(str_f64("C:\\tmp\\x.txt"), undefined);
        let obj = crate::url::object_from_f64(out).expect("URL object");
        assert_eq!(
            crate::url::object_prop_string(obj, "href"),
            "file:///C:/tmp/x.txt"
        );

        // Relative inputs resolve against the cwd (Node uses
        // `path.win32.resolve` before building the URL).
        let out = super::js_url_path_to_file_url(str_f64("x.txt"), undefined);
        let obj = crate::url::object_from_f64(out).expect("URL object");
        let href = crate::url::object_prop_string(obj, "href");
        let drive = std::env::current_dir().expect("cwd").to_string_lossy()[..2].to_string();
        assert!(
            href.starts_with(&format!("file:///{drive}/")),
            "href {href:?} does not start with the cwd drive"
        );
        assert!(href.ends_with("/x.txt"), "href {href:?}");
    }

    #[cfg(not(windows))]
    #[test]
    fn file_url_conversions_default_to_posix_elsewhere() {
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

        let path = super::js_url_file_url_to_path(str_f64("file:///tmp/x.txt"), undefined);
        assert_eq!(super::string_from_js_value(path), "/tmp/x.txt");
    }
}

/// Read the `windows` option from a `{ windows }` options argument (#2975).
/// Node's `fileURLToPath`/`pathToFileURL` default this to the PLATFORM
/// (`options?.windows ?? isWindows`): a missing options argument, a missing
/// `windows` property, or an explicit `null`/`undefined` all fall back to
/// `cfg!(windows)`; any other value wins by truthiness (`{ windows: false }`
/// forces POSIX even on Windows, `{ windows: 1 }` forces win32 anywhere).
fn options_windows_flag(options: f64) -> bool {
    match object_from_f64(options) {
        Some(opts) => {
            let windows = object_prop_f64(opts, "windows");
            let jv = crate::value::JSValue::from_bits(windows.to_bits());
            if jv.is_undefined() || jv.is_null() {
                cfg!(windows)
            } else {
                crate::value::js_is_truthy(windows) != 0
            }
        }
        None => cfg!(windows),
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-decode a POSIX file-URL pathname to its raw byte sequence. Bytes
/// are returned verbatim (no UTF-8 validation) so `fileURLToPathBuffer` can
/// preserve paths whose decoded bytes are not valid UTF-8. Mirrors Node's
/// `getPathFromURLPosix`: an encoded `/` (`%2f`/`%2F`) is rejected, but an
/// encoded `\` is decoded through as an ordinary byte.
fn decode_file_url_pathname_bytes_posix(pathname: &str) -> Vec<u8> {
    let bytes = pathname.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if bytes[i + 1] == b'2' && (bytes[i + 2] | 0x20) == b'f' {
                throw_url_type_error_with_code(
                    "File URL path must not include encoded / characters",
                    "ERR_INVALID_FILE_URL_PATH",
                );
            }
            if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Percent-decode a Windows file-URL pathname to raw bytes, converting `/`
/// separators to `\`. Mirrors Node's `getPathFromURLWin32`: encoded `/`
/// (`%2f`) AND encoded `\` (`%5c`) are both rejected; the rest decodes through.
fn decode_file_url_pathname_bytes_win32(pathname: &str) -> Vec<u8> {
    let bytes = pathname.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = bytes[i + 1];
            let h2 = bytes[i + 2] | 0x20; // lowercase the second hex digit
            if (h1 == b'2' && h2 == b'f') || (h1 == b'5' && h2 == b'c') {
                throw_url_type_error_with_code(
                    "File URL path must not include encoded \\ or / characters",
                    "ERR_INVALID_FILE_URL_PATH",
                );
            }
            if let (Some(hi), Some(lo)) = (hex_nibble(h1), hex_nibble(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        // Convert forward slashes to backslashes as Node does pre-decode.
        out.push(if bytes[i] == b'/' { b'\\' } else { bytes[i] });
        i += 1;
    }
    out
}

/// Shared `file:` URL → path parsing for both `fileURLToPath` (UTF-8 string)
/// and `fileURLToPathBuffer` (raw bytes). Returns the decoded path bytes,
/// throwing the same scheme/host/encoded-slash errors as Node. When `windows`
/// is true, applies Node's Win32 conversion (UNC hosts, drive-letter
/// validation, `/`→`\`, encoded-`\` rejection) instead of the POSIX path
/// (which still rejects non-empty/non-localhost hosts on darwin). #2975
fn file_url_to_path_bytes(url_f64: f64, windows: bool) -> Vec<u8> {
    let url_string = if is_js_string_value(url_f64) {
        string_from_js_value(url_f64)
    } else if let Some(obj) = object_from_f64(url_f64) {
        if !is_url_object_shape(obj) {
            throw_invalid_url_arg(url_f64, true);
        }
        object_prop_string(obj, "href")
    } else {
        throw_invalid_url_arg(url_f64, true);
    };

    let Some(after_scheme) = url_string.strip_prefix("file:") else {
        throw_url_type_error_with_code("The URL must be of scheme file", "ERR_INVALID_URL_SCHEME");
    };

    let (host, pathname) = if let Some(authority_and_path) = after_scheme.strip_prefix("//") {
        let path_start = authority_and_path
            .find('/')
            .unwrap_or(authority_and_path.len());
        (
            &authority_and_path[..path_start],
            &authority_and_path[path_start..],
        )
    } else {
        ("", after_scheme)
    };
    let pathname = pathname.split(['?', '#']).next().unwrap_or_default();

    if windows {
        // Win32: a non-empty/non-localhost host is a UNC share `\\host\path`.
        let mut decoded = decode_file_url_pathname_bytes_win32(pathname);
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            let mut out = b"\\\\".to_vec();
            out.extend_from_slice(host.as_bytes());
            out.extend_from_slice(&decoded);
            return out;
        }
        // No host: pathname must be `/<drive-letter>:...`. Node validates the
        // decoded form: position 1 is an ASCII letter, position 2 is `:`.
        let letter = decoded.get(1).copied().unwrap_or(0) | 0x20;
        let sep = decoded.get(2).copied().unwrap_or(0);
        if !letter.is_ascii_lowercase() || sep != b':' {
            throw_url_type_error_with_code(
                "File URL path must be absolute",
                "ERR_INVALID_FILE_URL_PATH",
            );
        }
        // Strip the leading `\` (was `/`) so `\C:\x` → `C:\x`.
        decoded.remove(0);
        decoded
    } else {
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            throw_url_type_error_with_code(
                "File URL host must be \"localhost\" or empty on darwin",
                "ERR_INVALID_FILE_URL_HOST",
            );
        }
        decode_file_url_pathname_bytes_posix(pathname)
    }
}

/// POSIX file URL decoder for fs PathLike URL objects. This intentionally
/// routes through the same parser as `fileURLToPath()`, so encoded separators
/// and host/scheme errors surface with Node-compatible codes.
pub(crate) fn file_url_to_path_string_posix(url_f64: f64) -> String {
    String::from_utf8_lossy(&file_url_to_path_bytes(url_f64, false)).into_owned()
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
                String::from_utf8_lossy(&file_url_to_path_bytes(base_f64, false)).into_owned(),
            );
        }
        return Some(s);
    }
    if let Some(obj) = object_from_f64(base_f64) {
        if is_url_object_shape(obj) {
            return Some(
                String::from_utf8_lossy(&file_url_to_path_bytes(base_f64, false)).into_owned(),
            );
        }
    }
    None
}

/// Convert a file:// URL to a filesystem path
/// Strips the "file://" prefix and percent-decodes the result
/// js_url_file_url_to_path(url_f64: f64, options_f64: f64) -> f64 (NaN-boxed string)
#[no_mangle]
pub extern "C" fn js_url_file_url_to_path(url_f64: f64, options_f64: f64) -> f64 {
    let windows = options_windows_flag(options_f64);
    let decoded = String::from_utf8_lossy(&file_url_to_path_bytes(url_f64, windows)).into_owned();
    create_string_f64(&decoded)
}

/// `url.fileURLToPathBuffer(url[, options])` (#2541) — the Buffer-returning
/// counterpart to `fileURLToPath`. Returns the decoded path's raw bytes as a
/// `Buffer`, preserving percent-encoded sequences that are not valid UTF-8
/// (where the string form would lossily substitute U+FFFD). Same scheme/host
/// validation as `fileURLToPath`.
/// js_url_file_url_to_path_buffer(url_f64: f64, options_f64: f64) -> f64 (NaN-boxed Buffer ptr)
#[no_mangle]
pub extern "C" fn js_url_file_url_to_path_buffer(url_f64: f64, options_f64: f64) -> f64 {
    let windows = options_windows_flag(options_f64);
    let bytes = file_url_to_path_bytes(url_f64, windows);
    let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
    unsafe {
        (*buf).length = bytes.len() as u32;
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::buffer::buffer_data_mut(buf),
                bytes.len(),
            );
        }
    }
    crate::value::js_nanbox_pointer(buf as i64)
}

/// Percent-encode a file-URL path component (after separator normalization),
/// keeping the WHATWG path-safe set plus `/` and `:` (drive-letter colon).
fn encode_file_url_path(path: &str) -> String {
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

#[no_mangle]
pub extern "C" fn js_url_path_to_file_url(path_f64: f64, options_f64: f64) -> f64 {
    if !is_js_string_value(path_f64) {
        throw_invalid_url_arg(path_f64, false);
    }
    let path = get_string_content(path_f64);
    let windows = options_windows_flag(options_f64);

    let href = if windows {
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
    };
    let obj = create_url_object(&href);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `url.domainToASCII(domain)` (#3059). Node Web-IDL-stringifies the argument
/// (`String(domain)`; a Symbol throws TypeError) and runs the full WHATWG host
/// parser, so a numeric / IPv4-shorthand domain canonicalizes to a dotted-quad
/// IPv4 address (`123` → `"0.0.0.123"`, `0x7f.1` → `"127.0.0.1"`) rather than
/// being treated as a literal label. Unparsable hosts yield `""`.
#[no_mangle]
pub extern "C" fn js_url_domain_to_ascii(input_f64: f64) -> f64 {
    let input = string_from_header(js_url_coerce_string(input_f64));
    if input.chars().any(|c| c.is_ascii_whitespace()) || domain_has_forbidden_char(&input) {
        return create_string_f64("");
    }
    // `whatwg_canonicalize_host` runs IDNA *and* the WHATWG numeric/IPv4 host
    // parser, matching Node's `domainToASCII` exactly (IDN → punycode, numeric
    // → IPv4, invalid → None → ""). It supersedes the bare `idna::domain_to_ascii`.
    let out = whatwg_canonicalize_host(&input).unwrap_or_default();
    create_string_f64(&out)
}

/// `url.domainToUnicode(domain)` (#3059). Mirrors `domainToASCII`'s coercion
/// and WHATWG host parsing, but returns the Unicode IDN form. For numeric /
/// IPv4-shorthand hosts Node returns the canonical IPv4 address (`123` →
/// `"0.0.0.123"`); for registrable hostnames it returns the decoded Unicode
/// (`xn--mnchen-3ya.de` → `münchen.de`); invalid hosts yield `""`.
#[no_mangle]
pub extern "C" fn js_url_domain_to_unicode(input_f64: f64) -> f64 {
    let input = string_from_header(js_url_coerce_string(input_f64));
    if input.chars().any(|c| c.is_ascii_whitespace()) || domain_has_forbidden_char(&input) {
        return create_string_f64("");
    }
    let out = match whatwg_canonicalize_host(&input) {
        // Out-of-range / unparsable host → "" (matches Node).
        None => String::new(),
        // Numeric / IPv4-shorthand → canonical IPv4 address (Node yields the IP).
        Some(canon) if is_ipv4_host(&canon) => canon,
        // Registrable hostname → Unicode IDN form. IDNA must run on the
        // CANONICALIZED host, not the raw input: `/`, `?`, `#` and `\` terminate
        // the host, so `domainToUnicode("a/b")` is `"a"`. Feeding the raw input to
        // `domain_to_unicode` skipped that truncation and echoed `"a/b"` back.
        #[cfg(feature = "url-engine")]
        Some(canon) => idna::domain_to_unicode(&canon).0,
        // URL engine gated off: no IDNA, so return the canonical host unchanged.
        #[cfg(not(feature = "url-engine"))]
        Some(canon) => canon,
    };
    create_string_f64(&out)
}

/// WHATWG forbidden host code points that make `domainToASCII` /
/// `domainToUnicode` reject the input outright (Node returns `""`). Note `/`,
/// `?`, `#` and `\` are NOT here — those merely TERMINATE the host, so the prefix
/// is returned. A bracketed IPv6 literal legitimately contains `:`/`[`/`]` and is
/// exempt. Without this, `whatwg_canonicalize_host("a@b")` treated `a@` as
/// userinfo and yielded `"b"` where Node yields `""`.
fn domain_has_forbidden_char(input: &str) -> bool {
    if legacy_is_ipv6_hostname(input) {
        return false;
    }
    input.contains('@')
}

fn json_to_value(json: serde_json::Value) -> f64 {
    let s = json.to_string();
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    unsafe { f64::from_bits(crate::json::js_json_parse(ptr).bits()) }
}

fn null_f64() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

fn bool_f64(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// `url.urlToHttpOptions(url)` (#2976). Mirrors Node's shape exactly:
///
/// ```js
/// const options = {
///   __proto__: null,
///   ...url,                 // copy user-added enumerable own props first
///   protocol, hostname, hash, search, pathname,
///   path: `${pathname}${search}`, href,
/// };
/// if (port !== '') options.port = Number(port);
/// if (username || password) options.auth = `${decode(username)}:${decode(password)}`;
/// ```
///
/// Node throws `ERR_INVALID_ARG_TYPE` for non-object input rather than
/// returning an empty object. `auth` is percent-decoded; `port` is numeric.
#[no_mangle]
pub extern "C" fn js_url_to_http_options(url_f64: f64) -> f64 {
    let Some(obj) = object_from_f64(url_f64) else {
        // Node: `if (url == null || typeof url !== 'object')` → ERR_INVALID_ARG_TYPE.
        let message = format!(
            "The \"url\" argument must be of type object. Received {}",
            url_received(url_f64)
        );
        throw_url_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };

    // Standard URL component names. In Node these live on the URL prototype as
    // getters, so the `...url` spread copies only *user-added* own props. Perry
    // stores them as own fields, so we must skip them when replicating the
    // spread — otherwise we'd duplicate them (out of order) ahead of the fixed set.
    const URL_OWN: &[&str] = &[
        "href",
        "protocol",
        "host",
        "hostname",
        "port",
        "pathname",
        "search",
        "hash",
        "origin",
        "searchParams",
        "username",
        "password",
    ];

    let protocol = object_prop_string(obj, "protocol");
    let hostname = object_prop_string(obj, "hostname");
    let hash = object_prop_string(obj, "hash");
    let search = object_prop_string(obj, "search");
    let pathname = object_prop_string(obj, "pathname");
    let port_s = object_prop_string(obj, "port");
    let href = object_prop_string(obj, "href");
    let username = object_prop_string(obj, "username");
    let password = object_prop_string(obj, "password");
    let path = format!("{}{}", pathname, search);

    let obj_out = js_object_alloc(0, 0);

    // 1) Copy user-added enumerable own props (`...url`) first, in insertion
    //    order, skipping the standard URL component names.
    let keys = crate::object::js_object_keys(obj as *const ObjectHeader);
    let len = unsafe { (*keys).length };
    for i in 0..len {
        let key_f = crate::array::js_array_get_f64(keys, i);
        let key = get_string_content(key_f);
        if URL_OWN.contains(&key.as_str()) {
            continue;
        }
        let val = object_prop_f64(obj, &key);
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        crate::object::js_object_set_field_by_name(obj_out, key_ptr, val);
    }

    // 2) Fixed fields, in Node's order.
    set_named(obj_out, "protocol", create_string_f64(&protocol));
    set_named(obj_out, "hostname", create_string_f64(&hostname));
    set_named(obj_out, "hash", create_string_f64(&hash));
    set_named(obj_out, "search", create_string_f64(&search));
    set_named(obj_out, "pathname", create_string_f64(&pathname));
    set_named(obj_out, "path", create_string_f64(&path));
    set_named(obj_out, "href", create_string_f64(&href));

    // 3) Optional numeric `port` when non-empty.
    if !port_s.is_empty() {
        if let Ok(p) = port_s.parse::<u32>() {
            set_named(obj_out, "port", p as f64);
        }
    }

    // 4) Optional decoded `auth` when userinfo present. Node uses
    //    `decodeURIComponent` on each half (`u%20ser` → `u ser`, `p%40w` → `p@w`).
    if !username.is_empty() || !password.is_empty() {
        let auth = format!("{}:{}", url_decode(&username), url_decode(&password));
        set_named(obj_out, "auth", create_string_f64(&auth));
    }

    crate::value::js_nanbox_pointer(obj_out as i64)
}

/// Set an own property by name on a dynamically-grown object (no fixed key
/// array). Used by `urlToHttpOptions` where the field set is variable.
fn set_named(obj: *mut ObjectHeader, key: &str, value: f64) {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key_ptr, value);
}

fn legacy_format_from_object(obj: *mut ObjectHeader) -> String {
    let protocol = object_prop_string(obj, "protocol");
    let hostname = object_prop_string(obj, "hostname");
    let host = object_prop_string(obj, "host");
    let port = object_prop_string(obj, "port");
    let pathname = object_prop_string(obj, "pathname");
    let search = object_prop_string(obj, "search");
    let hash = object_prop_string(obj, "hash");
    let auth = object_prop_string(obj, "auth");
    // Legacy `format()` only emits `//` when `slashes` is truthy OR when the
    // protocol is one of the slash-bearing built-ins (http/https/ws/wss/ftp).
    let slashes_val = object_prop_f64(obj, "slashes");
    let slashes_explicit = slashes_val.to_bits() == 0x7FFC_0000_0000_0004u64;
    let proto_wants_slashes = matches!(
        protocol.trim_end_matches(':'),
        "http" | "https" | "ws" | "wss" | "ftp" | "file"
    );
    // Legacy `url.format()`: hierarchical schemes always get `//` regardless
    // of the `slashes` flag (Node ignores `slashes:false` for http/https/etc.).
    let use_slashes = slashes_explicit || proto_wants_slashes;
    let mut out = String::new();
    if !protocol.is_empty() {
        out.push_str(&protocol);
        if !protocol.ends_with(':') {
            out.push(':');
        }
    }
    let authority = if !host.is_empty() {
        host
    } else if !hostname.is_empty() && !port.is_empty() {
        format!("{hostname}:{port}")
    } else {
        hostname
    };
    if !authority.is_empty() {
        if use_slashes {
            out.push_str("//");
        }
        if !auth.is_empty() {
            out.push_str(&auth);
            out.push('@');
        }
        out.push_str(&authority);
    }
    out.push_str(&pathname);
    if !search.is_empty() {
        out.push_str(&search);
    } else {
        let query = object_prop_f64(obj, "query");
        if let Some(qobj) = object_from_f64(query) {
            let keys = crate::object::js_object_keys(qobj as *const ObjectHeader);
            let len = unsafe { (*keys).length };
            let mut parts = Vec::new();
            for i in 0..len {
                let key_f = crate::array::js_array_get_f64(keys, i);
                let key = get_string_content(key_f);
                let val_key = js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let val = crate::object::js_object_get_field_by_name_f64(qobj, val_key);
                parts.push(format!(
                    "{}={}",
                    legacy_querystring_escape(&key),
                    legacy_querystring_escape(&get_string_content(val))
                ));
            }
            if !parts.is_empty() {
                out.push('?');
                out.push_str(&parts.join("&"));
            }
        } else {
            let q = get_string_content(query);
            if !q.is_empty() {
                out.push('?');
                out.push_str(&q);
            }
        }
    }
    out.push_str(&hash);
    out
}

#[no_mangle]
pub extern "C" fn js_url_format(value: f64, options: f64) -> f64 {
    let Some(obj) = object_from_f64(value) else {
        let js_value = crate::value::JSValue::from_bits(value.to_bits());
        if js_value.is_any_string() {
            let ptr =
                crate::value::js_get_string_pointer_unified(value) as *mut crate::StringHeader;
            // Node's `url.format(str)` is `Url.parse(str).format()` — it NORMALIZES
            // rather than echoing the input, so `format("http://example.com?")` is
            // `"http://example.com/?"` (note the added root path).
            let s = string_from_header(ptr);
            let parsed = legacy_url_parse_impl(&s, false);
            return create_string_f64(&legacy_url_format_impl(&parsed));
        }
        throw_url_format_invalid_arg();
    };
    let href = object_prop_string(obj, "href");
    let mut out = if !href.is_empty() {
        href
    } else {
        legacy_format_from_object(obj)
    };
    if let Some(opts) = object_from_f64(options) {
        let false_bits = 0x7FFC_0000_0000_0003u64;
        if object_prop_f64(opts, "search").to_bits() == false_bits {
            if let Some(idx) = out.find('?') {
                out.truncate(idx);
            }
        }
        if object_prop_f64(opts, "fragment").to_bits() == false_bits {
            if let Some(idx) = out.find('#') {
                out.truncate(idx);
            }
        }
    }
    create_string_f64(&out)
}

const LEGACY_URL_KEYS: [&str; 12] = [
    "protocol", "slashes", "auth", "host", "port", "hostname", "hash", "search", "query",
    "pathname", "path", "href",
];

fn string_or_null(value: String) -> f64 {
    if value.is_empty() {
        null_f64()
    } else {
        create_string_f64(&value)
    }
}

fn create_legacy_url_object(values: [f64; 12]) -> *mut ObjectHeader {
    let obj = js_object_alloc(0, LEGACY_URL_KEYS.len() as u32);
    let mut keys = js_array_alloc(LEGACY_URL_KEYS.len() as u32);
    for (index, key) in LEGACY_URL_KEYS.iter().enumerate() {
        keys = js_array_push_f64(keys, create_string_f64(key));
        js_object_set_field_f64(obj, index as u32, values[index]);
    }
    js_object_set_keys(obj, keys);
    obj
}

#[no_mangle]
pub extern "C" fn js_url_legacy_url_new() -> f64 {
    let obj = create_legacy_url_object([
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
        null_f64(),
    ]);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_url_legacy_parse(
    input: f64,
    parse_query_string: f64,
    slashes_denote_host: f64,
) -> f64 {
    if !is_js_string_value(input) {
        throw_invalid_legacy_url_arg(input);
    }
    let s = get_string_content(input);
    let parse_qs = crate::value::js_is_truthy(parse_query_string) != 0;
    let u = legacy_url_parse_impl(&s, crate::value::js_is_truthy(slashes_denote_host) != 0);
    let href = legacy_url_format_impl(&u);

    // `query` is the raw string by default; with `parseQueryString` it is ALWAYS
    // an object (an empty one when there is no search), and `search` is nulled.
    let (search_value, query_value) = if parse_qs {
        let mut map = serde_json::Map::new();
        if let Some(raw) = u.query.as_deref() {
            for part in raw.split('&').filter(|p| !p.is_empty()) {
                let (k, v) = part.split_once('=').unwrap_or((part, ""));
                map.insert(url_decode(k), serde_json::Value::String(url_decode(v)));
            }
        }
        let q = json_to_value(serde_json::Value::Object(map));
        (opt_string_f64(u.search.clone()), q)
    } else {
        (
            opt_string_f64(u.search.clone()),
            opt_string_f64(u.query.clone()),
        )
    };

    // `path` is pathname + search, and is null only when both are absent.
    let path_value = if u.pathname.is_some() || u.search.is_some() {
        create_string_f64(&format!(
            "{}{}",
            u.pathname.as_deref().unwrap_or(""),
            u.search.as_deref().unwrap_or("")
        ))
    } else {
        null_f64()
    };

    let obj = create_legacy_url_object([
        opt_string_f64(u.protocol.clone()),
        match u.slashes {
            Some(true) => bool_f64(true),
            _ => null_f64(),
        },
        opt_string_f64(u.auth.clone()),
        opt_string_f64(u.host.clone()),
        opt_string_f64(u.port.clone()),
        opt_string_f64(u.hostname.clone()),
        opt_string_f64(u.hash.clone()),
        search_value,
        query_value,
        opt_string_f64(u.pathname.clone()),
        path_value,
        create_string_f64(&href),
    ]);
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `Some("")` is a legitimate value (`file:///a` has `host === ""`), so this is
/// NOT `string_or_null`, which collapses the empty string to null.
fn opt_string_f64(v: Option<String>) -> f64 {
    match v {
        Some(s) => create_string_f64(&s),
        None => null_f64(),
    }
}

fn protocol_null_or_slashes(input: &str, protocol_is_null: bool, host: &str) -> bool {
    protocol_is_null || input.starts_with("//") || input.contains("://") || !host.is_empty()
}

/// WHATWG `URL.join` of `to` onto base `from`, or `None` when `from` isn't a
/// parseable absolute URL. Cfg-paired: the off twin returns `None` (no `url`
/// crate), so the caller falls back to the hand-rolled `resolve_url`.
#[cfg(feature = "url-engine")]
fn legacy_url_join(from: &str, to: &str) -> Option<String> {
    let base = url::Url::parse(from).ok()?;
    Some(
        base.join(to)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| resolve_url(to, from)),
    )
}

#[cfg(not(feature = "url-engine"))]
fn legacy_url_join(_from: &str, _to: &str) -> Option<String> {
    None
}

#[no_mangle]
pub extern "C" fn js_url_legacy_resolve(from: f64, to: f64) -> f64 {
    if !is_js_string_value(from) {
        throw_invalid_legacy_url_arg(from);
    }
    if !is_js_string_value(to) {
        throw_invalid_legacy_url_arg(to);
    }
    let from_s = get_string_content(from);
    let to_s = get_string_content(to);
    let resolved = if to_s.starts_with('/') && !is_valid_absolute_url(&from_s) {
        to_s
    } else if let Some(j) = legacy_url_join(&from_s, &to_s) {
        j
    } else {
        resolve_url(&to_s, &from_s)
    };
    create_string_f64(&resolved)
}

#[no_mangle]
pub extern "C" fn js_url_legacy_resolve_object(from: f64, to: f64) -> f64 {
    let resolved = js_url_legacy_resolve(from, to);
    js_url_legacy_parse(resolved, bool_f64(false), bool_f64(false))
}

// ===========================================================================
// Legacy `url.parse` / `url.format` — a faithful port of Node's `lib/url.js`
// (`Url.prototype.parse` / `Url.prototype.format`). See #6375.
//
// The previous implementation was a hand-rolled approximation layered on the
// generic `parse_url()` helper. It computed the wrong thing structurally rather
// than being a few edge cases off: a plain relative path had its first segment
// stolen as a hostname (`parse("a/b/c").host === "a"`), there were no
// hostless/slashed protocol tables (so `mailto:` grew no auth/host and `file:`
// no empty-string host), IPv6 brackets were never stripped from `hostname`,
// `pathname` was invented as `"/"` for a bare query/hash, and `href` was just
// the input string rather than the result of `format()`.
// ===========================================================================

/// Protocols that never have a hostname.
fn legacy_is_hostless_protocol(p: &str) -> bool {
    matches!(p, "javascript" | "javascript:")
}

/// Protocols that always carry a `//`.
fn legacy_is_slashed_protocol(p: &str) -> bool {
    matches!(
        p,
        "http"
            | "http:"
            | "https"
            | "https:"
            | "ftp"
            | "ftp:"
            | "gopher"
            | "gopher:"
            | "file"
            | "file:"
            | "ws"
            | "ws:"
            | "wss"
            | "wss:"
    )
}

/// Node's `unsafeProtocol` — these skip `autoEscapeStr`.
fn legacy_is_unsafe_protocol(p: &str) -> bool {
    matches!(p, "javascript" | "javascript:")
}

#[derive(Default, Clone)]
pub(crate) struct LegacyUrl {
    pub protocol: Option<String>,
    pub slashes: Option<bool>,
    pub auth: Option<String>,
    pub host: Option<String>,
    pub port: Option<String>,
    pub hostname: Option<String>,
    pub hash: Option<String>,
    pub search: Option<String>,
    pub query: Option<String>,
    pub pathname: Option<String>,
}

/// `protocolPattern = /^[a-z0-9.+-]+:/i`
fn legacy_match_protocol(rest: &str) -> Option<String> {
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphanumeric() || c == '.' || c == '+' || c == '-' {
            i += 1;
        } else {
            break;
        }
    }
    if i > 0 && i < b.len() && b[i] == b':' {
        Some(rest[..=i].to_string())
    } else {
        None
    }
}

/// `hostPattern = /^\/\/[^@/]+@[^@/]+/` — a PREFIX match, not a full match. It
/// only requires one or more non-`@`/`/` chars, an `@`, then at least one more
/// non-`@`/`/` char; anything may follow. (Requiring the tail to be free of `/`
/// wrongly rejected `//user:pass@example.com:8000/foo`, so its authority was
/// never parsed.)
fn legacy_host_pattern(rest: &str) -> bool {
    let Some(after) = rest.strip_prefix("//") else {
        return false;
    };
    let mut user_len = 0usize;
    let mut at: Option<usize> = None;
    for (i, c) in after.char_indices() {
        if c == '@' {
            at = Some(i);
            break;
        }
        if c == '/' {
            return false;
        }
        user_len += 1;
    }
    let Some(at) = at.filter(|_| user_len > 0) else {
        return false;
    };
    matches!(after[at + 1..].chars().next(), Some(c) if c != '@' && c != '/')
}

/// `simplePathPattern = /^(\/\/?(?!\/)[^?\s]*)(\?[^\s]*)?$/`
fn legacy_simple_path(rest: &str) -> Option<(String, Option<String>)> {
    let b = rest.as_bytes();
    if b.is_empty() || b[0] != b'/' {
        return None;
    }
    // `\/\/?(?!\/)` — one or two slashes, never three.
    if b.len() >= 3 && b[1] == b'/' && b[2] == b'/' {
        return None;
    }
    let (p, s) = match rest.find('?') {
        Some(i) => (&rest[..i], Some(rest[i..].to_string())),
        None => (rest, None),
    };
    if p.chars().any(char::is_whitespace) {
        return None;
    }
    if s.as_deref()
        .is_some_and(|v| v.chars().any(char::is_whitespace))
    {
        return None;
    }
    Some((p.to_string(), s))
}

/// Node's `autoEscapeStr` — percent-escape the chars that are never allowed to
/// appear literally in the post-host remainder.
fn legacy_auto_escape(rest: &str) -> String {
    let mut out = String::with_capacity(rest.len());
    for c in rest.chars() {
        match c {
            '\t' => out.push_str("%09"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            ' ' => out.push_str("%20"),
            '"' => out.push_str("%22"),
            '\'' => out.push_str("%27"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '\\' => out.push_str("%5C"),
            '^' => out.push_str("%5E"),
            '`' => out.push_str("%60"),
            '{' => out.push_str("%7B"),
            '|' => out.push_str("%7C"),
            '}' => out.push_str("%7D"),
            _ => out.push(c),
        }
    }
    out
}

fn legacy_is_ipv6_hostname(h: &str) -> bool {
    let b = h.as_bytes();
    b.len() >= 2 && b[0] == b'[' && b[b.len() - 1] == b']'
}

/// Node's `parseHost()`: split a trailing `:port` off `host`.
fn legacy_split_port(host: &str) -> (String, Option<String>) {
    // `portPattern = /:[0-9]*$/`
    if let Some(idx) = host.rfind(':') {
        let tail = &host[idx + 1..];
        if tail.chars().all(|c| c.is_ascii_digit()) {
            // A `:` inside an unbracketed IPv6 literal is not a port separator,
            // but Node's regex is applied to `host` as-is; `[::1]:8080` has its
            // colon after the `]`, so this is safe.
            if !(legacy_is_ipv6_hostname(host)) {
                let port = if tail.is_empty() {
                    None
                } else {
                    Some(tail.to_string())
                };
                return (host[..idx].to_string(), port);
            }
        }
    }
    (host.to_string(), None)
}

/// Node's `Url.prototype.parse`.
pub(crate) fn legacy_url_parse_impl(url: &str, slashes_denote_host: bool) -> LegacyUrl {
    let mut u = LegacyUrl::default();
    let chars: Vec<char> = url.chars().collect();

    // One pass: trim surrounding whitespace, fold backslashes to forward slashes
    // (only before the first `?`/`#`), and note whether an `@` / `#` appears.
    let (mut has_hash, mut has_at) = (false, false);
    let (mut start, mut end): (Option<usize>, Option<usize>) = (None, None);
    let mut rest = String::new();
    let mut last_pos = 0usize;
    let (mut in_ws, mut split) = (false, false);
    for (i, &c) in chars.iter().enumerate() {
        let code = c as u32;
        let is_ws = code < 33 || code == 0x00A0 || code == 0xFEFF;
        if start.is_none() {
            if is_ws {
                continue;
            }
            last_pos = i;
            start = Some(i);
        } else if in_ws {
            if !is_ws {
                end = None;
                in_ws = false;
            }
        } else if is_ws {
            end = Some(i);
            in_ws = true;
        }
        if !split {
            match c {
                '@' => has_at = true,
                '#' => {
                    has_hash = true;
                    split = true;
                }
                '?' => split = true,
                '\\' => {
                    if i > last_pos {
                        rest.extend(chars[last_pos..i].iter());
                    }
                    rest.push('/');
                    last_pos = i + 1;
                }
                _ => {}
            }
        } else if !has_hash && c == '#' {
            has_hash = true;
        }
    }
    if let Some(s) = start {
        if last_pos == s {
            rest = match end {
                None => chars[s..].iter().collect(),
                Some(e) => chars[s..e].iter().collect(),
            };
        } else {
            match end {
                None if last_pos < chars.len() => rest.extend(chars[last_pos..].iter()),
                Some(e) if last_pos < e => rest.extend(chars[last_pos..e].iter()),
                _ => {}
            }
        }
    }

    // Fast path: a simple path plus an optional query, no `#` and no `@`.
    if !slashes_denote_host && !has_hash && !has_at {
        if let Some((pathname, search)) = legacy_simple_path(&rest) {
            u.pathname = Some(pathname);
            if let Some(s) = search {
                u.query = Some(s[1..].to_string());
                u.search = Some(s);
            }
            return u;
        }
    }

    let proto = legacy_match_protocol(&rest);
    let lower_proto = proto.as_ref().map(|p| p.to_lowercase()).unwrap_or_default();
    if let Some(p) = &proto {
        u.protocol = Some(lower_proto.clone());
        rest = rest[p.len()..].to_string();
    }

    // `user@server` is ALWAYS a hostname, and `//foo/bar` resolves to host=foo —
    // but a bare relative path (`a/b/c`) must NOT have its first segment taken as
    // an authority, which is what the old implementation did.
    let mut slashes = false;
    if slashes_denote_host || proto.is_some() || legacy_host_pattern(&rest) {
        slashes = rest.starts_with("//");
        if slashes && !legacy_is_hostless_protocol(&lower_proto) {
            rest = rest[2..].to_string();
            u.slashes = Some(true);
        }
    }

    if !legacy_is_hostless_protocol(&lower_proto)
        && (slashes || (proto.is_some() && !legacy_is_slashed_protocol(&lower_proto)))
    {
        // The first `/`, `?` or `#` ends the host. A `@` moves the auth boundary
        // (and clears any earlier non-host char), so `http://a@b@c/` is auth=`a@b`.
        let rb: Vec<char> = rest.chars().collect();
        let (mut host_end, mut at_sign, mut non_host): (
            Option<usize>,
            Option<usize>,
            Option<usize>,
        ) = (None, None, None);
        for (i, &c) in rb.iter().enumerate() {
            match c {
                '\t' | '\n' | '\r' | ' ' | '"' | '%' | '\'' | ';' | '<' | '>' | '\\' | '^'
                | '`' | '{' | '|' | '}' => {
                    if non_host.is_none() {
                        non_host = Some(i);
                    }
                }
                '#' | '/' | '?' => {
                    if non_host.is_none() {
                        non_host = Some(i);
                    }
                    host_end = Some(i);
                }
                '@' => {
                    at_sign = Some(i);
                    non_host = None;
                }
                _ => {}
            }
            if host_end.is_some() {
                break;
            }
        }
        let mut s = 0usize;
        if let Some(at) = at_sign {
            let raw: String = rb[..at].iter().collect();
            u.auth = Some(url_decode(&raw));
            s = at + 1;
        }
        let host_str: String = match non_host {
            None => {
                let h: String = rb[s..].iter().collect();
                rest = String::new();
                h
            }
            Some(nh) => {
                let h: String = rb[s..nh].iter().collect();
                rest = rb[nh..].iter().collect();
                h
            }
        };

        let (hostname, port) = legacy_split_port(&host_str);
        u.port = port;
        let mut hostname = if hostname.len() > 255 {
            String::new()
        } else {
            // Hostnames are always lower case.
            hostname.to_lowercase()
        };
        let ipv6 = legacy_is_ipv6_hostname(&hostname);

        // `host` retains the brackets and the port; `hostname` keeps neither.
        let p = u.port.as_ref().map(|p| format!(":{p}")).unwrap_or_default();
        u.host = Some(format!("{hostname}{p}"));
        if ipv6 {
            hostname = hostname[1..hostname.len() - 1].to_string();
            if !rest.starts_with('/') {
                rest.insert(0, '/');
            }
        }
        u.hostname = Some(hostname);
    }

    if !legacy_is_unsafe_protocol(&lower_proto) {
        rest = legacy_auto_escape(&rest);
    }

    let rb: Vec<char> = rest.chars().collect();
    let (mut question, mut hash_idx): (Option<usize>, Option<usize>) = (None, None);
    for (i, &c) in rb.iter().enumerate() {
        if c == '#' {
            u.hash = Some(rb[i..].iter().collect());
            hash_idx = Some(i);
            break;
        } else if c == '?' && question.is_none() {
            question = Some(i);
        }
    }
    if let Some(q) = question {
        let e = hash_idx.unwrap_or(rb.len());
        u.search = Some(rb[q..e].iter().collect());
        u.query = Some(rb[q + 1..e].iter().collect());
    }
    let use_q = question.is_some() && (hash_idx.is_none() || question < hash_idx);
    match if use_q { question } else { hash_idx } {
        // No `?`/`#` at all — everything left is the pathname.
        None => {
            if !rb.is_empty() {
                u.pathname = Some(rb.iter().collect());
            }
        }
        // A leading `?`/`#` means there IS no pathname (Node leaves it null).
        Some(0) => {}
        Some(f) => u.pathname = Some(rb[..f].iter().collect()),
    }
    // Only a slashed protocol with a real host gets an implied root pathname.
    if legacy_is_slashed_protocol(&lower_proto)
        && u.hostname.as_deref().is_some_and(|h| !h.is_empty())
        && u.pathname.is_none()
    {
        u.pathname = Some("/".to_string());
    }
    u
}

/// Node's `Url.prototype.format`. `href` is THIS, not the raw input string.
pub(crate) fn legacy_url_format_impl(u: &LegacyUrl) -> String {
    let mut auth = u.auth.clone().unwrap_or_default();
    if !auth.is_empty() {
        auth = legacy_escape_auth(&auth);
        auth.push('@');
    }
    let mut protocol = u.protocol.clone().unwrap_or_default();
    let mut pathname = u.pathname.clone().unwrap_or_default();
    let mut hash = u.hash.clone().unwrap_or_default();
    let mut search = u.search.clone().unwrap_or_default();

    let mut host = String::new();
    if let Some(h) = u.host.as_deref().filter(|h| !h.is_empty()) {
        host = format!("{auth}{h}");
    } else if let Some(hn) = u.hostname.as_deref().filter(|h| !h.is_empty()) {
        // A bare IPv6 hostname has to be re-wrapped in brackets.
        let shown = if hn.contains(':') && !legacy_is_ipv6_hostname(hn) {
            format!("[{hn}]")
        } else {
            hn.to_string()
        };
        host = format!("{auth}{shown}");
        if let Some(p) = &u.port {
            host.push(':');
            host.push_str(p);
        }
    }

    if !protocol.is_empty() && !protocol.ends_with(':') {
        protocol.push(':');
    }
    pathname = pathname.replace('#', "%23").replace('?', "%3F");

    // Only the slashed protocols get the `//` — not `mailto:`, `xmpp:`, … unless
    // they had one to begin with.
    if u.slashes == Some(true) || legacy_is_slashed_protocol(&protocol) {
        if u.slashes == Some(true) || !host.is_empty() {
            if !pathname.is_empty() && !pathname.starts_with('/') {
                pathname.insert(0, '/');
            }
            host = format!("//{host}");
        } else if protocol.starts_with("file") {
            host = "//".to_string();
        }
    }

    search = search.replace('#', "%23");
    if !hash.is_empty() && !hash.starts_with('#') {
        hash.insert(0, '#');
    }
    if !search.is_empty() && !search.starts_with('?') {
        search.insert(0, '?');
    }
    format!("{protocol}{host}{pathname}{search}{hash}")
}

/// Node's `noEscapeAuth` set: everything outside it is percent-encoded when the
/// auth section is formatted. An `@` is NOT in the set, so
/// `format(parse("http://a@b@c/"))` is `"http://a%40b@c/"` — the auth is `a@b`
/// but the `@` inside it must be escaped or the href would re-parse differently.
fn legacy_escape_auth(auth: &str) -> String {
    let mut out = String::with_capacity(auth.len());
    for c in auth.chars() {
        let safe = c.is_ascii_alphanumeric()
            || matches!(
                c,
                '-' | '.'
                    | '_'
                    | '~'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | ':'
            );
        if safe {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
