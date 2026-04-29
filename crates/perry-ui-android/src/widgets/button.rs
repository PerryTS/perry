use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;
use jni::objects::JValue;

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

/// Create a Button with a label and closure callback. Returns widget handle.
pub fn create(label_ptr: *const u8, on_press: f64) -> i64 {
    let label = str_from_header(label_ptr);
    unsafe {
        __android_log_print(
            3,
            b"PerryButton\0".as_ptr(),
            b"create: label='%s'\0".as_ptr(),
            label.as_ptr(),
        );
    }
    let mut env = jni_bridge::get_env();

    // Check for pending exception from prior JNI calls
    if env.exception_check().unwrap_or(false) {
        unsafe {
            __android_log_print(
                6,
                b"PerryButton\0".as_ptr(),
                b"create: PENDING EXCEPTION before get_activity!\0".as_ptr(),
            );
        }
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }

    // Ensure we have local ref space
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    unsafe {
        __android_log_print(
            3,
            b"PerryButton\0".as_ptr(),
            b"create: got activity, using cached constructor\0".as_ptr(),
        );
    }

    // Use cached class and constructor to avoid FindClass overhead
    let button_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.button_class.as_obj()).unwrap());
    let ctor_id = jni_bridge::with_cache(|c| c.button_init);
    let button_cls: &jni::objects::JClass = (&button_class).into();

    unsafe {
        __android_log_print(
            3,
            b"PerryButton\0".as_ptr(),
            b"create: calling NewObject with cached ctor\0".as_ptr(),
        );
    }

    let button = match unsafe {
        env.new_object_unchecked(button_cls, ctor_id, &[JValue::Object(&activity).as_jni()])
    } {
        Ok(b) => b,
        Err(e) => {
            let msg = format!("Failed to create Button: {:?}\0", e);
            unsafe {
                __android_log_print(
                    6,
                    b"PerryButton\0".as_ptr(),
                    b"create: FAILED: %s\0".as_ptr(),
                    msg.as_ptr(),
                );
            }
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            unsafe {
                let _ = env.pop_local_frame(&jni::objects::JObject::null());
            }
            panic!("Failed to create Button: {:?}", e);
        }
    };
    unsafe {
        __android_log_print(
            3,
            b"PerryButton\0".as_ptr(),
            b"create: Button created OK\0".as_ptr(),
        );
    }

    // Set label text
    let jstr = env.new_string(label).expect("Failed to create JNI string");
    let _ = env.call_method(
        &button,
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&jstr)],
    );

    // Disable ALL CAPS (Material default) to match iOS mixed-case behavior
    let _ = env.call_method(&button, "setAllCaps", "(Z)V", &[JValue::Bool(0)]);

    // Register callback and set up OnClickListener via PerryBridge
    let cb_key = callback::register(on_press);
    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setOnClickCallback",
        "(Landroid/view/View;J)V",
        &[JValue::Object(&button), JValue::Long(cb_key)],
    );

    let global = env
        .new_global_ref(button)
        .expect("Failed to create global ref");
    let handle = super::register_widget(global);
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
        }
        unsafe {
            perry_geisterhand_register(handle, 0, 0, on_press, label_ptr);
        }
    }
    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }
    handle
}

