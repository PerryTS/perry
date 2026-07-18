//! Dispatch table for native module method calls that escape into the runtime tower from
//! `js_native_call_method`.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

/// #3712: coerce a NaN-boxed value to an owned UTF-8 `String`, matching the
/// `${val}` coercion Node applies before building HTTP error messages.
unsafe fn http_value_to_owned_string(v: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(v);
    if ptr.is_null() {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

/// #3712: read the raw bytes of a string value, or `None` if it is not a
/// string. `js_string_materialize_to_heap` returns null for non-strings (no
/// coercion) and handles SSO short strings, so a genuine empty string yields
/// `Some([])` while a number/object/undefined yields `None`.
unsafe fn http_string_bytes(v: f64) -> Option<Vec<u8>> {
    let ptr = crate::string::js_string_materialize_to_heap(v);
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    Some(std::slice::from_raw_parts(data, len).to_vec())
}

/// Node's HTTP token char set (lib/_http_common `tokenRegExp`):
/// `^[\^_`a-zA-Z\-0-9!#$%&'*+.|~]+$`.
fn http_is_token_byte(b: u8) -> bool {
    matches!(b,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
        | b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*'
        | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
}

/// #3712: `http.validateHeaderName(name[, label])`. Throws
/// `TypeError [ERR_INVALID_HTTP_TOKEN]` for a non-string / empty / non-token
/// name; otherwise returns undefined.
///
/// Exposed as a `#[no_mangle]` extern so codegen's static dispatch table can
/// emit a direct call for `http.validateHeaderName(...)`; the bound-closure
/// value form routes here too via `dispatch_native_module_method`.
///
/// # Safety
/// `name`/`label` are NaN-boxed `JSValue` bits.
#[no_mangle]
pub unsafe extern "C" fn js_http_validate_header_name(name: f64, label: f64) -> f64 {
    let valid = match http_string_bytes(name) {
        Some(bytes) => !bytes.is_empty() && bytes.iter().all(|&b| http_is_token_byte(b)),
        None => false,
    };
    if valid {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // `label || 'Header name'` — fall back to the default for any falsy /
    // non-string label.
    let label_str = match http_string_bytes(label) {
        Some(bytes) if !bytes.is_empty() => String::from_utf8_lossy(&bytes).into_owned(),
        _ => "Header name".to_string(),
    };
    let display = http_value_to_owned_string(name);
    let message = format!("{label_str} must be a valid HTTP token [\"{display}\"]");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_HTTP_TOKEN")
}

/// #3712: `http.validateHeaderValue(name, value)`. Throws
/// `TypeError [ERR_HTTP_INVALID_HEADER_VALUE]` for an undefined value and
/// `TypeError [ERR_INVALID_CHAR]` for invalid header characters; otherwise
/// returns undefined.
///
/// # Safety
/// `name`/`value` are NaN-boxed `JSValue` bits.
#[no_mangle]
pub unsafe extern "C" fn js_http_validate_header_value(name: f64, value: f64) -> f64 {
    if value.to_bits() == crate::value::TAG_UNDEFINED {
        let name_disp = http_value_to_owned_string(name);
        let message = format!("Invalid value \"undefined\" for header \"{name_disp}\"");
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_HTTP_INVALID_HEADER_VALUE");
    }
    // Node coerces the value to a string, then rejects control chars outside
    // the `\t`, `\x20-\x7e`, `\x80-\xff` ranges (lib/_http_common
    // `headerCharRegex`). Multi-byte UTF-8 bytes are all >= 0x80, so allowed.
    let value_str = http_value_to_owned_string(value);
    let has_invalid = value_str
        .as_bytes()
        .iter()
        .any(|&b| (b < 0x20 && b != b'\t') || b == 0x7f);
    if has_invalid {
        let name_disp = http_value_to_owned_string(name);
        let message = format!("Invalid character in header content [\"{name_disp}\"]");
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_CHAR");
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// #3712: `http.setMaxIdleHTTPParsers(max)` / `http.setGlobalProxyFromEnv(...)`
/// — deterministic no-ops returning undefined (Perry has no shared parser pool
/// or env-driven proxy state). Exposed for the static dispatch table.
#[no_mangle]
pub extern "C" fn js_http_setter_noop(_value: f64) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_http_connection_listener_noop(_socket: f64) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Dispatch a method call on a native module namespace object.
/// Extracts the module name from the object and dispatches to the appropriate
/// runtime function based on (module_name, method_name).

/// Per-call marshalling context shared by every `nm_dispatch_<module>` fn.
pub(crate) struct NmCtx {
    pub obj: *const ObjectHeader,
    pub args_ptr: *const f64,
    pub args_len: usize,
    pub assert_skip_prototype: bool,
}

/// General (non-path) marshalling closures from the old prologue. Identifiers are
/// passed in so the `let` bindings carry call-site hygiene (visible to the arms).
macro_rules! nm_general_closures {
    ($obj:ident, $args_ptr:ident, $args_len:ident, $arg:ident, $i32_arg:ident, $bool_to_f64:ident, $str_to_f64:ident, $pack_args:ident, $pack_args_from:ident, $bool_tag:ident, $ptr_addr:ident, $optional_ptr_addr:ident, $_arg_event_ptr:ident, $arg_bits:ident, $_arg_closure_ptr:ident, $ptr_to_f64:ident, $typed_kind:ident) => {
        let $arg = |n: usize| -> f64 {
            if n < $args_len && !$args_ptr.is_null() {
                *$args_ptr.add(n)
            } else {
                f64::from_bits(JSValue::undefined().bits())
            }
        };
        let $i32_arg = |n: usize| -> i32 {
            let v = $arg(n);
            let bits = v.to_bits();
            if (bits >> 48) == 0x7FFE {
                return (bits & 0xFFFF_FFFF) as u32 as i32;
            }
            if v.is_nan() || v.is_infinite() {
                0
            } else {
                v as i32
            }
        };
        let $bool_to_f64 = |v: i32| -> f64 {
            if v != 0 {
                f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
            } else {
                f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
            }
        };
        let $str_to_f64 = |ptr: *mut crate::StringHeader| -> f64 {
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        };
        let $pack_args = || -> *mut crate::array::ArrayHeader {
            let mut arr = crate::array::js_array_alloc($args_len as u32);
            for i in 0..$args_len {
                arr = crate::array::js_array_push_f64(arr, $arg(i));
            }
            arr
        };
        let $pack_args_from = |start: usize| -> *mut crate::array::ArrayHeader {
            let len = $args_len.saturating_sub(start);
            let mut arr = crate::array::js_array_alloc(len as u32);
            for i in start..$args_len {
                arr = crate::array::js_array_push_f64(arr, $arg(i));
            }
            arr
        };
        let $bool_tag = |v: bool| -> f64 {
            if v {
                f64::from_bits(0x7FFC_0000_0000_0004)
            } else {
                f64::from_bits(0x7FFC_0000_0000_0003)
            }
        };
        let $ptr_addr = |v: f64| -> usize {
            let bits = v.to_bits();
            if (bits >> 48) >= 0x7FF8 {
                (bits & 0x0000_FFFF_FFFF_FFFF) as usize
            } else {
                bits as usize
            }
        };
        let $optional_ptr_addr = |v: f64| -> usize {
            let value = JSValue::from_bits(v.to_bits());
            if value.is_undefined() || value.is_null() {
                0
            } else {
                $ptr_addr(v)
            }
        };
        let $_arg_event_ptr = |n: usize| -> *const crate::StringHeader {
            crate::value::js_get_string_pointer_unified($arg(n)) as *const crate::StringHeader
        };
        let $arg_bits = |n: usize| -> i64 { $arg(n).to_bits() as i64 };
        let $_arg_closure_ptr = |n: usize| -> *const crate::closure::ClosureHeader {
            if n >= $args_len {
                return std::ptr::null();
            }
            let v = $arg(n);
            let jsv = JSValue::from_bits(v.to_bits());
            if jsv.is_undefined() || jsv.is_null() {
                std::ptr::null()
            } else {
                $ptr_addr(v) as *const crate::closure::ClosureHeader
            }
        };
        let $ptr_to_f64 = |ptr: *const u8| -> f64 { f64::from_bits(JSValue::pointer(ptr).bits()) };
        let $typed_kind = |v: f64| -> Option<u8> {
            let addr = $ptr_addr(v);
            if crate::buffer::is_uint8array_buffer(addr) {
                Some(crate::typedarray::KIND_UINT8)
            } else {
                crate::typedarray::lookup_typed_array_kind(addr)
            }
        };
        let _ = (
            &$arg,
            &$i32_arg,
            &$bool_to_f64,
            &$str_to_f64,
            &$pack_args,
            &$pack_args_from,
            &$bool_tag,
            &$ptr_addr,
            &$optional_ptr_addr,
            &$_arg_event_ptr,
            &$arg_bits,
            &$_arg_closure_ptr,
            &$ptr_to_f64,
            &$typed_kind,
        );
    };
}

/// Thin router — extract+normalize the module name, dispatch via the per-module
/// registry. Names no bucket fn, so unused modules dead-strip.
pub(crate) unsafe fn dispatch_native_module_method(
    obj: *const ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Extract the module name from field 0 of the namespace object
    let module_field = js_object_get_field(obj as *mut _, 0);
    let module_name = if module_field.is_string() {
        let str_ptr = module_field.as_string_ptr();
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
    } else {
        ""
    };
    let (module_name, assert_skip_prototype) = match module_name {
        "assert.instance" => ("assert", false),
        "assert.instance.skip" => ("assert", true),
        "assert/strict.instance" => ("assert/strict", false),
        "assert/strict.instance.skip" => ("assert/strict", true),
        "path/posix" => ("path.posix", false),
        "path/win32" => ("path.win32", false),
        "async_hooks.default" => ("async_hooks", false),
        // #3687: cluster default-import method calls (`cluster.fork()`,
        // `cluster.emit(...)`) dispatch against the base `cluster` arms.
        "cluster.default" => ("cluster", false),
        // #6563: `(await import("node-pty")).default.spawn(...)` — the CJS
        // interop shape esbuild-bundled consumers produce.
        "node-pty.default" => ("node-pty", false),
        "os.default" => ("os", false),
        "path.default" => ("path", false),
        "path.posix.default" => ("path.posix", false),
        "path.win32.default" => ("path.win32", false),
        "process.default" => ("process", false),
        "querystring.default" => ("querystring", false),
        "url.default" => ("url", false),
        "util.default" => ("util", false),
        "vm.default" => ("vm", false),
        // #3987-adjacent: `process.getBuiltinModule("punycode")` returns the
        // CJS-default namespace (`punycode.default`); without this alias its
        // method calls dispatched as `("punycode.default", "decode")` — which
        // has no arm — and returned `undefined`. The base `("punycode", …)`
        // arms below already implement decode/encode/toASCII/toUnicode.
        "punycode.default" => ("punycode", false),
        _ => (module_name, false),
    };
    let ctx = NmCtx {
        obj,
        args_ptr,
        args_len,
        assert_skip_prototype,
    };
    match super::native_module_registry::nm_dispatch_lookup(module_name) {
        Some(f) => f(&ctx, module_name, method_name),
        None => f64::from_bits(JSValue::undefined().bits()),
    }
}

// ── per-module dispatch buckets, split out for file-size (pure relocation) ──
// The `nm_general_closures!` macro defined above is in textual scope for these
// child modules because they are declared AFTER the `macro_rules!` definition.
mod dispatch_a_c;
mod dispatch_d_i;
mod dispatch_m_p;
mod dispatch_q_u;
mod dispatch_util;
mod dispatch_v_z;

pub(crate) use dispatch_a_c::{
    nm_dispatch_assert, nm_dispatch_async_hooks, nm_dispatch_bigint, nm_dispatch_buffer,
    nm_dispatch_bun, nm_dispatch_child_process, nm_dispatch_cluster, nm_dispatch_console,
    nm_dispatch_crypto,
};
pub(crate) use dispatch_d_i::{
    nm_dispatch_dgram, nm_dispatch_dns, nm_dispatch_domain, nm_dispatch_events, nm_dispatch_fs,
    nm_dispatch_http, nm_dispatch_inspector,
};
pub(crate) use dispatch_m_p::{
    nm_dispatch_module, nm_dispatch_net, nm_dispatch_node_pty, nm_dispatch_os, nm_dispatch_path,
    nm_dispatch_perf, nm_dispatch_process,
};
pub(crate) use dispatch_q_u::{
    nm_dispatch_punycode, nm_dispatch_querystring, nm_dispatch_readline, nm_dispatch_repl,
    nm_dispatch_sea, nm_dispatch_sqlite, nm_dispatch_stream, nm_dispatch_timers, nm_dispatch_tls,
    nm_dispatch_tty, nm_dispatch_url,
};
pub(crate) use dispatch_util::nm_dispatch_util;
pub(crate) use dispatch_v_z::{nm_dispatch_v8, nm_dispatch_vm, nm_dispatch_wasi, nm_dispatch_zlib};
