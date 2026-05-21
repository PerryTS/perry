use crate::*;

// =============================================================================
// Save File Dialog
// =============================================================================

/// Open a save file dialog. Calls callback with selected path or undefined.
#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    callback: f64,
    default_name_ptr: i64,
    allowed_types_ptr: i64,
) {
    file_dialog::save_dialog(
        callback,
        default_name_ptr as *const u8,
        allowed_types_ptr as *const u8,
    );
}

// =============================================================================
// State TextField Binding (two-way)
// =============================================================================

/// Bind a TextField to a state cell (two-way binding).
#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Alert Dialog
// =============================================================================

/// Show an alert dialog with custom buttons.
/// `buttons` is a NaN-boxed JS array of string labels; the callback is
/// invoked with the 0-based index of the clicked button. Called from
/// `alertWithButtons(title, message, buttons, cb)` in TS.
#[no_mangle]
pub extern "C" fn perry_ui_alert(title_ptr: i64, message_ptr: i64, buttons: f64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let buttons_ptr = unsafe { js_nanbox_get_pointer(buttons) };
    widgets::alert::show(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr,
        callback,
    );
}

/// Simple 2-arg alert: shows NSAlert with the title/message and a single
/// "OK" button. Called from `alert(title, message)` in TS.
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    widgets::alert::show_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Sheet (Modal Panel)
// =============================================================================

/// Create a sheet (panel) with a body widget, width, and height. Returns
/// the sheet handle. #1033: aligned with the TS surface
/// `sheetCreate(body, width, height): Widget` and the perry-dispatch row
/// `[Widget, F64, F64]`. The previous `(width, height, title)` signature
/// silently dropped the body handle (it landed in X0; this fn read width
/// from D0), producing a blank sheet at the requested size.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(body_handle: i64, width: f64, height: f64) -> i64 {
    widgets::sheet::create(body_handle, width, height)
}

/// Present a sheet on the key window.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(sheet_handle: i64) {
    widgets::sheet::present(sheet_handle);
}

/// Dismiss a sheet.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(sheet_handle: i64) {
    widgets::sheet::dismiss(sheet_handle);
}

// =============================================================================
// App Lifecycle Hooks
// =============================================================================

/// Register an onTerminate callback.
#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(callback: f64) {
    app::register_on_terminate(callback);
}

/// Register an onActivate callback.
#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(callback: f64) {
    app::register_on_activate(callback);
}

// =============================================================================
// Toolbar
// =============================================================================

/// Create a toolbar. Returns handle.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    widgets::toolbar::create()
}

/// Add an item to a toolbar.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    toolbar_handle: i64,
    label_ptr: i64,
    icon_ptr: i64,
    callback: f64,
) {
    widgets::toolbar::add_item(
        toolbar_handle,
        label_ptr as *const u8,
        icon_ptr as *const u8,
        callback,
    );
}

/// Attach a toolbar to the key window.
#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(toolbar_handle: i64) {
    widgets::toolbar::attach(toolbar_handle);
}

// =============================================================================
// Keychain (perry/system)
// =============================================================================

/// Save a value to the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_save(key_ptr: i64, value_ptr: i64) {
    crate::keychain::save(key_ptr as *const u8, value_ptr as *const u8);
}

/// Get a value from the keychain. Returns NaN-boxed string or TAG_UNDEFINED.
#[no_mangle]
pub extern "C" fn perry_system_keychain_get(key_ptr: i64) -> f64 {
    crate::keychain::get(key_ptr as *const u8)
}

/// Delete a value from the keychain.
#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(key_ptr: i64) {
    crate::keychain::delete(key_ptr as *const u8);
}

// =============================================================================
// Notifications (perry/system)
// =============================================================================

/// Send a local notification.
#[no_mangle]
pub extern "C" fn perry_system_notification_send(title_ptr: i64, body_ptr: i64) {
    crate::notifications::send(title_ptr as *const u8, body_ptr as *const u8);
}

/// Register for remote (APNs) notifications. `callback` is invoked once with
/// the device token hex string when iOS/macOS negotiates one.
#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(callback: f64) {
    crate::notifications::register_remote(callback);
}

/// Register a handler for foreground remote-notification payloads.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(callback: f64) {
    crate::notifications::on_receive(callback);
}

/// Background-receive (#98) — no-op on macOS. Desktop apps don't have an
/// equivalent of `application:didReceiveRemoteNotification:fetchCompletionHandler:`;
/// the foreground delegate fires for both foregrounded and background app
/// states (NSApplication doesn't suspend background processes the way iOS
/// does), so user code targeting macOS should register `notificationOnReceive`
/// instead. Stub kept so cross-platform user code linking in macOS doesn't
/// fail to resolve the symbol.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    seconds: f64,
    repeats: f64,
) {
    crate::notifications::schedule_interval(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        seconds,
        repeats,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    timestamp_ms: f64,
) {
    crate::notifications::schedule_calendar(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        timestamp_ms,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    id_ptr: i64,
    title_ptr: i64,
    body_ptr: i64,
    lat: f64,
    lon: f64,
    radius: f64,
) {
    crate::notifications::schedule_location(
        id_ptr as *const u8,
        title_ptr as *const u8,
        body_ptr as *const u8,
        lat,
        lon,
        radius,
    );
}

#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(id_ptr: i64) {
    crate::notifications::cancel(id_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(callback: f64) {
    crate::notifications::set_on_tap(callback);
}
