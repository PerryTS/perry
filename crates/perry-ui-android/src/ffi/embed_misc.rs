//! Folder dialog, embed native view, missing-stubs (frame-split, screen
//! metrics, audio capture/recording, camera), screenshot fallback,
//! layout-change stubs, app-files-dir helpers, textfield stubs and the
//! `perry/media` AVPlayer-backed thunks. Originally `lib.rs` lines
//! 2065-2547.

use crate::{audio, camera, file_dialog, jni_bridge, media_playback, widgets};

// =============================================================================
// Folder Dialog (parity with iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    // On Android, use the same file dialog (SAF) for folder picking
    file_dialog::open_dialog(callback);
}

// =============================================================================
// Embed Native View (parity with iOS embed_nsview)
// =============================================================================

/// Register an external Android View (from a native library) as a Perry widget.
/// The pointer must be a JNI GlobalRef to an Android View object.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(view_ptr: i64) -> i64 {
    if view_ptr == 0 {
        return 0;
    }
    // On Android, the native view pointer is a raw JNI object pointer.
    // Convert it to a GlobalRef and register as a widget.
    let env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let obj = unsafe { jni::objects::JObject::from_raw(view_ptr as jni::sys::jobject) };
    let global = match env.new_global_ref(obj) {
        Ok(g) => g,
        Err(_) => {
            unsafe {
                env.pop_local_frame(&jni::objects::JObject::null());
            }
            return 0;
        }
    };
    let handle = widgets::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

// =============================================================================
// Missing stubs — platform functions not yet implemented on Android
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create(_left_width: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(_parent: i64, _child: i64) {}

/// Query display metrics from the Android system.
/// Returns (widthDp, heightDp, density).
fn query_display_metrics() -> (f64, f64, f64) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    // Get Application context: ActivityThread.currentApplication()
    let result = (|| -> Option<(f64, f64, f64)> {
        let app = env
            .call_static_method(
                "android/app/ActivityThread",
                "currentApplication",
                "()Landroid/app/Application;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        if app.is_null() {
            return None;
        }

        // Get Resources
        let res = env
            .call_method(
                &app,
                "getResources",
                "()Landroid/content/res/Resources;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        // Get DisplayMetrics
        let dm = env
            .call_method(
                &res,
                "getDisplayMetrics",
                "()Landroid/util/DisplayMetrics;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;

        let width_px = env.get_field(&dm, "widthPixels", "I").ok()?.i().ok()? as f64;
        let height_px = env.get_field(&dm, "heightPixels", "I").ok()?.i().ok()? as f64;
        let density = env.get_field(&dm, "density", "F").ok()?.f().ok()? as f64;

        if density > 0.0 {
            Some((width_px / density, height_px / density, density))
        } else {
            None
        }
    })();

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    result.unwrap_or((412.0, 915.0, 2.625))
}

#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    query_display_metrics().0
}

#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    query_display_metrics().1
}

#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    query_display_metrics().2
}

/// perry_get_device_idiom() → "phone" (raw `*mut StringHeader`, NaN-boxed
/// by the `ReturnKind::Str` dispatch row). Android tablets are not yet
/// discriminated (needs a JNI smallest-width check); report the phone
/// form factor, matching the previous numeric code 0.
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(b"phone".as_ptr(), 5) }
}

