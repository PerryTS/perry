//! BloomView — a native render-surface host widget (issue #2395 / #5519).
//!
//! Reserves an `android.view.SurfaceView` in the Perry UI view tree for an
//! external GPU renderer (e.g. the Bloom engine) to draw into. Perry UI only
//! owns the view; `bloomViewGetNativeHandle` returns the `ANativeWindow*` of the
//! view's `Surface` (via `ANativeWindow_fromSurface`), which user TypeScript
//! hands to the engine's `attachToSurface` → `bloom_attach_native`.
//!
//! Note: a `SurfaceView`'s `Surface` only becomes valid once it's laid out and
//! `surfaceCreated` has fired, so `get_native_handle` returns 0 until then — the
//! host should attach after the view is on screen (or retry).

use crate::jni_bridge;
use jni::objects::JValue;
use std::sync::Mutex;

// NDK libandroid: turn a Java `android.view.Surface` into a native window the
// GPU backend (wgpu) can build a swapchain on.
#[link(name = "android")]
extern "C" {
    fn ANativeWindow_fromSurface(
        env: *mut jni::sys::JNIEnv,
        surface: jni::sys::jobject,
    ) -> *mut std::ffi::c_void;
}

// `ANativeWindow_fromSurface` returns a window with a +1 reference, so creating
// one per `get_native_handle` call (the host polls it every frame until the
// surface is ready) would leak a reference each time. Cache the first window we
// build per widget handle and hand the same pointer back on later calls — the
// single retained reference lives for the BloomView's lifetime. Keyed by the
// registry handle; a small Vec since an app has at most a handful of BloomViews.
static BLOOM_WINDOWS: Mutex<Vec<(i64, i64)>> = Mutex::new(Vec::new());

/// Create a BloomView host sized `width` × `height` dp. Returns the widget
/// handle, or 0 on JNI failure.
pub fn create(width: f64, height: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    let activity = super::get_activity(&mut env);
    let view = match env.new_object(
        "android/view/SurfaceView",
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

    // Let the host view take focus so the attached engine can route key/touch.
    let _ = env.call_method(&view, "setFocusable", "(Z)V", &[JValue::Bool(1)]);
    let _ = env.call_method(&view, "setFocusableInTouchMode", "(Z)V", &[JValue::Bool(1)]);

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

    let handle = super::register_widget(global_ref);
    if width.is_finite() && width >= 1.0 {
        super::set_width(handle, width);
    }
    if height.is_finite() && height >= 1.0 {
        super::set_height(handle, height);
    }
    handle
}

/// Return the `ANativeWindow*` of the BloomView's `Surface` as an integer, for
/// handing to an external GPU renderer. Returns 0 if the handle is unknown or
/// the surface isn't ready yet (not laid out / `surfaceCreated` not fired).
///
/// The `ANativeWindow*` is created once per BloomView (on the first call that
/// finds a ready surface) and cached; later calls return the same pointer, so
/// the host polling this every frame doesn't leak a window reference per call.
/// The cached reference is released when the engine's attach takes over and on
/// process teardown.
pub fn get_native_handle(handle: i64) -> i64 {
    // Return the window we already built for this view, if any.
    if let Ok(cache) = BLOOM_WINDOWS.lock() {
        if let Some(&(_, win)) = cache.iter().find(|&&(h, _)| h == handle) {
            return win;
        }
    }
    let Some(view_ref) = super::get_widget(handle) else {
        return 0;
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    let result = (|| -> Option<i64> {
        // holder = surfaceView.getHolder()
        let holder = env
            .call_method(
                view_ref.as_obj(),
                "getHolder",
                "()Landroid/view/SurfaceHolder;",
                &[],
            )
            .ok()?
            .l()
            .ok()?;
        // surface = holder.getSurface()
        let surface = env
            .call_method(&holder, "getSurface", "()Landroid/view/Surface;", &[])
            .ok()?
            .l()
            .ok()?;
        // Bail unless the surface is backed by a live buffer queue.
        let valid = env
            .call_method(&surface, "isValid", "()Z", &[])
            .ok()?
            .z()
            .ok()?;
        if !valid {
            return None;
        }
        let env_raw = env.get_native_interface();
        let win = unsafe { ANativeWindow_fromSurface(env_raw, surface.as_raw()) };
        if win.is_null() {
            None
        } else {
            Some(win as i64)
        }
    })();

    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }
    // Cache the window so the next poll reuses this reference instead of
    // acquiring a fresh one (only on success — keep retrying while not ready).
    if let Some(win) = result {
        if let Ok(mut cache) = BLOOM_WINDOWS.lock() {
            if !cache.iter().any(|&(h, _)| h == handle) {
                cache.push((handle, win));
            }
        }
        return win;
    }
    0
}
