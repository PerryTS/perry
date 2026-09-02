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
use jni::JValue;

use perry_ffi::copy_string_from_raw as str_from_header;

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
    let _unit_id = unsafe { str_from_header(unit_id_ptr) };
    let size_key = unsafe { str_from_header(size_ptr) };
    let (w_dp, h_dp) = banner_size_dp(&size_key);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);
        let activity = super::get_activity(env);

        let view = env
            .new_object(
                jni::jni_str!("android/view/View"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("Failed to create View");

        let width_px = super::dp_to_px(env, w_dp);
        let height_px = super::dp_to_px(env, h_dp);
        let params = env
            .new_object(
                jni::jni_str!("android/widget/LinearLayout$LayoutParams"),
                jni::jni_sig!("(II)V"),
                &[JValue::Int(width_px), JValue::Int(height_px)],
            )
            .expect("Failed to create LayoutParams");
        let _ = env.call_method(
            &view,
            jni::jni_str!("setLayoutParams"),
            jni::jni_sig!("(Landroid/view/ViewGroup$LayoutParams;)V"),
            &[JValue::Object(&params)],
        );

        let global = jni_bridge::new_global_ref(env, view).expect("Failed to create global ref");
        let handle = super::register_widget(global);
        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
        handle
    })
}
