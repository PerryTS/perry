//! Android RichTooltip — issue #479. Long-press on the trigger View pops up
//! a `PopupWindow` hosting the content widget subtree (resolved by handle
//! via the WIDGETS registry). Touch-outside dismisses the popup, matching
//! the macOS `NSTooltipManager` twin's "click anywhere to dismiss" UX.
//!
//! Android has no hover model on touch devices, so `hover_delay_ms` from
//! the cross-platform API is ignored — the system long-press duration
//! (~500 ms) is used instead. The constraint is documented at the FFI
//! site in lib.rs.

use crate::jni_bridge;
use jni::objects::{JObject, JValue};

/// Attach a rich tooltip to `widget_handle` showing the subtree rooted at
/// `content_handle`. The popup auto-dismisses on outside touch.
pub fn set_rich_tooltip(widget_handle: i64, content_handle: i64, _hover_delay_ms: f64) {
    let Some(trigger) = super::get_widget(widget_handle) else {
        return;
    };
    let Some(content) = super::get_widget(content_handle) else {
        return;
    };

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setRichTooltip",
        "(Landroid/view/View;Landroid/view/View;)V",
        &[
            JValue::Object(trigger.as_obj()),
            JValue::Object(content.as_obj()),
        ],
    );
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    unsafe {
        env.pop_local_frame(&JObject::null());
    }
}
