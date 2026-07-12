//! System APIs — open_url, dark mode, preferences, keychain, notifications

use crate::jni_bridge;
use jni::objects::JValue;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: usize) -> *const u8;
    fn js_nanbox_string(ptr: *const u8) -> f64;
}

/// Open a URL in the default browser via Intent.ACTION_VIEW.
pub fn open_url(url_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = crate::widgets::get_activity(&mut env);

    let jurl = env.new_string(url).expect("Failed to create JNI string");
    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&jurl)],
        )
        .expect("Uri.parse")
        .l()
        .expect("uri");

    let action = env
        .new_string("android.intent.action.VIEW")
        .expect("action string");
    let intent = env
        .new_object(
            "android/content/Intent",
            "(Ljava/lang/String;Landroid/net/Uri;)V",
            &[JValue::Object(&action), JValue::Object(&uri)],
        )
        .expect("Intent");

    let _ = env.call_method(
        &activity,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&intent)],
    );

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Check if dark mode is enabled via Configuration.uiMode.
pub fn is_dark_mode() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);

    let resources = env
        .call_method(
            &activity,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        )
        .expect("getResources")
        .l()
        .expect("resources");

    let config = env
        .call_method(
            &resources,
            "getConfiguration",
            "()Landroid/content/res/Configuration;",
            &[],
        )
        .expect("getConfiguration")
        .l()
        .expect("configuration");

    let ui_mode = env
        .get_field(&config, "uiMode", "I")
        .expect("uiMode")
        .i()
        .expect("int");

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }

    // UI_MODE_NIGHT_MASK = 0x30, UI_MODE_NIGHT_YES = 0x20
    if (ui_mode & 0x30) == 0x20 {
        1
    } else {
        0
    }
}

