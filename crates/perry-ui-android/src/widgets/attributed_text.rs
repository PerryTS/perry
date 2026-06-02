//! Issue #710 — `AttributedText` on Android via a TextView backed by a
//! `SpannableStringBuilder`. Each `append` adds the new substring and
//! applies per-run spans (`StyleSpan` for bold/italic, `UnderlineSpan`
//! for underline, `ForegroundColorSpan` for color, `AbsoluteSizeSpan`
//! for explicit font size).

use crate::jni_bridge;
use jni::objects::{GlobalRef, JObject, JValue};
use jni::JNIEnv;
use std::cell::RefCell;
use std::collections::HashMap;

struct Buffer {
    /// `SpannableStringBuilder` — global ref so it survives between
    /// JNI frames.
    builder: GlobalRef,
    /// Total character length of the buffer; used as the next run's
    /// start offset and for the previous-length calc.
    len: i32,
}

thread_local! {
    static BUFFERS: RefCell<HashMap<i64, Buffer>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create an empty `TextView` ready to receive `append` runs.
pub fn create() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);
    let activity = super::get_activity(&mut env);
    let tv = env
        .new_object(
            "android/widget/TextView",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .expect("AttributedText TextView");
    let global = env.new_global_ref(&tv).expect("TextView global ref");

    // Empty SpannableStringBuilder.
    let ssb = env
        .new_object("android/text/SpannableStringBuilder", "()V", &[])
        .expect("SpannableStringBuilder()");
    let ssb_global = env.new_global_ref(&ssb).expect("SSB global ref");

    let handle = super::register_widget(global);
    BUFFERS.with(|b| {
        b.borrow_mut().insert(
            handle,
            Buffer {
                builder: ssb_global,
                len: 0,
            },
        );
    });

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
    handle
}

/// Append one styled run. See iOS/macOS twin for parameter semantics.
pub fn append(
    handle: i64,
    text_ptr: *const u8,
    bold: i64,
    italic: i64,
    underline: i64,
    font_size: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    let text = str_from_header(text_ptr);
    if text.is_empty() {
        return;
    }
    let Some(view_ref) = super::get_widget(handle) else {
        return;
    };

    // Snapshot the previous length so we know where the new run starts,
    // and read the SSB global out of the buffers map.
    let (ssb_ref, start) = match BUFFERS.with(|b| {
        b.borrow()
            .get(&handle)
            .map(|buf| (env_clone_global(&buf.builder), buf.len))
    }) {
        Some(pair) => pair,
        None => return,
    };

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    // Append the raw text to the SSB; the returned object is the SSB
    // itself but we don't need the return value.
    let java_text = match env.new_string(text) {
        Ok(s) => s,
        Err(_) => {
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return;
        }
    };
    let _ = env.call_method(
        ssb_ref.as_obj(),
        "append",
        "(Ljava/lang/CharSequence;)Landroid/text/SpannableStringBuilder;",
        &[JValue::Object(&java_text)],
    );

    let end = start + text.chars().count() as i32;

    // SPAN_EXCLUSIVE_EXCLUSIVE = 33 — the conventional flag for static
    // run spans that don't expand when adjacent text is inserted.
    const SPAN_FLAG: i32 = 33;

    if bold != 0 || italic != 0 {
        // Build a Typeface style int: BOLD = 1, ITALIC = 2, BOLD_ITALIC = 3.
        let style: i32 = (bold != 0) as i32 | ((italic != 0) as i32) << 1;
        if let Ok(style_span) = env.new_object(
            "android/text/style/StyleSpan",
            "(I)V",
            &[JValue::Int(style)],
        ) {
            set_span(
                &mut env,
                ssb_ref.as_obj(),
                &style_span,
                start,
                end,
                SPAN_FLAG,
            );
        }
    }

    if underline != 0 {
        if let Ok(u_span) = env.new_object("android/text/style/UnderlineSpan", "()V", &[]) {
            set_span(&mut env, ssb_ref.as_obj(), &u_span, start, end, SPAN_FLAG);
        }
    }

    if a > 0.0 {
        let argb = rgba_to_argb(r, g, b, a);
        if let Ok(c_span) = env.new_object(
            "android/text/style/ForegroundColorSpan",
            "(I)V",
            &[JValue::Int(argb)],
        ) {
            set_span(&mut env, ssb_ref.as_obj(), &c_span, start, end, SPAN_FLAG);
        }
    }

    if font_size > 0.0 {
        // AbsoluteSizeSpan(size, dip) — dip=true means the int is in dp.
        if let Ok(sz_span) = env.new_object(
            "android/text/style/AbsoluteSizeSpan",
            "(IZ)V",
            &[JValue::Int(font_size.round() as i32), JValue::Bool(1)],
        ) {
            set_span(&mut env, ssb_ref.as_obj(), &sz_span, start, end, SPAN_FLAG);
        }
    }

    // Push the buffer onto the TextView. Calling setText(CharSequence)
    // copies-on-write internally on most Android versions; safer to do
    // it every append rather than try to incrementally update.
    let _ = env.call_method(
        view_ref.as_obj(),
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(ssb_ref.as_obj())],
    );

    unsafe {
        env.pop_local_frame(&JObject::null());
    }

    BUFFERS.with(|b| {
        if let Some(buf) = b.borrow_mut().get_mut(&handle) {
            buf.len = end;
        }
    });
}

/// Reset the buffer back to empty.
pub fn clear(handle: i64) {
    let Some(view_ref) = super::get_widget(handle) else {
        return;
    };

    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let ssb = match env.new_object("android/text/SpannableStringBuilder", "()V", &[]) {
        Ok(o) => o,
        Err(_) => {
            unsafe {
                env.pop_local_frame(&JObject::null());
            }
            return;
        }
    };
    let _ = env.call_method(
        view_ref.as_obj(),
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&ssb)],
    );
    let global = env.new_global_ref(&ssb).expect("SSB global ref");

    BUFFERS.with(|b| {
        if let Some(buf) = b.borrow_mut().get_mut(&handle) {
            buf.builder = global;
            buf.len = 0;
        }
    });

    unsafe {
        env.pop_local_frame(&JObject::null());
    }
}

fn set_span(env: &mut JNIEnv, ssb: &JObject, span: &JObject, start: i32, end: i32, flags: i32) {
    let _ = env.call_method(
        ssb,
        "setSpan",
        "(Ljava/lang/Object;III)V",
        &[
            JValue::Object(span),
            JValue::Int(start),
            JValue::Int(end),
            JValue::Int(flags),
        ],
    );
}

fn rgba_to_argb(r: f64, g: f64, b: f64, a: f64) -> i32 {
    let to_u8 = |v: f64| -> u32 { (v.clamp(0.0, 1.0) * 255.0).round() as u32 };
    let argb = (to_u8(a) << 24) | (to_u8(r) << 16) | (to_u8(g) << 8) | to_u8(b);
    argb as i32
}

/// Bump the refcount on a stored `GlobalRef` so callers get their own.
/// JNI `GlobalRef` is not `Clone`, but `as_obj` returns the underlying
/// `JObject<'static>` we can re-wrap as a new global via JNIEnv.
fn env_clone_global(g: &GlobalRef) -> GlobalRef {
    let mut env = jni_bridge::get_env();
    env.new_global_ref(g.as_obj())
        .expect("clone SSB global ref")
}
