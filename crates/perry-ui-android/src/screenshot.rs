//! Screenshot capture for Android (behind geisterhand feature).
//!
//! Uses JNI to capture the root View as a PNG bitmap.
//! Draws the root view onto a Canvas-backed Bitmap, compresses to PNG,
//! and returns the raw bytes via a malloc'd buffer.

use jni::objects::{JObject, JValue};

use crate::jni_bridge;
use crate::widgets;

#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(out_len: *mut usize) -> *mut u8 {
    unsafe {
        *out_len = 0;
    }

    let mut env = match std::panic::catch_unwind(|| jni_bridge::get_env()) {
        Ok(env) => env,
        Err(_) => return std::ptr::null_mut(),
    };

    let _ = env.push_local_frame(32);

    let result = (|| -> Option<Vec<u8>> {
        // Get the Activity
        let activity = widgets::get_activity(&mut env);
        if activity.is_null() {
            return None;
        }

        // Get the root view: activity.getWindow().getDecorView().getRootView()
        let window = env
            .call_method(&activity, "getWindow", "()Landroid/view/Window;", &[])
            .ok()?
            .l()
            .ok()?;
        if window.is_null() {
            return None;
        }

        let decor_view = env
            .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])
            .ok()?
            .l()
            .ok()?;
        if decor_view.is_null() {
            return None;
        }

        let root_view = env
            .call_method(&decor_view, "getRootView", "()Landroid/view/View;", &[])
            .ok()?
            .l()
            .ok()?;
        if root_view.is_null() {
            return None;
        }

        // Get view dimensions
        let width = env
            .call_method(&root_view, "getWidth", "()I", &[])
            .ok()?
            .i()
            .ok()?;
        let height = env
            .call_method(&root_view, "getHeight", "()I", &[])
            .ok()?
            .i()
            .ok()?;
        if width <= 0 || height <= 0 {
            return None;
        }

        // Create a Bitmap: Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        let bitmap_cls = env.find_class("android/graphics/Bitmap").ok()?;
        let config_cls = env.find_class("android/graphics/Bitmap$Config").ok()?;
        let argb_config = env
            .get_static_field(&config_cls, "ARGB_8888", "Landroid/graphics/Bitmap$Config;")
            .ok()?
            .l()
            .ok()?;

        let bitmap = env
            .call_static_method(
                &bitmap_cls,
                "createBitmap",
                "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
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
        let canvas_cls = env.find_class("android/graphics/Canvas").ok()?;
        let canvas = env
            .new_object(
                &canvas_cls,
                "(Landroid/graphics/Bitmap;)V",
                &[JValue::Object(&bitmap)],
            )
            .ok()?;

        let _ = env.call_method(
            &root_view,
            "draw",
            "(Landroid/graphics/Canvas;)V",
            &[JValue::Object(&canvas)],
        );

        // Clear any exception from draw (some views may throw)
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }

        // Compress bitmap to PNG: bitmap.compress(CompressFormat.PNG, 100, outputStream)
        let baos_cls = env.find_class("java/io/ByteArrayOutputStream").ok()?;
        let baos = env.new_object(&baos_cls, "()V", &[]).ok()?;

        let compress_format_cls = env
            .find_class("android/graphics/Bitmap$CompressFormat")
            .ok()?;
        let png_format = env
            .get_static_field(
                &compress_format_cls,
                "PNG",
                "Landroid/graphics/Bitmap$CompressFormat;",
            )
            .ok()?
            .l()
            .ok()?;

        let _ = env.call_method(
            &bitmap,
            "compress",
            "(Landroid/graphics/Bitmap$CompressFormat;ILjava/io/OutputStream;)Z",
            &[
                JValue::Object(&png_format),
                JValue::Int(100),
                JValue::Object(&baos),
            ],
        );

        // Get byte array from ByteArrayOutputStream and convert to Vec<u8>
        let byte_array_obj = env
            .call_method(&baos, "toByteArray", "()[B", &[])
            .ok()?
            .l()
            .ok()?;
        if byte_array_obj.is_null() {
            return None;
        }

        let byte_array: jni::objects::JByteArray = byte_array_obj.into();
        let data = env.convert_byte_array(byte_array).ok()?;

        // Recycle the bitmap to free native memory
        let _ = env.call_method(&bitmap, "recycle", "()V", &[]);

        // Clear any lingering JNI exception
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }

        if data.is_empty() {
            None
        } else {
            Some(data)
        }
    })();

    unsafe {
        env.pop_local_frame(&JObject::null());
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
}