/// Set a preference value using SharedPreferences.
pub fn preferences_set(key_ptr: *const u8, value: f64) {
    let key = str_from_header(key_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_prefs").expect("pref name");
    let prefs = env
        .call_method(
            &activity,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&pref_name), JValue::Int(0)], // MODE_PRIVATE = 0
        )
        .expect("getSharedPreferences")
        .l()
        .expect("prefs");

    let editor = env
        .call_method(
            &prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[],
        )
        .expect("edit")
        .l()
        .expect("editor");

    let jkey = env.new_string(key).expect("key string");

    // Check if value is a NaN-boxed string
    let bits = value.to_bits();
    let tag = (bits >> 48) as u16;
    if tag == 0x7FFF {
        // String value — extract and store as string
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
        let s = str_from_header(ptr);
        let jval = env.new_string(s).expect("value string");
        let _ = env.call_method(
            &editor,
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&jkey), JValue::Object(&jval)],
        );
    } else {
        // Numeric value
        let _ = env.call_method(
            &editor,
            "putFloat",
            "(Ljava/lang/String;F)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&jkey), JValue::Float(value as f32)],
        );
    }

    let _ = env.call_method(&editor, "apply", "()V", &[]);

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Get a preference value from SharedPreferences.
///
/// IMPORTANT: Android SharedPreferences stores typed values. Calling
/// `getString` on a float key (or `getFloat` on a string key) throws
/// ClassCastException and leaves a *pending* JNI exception. That poisons
/// every subsequent JNI call in `nativeMain` (text/hstack create panics,
/// half the UI vanishes). Always read via `getAll` and branch on type, and
/// clear any leftover exception before returning.
pub fn preferences_get(key_ptr: *const u8) -> f64 {
    let key = str_from_header(key_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(24);

    let finish = |env: &mut jni::JNIEnv, result: f64| -> f64 {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
        result
    };

    let activity = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::widgets::get_activity(&mut env)
    })) {
        Ok(a) => a,
        Err(_) => return finish(&mut env, 0.0),
    };

    let pref_name = match env.new_string("perry_prefs") {
        Ok(s) => s,
        Err(_) => return finish(&mut env, 0.0),
    };
    let prefs = match env.call_method(
        &activity,
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        &[JValue::Object(&pref_name), JValue::Int(0)],
    ) {
        Ok(v) => match v.l() {
            Ok(o) => o,
            Err(_) => return finish(&mut env, 0.0),
        },
        Err(_) => return finish(&mut env, 0.0),
    };

    let jkey = match env.new_string(key) {
        Ok(s) => s,
        Err(_) => return finish(&mut env, 0.0),
    };

    // getAll avoids ClassCastException from typed getters.
    let map = match env.call_method(&prefs, "getAll", "()Ljava/util/Map;", &[]) {
        Ok(v) => match v.l() {
            Ok(o) => o,
            Err(_) => return finish(&mut env, 0.0),
        },
        Err(_) => return finish(&mut env, 0.0),
    };

    let entry = match env.call_method(
        &map,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        &[JValue::Object(&jkey)],
    ) {
        Ok(v) => match v.l() {
            Ok(o) => o,
            Err(_) => return finish(&mut env, 0.0),
        },
        Err(_) => return finish(&mut env, 0.0),
    };

    if entry.is_null() {
        return finish(&mut env, 0.0);
    }

    // Branch on runtime type without throwing.
    if let Ok(true) = env.is_instance_of(&entry, "java/lang/String") {
        let jstr: jni::objects::JString = entry.into();
        if let Ok(java_str) = env.get_string(&jstr) {
            let s: String = java_str.into();
            let bytes = s.as_bytes();
            let ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len()) };
            let result = unsafe { js_nanbox_string(ptr) };
            return finish(&mut env, result);
        }
        return finish(&mut env, 0.0);
    }

    if let Ok(true) = env.is_instance_of(&entry, "java/lang/Float") {
        if let Ok(f) = env.call_method(&entry, "floatValue", "()F", &[]) {
            if let Ok(v) = f.f() {
                return finish(&mut env, v as f64);
            }
        }
        return finish(&mut env, 0.0);
    }

    if let Ok(true) = env.is_instance_of(&entry, "java/lang/Double") {
        if let Ok(d) = env.call_method(&entry, "doubleValue", "()D", &[]) {
            if let Ok(v) = d.d() {
                return finish(&mut env, v);
            }
        }
        return finish(&mut env, 0.0);
    }

    if let Ok(true) = env.is_instance_of(&entry, "java/lang/Integer") {
        if let Ok(i) = env.call_method(&entry, "intValue", "()I", &[]) {
            if let Ok(v) = i.i() {
                return finish(&mut env, v as f64);
            }
        }
        return finish(&mut env, 0.0);
    }

    if let Ok(true) = env.is_instance_of(&entry, "java/lang/Long") {
        if let Ok(l) = env.call_method(&entry, "longValue", "()J", &[]) {
            if let Ok(v) = l.j() {
                return finish(&mut env, v as f64);
            }
        }
        return finish(&mut env, 0.0);
    }

    finish(&mut env, 0.0)
}

/// Save a value to the keychain (SharedPreferences with private mode).
pub fn keychain_save(key_ptr: *const u8, value_ptr: *const u8) {
    let key = str_from_header(key_ptr);
    let value = str_from_header(value_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_keychain").expect("pref name");
    let prefs = env
        .call_method(
            &activity,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&pref_name), JValue::Int(0)],
        )
        .expect("getSharedPreferences")
        .l()
        .expect("prefs");

    let editor = env
        .call_method(
            &prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[],
        )
        .expect("edit")
        .l()
        .expect("editor");

    let jkey = env.new_string(key).expect("key string");
    let jval = env.new_string(value).expect("value string");
    let _ = env.call_method(
        &editor,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        &[JValue::Object(&jkey), JValue::Object(&jval)],
    );
    let _ = env.call_method(&editor, "apply", "()V", &[]);

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Get a value from the keychain.
pub fn keychain_get(key_ptr: *const u8) -> f64 {
    let key = str_from_header(key_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_keychain").expect("pref name");
    let prefs = env
        .call_method(
            &activity,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&pref_name), JValue::Int(0)],
        )
        .expect("getSharedPreferences")
        .l()
        .expect("prefs");

    let jkey = env.new_string(key).expect("key string");
    let result = env.call_method(
        &prefs,
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[
            JValue::Object(&jkey),
            JValue::Object(&jni::objects::JObject::null()),
        ],
    );

    let mut ret_val = f64::from_bits(0x7FFC_0000_0000_0001u64); // undefined
    if let Ok(val) = result {
        if let Ok(obj) = val.l() {
            if !obj.is_null() {
                let jstr: jni::objects::JString = obj.into();
                let s: String = env.get_string(&jstr).expect("get string").into();
                let bytes = s.as_bytes();
                let ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len()) };
                ret_val = unsafe { js_nanbox_string(ptr) };
            }
        }
    }

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    ret_val
}

