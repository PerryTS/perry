//! Deep links (issue #583) — Android implementation.
//!
//! Delegates to PerryBridge.kt, which owns the Activity lifecycle. The
//! Kotlin side is the source of truth for:
//!
//!   - the launch URL (extracted from `getIntent().getData()` in
//!     `onCreate`),
//!   - foreground URL deliveries (from `onNewIntent`),
//!   - the registered handler key (set via `appOnOpenUrl` JNI call —
//!     replaces the previous handler, single-sink semantics matching
//!     iOS / macOS).
//!
//! The Kotlin side calls `nativeInvokeDeepLinkCallback(key, url, source)`
//! whenever a URL needs to fire the handler — including replaying a
//! cold-start URL once the JS side has registered its handler.
//!
//! Apps need an `<intent-filter>` in AndroidManifest.xml for the OS to
//! deliver URLs at all. The build-time `perry.deepLinks` config in
//! `package.json` generates these filters automatically when the
//! Android template is materialized in `build_and_run_android`.

use crate::callback;
use crate::jni_bridge;
use jni::JValue;

/// Register the deep-link handler. Stores the closure-key on the Kotlin
/// side. Setting a fresh handler replaces the previous one. If a URL
/// arrived before the handler was registered (cold-start) the Kotlin side
/// replays it through `nativeInvokeDeepLinkCallback` immediately.
pub fn set_handler(callback: f64) {
    let key = callback::register(callback);

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let _ = env.call_static_method(
            bridge_cls,
            jni::jni_str!("appOnOpenUrl"),
            jni::jni_sig!("(J)V"),
            &[JValue::Long(key)],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }
    })
}

/// Read the cold-start URL (or `""` if the app was launched without one).
/// Walks across to the Kotlin side, which owns the `intent.data` snapshot.
pub fn launch_url() -> String {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 8);

        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(&c.perry_bridge_class).unwrap());
        let bridge_cls: &jni::objects::JClass = &bridge_class;
        let result_str: String = match env.call_static_method(
            bridge_cls,
            jni::jni_str!("appGetLaunchUrl"),
            jni::jni_sig!("()Ljava/lang/String;"),
            &[],
        ) {
            Ok(v) => match v.l() {
                Ok(obj) => {
                    let jstr: jni::objects::JString =
                        unsafe { jni::objects::JString::from_raw(env, obj.into_raw()) };
                    jstr.try_to_string(env)
                        .map(|s| s.into())
                        .unwrap_or_default()
                }
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        };

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &jni::objects::JObject::null());
        }

        result_str
    })
}
