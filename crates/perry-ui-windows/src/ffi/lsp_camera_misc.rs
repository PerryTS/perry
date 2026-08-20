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

/// Copy a raw `*const StringHeader` (passed as i64) into owned storage.
unsafe fn copy_string(ptr_val: i64) -> Option<String> {
    if ptr_val == 0 {
        return None;
    }
    Some(unsafe { perry_ffi::copy_string_from_raw(ptr_val as *const u8) })
}

#[no_mangle]
pub extern "C" fn perry_ui_show_toast(msg_ptr: i64) {
    unsafe {
        if let Some(message) = copy_string(msg_ptr) {
            crate::widgets::toast::show_toast_handler(message.as_ptr(), message.len());
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, id_ptr: i64) -> i64 {
    let handle = crate::ffi::widget_create::perry_ui_text_create(text_ptr);
    unsafe {
        if let Some(id) = copy_string(id_ptr).filter(|id| !id.is_empty()) {
            crate::widgets::text_registry::register_text_id_handler(handle, id.as_ptr(), id.len());
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
        let id = copy_string(id_ptr).expect("id_ptr was checked for null");
        let value = copy_string(value_ptr);
        let (value_data, value_len) = value
            .as_ref()
            .map_or((std::ptr::null(), 0), |value| (value.as_ptr(), value.len()));
        crate::widgets::text_registry::set_text_handler(
            id.as_ptr(),
            id.len(),
            value_data,
            value_len,
        );
    }
}