/// Delete a value from the keychain.
pub fn keychain_delete(key_ptr: *const u8) {
    let key = str_from_header(key_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_keychain").expect("pref name");
    let prefs = env
        .call_method(
            &activity,
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[JValue::Object(&pref_name), JValue::Int(0)],
        )
        .expect("getSharedPreferences")
        .l()
        .expect("prefs");

    let editor = env
        .call_method(
            &prefs,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[],
        )
        .expect("edit")
        .l()
        .expect("editor");

    let jkey = env.new_string(key).expect("key string");
    let _ = env.call_method(
        &editor,
        "remove",
        "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        &[JValue::Object(&jkey)],
    );
    let _ = env.call_method(&editor, "apply", "()V", &[]);

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Play a haptic feedback effect via the system Vibrator service
/// (perry/system hapticPlay). Requires `android.permission.VIBRATE`
/// (declared in the app template's AndroidManifest.xml — a normal
/// permission, no runtime prompt).
pub fn haptic_play(type_ptr: *const u8) {
    let name = str_from_header(type_ptr);

    // VibrationEffect.createPredefined effect ids (public constants,
    // API 29+): EFFECT_CLICK=0, EFFECT_DOUBLE_CLICK=1, EFFECT_TICK=2,
    // EFFECT_HEAVY_CLICK=5.
    let effect_id: i32 = match name {
        "success" | "medium" | "start" | "stop" => 0, // EFFECT_CLICK
        "error" | "warning" => 1,                     // EFFECT_DOUBLE_CLICK (double buzz)
        "heavy" => 5,                                 // EFFECT_HEAVY_CLICK
        // light / click / selection / directionUp / directionDown /
        // unknown — the subtle tick.
        _ => 2, // EFFECT_TICK
    };
    // Duration (ms) for the pre-API-29 `vibrate(long)` fallback.
    let fallback_ms: i64 = match name {
        "error" | "warning" => 80,
        "heavy" => 60,
        "success" | "medium" | "start" | "stop" => 40,
        _ => 20,
    };

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    // Haptics are fire-and-forget: any JNI failure (no vibrator
    // service, missing VIBRATE permission, exotic OEM builds) degrades
    // to the documented no-op instead of panicking the runtime.
    if haptic_play_inner(&mut env, effect_id, fallback_ms).is_none() {
        let _ = env.exception_clear();
    }

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

fn haptic_play_inner(env: &mut jni::JNIEnv, effect_id: i32, fallback_ms: i64) -> Option<()> {
    let activity = crate::widgets::get_activity(env);

    // Context.VIBRATOR_SERVICE = "vibrator". Deprecated in favor of
    // VIBRATOR_MANAGER_SERVICE on API 31+ but still returns the default
    // Vibrator on every current Android version.
    let service_name = env.new_string("vibrator").ok()?;
    let vibrator = env
        .call_method(
            &activity,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&service_name)],
        )
        .ok()?
        .l()
        .ok()?;
    if vibrator.is_null() {
        return Some(()); // no vibrator on this device — documented no-op
    }

    let sdk_int = env
        .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
        .ok()?
        .i()
        .ok()?;

    if sdk_int >= 29 {
        // VibrationEffect.createPredefined(int) + vibrate(VibrationEffect).
        let effect = env
            .call_static_method(
                "android/os/VibrationEffect",
                "createPredefined",
                "(I)Landroid/os/VibrationEffect;",
                &[JValue::Int(effect_id)],
            )
            .ok()?
            .l()
            .ok()?;
        env.call_method(
            &vibrator,
            "vibrate",
            "(Landroid/os/VibrationEffect;)V",
            &[JValue::Object(&effect)],
        )
        .ok()?;
    } else {
        // Pre-29: the deprecated-but-present one-shot vibrate(long).
        env.call_method(&vibrator, "vibrate", "(J)V", &[JValue::Long(fallback_ms)])
            .ok()?;
    }
    Some(())
}

/// Tap-callback registration key (#97). Set via `notification_on_tap` and
/// read by `Java_com_perry_app_PerryBridge_nativeNotificationTap` when the
/// user taps a notification. `0` means "no tap callback registered".
static NOTIFICATION_TAP_KEY: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// FCM device-token callback key (#95). Set via `notification_register_remote`
/// and read by `Java_com_perry_app_PerryBridge_nativeNotificationToken` when
/// FCM hands us a registration token (initial fetch + future rotations).
static NOTIFICATION_REMOTE_TOKEN_KEY: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

/// FCM payload-receive callback key (#95). Set via `notification_on_receive`
/// and read by `Java_com_perry_app_PerryBridge_nativeNotificationReceive`
/// when a foreground push payload arrives.
static NOTIFICATION_RECEIVE_KEY: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

/// FCM background payload-receive callback key (#98). Set via
/// `notification_on_background_receive` and read by
/// `Java_com_perry_app_PerryBridge_nativeNotificationBackgroundReceive` when
/// the FCM service runs (which is what Android calls for both foreground and
/// background data-payload delivery — there's no native distinction at the
/// FCM layer, unlike iOS's split delegate methods).
static NOTIFICATION_BACKGROUND_RECEIVE_KEY: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

/// Store the JS closure that fires when a notification is tapped (#97).
/// The closure is stashed in `crate::callback::register` so the global
/// callback table keeps it alive across GC; the returned key is saved in
/// `NOTIFICATION_TAP_KEY` for the JNI side to look up.
pub fn notification_on_tap(callback: f64) {
    let key = crate::callback::register(callback);
    NOTIFICATION_TAP_KEY.store(key, std::sync::atomic::Ordering::Relaxed);
}

/// JNI entry point — fired from `PerryNotificationReceiver.onReceive` when
/// the user taps a notification. Looks up the registered tap callback and
/// invokes it with `(id, undefined)`. The `action` parameter (from the TS
/// surface) is always `undefined` for #97 because action-button registration
/// isn't wired yet — same shape as the Apple side.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeNotificationTap(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    id: jni::objects::JString,
) {
    let key = NOTIFICATION_TAP_KEY.load(std::sync::atomic::Ordering::Relaxed);
    if key == 0 {
        return;
    }

    let rust_str: String = env.get_string(&id).map(|s| s.into()).unwrap_or_default();
    let bytes = rust_str.as_bytes();
    let id_value = unsafe {
        let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len());
        js_nanbox_string(ptr)
    };
    // `action` is always undefined until #97 follow-up wires action buttons.
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let action_value = f64::from_bits(TAG_UNDEFINED);

    crate::callback::invoke2(key, id_value, action_value);
}

