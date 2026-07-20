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

// --- Cross-platform toast + reactive setText stubs (Phase 2 v3.3) ---
// Full GTK4 implementation in perry-ui-gtk4. Present here so cross-platform
// code that calls showToast / setText links on Windows targets.

#[no_mangle]
pub extern "C" fn perry_ui_show_toast(_msg_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, _id_ptr: i64) -> i64 {
    crate::ffi::widget_create::perry_ui_text_create(text_ptr)
}

#[no_mangle]
pub extern "C" fn perry_ui_set_text(_id_ptr: i64, _value_ptr: i64) {}
