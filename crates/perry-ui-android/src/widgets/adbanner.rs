//! `AdBanner` widget (#867) — Android.
//!
//! Default build: a layout placeholder `View` sized to the requested
//! banner slot so the banner reserves the right space in the layout.
//! Swapping in a live `com.google.android.gms.ads.AdView` (set
//! `adUnitId` + `AdSize`, then `loadAd(AdRequest)`) is a follow-up that
//! depends on the `play-services-ads` Gradle artifact — see the JNI
//! bridge notes in `perry-ext-ads/src/lib.rs`. `unitId` is accepted for
//! API parity but unused by the placeholder.

use crate::jni_bridge;
use jni::objects::JValue;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

/// Banner dimensions in dp per size key (matches Google Mobile Ads'
/// standard `AdSize` constants).
fn banner_size_dp(size_key: &str) -> (f32, f32) {
    match size_key {
        "large-banner" => (320.0, 100.0),
        "medium-rectangle" => (300.0, 250.0),
        "full-banner" => (468.0, 60.0),
        "leaderboard" => (728.0, 90.0),
        _ => (320.0, 50.0),
    }
}

/// Create the banner placeholder view sized per `size_ptr`.
pub fn create(unit_id_ptr: *const u8, size_ptr: *const u8) -> i64 {
    let _unit_id = str_from_header(unit_id_ptr);
    let size_key = str_from_header(size_ptr);
    let (w_dp, h_dp) = banner_size_dp(size_key);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);
    let activity = super::get_activity(&mut env);

    let view = env
        .new_object(
            "android/view/View",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("Failed to create View");

    let width_px = super::dp_to_px(&mut env, w_dp);
    let height_px = super::dp_to_px(&mut env, h_dp);
    let params = env
        .new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(II)V",
            &[JValue::Int(width_px), JValue::Int(height_px)],
        )
        .expect("Failed to create LayoutParams");
    let _ = env.call_method(
        &view,
        "setLayoutParams",
        "(Landroid/view/ViewGroup$LayoutParams;)V",
        &[JValue::Object(&params)],
    );

    let global = env
        .new_global_ref(view)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}
