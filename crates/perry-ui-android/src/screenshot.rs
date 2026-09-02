//! Screenshot capture for Android (behind geisterhand feature).
//!
//! Uses JNI to capture the root View as a PNG bitmap.
//! Draws the root view onto a Canvas-backed Bitmap, compresses to PNG,
//! and returns the raw bytes via a malloc'd buffer.

use jni::objects::JObject;
use jni::JValue;

use crate::jni_bridge;
use crate::widgets;

#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(out_len: *mut usize) -> *mut u8 {
    unsafe {
        *out_len = 0;
    }

    if std::panic::catch_unwind(jni_bridge::get_vm).is_err() {
        return std::ptr::null_mut();
    }

    jni_bridge::try_with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        let result = (|| -> Option<Vec<u8>> {
            // Get the Activity
            let activity = widgets::get_activity(env);
            if activity.is_null() {
                return None;
            }

            // Get the root view: activity.getWindow().getDecorView().getRootView()
            let window = env
                .call_method(
                    &activity,
                    jni::jni_str!("getWindow"),
                    jni::jni_sig!("()Landroid/view/Window;"),
                    &[],
                )
                .ok()?
                .l()
                .ok()?;
            if window.is_null() {
                return None;
            }

            let decor_view = env
                .call_method(
                    &window,
                    jni::jni_str!("getDecorView"),
                    jni::jni_sig!("()Landroid/view/View;"),
                    &[],
                )
                .ok()?
                .l()
                .ok()?;
            if decor_view.is_null() {
                return None;
            }

            let root_view = env
                .call_method(
                    &decor_view,
                    jni::jni_str!("getRootView"),
                    jni::jni_sig!("()Landroid/view/View;"),
                    &[],
                )
                .ok()?
                .l()
                .ok()?;
            if root_view.is_null() {
                return None;
            }

            // Get view dimensions
            let width = env
                .call_method(
                    &root_view,
                    jni::jni_str!("getWidth"),
                    jni::jni_sig!("()I"),
                    &[],
                )
                .ok()?
                .i()
                .ok()?;
            let height = env
                .call_method(
                    &root_view,
                    jni::jni_str!("getHeight"),
                    jni::jni_sig!("()I"),
                    &[],
                )
                .ok()?
                .i()
                .ok()?;
            if width <= 0 || height <= 0 {
                return None;
            }

            // Create a Bitmap: Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
            let bitmap_cls = env
                .find_class(jni::jni_str!("android/graphics/Bitmap"))
                .ok()?;
            let config_cls = env
                .find_class(jni::jni_str!("android/graphics/Bitmap$Config"))
                .ok()?;
            let argb_config = env
                .get_static_field(
                    &config_cls,
                    jni::jni_str!("ARGB_8888"),
                    jni::jni_sig!("Landroid/graphics/Bitmap$Config;"),
                )
                .ok()?
                .l()
                .ok()?;

            let bitmap = env
                .call_static_method(
                    &bitmap_cls,
                    jni::jni_str!("createBitmap"),
                    jni::jni_sig!("(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;"),
                    &[
                        JValue::Int(width),
                        JValue::Int(height),
                        JValue::Object(&argb_config),
                    ],
                )
                .ok()?
                .l()
                .ok()?;
            if bitmap.is_null() {
                return None;
            }

            // Create a Canvas from the bitmap and draw the view onto it
            let canvas_cls = env
                .find_class(jni::jni_str!("android/graphics/Canvas"))
                .ok()?;
            let canvas = env
                .new_object(
                    &canvas_cls,
                    jni::jni_sig!("(Landroid/graphics/Bitmap;)V"),
                    &[JValue::Object(&bitmap)],
                )
                .ok()?;

            let _ = env.call_method(
                &root_view,
                jni::jni_str!("draw"),
                jni::jni_sig!("(Landroid/graphics/Canvas;)V"),
                &[JValue::Object(&canvas)],
            );

            // Clear any exception from draw (some views may throw)
            if env.exception_check() {
                let _ = env.exception_clear();
            }

            // Compress bitmap to PNG: bitmap.compress(CompressFormat.PNG, 100, outputStream)
            let baos_cls = env
                .find_class(jni::jni_str!("java/io/ByteArrayOutputStream"))
                .ok()?;
            let baos = env.new_object(&baos_cls, jni::jni_sig!("()V"), &[]).ok()?;

            let compress_format_cls = env
                .find_class(jni::jni_str!("android/graphics/Bitmap$CompressFormat"))
                .ok()?;
            let png_format = env
                .get_static_field(
                    &compress_format_cls,
                    jni::jni_str!("PNG"),
                    jni::jni_sig!("Landroid/graphics/Bitmap$CompressFormat;"),
                )
                .ok()?
                .l()
                .ok()?;

            let _ = env.call_method(
                &bitmap,
                jni::jni_str!("compress"),
                jni::jni_sig!("(Landroid/graphics/Bitmap$CompressFormat;ILjava/io/OutputStream;)Z"),
                &[
                    JValue::Object(&png_format),
                    JValue::Int(100),
                    JValue::Object(&baos),
                ],
            );

            // Get byte array from ByteArrayOutputStream and convert to Vec<u8>
            let byte_array_obj = env
                .call_method(
                    &baos,
                    jni::jni_str!("toByteArray"),
                    jni::jni_sig!("()[B"),
                    &[],
                )
                .ok()?
                .l()
                .ok()?;
            if byte_array_obj.is_null() {
                return None;
            }

            let byte_array: jni::objects::JByteArray = byte_array_obj.into();
            let data = env.convert_byte_array(byte_array).ok()?;

            // Recycle the bitmap to free native memory
            let _ = env.call_method(&bitmap, jni::jni_str!("recycle"), jni::jni_sig!("()V"), &[]);

            // Clear any lingering JNI exception
            if env.exception_check() {
                let _ = env.exception_clear();
            }

            if data.is_empty() {
                None
            } else {
                Some(data)
            }
        })();

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }

        match result {
            Some(data) => {
                let len = data.len();
                let buf = unsafe { libc::malloc(len) as *mut u8 };
                if buf.is_null() {
                    return std::ptr::null_mut();
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), buf, len);
                    *out_len = len;
                }
                buf
            }
            None => std::ptr::null_mut(),
        }
    })
    .unwrap_or(std::ptr::null_mut())
}
