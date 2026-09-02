//! Issue #710 — `AttributedText` on Android via a TextView backed by a
//! `SpannableStringBuilder`. Each `append` adds the new substring and
//! applies per-run spans (`StyleSpan` for bold/italic, `UnderlineSpan`
//! for underline, `ForegroundColorSpan` for color, `AbsoluteSizeSpan`
//! for explicit font size).

use crate::jni_bridge;
use crate::jni_bridge::GlobalRef;
use jni::objects::JObject;
use jni::{Env, JValue};
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

use perry_ffi::copy_string_from_raw as str_from_header;

/// Create an empty `TextView` ready to receive `append` runs.
pub fn create() -> i64 {
    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);
        let activity = super::get_activity(env);
        let tv = env
            .new_object(
                jni::jni_str!("android/widget/TextView"),
                jni::jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(&activity)],
            )
            .expect("AttributedText TextView");
        let global = jni_bridge::new_global_ref(env, &tv).expect("TextView global ref");

        // Empty SpannableStringBuilder.
        let ssb = env
            .new_object(
                jni::jni_str!("android/text/SpannableStringBuilder"),
                jni::jni_sig!("()V"),
                &[],
            )
            .expect("SpannableStringBuilder()");
        let ssb_global = jni_bridge::new_global_ref(env, &ssb).expect("SSB global ref");

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
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
        handle
    })
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
    let text = unsafe { str_from_header(text_ptr) };
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

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 32);

        // Append the raw text to the SSB; the returned object is the SSB
        // itself but we don't need the return value.
        let java_text = match env.new_string(&text) {
            Ok(s) => s,
            Err(_) => {
                unsafe {
                    let _ = jni_bridge::pop_local_frame(env, &JObject::null());
                }
                return;
            }
        };
        let _ = env.call_method(
            ssb_ref.as_obj(),
            jni::jni_str!("append"),
            jni::jni_sig!("(Ljava/lang/CharSequence;)Landroid/text/SpannableStringBuilder;"),
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
                jni::jni_str!("android/text/style/StyleSpan"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(style)],
            ) {
                set_span(env, ssb_ref.as_obj(), &style_span, start, end, SPAN_FLAG);
            }
        }

        if underline != 0 {
            if let Ok(u_span) = env.new_object(
                jni::jni_str!("android/text/style/UnderlineSpan"),
                jni::jni_sig!("()V"),
                &[],
            ) {
                set_span(env, ssb_ref.as_obj(), &u_span, start, end, SPAN_FLAG);
            }
        }

        if a > 0.0 {
            let argb = rgba_to_argb(r, g, b, a);
            if let Ok(c_span) = env.new_object(
                jni::jni_str!("android/text/style/ForegroundColorSpan"),
                jni::jni_sig!("(I)V"),
                &[JValue::Int(argb)],
            ) {
                set_span(env, ssb_ref.as_obj(), &c_span, start, end, SPAN_FLAG);
            }
        }

        if font_size > 0.0 {
            // AbsoluteSizeSpan(size, dip) — dip=true means the int is in dp.
            if let Ok(sz_span) = env.new_object(
                jni::jni_str!("android/text/style/AbsoluteSizeSpan"),
                jni::jni_sig!("(IZ)V"),
                &[JValue::Int(font_size.round() as i32), JValue::Bool(true)],
            ) {
                set_span(env, ssb_ref.as_obj(), &sz_span, start, end, SPAN_FLAG);
            }
        }

        // Push the buffer onto the TextView. Calling setText(CharSequence)
        // copies-on-write internally on most Android versions; safer to do
        // it every append rather than try to incrementally update.
        let _ = env.call_method(
            view_ref.as_obj(),
            jni::jni_str!("setText"),
            jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
            &[JValue::Object(ssb_ref.as_obj())],
        );

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }

        BUFFERS.with(|b| {
            if let Some(buf) = b.borrow_mut().get_mut(&handle) {
                buf.len = end;
            }
        });
    })
}

/// Reset the buffer back to empty.
pub fn clear(handle: i64) {
    let Some(view_ref) = super::get_widget(handle) else {
        return;
    };

    jni_bridge::with_env(|env| {
        let _ = jni_bridge::push_local_frame(env, 16);

        let ssb = match env.new_object(
            jni::jni_str!("android/text/SpannableStringBuilder"),
            jni::jni_sig!("()V"),
            &[],
        ) {
            Ok(o) => o,
            Err(_) => {
                unsafe {
                    let _ = jni_bridge::pop_local_frame(env, &JObject::null());
                }
                return;
            }
        };
        let _ = env.call_method(
            view_ref.as_obj(),
            jni::jni_str!("setText"),
            jni::jni_sig!("(Ljava/lang/CharSequence;)V"),
            &[JValue::Object(&ssb)],
        );
        let global = jni_bridge::new_global_ref(env, &ssb).expect("SSB global ref");

        BUFFERS.with(|b| {
            if let Some(buf) = b.borrow_mut().get_mut(&handle) {
                buf.builder = global;
                buf.len = 0;
            }
        });

        unsafe {
            let _ = jni_bridge::pop_local_frame(env, &JObject::null());
        }
    })
}

fn set_span(env: &mut Env, ssb: &JObject, span: &JObject, start: i32, end: i32, flags: i32) {
    let _ = env.call_method(
        ssb,
        jni::jni_str!("setSpan"),
        jni::jni_sig!("(Ljava/lang/Object;III)V"),
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
    jni_bridge::with_env(|env| {
        jni_bridge::new_global_ref(env, g.as_obj()).expect("clone SSB global ref")
    })
}