/// Set whether a button has a border.
/// On Android, buttons always have a background; toggle between Material styles.
pub fn set_bordered(handle: i64, bordered: bool) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        if !bordered {
            // Set a flat/borderless style by making background transparent
            let bridge_class = jni_bridge::with_cache(|c| {
                env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
            });
            let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
            let _ = env.call_static_method(
                bridge_cls,
                "setButtonBorderless",
                "(Landroid/view/View;Z)V",
                &[JValue::Object(view_ref.as_obj()), JValue::Bool(0)],
            );
        }
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set the text color of a button.
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let ai = (a * 255.0) as u32;
        let ri = (r * 255.0) as u32;
        let gi = (g * 255.0) as u32;
        let bi = (b * 255.0) as u32;
        let color = ((ai << 24) | (ri << 16) | (gi << 8) | bi) as i32;
        let _ = env.call_method(
            view_ref.as_obj(),
            "setTextColor",
            "(I)V",
            &[JValue::Int(color)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set the title text of a button.
pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jstr = env.new_string(title).expect("Failed to create JNI string");
        let _ = env.call_method(
            view_ref.as_obj(),
            "setText",
            "(Ljava/lang/CharSequence;)V",
            &[JValue::Object(&jstr)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// SF Symbol name → Unicode emoji/symbol mapping.
/// Maps commonly used SF Symbols to Unicode characters that render on all Android devices.
pub fn sf_symbol_to_emoji(name: &str) -> Option<&'static str> {
    match name {
        // File/folder icons
        "folder" | "folder.fill" => Some("\u{1F4C1}"), // 📁
        "doc" | "doc.fill" => Some("\u{1F4C4}"),       // 📄
        "doc.text" | "doc.text.fill" => Some("\u{1F4C4}"), // 📄
        "doc.plaintext" => Some("\u{1F4DD}"),          // 📝
        "doc.on.doc" => Some("\u{1F4CB}"),             // 📋
        "doc.on.clipboard" => Some("\u{1F4CB}"),       // 📋
        // Navigation & UI
        "xmark" => Some("\u{2715}"),                              // ✕
        "xmark.circle" | "xmark.circle.fill" => Some("\u{2716}"), // ✖
        "chevron.right" => Some("\u{203A}"),                      // ›
        "chevron.down" => Some("\u{25BC}"),                       // ▼
        "chevron.left" => Some("\u{2039}"),                       // ‹
        "chevron.up" => Some("\u{25B2}"),                         // ▲
        "chevron.left.forwardslash.chevron.right" => Some("</>"),
        "plus" | "plus.circle" | "plus.circle.fill" => Some("+"),
        "minus" | "minus.circle" | "minus.circle.fill" => Some("\u{2212}"), // −
        // Activity bar icons
        "magnifyingglass" => Some("\u{1F50D}"), // 🔍
        "sparkles" => Some("\u{2728}"),         // ✨
        "terminal" => Some(">_"),
        "gearshape" | "gearshape.fill" | "gearshape.2" => Some("\u{2699}\u{FE0F}"), // ⚙️
        "sidebar.left" => Some("\u{2630}"),                                         // ☰
        // Git/source control
        "arrow.triangle.branch" => Some("\u{1F500}"), // 🔀
        "arrow.triangle.2.circlepath" => Some("\u{1F504}"), // 🔄
        // Debug
        "ladybug" | "ladybug.fill" | "ant" | "ant.fill" => Some("\u{1F41B}"), // 🐛
        // Extensions
        "puzzlepiece"
        | "puzzlepiece.fill"
        | "puzzlepiece.extension"
        | "puzzlepiece.extension.fill" => Some("\u{1F9E9}"), // 🧩
        // Editor actions
        "square.and.pencil" => Some("\u{270F}\u{FE0F}"), // ✏️
        "trash" | "trash.fill" => Some("\u{1F5D1}\u{FE0F}"), // 🗑️
        "arrow.uturn.backward" => Some("\u{21A9}\u{FE0F}"), // ↩️
        "arrow.uturn.forward" => Some("\u{21AA}\u{FE0F}"), // ↪️
        "scissors" => Some("\u{2702}\u{FE0F}"),          // ✂️
        // Status & info
        "exclamationmark.triangle" | "exclamationmark.triangle.fill" => Some("\u{26A0}\u{FE0F}"), // ⚠️
        "info.circle" | "info.circle.fill" => Some("\u{2139}\u{FE0F}"), // ℹ️
        "checkmark" => Some("\u{2713}"),                                // ✓
        "checkmark.circle" | "checkmark.circle.fill" => Some("\u{2705}"), // ✅
        // Misc
        "arrow.down.circle" | "arrow.down.circle.fill" => Some("\u{2B07}\u{FE0F}"), // ⬇️
        "arrow.up.circle" | "arrow.up.circle.fill" => Some("\u{2B06}\u{FE0F}"),     // ⬆️
        "ellipsis" => Some("\u{22EF}"),                                             // ⋯
        "ellipsis.circle" => Some("\u{22EE}"),                                      // ⋮
        "star" | "star.fill" => Some("\u{2B50}"),                                   // ⭐
        "bell" | "bell.fill" => Some("\u{1F514}"),                                  // 🔔
        "person" | "person.fill" => Some("\u{1F464}"),                              // 👤
        "house" | "house.fill" => Some("\u{1F3E0}"),                                // 🏠
        "play" | "play.fill" => Some("\u{25B6}\u{FE0F}"),                           // ▶️
        "pause" | "pause.fill" => Some("\u{23F8}\u{FE0F}"),                         // ⏸️
        "stop" | "stop.fill" => Some("\u{23F9}\u{FE0F}"),                           // ⏹️
        // File type icons
        "swift" => Some("\u{1F4C4}"), // 📄
        "curlybraces" => Some("{ }"),
        _ => None,
    }
}

/// Set an icon on a button (equivalent of SF Symbols on iOS).
/// On Android, uses Unicode emoji which are universally supported.
pub fn set_image(handle: i64, name_ptr: *const u8) {
    let name = str_from_header(name_ptr);
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);

        let icon_str = if let Some(emoji) = sf_symbol_to_emoji(name) {
            emoji.to_string()
        } else {
            // Fallback: use the symbol name itself (truncated)
            let fallback: String = name.chars().take(3).collect();
            fallback
        };

        let jstr = env.new_string(&icon_str).expect("icon string");
        let _ = env.call_method(
            view_ref.as_obj(),
            "setText",
            "(Ljava/lang/CharSequence;)V",
            &[JValue::Object(&jstr)],
        );

        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}

/// Set the position of the image relative to the label.
/// 0=leading(left), 1=trailing(right), 2=above, 3=below
/// On Android, CompoundDrawables positioning — for button text icon, this is a no-op
/// since we render icons as text characters.
pub fn set_image_position(_handle: i64, _position: i64) {
    // No-op: icon is rendered as text, so position is inherently leading
}

/// Set the content tint color of a button (icon + text color).
/// On Android, this is equivalent to setTextColor for icon buttons.
pub fn set_content_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    // Reuse set_text_color since icons are rendered as text
    set_text_color(handle, r, g, b, a);
}

/// Set a single-tap handler for the button.
pub fn set_on_tap(handle: i64, callback: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let cb_key = callback::register(callback);
        let bridge_class =
            jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "setOnClickCallback",
            "(Landroid/view/View;J)V",
            &[JValue::Object(view_ref.as_obj()), JValue::Long(cb_key)],
        );
        unsafe {
            env.pop_local_frame(&jni::objects::JObject::null());
        }
    }
}