/// Register the JS closure that fires when FCM hands us a device token
/// (#95). Stores the closure in the global callback table, saves the key in
/// `NOTIFICATION_REMOTE_TOKEN_KEY` for the JNI side to look up, and asks
/// `PerryBridge.registerForRemoteNotifications` to kick off the initial
/// `FirebaseMessaging.getInstance().token` fetch.
pub fn notification_register_remote(callback: f64) {
    let key = crate::callback::register(callback);
    NOTIFICATION_REMOTE_TOKEN_KEY.store(key, std::sync::atomic::Ordering::Relaxed);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let activity = crate::widgets::get_activity(&mut env);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "registerForRemoteNotifications",
        "(Landroid/app/Activity;)V",
        &[JValue::Object(&activity)],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Register the JS closure that fires for foreground FCM payloads (#95).
/// `PerryFirebaseMessagingService.onMessageReceived` serializes the
/// `RemoteMessage` to JSON and calls `nativeNotificationReceive`, which
/// looks up this key and invokes the closure.
pub fn notification_on_receive(callback: f64) {
    let key = crate::callback::register(callback);
    NOTIFICATION_RECEIVE_KEY.store(key, std::sync::atomic::Ordering::Relaxed);
}

/// Register the JS closure that fires for background FCM payloads (#98).
/// FCM doesn't natively distinguish foreground from background delivery in
/// the service callback — `onMessageReceived` runs for both — so the
/// background callback fires for every payload that crosses the service.
/// If both `notificationOnReceive` and `notificationOnBackgroundReceive` are
/// registered, both fire (the foreground one first, then the background
/// one). The cold-start case (process not yet loaded) hits the
/// `UnsatisfiedLinkError` branch in `PerryFirebaseMessagingService` and is
/// logged; wiring `Application.onCreate` to load the native lib at process
/// spawn is a #98 follow-up.
pub fn notification_on_background_receive(callback: f64) {
    let key = crate::callback::register(callback);
    NOTIFICATION_BACKGROUND_RECEIVE_KEY.store(key, std::sync::atomic::Ordering::Relaxed);
}

/// JNI: FCM device token (#95). Looks up the registered remote-token
/// callback and invokes it with the token NaN-boxed string.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeNotificationToken(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    token: jni::objects::JString,
) {
    let key = NOTIFICATION_REMOTE_TOKEN_KEY.load(std::sync::atomic::Ordering::Relaxed);
    if key == 0 {
        return;
    }
    let rust_str: String = env.get_string(&token).map(|s| s.into()).unwrap_or_default();
    let bytes = rust_str.as_bytes();
    let token_value = unsafe {
        let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len());
        js_nanbox_string(ptr)
    };
    crate::callback::invoke1(key, token_value);
}

