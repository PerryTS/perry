//! turnloop P6: route this binding's SMTP onto perry-stdlib's turnloop engine.
//!
//! This crate is a separately linked `staticlib` with no Cargo edge to
//! perry-stdlib, so it reaches the engine through the `js_perry_smtp_*` C
//! symbols perry-stdlib exports (`turnloop_smtp::ffi`) — the same shape
//! `perry-ext-undici` uses to reach `js_fetch_set_global_proxy`. The engine
//! feature is deliberately NOT under `bundled-nodemailer`, so the well-known
//! flip that strips the bundled surface when `import 'nodemailer'` routes here
//! leaves these symbols in place.
//!
//! The MIME half does not move: the message is still rendered by this crate's
//! own `lettre` builder, byte for byte as before. Only the transport changes.
//!
//! A decline — a worker agent with no loop, the `tokio-wait-driver` A/B arm, an
//! unbuildable message — falls through to the `spawn_blocking` + lettre path,
//! which is why that path is not deleted.

use lettre::message::header::ContentType;
use lettre::Message;
use perry_ffi::{JsPromise, JsValue, Promise};

use crate::{build_info_object, MailOptions, SmtpConfig};

#[repr(C)]
#[derive(Clone, Copy)]
struct PerrySmtpSlice {
    ptr: *const u8,
    len: usize,
}

#[repr(C)]
struct PerrySmtpResult {
    ok: i32,
    response_code: i32,
    code: PerrySmtpSlice,
    message: PerrySmtpSlice,
    response: PerrySmtpSlice,
    message_id: PerrySmtpSlice,
    accepted: *const PerrySmtpSlice,
    accepted_len: usize,
    rejected: *const PerrySmtpSlice,
    rejected_len: usize,
}

type PerrySmtpDone = extern "C" fn(ctx: usize, result: *const PerrySmtpResult);

unsafe extern "C" {
    fn js_perry_smtp_available() -> i32;
    fn js_perry_smtp_begin() -> i64;
    fn js_perry_smtp_set_server(
        draft: i64,
        host_ptr: *const u8,
        host_len: usize,
        port: u16,
        implicit_tls: i32,
        require_tls: i32,
    );
    fn js_perry_smtp_set_auth(
        draft: i64,
        user_ptr: *const u8,
        user_len: usize,
        pass_ptr: *const u8,
        pass_len: usize,
    );
    fn js_perry_smtp_set_from(draft: i64, ptr: *const u8, len: usize);
    fn js_perry_smtp_add_recipient(draft: i64, ptr: *const u8, len: usize);
    fn js_perry_smtp_set_message(
        draft: i64,
        id_ptr: *const u8,
        id_len: usize,
        body_ptr: *const u8,
        body_len: usize,
    );
    fn js_perry_smtp_submit(draft: i64, verify: i32, ctx: usize, done: PerrySmtpDone) -> i32;
    fn js_perry_smtp_cancel(draft: i64);
}

/// Whether the turnloop engine is linked and this agent owns a loop.
///
/// The symbol is resolved at link time from perry-stdlib, which is always
/// co-linked with this wrapper; a build that somehow lacks it would fail to
/// link rather than reach here.
fn available() -> bool {
    // SAFETY: a plain predicate with no arguments; the callee touches nothing.
    unsafe { js_perry_smtp_available() != 0 }
}

fn slice_text(slice: PerrySmtpSlice) -> String {
    if slice.ptr.is_null() || slice.len == 0 {
        return String::new();
    }
    // SAFETY: the engine documents the slices as borrowed for the duration of
    // this callback, and the bytes are copied here.
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(slice.ptr, slice.len) })
        .into_owned()
}

fn write_draft(draft: i64, config: &SmtpConfig) {
    // SAFETY: every pointer/length pair below borrows a live local for the
    // duration of the call, which is the setters' documented contract.
    unsafe {
        js_perry_smtp_set_server(
            draft,
            config.host.as_ptr(),
            config.host.len(),
            config.port,
            i32::from(config.secure),
            0,
        );
        if let (Some(user), Some(pass)) = (&config.user, &config.pass) {
            js_perry_smtp_set_auth(draft, user.as_ptr(), user.len(), pass.as_ptr(), pass.len());
        }
    }
}

fn build(options: &MailOptions) -> Option<Message> {
    let builder = Message::builder()
        .from(options.from.parse().ok()?)
        .to(options.to.parse().ok()?)
        .subject(options.subject.clone());
    let message = if let Some(html) = options.html.clone() {
        builder.header(ContentType::TEXT_HTML).body(html)
    } else if let Some(text) = options.text.clone() {
        builder.header(ContentType::TEXT_PLAIN).body(text)
    } else {
        builder.body(String::new())
    };
    message.ok()
}

