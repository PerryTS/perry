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

use jni::objects::JObject;
use jni::JValue;

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
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);

        let activity = widgets::get_activity(env);

        // Create TextureView(context)
        let texture_view = env.new_object(
            jni::jni_str!("android/view/TextureView"),
            jni::jni_sig!("(Landroid/content/Context;)V"),
            &[JValue::Object(&activity)],
        );
        let texture_view = match texture_view {
            Ok(v) => v,
            Err(e) => {
                log(&format!("[camera] failed to create TextureView: {:?}", e));
                unsafe {
                    let _ = jni_bridge::pop_local_frame(env, &JObject::null());
                }
                return 0;
            }
        };

        let global =
            jni_bridge::new_global_ref(env, texture_view).expect("Failed to create global ref");
        let handle = widgets::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }

        log(&format!("[camera] created TextureView, handle={}", handle));
        handle
    })
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

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;

        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("startCamera"),
            jni::jni_sig!("(Landroid/view/TextureView;)V"),
            &[JValue::Object(view_ref.as_obj())],
        );

        if env.exception_check() {
            log("[camera] start: Java exception occurred");
            let _ = env.exception_clear();
        }

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        log("[camera] start called");
    })
}

/// Stop the camera capture session.
pub fn stop(_handle: i64) {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;

        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("stopCamera"),
            jni::jni_sig!("()V"),
            &[],
        );

        if env.exception_check() {
            let _ = env.exception_clear();
        }

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        log("[camera] stopped");
    })
}

/// Freeze the camera (pause preview, keep last frame).
pub fn freeze(_handle: i64) {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;

        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("freezeCamera"),
            jni::jni_sig!("()V"),
            &[],
        );

        if env.exception_check() {
            let _ = env.exception_clear();
        }

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        log("[camera] frozen");
    })
}

/// Unfreeze the camera (resume live preview).
pub fn unfreeze(_handle: i64) {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;

        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("unfreezeCamera"),
            jni::jni_sig!("()V"),
            &[],
        );

        if env.exception_check() {
            let _ = env.exception_clear();
        }

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        log("[camera] unfrozen");
    })
}

/// Sample the color at normalized coordinates (0.0-1.0) from the latest frame.
/// Returns packed RGB as f64: r * 65536 + g * 256 + b.
/// Returns -1.0 if no frame is available.
pub fn sample_color(x: f64, y: f64) -> f64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;

        let result = env.call_static_method(
            bridge_cls,
            jni::jni_str!("cameraSampleColor"),
            jni::jni_sig!("(DD)D"),
            &[JValue::Double(x), JValue::Double(y)],
        );

        if env.exception_check() {
            let _ = env.exception_clear();
            unsafe {
                let _ = jni_bridge::pop_local_frame(env, &JObject::null());
            }
            return -1.0;
        }

        let value = result.map(|v| v.d().unwrap_or(-1.0)).unwrap_or(-1.0);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        value
    })
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

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;

        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("setCameraTapCallback"),
            jni::jni_sig!("(Landroid/view/View;J)V"),
            &[JValue::Object(view_ref.as_obj()), JValue::Long(key)],
        );

        if env.exception_check() {
            let _ = env.exception_clear();
        }

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        log(&format!("[camera] set_on_tap: key={}", key));
    })
}