/// JNI: FCM foreground push (#95). Parses the JSON payload via the runtime's
/// `js_json_parse` to produce a Perry object, then invokes the registered
/// receive callback with it.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeNotificationReceive(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    payload_json: jni::objects::JString,
) {
    let key = NOTIFICATION_RECEIVE_KEY.load(std::sync::atomic::Ordering::Relaxed);
    if key == 0 {
        return;
    }
    let rust_str: String = env
        .get_string(&payload_json)
        .map(|s| s.into())
        .unwrap_or_default();
    let bytes = rust_str.as_bytes();

    extern "C" {
        fn js_json_parse(text_ptr: *const u8) -> u64;
    }
    let payload_value = unsafe {
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len());
        let bits = js_json_parse(str_ptr);
        f64::from_bits(bits)
    };
    crate::callback::invoke1(key, payload_value);
}

/// JNI: FCM background push (#98). Same JSON→object shape as
/// `nativeNotificationReceive`. The user's callback may return a Promise;
/// `crate::callback::invoke1` runs the microtask pump after the call so any
/// `.then` chains attached synchronously fire before this method returns
/// — that gives the user's async work a window to complete inside the FCM
/// service's own runtime budget. We don't gate the JNI return on the
/// Promise actually settling (no UIBackgroundFetchResult equivalent on
/// Android), so deeply-nested awaits past the pump's drain may be cut off
/// when the OS suspends the process; v1 limitation, document for users.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeNotificationBackgroundReceive(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    payload_json: jni::objects::JString,
) {
    let key = NOTIFICATION_BACKGROUND_RECEIVE_KEY.load(std::sync::atomic::Ordering::Relaxed);
    if key == 0 {
        return;
    }
    let rust_str: String = env
        .get_string(&payload_json)
        .map(|s| s.into())
        .unwrap_or_default();
    let bytes = rust_str.as_bytes();

    extern "C" {
        fn js_json_parse(text_ptr: *const u8) -> u64;
        fn js_promise_run_microtasks() -> i32;
    }
    let payload_value = unsafe {
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len());
        let bits = js_json_parse(str_ptr);
        f64::from_bits(bits)
    };
    crate::callback::invoke1(key, payload_value);

    // Drive the microtask pump a few times so an async callback that awaits
    // already-resolved Promises (e.g., a cached value, an in-memory write)
    // gets a chance to finish before we hand control back to the FCM service.
    // Each tick may schedule new work, so loop until it stabilizes (or we hit
    // the cap, to keep this call bounded).
    for _ in 0..8 {
        let drained = unsafe { js_promise_run_microtasks() };
        if drained == 0 {
            break;
        }
    }
}