// Audio capture (AudioRecord via JNI)
#[no_mangle]
pub extern "C" fn perry_system_audio_start() -> i64 {
    audio::start()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop() {
    audio::stop()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_level() -> f64 {
    audio::get_level()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_peak() -> f64 {
    audio::get_peak()
}
#[no_mangle]
pub extern "C" fn perry_system_audio_get_waveform(count: f64) -> f64 {
    audio::get_waveform(count)
}
#[no_mangle]
pub extern "C" fn perry_system_get_device_model() -> i64 {
    audio::get_device_model()
}
/// Bug-report-flow utility: stable OS-version string. Android stub —
/// native impl will read `Build.VERSION.RELEASE` via JNI.
#[no_mangle]
pub extern "C" fn perry_system_get_os_version() -> i64 {
    perry_runtime::stub_diag::perry_stub_warn(
        "perry_system_get_os_version",
        "Android getOSVersion (Build.VERSION.RELEASE) not yet implemented",
        None,
    );
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
#[no_mangle]
pub extern "C" fn perry_system_audio_set_output_filename(filename_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        crate::app::str_from_header(ptr)
    }
    let filename = str_from_header(filename_ptr as *const u8);
    audio::set_output_filename(filename);
}
#[no_mangle]
pub extern "C" fn perry_system_audio_start_recording() {
    audio::start_recording();
}
#[no_mangle]
pub extern "C" fn perry_system_audio_stop_recording() {
    audio::stop_recording();
}

// Camera (Camera2 API via JNI)
#[no_mangle]
pub extern "C" fn perry_ui_camera_create() -> i64 {
    camera::create()
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_start(handle: i64) {
    camera::start(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_stop(handle: i64) {
    camera::stop(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_freeze(handle: i64) {
    camera::freeze(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_unfreeze(handle: i64) {
    camera::unfreeze(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_sample_color(x: f64, y: f64) -> f64 {
    camera::sample_color(x, y)
}
#[no_mangle]
pub extern "C" fn perry_ui_camera_set_on_tap(handle: i64, callback: f64) {
    camera::set_on_tap(handle, callback)
}

// Geisterhand screenshot stub: only when the geisterhand feature is OFF.
// When the feature is ON, `screenshot::perry_ui_screenshot_capture` is the
// real implementation and providing this stub here would be a duplicate
// `#[no_mangle]` symbol.
#[cfg(not(feature = "geisterhand"))]
#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(_out_len: *mut usize) -> *mut u8 {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn perry_on_layout_change(_callback: f64) {}

#[no_mangle]
pub extern "C" fn __wrapper_perry_on_layout_change(_callback: f64) {}

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
}

extern "C" {
    fn js_nanbox_string(ptr: *const u8) -> f64;
}

fn get_app_files_dir_string() -> f64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);
    let result = (|| -> Option<f64> {
        let activity = env
            .call_static_method(
                "com/perry/app/PerryBridge",
                "getActivity",
                "()Landroid/app/Activity;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        if activity.is_null() {
            return None;
        }
        let files_dir = env
            .call_method(&activity, "getFilesDir", "()Ljava/io/File;", &[])
            .ok()?
            .l()
            .ok()?;
        if files_dir.is_null() {
            return None;
        }
        let abs_path = env
            .call_method(&files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        let rust_str = env.get_string((&abs_path).into()).ok()?;
        let bytes = rust_str.to_str().unwrap_or("").as_bytes();
        if bytes.is_empty() {
            return None;
        }
        // Append /workspace to the files dir
        let mut path = String::from_utf8_lossy(bytes).to_string();
        path.push_str("/workspace");
        crate::log_debug(&format!("get_app_files_dir: path={}", path));
        let path_bytes = path.as_bytes();
        let str_ptr = unsafe { js_string_from_bytes(path_bytes.as_ptr(), path_bytes.len() as i64) };
        // NaN-box the string pointer so Perry can use it as a string value
        let nanboxed = unsafe { js_nanbox_string(str_ptr) };
        Some(nanboxed)
    })();
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    // Return empty string NaN-boxed (not 0, which is integer 0)
    result.unwrap_or_else(|| unsafe { js_nanbox_string(std::ptr::null()) })
}

#[no_mangle]
pub extern "C" fn hone_get_app_files_dir() -> f64 {
    get_app_files_dir_string()
}

#[no_mangle]
pub extern "C" fn __wrapper_hone_get_app_files_dir() -> f64 {
    get_app_files_dir_string()
}

#[no_mangle]
pub extern "C" fn hone_get_documents_dir() -> f64 {
    get_app_files_dir_string()
}

#[no_mangle]
pub extern "C" fn __wrapper_hone_get_documents_dir() -> f64 {
    get_app_files_dir_string()
}

// =============================================================================
// Stubs for UI functions not yet implemented on Android
// =============================================================================

/// perry_ui_poll_open_file() — macOS "Open With" not applicable on Android
#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    0 // null (no file)
}

/// perry_ui_textfield_blur_all() — dismiss all keyboard focus
#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {
    // TODO: hide soft keyboard via InputMethodManager
}

/// perry_ui_textfield_set_on_focus(handle, callback) — on-focus callback for textfield
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(_handle: f64, _callback: f64) {
    // TODO: wire OnFocusChangeListener
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(_handle: i64, _next_handle: i64) {
    // Android handles tab/next navigation automatically
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_borderless(handle: i64, borderless: f64) {
    widgets::textfield::set_borderless(handle, borderless);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::textfield::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_font_size(handle: i64, size: f64) {
    widgets::textfield::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::textfield::set_text_color(handle, r, g, b, a);
}

/// perry_ui_widget_add_overlay(parent, child) — add overlay view
#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent: f64, _child: f64) {
    // TODO: add child as overlay in FrameLayout
}

/// perry_ui_widget_set_overlay_frame(child, x, y, w, h) — position overlay
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(
    _child: f64,
    _x: f64,
    _y: f64,
    _w: f64,
    _h: f64,
) {
    // TODO: set FrameLayout.LayoutParams with margins
}

// =============================================================================
// perry/media — streaming media playback (issue #351). AVPlayer-backed.
// See `media_playback.rs` for the implementation; everything below is a
// thin FFI thunk that the codegen-emitted `perry_media_*` declarations
// resolve to at link time.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_media_create_player(url_ptr: i64) -> i64 {
    media_playback::create_player(url_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_media_play(handle: f64) {
    media_playback::play(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_pause(handle: f64) {
    media_playback::pause(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_stop(handle: f64) {
    media_playback::stop(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_seek(handle: f64, seconds: f64) {
    media_playback::seek(handle, seconds);
}

#[no_mangle]
pub extern "C" fn perry_media_set_volume(handle: f64, volume: f64) {
    media_playback::set_volume(handle, volume);
}

#[no_mangle]
pub extern "C" fn perry_media_set_rate(handle: f64, rate: f64) {
    media_playback::set_rate(handle, rate);
}

#[no_mangle]
pub extern "C" fn perry_media_get_current_time(handle: f64) -> f64 {
    media_playback::get_current_time(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_duration(handle: f64) -> f64 {
    media_playback::get_duration(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_state(handle: f64) -> i64 {
    media_playback::get_state(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_is_playing(handle: f64) -> f64 {
    media_playback::is_playing(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_on_state_change(handle: f64, closure: f64) {
    media_playback::on_state_change(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_on_time_update(handle: f64, closure: f64) {
    media_playback::on_time_update(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_set_now_playing(
    handle: f64,
    title_ptr: i64,
    artist_ptr: i64,
    album_ptr: i64,
    artwork_ptr: i64,
) {
    media_playback::set_now_playing(
        handle,
        title_ptr as *const u8,
        artist_ptr as *const u8,
        album_ptr as *const u8,
        artwork_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_media_destroy(handle: f64) {
    media_playback::destroy(handle);
}
