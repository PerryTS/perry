//! Android Calendar widget — issue #481. Backed by `android.widget.CalendarView`
//! through the PerryBridge.kt static helpers (`calendarCreate`,
//! `calendarSetDate`, `calendarGetSelectedDate`). The Kotlin side wires the
//! native `OnDateChangeListener` to `nativeInvokeCallbackWithString(key, iso)`
//! where `iso` is `yyyy-MM-dd`; the Rust side NaN-boxes it as a Perry string,
//! matching the macOS / gtk4 twin behavior in
//! `crates/perry-ui-macos/src/widgets/calendar.rs`.

use crate::callback;
use crate::jni_bridge;
use jni::objects::{JObject, JValue};

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Create a `CalendarView` with the given initial year/month (month is 1-based).
/// `on_change` is the Perry closure invoked with a `yyyy-MM-dd` string each
/// time the user picks a new date.
pub fn create(year: i64, month: i64, on_change: f64) -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let cb_key = if on_change != 0.0 {
        callback::register(on_change)
    } else {
        0
    };

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    let result = env.call_static_method(
        bridge_cls,
        "calendarCreate",
        "(JJJ)Landroid/widget/CalendarView;",
        &[
            JValue::Long(year),
            JValue::Long(month),
            JValue::Long(cb_key),
        ],
    );

    let handle = match result {
        Ok(jv) => match jv.l() {
            Ok(obj) if !obj.is_null() => {
                let g = env
                    .new_global_ref(obj)
                    .expect("Failed to global-ref CalendarView");
                super::register_widget(g)
            }
            _ => 0,
        },
        Err(e) => {
            unsafe {
                let msg = format!("calendarCreate failed: {:?}\0", e);
                __android_log_print(
                    6,
                    b"PerryCalendar\0".as_ptr(),
                    b"%s\0".as_ptr(),
                    msg.as_ptr(),
                );
                if env.exception_check().unwrap_or(false) {
                    let _ = env.exception_describe();
                    let _ = env.exception_clear();
                }
            }
            0
        }
    };

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    handle
}

/// Programmatically set the selected date (year, 1-based month, day).
pub fn set_date(handle: i64, year: i64, month: i64, day: i64) {
    if let Some(view) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "calendarSetDate",
            "(Landroid/widget/CalendarView;JJJ)V",
            &[
                JValue::Object(view.as_obj()),
                JValue::Long(year),
                JValue::Long(month),
                JValue::Long(day),
            ],
        );
        unsafe {
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Get the selected date as a NaN-boxed Perry string (`yyyy-MM-dd`). Returns
/// undefined if the widget handle is unknown or JNI errors.
pub fn get_selected_date(handle: i64) -> f64 {
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
        "calendarGetSelectedDate",
        "(Landroid/widget/CalendarView;)Ljava/lang/String;",
        &[JValue::Object(view.as_obj())],
    );
    let date_str: Option<String> = match result {
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
    match date_str {
        Some(s) if !s.is_empty() => {
            let bytes = s.as_bytes();
            unsafe {
                let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                js_nanbox_string(ptr as i64)
            }
        }
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
