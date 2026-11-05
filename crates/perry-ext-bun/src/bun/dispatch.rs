//! Handle-dispatch extension for `Bun.Transpiler`.
//!
//! `new Bun.Transpiler(opts)` returns a native handle id. Typed calls go
//! through the native table (`native_table/bun.rs`); a call on a receiver whose
//! static type was erased reaches the runtime's handle-method dispatch, which
//! asks each registered extension. This one claims only live `BunTranspiler`
//! handles and the four methods the table supports, with the same argument
//! conventions (`NA_STR` source / optional loader, `scan*` results parsed from
//! their JSON text). Anything else is left unclaimed.

use std::sync::Once;

use perry_ffi::{alloc_string, get_handle, Handle, JsValue, StringHeader};

use super::{
    js_bun_transpiler_scan, js_bun_transpiler_scan_imports, js_bun_transpiler_transform,
    js_bun_transpiler_transform_sync, BunTranspiler,
};

extern "C" {
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    /// Same entry the `NR_OBJ_FROM_JSON_STR` rows pipe their result through.
    fn js_json_parse_or_null(text: *const StringHeader) -> u64;
}

/// Idempotent; called from `js_bun_transpiler_new`.
pub(super) fn ensure_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        // SAFETY: a plain registration with a `'static` function pointer.
        unsafe { js_register_handle_method_dispatch_extension(transpiler_method_dispatch_ext) };
    });
}

fn bits(v: JsValue) -> f64 {
    f64::from_bits(v.bits())
}

/// A heap `StringHeader` for a string argument (short or heap), or null.
fn string_ptr(args: &[f64], i: usize) -> *const StringHeader {
    let Some(v) = args.get(i) else {
        return std::ptr::null();
    };
    match JsValue::from_bits(v.to_bits()).to_owned_string() {
        Some(s) => alloc_string(&s).as_raw() as *const StringHeader,
        None => std::ptr::null(),
    }
}

unsafe extern "C" fn transpiler_method_dispatch_ext(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    if handle <= 0 || name_ptr.is_null() || name_len == 0 || out.is_null() {
        return 0;
    }
    if get_handle::<BunTranspiler>(handle as Handle).is_none() {
        return 0;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) else {
        return 0;
    };
    let args: &[f64] = if args_len == 0 || args_ptr.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    let h = handle as Handle;
    let value = match name {
        "transformSync" => {
            let r = js_bun_transpiler_transform_sync(h, string_ptr(args, 0), string_ptr(args, 1));
            bits(JsValue::from_string_ptr(r))
        }
        "transform" => {
            let p = js_bun_transpiler_transform(h, string_ptr(args, 0), string_ptr(args, 1));
            bits(JsValue::from_object_ptr(p))
        }
        "scanImports" => {
            let json = js_bun_transpiler_scan_imports(h, string_ptr(args, 0));
            f64::from_bits(js_json_parse_or_null(json))
        }
        "scan" => {
            let json = js_bun_transpiler_scan(h, string_ptr(args, 0));
            f64::from_bits(js_json_parse_or_null(json))
        }
        _ => return 0,
    };
    *out = value;
    1
}
