//! Android Chart widget — issue #474. Backed by a custom `android.view.View`
//! subclass `PerryChartView` declared inline in PerryBridge.kt. The view's
//! `onDraw(Canvas)` dispatches on a `kind` integer (0=line, 1=bar, 2=pie)
//! and renders the data points stored in `MutableList<Pair<String, Double>>`.
//!
//! Data points are pushed across via three FFI helpers — `chartAddDataPoint`,
//! `chartClearData`, `chartSetTitle` — and Rust calls `chartReload` to
//! request a redraw. The Rust side intentionally does not hold a parallel
//! copy of the data: PerryChartView owns it.

use crate::app::str_from_header;
use crate::jni_bridge;
use jni::objects::{JObject, JValue};

/// Create a PerryChartView with the requested kind (0=line, 1=bar, 2=pie)
/// and layout size hint.
pub fn create(kind: i64, width: f64, height: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "chartCreate",
        "(JDD)Landroid/view/View;",
        &[
            JValue::Long(kind),
            JValue::Double(width),
            JValue::Double(height),
        ],
    );
    let handle = match result {
        Ok(jv) => match jv.l() {
            Ok(obj) if !obj.is_null() => {
                let g = env.new_global_ref(obj).expect("global-ref Chart");
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

pub fn add_data_point(handle: i64, label_ptr: *const u8, value: f64) {
    let label = str_from_header(label_ptr);
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jlabel = env.new_string(label).expect("chart label string");
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "chartAddDataPoint",
            "(Landroid/view/View;Ljava/lang/String;D)V",
            &[
                JValue::Object(view.as_obj()),
                JValue::Object(&jlabel),
                JValue::Double(value),
            ],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

pub fn clear_data(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "chartClearData",
            "(Landroid/view/View;)V",
            &[JValue::Object(view.as_obj())],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jtitle = env.new_string(title).expect("chart title string");
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "chartSetTitle",
            "(Landroid/view/View;Ljava/lang/String;)V",
            &[JValue::Object(view.as_obj()), JValue::Object(&jtitle)],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

pub fn reload(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        // PerryChartView extends View; invalidate() schedules onDraw.
        let _ = env.call_method(view.as_obj(), "postInvalidate", "()V", &[]);
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}
