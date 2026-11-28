//! System Back (button or gesture) → the app's `onBackPressed` handler.
//!
//! Without a handler Android's default applies (the activity finishes). With
//! one, `PerryActivity` asks the handler first: a truthy return means the app
//! handled Back (popped its own navigation); falsy falls through to the
//! default.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::callback;

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call0(closure: *const u8, this: perry_ffi::JsThis) -> f64;
    fn js_is_truthy(value: f64) -> i32;
    fn js_promise_run_microtasks() -> i32;
}

/// Key of the registered handler in the (GC-rooted) callback store; 0 = none.
static BACK_KEY: AtomicI64 = AtomicI64::new(0);

pub fn set_on_back(cb: f64) {
    let key = BACK_KEY.load(Ordering::Acquire);
    if key != 0 {
        callback::replace(key, cb);
    } else {
        BACK_KEY.store(callback::register(cb), Ordering::Release);
    }
}

/// JNI entry point, on the UI thread. Returns true when the app handled Back.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeBackPressed(
    _env: jni::EnvUnowned,
    _class: jni::objects::JClass,
) -> jni::sys::jboolean {
    let key = BACK_KEY.load(Ordering::Acquire);
    if key == 0 {
        return false;
    }
    let Some(closure) = callback::get(key) else {
        return false;
    };
    let handled = unsafe {
        let ptr = js_nanbox_get_pointer(closure) as *const u8;
        let result = js_closure_call0(ptr, perry_ffi::JsThis::UNDEFINED);
        js_is_truthy(result) != 0
    };
    unsafe { while js_promise_run_microtasks() > 0 {} }
    handled
}
