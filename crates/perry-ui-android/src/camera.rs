//! Camera widget — live camera preview with color sampling (Android Camera2 API).
//!
//! Architecture: Rust creates a TextureView widget and delegates camera lifecycle
//! management to Kotlin PerryBridge via JNI. Frame capture for color sampling uses
//! an ImageReader on the Kotlin side; Rust calls into it for pixel reads.
//!
//! Matches the iOS camera API surface:
//! - create() → widget handle (TextureView)
//! - start(handle) → open camera, bind preview
//! - stop(handle) → close camera session
//! - freeze(handle) / unfreeze(handle) → pause/resume preview
//! - sample_color(x, y) → packed RGB from latest frame
//! - set_on_tap(handle, callback) → tap gesture on camera view

use jni::objects::{JObject, JValue};

use crate::callback;
use crate::jni_bridge;
use crate::widgets;

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

fn log(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe {
        __android_log_print(
            3,
            b"PerryCamera\0".as_ptr(),
            b"%s\0".as_ptr(),
            c_msg.as_ptr(),
        );
    }
}

/// Create a TextureView widget for camera preview. Returns widget handle.
pub fn create() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = widgets::get_activity(&mut env);

    // Create TextureView(context)
    let texture_view = env.new_object(
        "android/view/TextureView",
        "(Landroid/content/Context;)V",
        &[JValue::Object(&activity)],
    );
    let texture_view = match texture_view {
        Ok(v) => v,
        Err(e) => {
            log(&format!("[camera] failed to create TextureView: {:?}", e));
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return 0;
        }
    };

    let global = env
        .new_global_ref(texture_view)
        .expect("Failed to create global ref");
    let handle = widgets::register_widget(global);
    unsafe {
        env.pop_local_frame(&JObject::null());
    }

    log(&format!("[camera] created TextureView, handle={}", handle));
    handle
}

/// Start the camera capture session. Passes the TextureView to Kotlin for Camera2 setup.
pub fn start(handle: i64) {
    let view_ref = match widgets::get_widget(handle) {
        Some(v) => v,
        None => {
            log("[camera] start: invalid handle");
            return;
        }
    };

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    let _ = env.call_static_method(
        bridge_cls,
        "startCamera",
        "(Landroid/view/TextureView;)V",
        &[JValue::Object(view_ref.as_obj())],
    );

    if env.exception_check().unwrap_or(false) {
        log("[camera] start: Java exception occurred");
        let _ = env.exception_clear();
    }

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    log("[camera] start called");
}

/// Stop the camera capture session.
pub fn stop(_handle: i64) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    let _ = env.call_static_method(bridge_cls, "stopCamera", "()V", &[]);

    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    log("[camera] stopped");
}

/// Freeze the camera (pause preview, keep last frame).
pub fn freeze(_handle: i64) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    let _ = env.call_static_method(bridge_cls, "freezeCamera", "()V", &[]);

    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    log("[camera] frozen");
}

/// Unfreeze the camera (resume live preview).
pub fn unfreeze(_handle: i64) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    let _ = env.call_static_method(bridge_cls, "unfreezeCamera", "()V", &[]);

    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    log("[camera] unfrozen");
}

/// Sample the color at normalized coordinates (0.0-1.0) from the latest frame.
/// Returns packed RGB as f64: r * 65536 + g * 256 + b.
/// Returns -1.0 if no frame is available.
pub fn sample_color(x: f64, y: f64) -> f64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    let result = env.call_static_method(
        bridge_cls,
        "cameraSampleColor",
        "(DD)D",
        &[JValue::Double(x), JValue::Double(y)],
    );

    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
        return -1.0;
    }

    let value = result.map(|v| v.d().unwrap_or(-1.0)).unwrap_or(-1.0);
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    value
}

/// Set a tap handler that receives normalized (x, y) coordinates.
pub fn set_on_tap(handle: i64, callback_f64: f64) {
    let view_ref = match widgets::get_widget(handle) {
        Some(v) => v,
        None => {
            log("[camera] set_on_tap: invalid handle");
            return;
        }
    };

    let key = callback::register(callback_f64);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    let _ = env.call_static_method(
        bridge_cls,
        "setCameraTapCallback",
        "(Landroid/view/View;J)V",
        &[JValue::Object(view_ref.as_obj()), JValue::Long(key)],
    );

    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    log(&format!("[camera] set_on_tap: key={}", key));
}
