//! Android RichText editor — issue #478. Backed by `android.widget.EditText`
//! with `android.text.SpannableStringBuilder` storage. Bold/italic/underline
//! toggles apply `StyleSpan` / `UnderlineSpan` to the current selection
//! (or insertion point) on the Kotlin side. HTML round-trips through
//! `android.text.Html.fromHtml(s, FROM_HTML_MODE_COMPACT)` and
//! `Html.toHtml(spanned, TO_HTML_PARAGRAPH_LINES_CONSECUTIVE)`.
//!
//! All PerryBridge.kt helpers under the `richText*` prefix run on the UI
//! thread; the Rust side simply forwards calls. set_html returns an i64
//! status (1 = ok, 0 = invalid handle / parse failure) to match the
//! macOS/iOS twin's contract.

use crate::app::str_from_header;
use crate::jni_bridge;
use jni::objects::{JObject, JValue};

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

fn call_bridge_void(name: &str, sig: &str, args: &[JValue]) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(bridge_cls, name, sig, args);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
}

/// Create a rich-text EditText. Note that `width`/`height` here are layout
/// hints; the actual LayoutParams are applied by the parent stack when this
/// widget is added.
pub fn create(width: f64, height: f64, on_change: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let cb_key = if on_change != 0.0 {
        crate::callback::register(on_change)
    } else {
        0
    };
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "richTextCreate",
        "(DDJ)Landroid/widget/EditText;",
        &[
            JValue::Double(width),
            JValue::Double(height),
            JValue::Long(cb_key),
        ],
    );
    let handle = match result {
        Ok(jv) => match jv.l() {
            Ok(obj) if !obj.is_null() => {
                let g = env.new_global_ref(obj).expect("global-ref RichText");
                super::register_widget(g)
            }
            _ => 0,
        },
        Err(_) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            0
        }
    };
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    handle
}

/// Replace the entire content with a plain string.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jtext = env.new_string(text).expect("rich_text set_string text");
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "richTextSetString",
            "(Landroid/widget/EditText;Ljava/lang/String;)V",
            &[JValue::Object(view.as_obj()), JValue::Object(&jtext)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Read the current plain-text content out, returning a NaN-boxed Perry string.
pub fn get_string(handle: i64) -> f64 {
    let Some(view) = super::get_widget(handle) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "richTextGetString",
        "(Landroid/widget/EditText;)Ljava/lang/String;",
        &[JValue::Object(view.as_obj())],
    );
    let text: Option<String> = match result {
        Ok(jv) => match jv.l() {
            Ok(obj) if !obj.is_null() => {
                let jstr: jni::objects::JString = obj.into();
                env.get_string(&jstr).map(|s| s.into()).ok()
            }
            _ => None,
        },
        Err(_) => None,
    };
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    match text {
        Some(s) => {
            let bytes = s.as_bytes();
            unsafe {
                let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                js_nanbox_string(p as i64)
            }
        }
        None => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Parse HTML via `Html.fromHtml(s, FROM_HTML_MODE_COMPACT)` and set it as
/// the EditText content. Returns 1 on success, 0 on invalid handle.
pub fn set_html(handle: i64, html_ptr: *const u8) -> i64 {
    let html = str_from_header(html_ptr);
    let Some(view) = super::get_widget(handle) else {
        return 0;
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let jhtml = env.new_string(html).expect("rich_text html string");
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "richTextSetHtml",
        "(Landroid/widget/EditText;Ljava/lang/String;)V",
        &[JValue::Object(view.as_obj()), JValue::Object(&jhtml)],
    );
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
        return 0;
    }
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    1
}

/// Serialize the current rich content as HTML via
/// `Html.toHtml(spanned, TO_HTML_PARAGRAPH_LINES_CONSECUTIVE)`, returning a
/// NaN-boxed Perry string.
pub fn get_html(handle: i64) -> f64 {
    let Some(view) = super::get_widget(handle) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "richTextGetHtml",
        "(Landroid/widget/EditText;)Ljava/lang/String;",
        &[JValue::Object(view.as_obj())],
    );
    let html: Option<String> = match result {
        Ok(jv) => match jv.l() {
            Ok(obj) if !obj.is_null() => {
                let jstr: jni::objects::JString = obj.into();
                env.get_string(&jstr).map(|s| s.into()).ok()
            }
            _ => None,
        },
        Err(_) => None,
    };
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    match html {
        Some(s) => {
            let bytes = s.as_bytes();
            unsafe {
                let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                js_nanbox_string(p as i64)
            }
        }
        None => f64::from_bits(TAG_UNDEFINED),
    }
}

pub fn toggle_bold(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        call_bridge_void(
            "richTextToggleBold",
            "(Landroid/widget/EditText;)V",
            &[JValue::Object(view.as_obj())],
        );
    }
}

pub fn toggle_italic(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        call_bridge_void(
            "richTextToggleItalic",
            "(Landroid/widget/EditText;)V",
            &[JValue::Object(view.as_obj())],
        );
    }
}

pub fn toggle_underline(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        call_bridge_void(
            "richTextToggleUnderline",
            "(Landroid/widget/EditText;)V",
            &[JValue::Object(view.as_obj())],
        );
    }
}