/// Schedule a fire-after-N-seconds notification via PerryBridge (#96).
/// `repeats` is JS-truthy-coerced before crossing the JNI boundary.
pub fn notification_schedule_interval(
    id_ptr: *const u8,
    title_ptr: *const u8,
    body_ptr: *const u8,
    seconds: f64,
    repeats: f64,
) {
    extern "C" {
        fn js_is_truthy(value: f64) -> i32;
    }
    let repeats_bool = unsafe { js_is_truthy(repeats) != 0 };
    let id = str_from_header(id_ptr);
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);
    let activity = crate::widgets::get_activity(&mut env);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let jid = env.new_string(id).expect("id");
    let jtitle = env.new_string(title).expect("title");
    let jbody = env.new_string(body).expect("body");
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "scheduleInterval",
        "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;DZ)V",
        &[
            JValue::Object(&activity),
            JValue::Object(&jid),
            JValue::Object(&jtitle),
            JValue::Object(&jbody),
            JValue::Double(seconds),
            JValue::Bool(if repeats_bool { 1u8 } else { 0u8 }),
        ],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Schedule a fire-at-wallclock-ms notification via PerryBridge (#96).
pub fn notification_schedule_calendar(
    id_ptr: *const u8,
    title_ptr: *const u8,
    body_ptr: *const u8,
    timestamp_ms: f64,
) {
    let id = str_from_header(id_ptr);
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);
    let activity = crate::widgets::get_activity(&mut env);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let jid = env.new_string(id).expect("id");
    let jtitle = env.new_string(title).expect("title");
    let jbody = env.new_string(body).expect("body");
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "scheduleCalendar",
        "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;D)V",
        &[
            JValue::Object(&activity),
            JValue::Object(&jid),
            JValue::Object(&jtitle),
            JValue::Object(&jbody),
            JValue::Double(timestamp_ms),
        ],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Logged no-op — Geofencing requires `FUSED_LOCATION_PROVIDER` + a
/// runtime `ACCESS_FINE_LOCATION` grant. That's a separate scope expansion
/// (#96 follow-up); programs targeting Android should fall back to
/// app-side geofence wiring or use interval/calendar triggers.
pub fn notification_schedule_location(
    _id_ptr: *const u8,
    _title_ptr: *const u8,
    _body_ptr: *const u8,
    _lat: f64,
    _lon: f64,
    _radius: f64,
) {
    extern "C" {
        fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
    }
    unsafe {
        __android_log_print(
            5,
            b"PerryNotification\0".as_ptr(),
            b"schedule_location: Geofencing API not wired on Android (#96 follow-up); skipped\0"
                .as_ptr(),
        );
    }
}

/// Cancel a scheduled or already-displayed notification by id (#96).
pub fn notification_cancel(id_ptr: *const u8) {
    let id = str_from_header(id_ptr);

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    let activity = crate::widgets::get_activity(&mut env);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let jid = env.new_string(id).expect("id");
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "cancelNotification",
        "(Landroid/app/Activity;Ljava/lang/String;)V",
        &[JValue::Object(&activity), JValue::Object(&jid)],
    );
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}

/// Send a notification via PerryBridge.
pub fn notification_send(title_ptr: *const u8, body_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());

    let jtitle = env.new_string(title).expect("title string");
    let jbody = env.new_string(body).expect("body string");

    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "sendNotification",
        "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Object(&activity),
            JValue::Object(&jtitle),
            JValue::Object(&jbody),
        ],
    );

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
}
