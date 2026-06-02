//! Android toast presenter for `showToast(msg)` (Phase 2 v3.3).
//!
//! Bridges the cross-platform `perry_arkts_show_toast` handler registry
//! (perry-runtime/src/ui_text_registry.rs) to Android's one-shot
//! `android.widget.Toast`. Toast sequencing on Android is handled natively
//! by the system; no per-process FIFO queue is needed here.
//!
//! ## Wiring
//!
//! `app::register_cross_platform_text_handlers` calls
//! `js_register_show_toast_handler` at app startup, passing
//! `show_toast_handler` here as the registered fn pointer. When user TS
//! code calls `showToast("Saved!")`, the codegen emits
//! `perry_arkts_show_toast`; the runtime decodes the NaN-boxed string and
//! forwards to this handler.
//!
//! `Toast.show()` must be called on the UI thread. The call is posted via
//! `PerryBridge.showToast(String)`, whose Kotlin implementation posts to
//! the UI thread's `Handler(Looper.getMainLooper())`.

use jni::objects::JValue;

/// Cross-platform handler entry point. Registered with
/// `js_register_show_toast_handler` at app startup.
/// Forwards the message to `PerryBridge.showToast(String)` on the UI thread.
pub extern "C" fn show_toast_handler(msg_ptr: *const u8, msg_len: usize) {
    if msg_ptr.is_null() {
        return;
    }
    let msg = unsafe {
        let bytes = std::slice::from_raw_parts(msg_ptr, msg_len);
        String::from_utf8_lossy(bytes).into_owned()
    };

    let mut env = crate::jni_bridge::get_env();
    let _ = env.push_local_frame(8);

    if let Ok(jstr) = env.new_string(&msg) {
        let bridge_class = crate::jni_bridge::with_cache(|c| {
            env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
        });
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "showToast",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&jstr)],
        );
    }

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}
