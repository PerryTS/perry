// FFI: LSP bridge stubs, camera stubs (#191), cross-platform
// toast + reactive setText stubs (Phase 2 v3.3).

// =============================================================================
// LSP bridge stubs (not yet implemented on Windows)
// =============================================================================

#[no_mangle]
pub extern "C" fn hone_lsp_start(_cmd: i64, _args: i64, _cwd: i64) -> i64 {
    -1
}

#[no_mangle]
pub extern "C" fn hone_lsp_poll(_handle: i64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn hone_lsp_send(_handle: i64, _msg: i64) {}

#[no_mangle]
pub extern "C" fn hone_lsp_stop(_handle: i64) {}

// --- Camera stubs (issue #191) ---
// Real implementations live in `perry-ui-ios` and `perry-ui-android`. The
// Windows backend doesn't have a camera capture pipeline yet; these no-ops
// let user code that targets multiple platforms link cleanly.

#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_start(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(_x: f64, _y: f64) -> f64 {
    -1.0
}

#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(_handle: i64, _callback: f64) {}

// NOTE: a fake no-op `setjmp` stub used to live here so links succeeded when
// codegen host-cfg-gated the setjmp variant and emitted the bare `setjmp`
// name into Windows-target objects (MSVCRT only exports `_setjmp`/
// `_setjmpex`). Codegen now selects the setjmp ABI from the compile target's
// triple (`perry-codegen/src/setjmp_abi.rs`), so Windows targets always get
// the real 2-arg `_setjmp` — and a leftover bare-`setjmp` reference failing
// to link is the DESIRED loud failure, not something to paper over with a
// stub that silently corrupts try/catch (always-0 return, no saved context).

// --- Cross-platform toast + reactive setText entry points (Phase 2 v3.3) ---
//
// These are the dispatch-table rows (`setText` → `perry_ui_set_text`, etc.)
// that user TS hits directly. They were `{}` no-ops "so cross-platform code
// links on Windows targets" — which made `setText(id, value)` silently do
// nothing on native Windows from EVERY call site (button handlers, timers,
// async continuations), and `Text(content, id)` never registered its id at
// all. Mirror the macOS shims (`perry-ui-macos/src/lib_ffi/window_misc.rs`,
// #599): decode the StringHeaders and forward to the shared registry
// handlers that `register_cross_platform_text_handlers` also wires up.

/// Decode a raw `*const StringHeader` (passed as i64) into (data, len).
/// Returns (null, 0) for a null pointer — the registry handlers treat
/// that as an empty string.
unsafe fn string_parts(ptr_val: i64) -> (*const u8, usize) {
    if ptr_val == 0 {
        return (std::ptr::null(), 0);
    }
    let p = ptr_val as *const u8;
    let header = p as *const perry_runtime::string::StringHeader;
    let len = (*header).byte_len as usize;
    (
        p.add(std::mem::size_of::<perry_runtime::string::StringHeader>()),
        len,
    )
}

#[no_mangle]
pub extern "C" fn perry_ui_show_toast(msg_ptr: i64) {
    unsafe {
        let (msg_data, msg_len) = string_parts(msg_ptr);
        if !msg_data.is_null() {
            crate::widgets::toast::show_toast_handler(msg_data, msg_len);
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, id_ptr: i64) -> i64 {
    let handle = crate::ffi::widget_create::perry_ui_text_create(text_ptr);
    unsafe {
        let (id_data, id_len) = string_parts(id_ptr);
        if !id_data.is_null() && id_len > 0 {
            crate::widgets::text_registry::register_text_id_handler(handle, id_data, id_len);
        }
    }
    handle
}

#[no_mangle]
pub extern "C" fn perry_ui_set_text(id_ptr: i64, value_ptr: i64) {
    if id_ptr == 0 {
        return;
    }
    unsafe {
        let (id_data, id_len) = string_parts(id_ptr);
        let (val_data, val_len) = string_parts(value_ptr);
        crate::widgets::text_registry::set_text_handler(id_data, id_len, val_data, val_len);
    }
}
