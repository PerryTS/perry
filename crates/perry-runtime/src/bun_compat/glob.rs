//! `Bun.Glob` backed by Perry's native `node:fs` glob walker.

use super::*;

extern "C" fn bun_glob_scan(closure: *const ClosureHeader, options: f64) -> f64 {
    crate::fs::js_bun_glob_async_iterator(captured(closure), options)
}

extern "C" fn bun_glob_scan_sync(closure: *const ClosureHeader, options: f64) -> f64 {
    crate::fs::js_bun_glob_sync_iterator(captured(closure), options)
}

extern "C" fn bun_glob_match(closure: *const ClosureHeader, path: f64) -> f64 {
    match crate::fs::bun_glob_matches(captured(closure), path) {
        Ok(matches) => bool_value(matches),
        Err(error) => crate::exception::js_throw(error),
    }
}

/// `new Bun.Glob(pattern)`. The returned object owns the pattern through each
/// method closure, so it remains valid across arbitrary intervening GC.
#[no_mangle]
pub extern "C-unwind" fn js_bun_glob_new(pattern: f64) -> f64 {
    if !is_string_value(pattern) {
        let message = b"Bun.Glob constructor requires a string pattern";
        let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let error = crate::error::js_error_new_with_message(message);
        crate::exception::js_throw(f64::from_bits(JSValue::pointer(error as *const u8).bits()));
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern = scope.root_nanbox_f64(pattern);
    let obj = js_object_alloc(0, 4);
    set_field(
        obj,
        b"scan",
        bound_method1(bun_glob_scan, pattern.get_nanbox_f64()),
    );
    set_field(
        obj,
        b"scanSync",
        bound_method1(bun_glob_scan_sync, pattern.get_nanbox_f64()),
    );
    set_field(
        obj,
        b"match",
        bound_method1(bun_glob_match, pattern.get_nanbox_f64()),
    );
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}
