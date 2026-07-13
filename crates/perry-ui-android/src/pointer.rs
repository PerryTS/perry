//! Continuous pointer events for perry/ui on Android (issue #1868).
//!
//! Bridges to `PerryBridge.setOnPointerCallbacks(view, downKey, moveKey, upKey)`
//! which installs a `View.OnTouchListener` that routes `ACTION_DOWN`,
//! `ACTION_MOVE`, `ACTION_UP`, and `ACTION_CANCEL` (mapped to UP) back
//! into native via the `nativeInvokePointerCallback(key, x, y, button)`
//! external method.
//!
//! Coordinates from `MotionEvent` are device pixels — the Kotlin side
//! divides by `displayMetrics.density` before crossing the JNI boundary
//! so the JS callback receives widget-local *points* (top-left origin).
//!
//! `button` is always `0` on touch; the same dispatcher will surface
//! `BUTTON_*` masks for stylus / mouse input on Chromebooks / DeX in a
//! follow-up.

use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_pointer_event_new(x: f64, y: f64, button: u32, pointer_type: u32) -> f64;
}

const POINTER_TYPE_TOUCH: u32 = 1;

/// Called from the Kotlin `OnTouchListener` for every motion event.
/// `key` indexes the per-phase callback registered in
/// [`crate::callback`]. `button`: web-style 0/1/2 (always 0 on a
/// finger touch).
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokePointerCallback(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    x: jni::sys::jdouble,
    y: jni::sys::jdouble,
    button: jni::sys::jint,
) {
    let closure_f64 = callback::get(key as i64);
    let Some(closure_f64) = closure_f64 else {
        return;
    };
    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    unsafe {
        let pe = js_pointer_event_new(x, y, button.max(0) as u32, POINTER_TYPE_TOUCH);
        js_closure_call1(closure_ptr, pe);
    }
}

fn install_touch_listener(handle: i64, down_key: i64, move_key: i64, up_key: i64) {
    let Some(view_ref) = crate::widgets::get_widget(handle) else {
        return;
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setOnPointerCallbacks",
        "(Landroid/view/View;JJJ)V",
        &[
            JValue::Object(view_ref.as_obj()),
            JValue::Long(down_key),
            JValue::Long(move_key),
            JValue::Long(up_key),
        ],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Per-widget keys for the three callback slots. We keep them across
    /// repeated `set_on_*` calls so re-registering only updates the
    /// closure pointed to by the existing key.
    static KEYS: RefCell<HashMap<i64, (i64, i64, i64)>> = RefCell::new(HashMap::new());
}

fn ensure_keys(handle: i64) -> (i64, i64, i64) {
    KEYS.with(|m| {
        if let Some(&keys) = m.borrow().get(&handle) {
            return keys;
        }
        // Sentinel zero closures — register a no-op f64 that will be
        // overwritten by the real callback on first `set_on_*`. We
        // can't pre-allocate sensibly without a real closure, so we
        // register the same closure for now and let it be replaced.
        let down = callback::register(0.0);
        let mov = callback::register(0.0);
        let up = callback::register(0.0);
        let keys = (down, mov, up);
        m.borrow_mut().insert(handle, keys);
        install_touch_listener(handle, down, mov, up);
        keys
    })
}

pub fn set_on_mouse_down(handle: i64, callback_f64: f64) {
    let (down, _, _) = ensure_keys(handle);
    callback::replace(down, callback_f64);
}

pub fn set_on_mouse_up(handle: i64, callback_f64: f64) {
    let (_, _, up) = ensure_keys(handle);
    callback::replace(up, callback_f64);
}

pub fn set_on_mouse_move(handle: i64, callback_f64: f64) {
    let (_, mov, _) = ensure_keys(handle);
    callback::replace(mov, callback_f64);
}
