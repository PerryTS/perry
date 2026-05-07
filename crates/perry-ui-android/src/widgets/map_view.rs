//! Android MapView widget — issue #517. Backed by Google Maps SDK MapView
//! through PerryBridge.kt static helpers (`mapViewCreate` / `mapViewSetRegion`
//! / `mapViewAddPin` / `mapViewClearPins` / `mapViewSetMapType`).
//! PerryActivity forwards onResume / onPause / onLowMemory / onDestroy
//! to `PerryBridge.forwardMapsLifecycle` so MapView's lifecycle stays
//! correct.

use crate::app::str_from_header;
use crate::jni_bridge;
use jni::objects::{JObject, JValue};

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

unsafe fn call_bridge_static_object<'a>(
    env: &mut jni::JNIEnv<'a>,
    name: &str,
    sig: &str,
    args: &[JValue],
) -> Option<JObject<'a>> {
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let cls: &jni::objects::JClass = (&bridge_class).into();
    match env.call_static_method(cls, name, sig, args) {
        Ok(jvalue) => jvalue.l().ok(),
        Err(e) => {
            let msg = format!("PerryBridge.{} failed: {:?}\0", name, e);
            __android_log_print(6, b"PerryMap\0".as_ptr(), b"%s\0".as_ptr(), msg.as_ptr());
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            None
        }
    }
}

unsafe fn call_bridge_static_void(
    env: &mut jni::JNIEnv,
    name: &str,
    sig: &str,
    args: &[JValue],
) {
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let cls: &jni::objects::JClass = (&bridge_class).into();
    if let Err(e) = env.call_static_method(cls, name, sig, args) {
        let msg = format!("PerryBridge.{} failed: {:?}\0", name, e);
        __android_log_print(6, b"PerryMap\0".as_ptr(), b"%s\0".as_ptr(), msg.as_ptr());
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
    }
}

pub fn create(width: f64, height: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let map_view = unsafe {
        call_bridge_static_object(
            &mut env,
            "mapViewCreate",
            "(DD)Lcom/google/android/gms/maps/MapView;",
            &[JValue::Double(width), JValue::Double(height)],
        )
    };
    let handle = match map_view {
        Some(obj) => {
            let global = env
                .new_global_ref(obj)
                .expect("Failed to global-ref MapView");
            super::register_widget(global)
        }
        None => 0,
    };
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    handle
}

pub fn set_region(handle: i64, lat: f64, lon: f64, lat_span: f64, lon_span: f64) {
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(4);
        unsafe {
            call_bridge_static_void(
                &mut env,
                "mapViewSetRegion",
                "(Lcom/google/android/gms/maps/MapView;DDDD)V",
                &[
                    JValue::Object(view.as_obj()),
                    JValue::Double(lat),
                    JValue::Double(lon),
                    JValue::Double(lat_span),
                    JValue::Double(lon_span),
                ],
            );
            env.pop_local_frame(&JObject::null());
        }
    }
}

pub fn add_pin(handle: i64, lat: f64, lon: f64, title_ptr: *const u8) {
    if let Some(view) = super::get_widget(handle) {
        let title = str_from_header(title_ptr);
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(4);
        let jstr = match env.new_string(title) {
            Ok(s) => s,
            Err(_) => return,
        };
        unsafe {
            call_bridge_static_void(
                &mut env,
                "mapViewAddPin",
                "(Lcom/google/android/gms/maps/MapView;DDLjava/lang/String;)V",
                &[
                    JValue::Object(view.as_obj()),
                    JValue::Double(lat),
                    JValue::Double(lon),
                    JValue::Object(&jstr),
                ],
            );
            env.pop_local_frame(&JObject::null());
        }
    }
}

pub fn clear_pins(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(4);
        unsafe {
            call_bridge_static_void(
                &mut env,
                "mapViewClearPins",
                "(Lcom/google/android/gms/maps/MapView;)V",
                &[JValue::Object(view.as_obj())],
            );
            env.pop_local_frame(&JObject::null());
        }
    }
}

pub fn set_map_type(handle: i64, style: i64) {
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(4);
        unsafe {
            call_bridge_static_void(
                &mut env,
                "mapViewSetMapType",
                "(Lcom/google/android/gms/maps/MapView;J)V",
                &[JValue::Object(view.as_obj()), JValue::Long(style)],
            );
            env.pop_local_frame(&JObject::null());
        }
    }
}
