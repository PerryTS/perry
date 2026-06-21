//! BloomView — a native render-surface host widget (issue #2395).
//!
//! Reserves a bare `android.view.View` in the Perry UI view tree for an external
//! GPU renderer (e.g. the Bloom engine) to draw into. Perry UI only owns the
//! view; user TypeScript drives the renderer. Mirrors the Windows implementation
//! conceptually — Android has no HWND, so `bloomViewGetHwnd` echoes the registry
//! handle as a stable token (a real `SurfaceView`/`TextureView` + JNI surface
//! bridge is the follow-up for live embedding).

use crate::jni_bridge;
use jni::objects::JValue;

/// Create a BloomView host. Returns the widget handle, or 0 on JNI failure.
pub fn create(_width: f64, _height: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    let activity = super::get_activity(&mut env);
    let view = match env.new_object(
        "android/view/View",
        "(Landroid/content/Context;)V",
        &[JValue::Object(&activity)],
    ) {
        Ok(v) => v,
        Err(_) => {
            unsafe {
                let _ = env.pop_local_frame(&jni::objects::JObject::null());
            }
            return 0;
        }
    };

    let global_ref = match env.new_global_ref(&view) {
        Ok(g) => g,
        Err(_) => {
            unsafe {
                let _ = env.pop_local_frame(&jni::objects::JObject::null());
            }
            return 0;
        }
    };
    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }

    super::register_widget(global_ref)
}

/// Android has no HWND; echo the registry handle as a stable token for the
/// caller — but validate it first (return 0 for an unknown/stale handle), so
/// downstream renderers never treat a bogus handle as attachable.
pub fn get_native_handle(handle: i64) -> i64 {
    match super::get_widget(handle) {
        Some(_) => handle,
        None => 0,
    }
}
