//! Image — ImageView for file images and system icons

use crate::jni_bridge;
use jni::objects::JValue;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

/// Create an image from a file path.
/// For relative paths, tries the Android assets directory first (bundled in APK),
/// then falls back to BitmapFactory.decodeFile for absolute paths.
pub fn create_file(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    crate::log_debug(&format!("image create_file: path={}", path));
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    let image_view = env
        .new_object(
            "android/widget/ImageView",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create ImageView");

    // Ensure the ImageView adjusts bounds to its LayoutParams and scales content
    let _ = env.call_method(
        &image_view,
        "setAdjustViewBounds",
        "(Z)V",
        &[JValue::Bool(1)],
    );
    // ScaleType.FIT_CENTER = enum ordinal → use setScaleType with enum constant
    let scale_type_class = env
        .find_class("android/widget/ImageView$ScaleType")
        .expect("ScaleType");
    let fit_center = env
        .get_static_field(
            &scale_type_class,
            "FIT_CENTER",
            "Landroid/widget/ImageView$ScaleType;",
        )
        .expect("FIT_CENTER")
        .l()
        .expect("scale type");
    let _ = env.call_method(
        &image_view,
        "setScaleType",
        "(Landroid/widget/ImageView$ScaleType;)V",
        &[JValue::Object(&fit_center)],
    );

    let mut loaded = false;

    // For relative paths, try loading from APK assets first
    if !path.starts_with('/') {
        if let Ok(asset_mgr) = env.call_method(
            &activity,
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            &[],
        ) {
            if let Ok(mgr) = asset_mgr.l() {
                if !mgr.is_null() {
                    let jpath = env.new_string(path).expect("asset path string");
                    // AssetManager.open(path) -> InputStream
                    let stream = env.call_method(
                        &mgr,
                        "open",
                        "(Ljava/lang/String;)Ljava/io/InputStream;",
                        &[JValue::Object(&jpath)],
                    );
                    if let Ok(stream_val) = stream {
                        if let Ok(stream_obj) = stream_val.l() {
                            if !stream_obj.is_null() {
                                // BitmapFactory.decodeStream(inputStream)
                                let bitmap = env.call_static_method(
                                    "android/graphics/BitmapFactory",
                                    "decodeStream",
                                    "(Ljava/io/InputStream;)Landroid/graphics/Bitmap;",
                                    &[JValue::Object(&stream_obj)],
                                );
                                if let Ok(bmp_val) = bitmap {
                                    if let Ok(bmp) = bmp_val.l() {
                                        if !bmp.is_null() {
                                            let _ = env.call_method(
                                                &image_view,
                                                "setImageBitmap",
                                                "(Landroid/graphics/Bitmap;)V",
                                                &[JValue::Object(&bmp)],
                                            );
                                            loaded = true;
                                            crate::log_debug(
                                                "image create_file: loaded from assets",
                                            );
                                        }
                                    }
                                }
                                let _ = env.call_method(&stream_obj, "close", "()V", &[]);
                            }
                        }
                    }
                    // Clear any FileNotFoundException from assets.open()
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                }
            }
        }
    }

    // Fall back to filesystem (absolute path or if assets failed)
    if !loaded {
        let jpath = env.new_string(path).expect("Failed to create JNI string");
        let bitmap = env.call_static_method(
            "android/graphics/BitmapFactory",
            "decodeFile",
            "(Ljava/lang/String;)Landroid/graphics/Bitmap;",
            &[JValue::Object(&jpath)],
        );
        if let Ok(bmp_val) = bitmap {
            if let Ok(bmp) = bmp_val.l() {
                if !bmp.is_null() {
                    let _ = env.call_method(
                        &image_view,
                        "setImageBitmap",
                        "(Landroid/graphics/Bitmap;)V",
                        &[JValue::Object(&bmp)],
                    );
                }
            }
        }
    }

    let global = env
        .new_global_ref(image_view)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

/// Create an image from a named system icon (SF Symbol name → Material Icon).
/// Uses the same SF Symbol → Material Icons mapping as button.rs.
pub fn create_symbol(name_ptr: *const u8) -> i64 {
    let name = str_from_header(name_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);

    // Use a TextView styled as an icon (Material Icons font) since Android
    // doesn't have SF Symbols and resource-based icons require compile-time R.id
    let text_view = env
        .new_object(
            "android/widget/TextView",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create TextView for symbol");

    // Map SF Symbol name to emoji/Unicode character
    let icon_str = if let Some(emoji) = super::button::sf_symbol_to_emoji(name) {
        emoji.to_string()
    } else {
        name.chars().take(3).collect()
    };
    let jstr = env.new_string(&icon_str).expect("icon string");
    let _ = env.call_method(
        &text_view,
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&jstr)],
    );

    // Set default size (24sp)
    let _ = env.call_method(
        &text_view,
        "setTextSize",
        "(IF)V",
        &[JValue::Int(2), JValue::Float(24.0)],
    ); // COMPLEX_UNIT_SP=2

    // Center gravity
    let _ = env.call_method(&text_view, "setGravity", "(I)V", &[JValue::Int(0x11)]); // Gravity.CENTER = 0x11

    let global = env
        .new_global_ref(text_view)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

/// Set the size of an image widget.
pub fn set_size(handle: i64, width: f64, height: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);

        let w = super::dp_to_px(&mut env, width as f32);
        let h = super::dp_to_px(&mut env, height as f32);

        // Create LayoutParams
        let params = env
            .new_object(
                "android/view/ViewGroup$LayoutParams",
                "(II)V",
                &[JValue::Int(w), JValue::Int(h)],
            )
            .expect("LayoutParams");

        let _ = env.call_method(
            view_ref.as_obj(),
            "setLayoutParams",
            "(Landroid/view/ViewGroup$LayoutParams;)V",
            &[JValue::Object(&params)],
        );

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set the tint color of an image.
pub fn set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);

        let ai = (a * 255.0) as u32;
        let ri = (r * 255.0) as u32;
        let gi = (g * 255.0) as u32;
        let bi = (b * 255.0) as u32;
        let color = ((ai << 24) | (ri << 16) | (gi << 8) | bi) as i32;

        // setColorFilter(int color, PorterDuff.Mode mode)
        let mode_class = env
            .find_class("android/graphics/PorterDuff$Mode")
            .expect("PorterDuff$Mode");
        let src_in = env
            .get_static_field(&mode_class, "SRC_IN", "Landroid/graphics/PorterDuff$Mode;")
            .expect("SRC_IN")
            .l()
            .expect("mode");

        let _ = env.call_method(
            view_ref.as_obj(),
            "setColorFilter",
            "(ILandroid/graphics/PorterDuff$Mode;)V",
            &[JValue::Int(color), JValue::Object(&src_in)],
        );

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}