/// `true` means the engine accepted the exchange and WILL settle `promise`
/// exactly once; `false` means nothing was submitted.
pub(crate) fn try_send(config: &SmtpConfig, options: &MailOptions, promise: *mut Promise) -> bool {
    if !available() {
        return false;
    }
    let Some(message) = build(options) else {
        return false;
    };
    let envelope = message.envelope().clone();
    let Some(from) = envelope.from().map(ToString::to_string) else {
        return false;
    };
    let recipients: Vec<String> = envelope.to().iter().map(ToString::to_string).collect();
    if recipients.is_empty() {
        return false;
    }
    // SAFETY: `begin` returns 0 when no draft could be made, which is checked.
    let draft = unsafe { js_perry_smtp_begin() };
    if draft == 0 {
        return false;
    }
    write_draft(draft, config);
    let body = message.formatted();
    // The id this surface has always reported. Generated here, not taken from
    // the rendered head, so JS sees the same shape as before.
    let message_id = format!("<{}@perry>", uuid::Uuid::new_v4());
    // SAFETY: each pointer/length borrows a live local across its own call.
    unsafe {
        js_perry_smtp_set_from(draft, from.as_ptr(), from.len());
        for to in &recipients {
            js_perry_smtp_add_recipient(draft, to.as_ptr(), to.len());
        }
        js_perry_smtp_set_message(
            draft,
            message_id.as_ptr(),
            message_id.len(),
            body.as_ptr(),
            body.len(),
        );
    }
    let ctx = promise as usize;
    MESSAGE_IDS.with(|ids| ids.borrow_mut().insert(ctx, message_id));
    // SAFETY: `ctx` is this exchange's promise address and `on_send` is a plain
    // `extern "C"` function; the engine calls it at most once.
    let accepted = unsafe { js_perry_smtp_submit(draft, 0, ctx, on_send) != 0 };
    if !accepted {
        MESSAGE_IDS.with(|ids| ids.borrow_mut().remove(&ctx));
        // SAFETY: the draft was never submitted, so nothing else holds it.
        unsafe { js_perry_smtp_cancel(draft) };
    }
    accepted
}

pub(crate) fn try_verify(config: &SmtpConfig, promise: *mut Promise) -> bool {
    if !available() {
        return false;
    }
    // SAFETY: as in `try_send`.
    let draft = unsafe { js_perry_smtp_begin() };
    if draft == 0 {
        return false;
    }
    write_draft(draft, config);
    // SAFETY: `on_verify` is a plain `extern "C"` function called at most once.
    let accepted = unsafe { js_perry_smtp_submit(draft, 1, promise as usize, on_verify) != 0 };
    if !accepted {
        // SAFETY: never submitted.
        unsafe { js_perry_smtp_cancel(draft) };
    }
    accepted
}

thread_local! {
    /// The `messageId` promised to JS, from submission to delivery. A plain
    /// `String` keyed by the promise address — no JS value, so a moving
    /// collection has nothing here to invalidate.
    static MESSAGE_IDS: std::cell::RefCell<std::collections::HashMap<usize, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

extern "C" fn on_send(ctx: usize, result: *const PerrySmtpResult) {
    let message_id = MESSAGE_IDS
        .with(|ids| ids.borrow_mut().remove(&ctx))
        .unwrap_or_default();
    // SAFETY: the engine passes a live `PerrySmtpResult` for this call only.
    let result = unsafe { &*result };
    // SAFETY: `ctx` is the promise this exchange was submitted with, and the
    // engine calls back exactly once, so this is its single resolution.
    let promise = unsafe { JsPromise::from_raw(ctx as *mut Promise) };
    if result.ok != 0 {
        let response = slice_text(result.response);
        promise.resolve_with(move || build_info_object(&message_id, &response));
    } else {
        let message = failure_text(result);
        promise.reject_string(&format!("Failed to send email: {message}"));
    }
}

extern "C" fn on_verify(ctx: usize, result: *const PerrySmtpResult) {
    // SAFETY: as in `on_send`.
    let result = unsafe { &*result };
    let promise = unsafe { JsPromise::from_raw(ctx as *mut Promise) };
    if result.ok != 0 {
        promise.resolve(JsValue::from_bool(true));
    } else {
        let message = failure_text(result);
        promise.reject_string(&format!("Connection test failed: {message}"));
    }
}

fn failure_text(result: &PerrySmtpResult) -> String {
    let message = slice_text(result.message);
    let code = slice_text(result.code);
    if code.is_empty() {
        message
    } else {
        format!("{code}: {message}")
    }
}
