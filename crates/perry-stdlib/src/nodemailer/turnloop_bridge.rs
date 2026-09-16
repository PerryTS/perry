//! Routes `nodemailer` onto the turnloop SMTP engine, and back.
//!
//! The MIME half does not move. The message is rendered by the same `lettre`
//! 0.11 builder as before (`turnloop_smtp::message` re-exports it), so the
//! bytes that reach the wire are produced by the same code and only the
//! transport changed — `turnloop_smtp::Connection` over a turnloop socket
//! instead of `AsyncSmtpTransport<Tokio1Executor>`.
//!
//! A decline (`Dispatched::Declined`) means the caller runs its existing lettre
//! future: that is the case on a worker agent with no loop, in the
//! `tokio-wait-driver` A/B arm, when the message cannot be built, or when the
//! TLS client configuration is unavailable.

use lettre::message::header::ContentType;
use lettre::Message;

use crate::common::get_handle;
use crate::turnloop_smtp::{self, MailJob, Outcome, Sink, SmtpConfig as EngineConfig};

use super::{MailOptions, SmtpTransportHandle};

pub(super) enum Dispatched {
    Accepted,
    Declined,
}

/// Perry's transporter options in the engine's shape. `secure: true` is
/// implicit TLS from the first byte (`lettre`'s `relay`); `false` is
/// opportunistic STARTTLS (`starttls_relay`), which is what this surface did
/// before — so `require_tls` stays false and a server with no STARTTLS is still
/// served in the clear, exactly as lettre's `starttls_relay` did.
fn engine_config(config: &super::SmtpConfig) -> EngineConfig {
    EngineConfig {
        host: config.host.clone(),
        port: config.port,
        implicit_tls: config.secure,
        require_tls: false,
        user: config.user.clone(),
        pass: config.pass.clone(),
        client_name: "[127.0.0.1]".to_string(),
    }
}

/// Render the message with the same builder the lettre path uses.
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

pub(super) fn try_send(
    transporter: crate::common::Handle,
    options: &MailOptions,
    promise_ptr: usize,
) -> Dispatched {
    let Some(config) =
        get_handle::<SmtpTransportHandle>(transporter).map(|w| engine_config(&w.config))
    else {
        return Dispatched::Declined;
    };
    let Some(message) = build(options) else {
        return Dispatched::Declined;
    };
    let envelope = message.envelope().clone();
    let Some(from) = envelope.from().map(ToString::to_string) else {
        return Dispatched::Declined;
    };
    let to: Vec<String> = envelope.to().iter().map(ToString::to_string).collect();
    if to.is_empty() {
        return Dispatched::Declined;
    }
    // The id Perry has always reported. Generated here rather than taken from
    // the rendered head so the value JS sees is unchanged by this migration.
    let message_id = format!("<{}@perry>", uuid::Uuid::new_v4());
    let job = MailJob {
        from,
        to,
        message_id: message_id.clone(),
        message: message.formatted(),
    };
    let sink = Sink {
        ctx: promise_ptr,
        on_done: settle_send,
    };
    match turnloop_smtp::send(&config, job, sink) {
        Ok(()) => {
            MESSAGE_IDS.with(|ids| ids.borrow_mut().insert(promise_ptr, message_id));
            Dispatched::Accepted
        }
        Err(_) => Dispatched::Declined,
    }
}

pub(super) fn try_verify(transporter: crate::common::Handle, promise_ptr: usize) -> Dispatched {
    let Some(config) =
        get_handle::<SmtpTransportHandle>(transporter).map(|w| engine_config(&w.config))
    else {
        return Dispatched::Declined;
    };
    let sink = Sink {
        ctx: promise_ptr,
        on_done: settle_verify,
    };
    match turnloop_smtp::verify(&config, sink) {
        Ok(()) => Dispatched::Accepted,
        Err(_) => Dispatched::Declined,
    }
}

thread_local! {
    /// The `messageId` promised to JS, held from submission to delivery. A
    /// plain `String` keyed by the promise address: no JS value, so there is
    /// nothing here for a moving collector to invalidate.
    static MESSAGE_IDS: std::cell::RefCell<std::collections::HashMap<usize, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Build the `info` object nodemailer resolves with. Runs inside a deferred
/// resolution, i.e. on the owning thread — never in the engine's sink.
fn settle_send(ctx: usize, outcome: Outcome) {
    let message_id = MESSAGE_IDS
        .with(|ids| ids.borrow_mut().remove(&ctx))
        .unwrap_or_default();
    match outcome {
        Outcome::Sent(info) => {
            let response = info.response.clone();
            crate::common::async_bridge::queue_deferred_resolution(ctx, true, move || unsafe {
                info_object(&message_id, &response)
            });
        }
        Outcome::Verified => {
            crate::common::async_bridge::queue_deferred_resolution(ctx, true, move || unsafe {
                info_object(&message_id, "")
            });
        }
        Outcome::Err(error) => {
            let message = format!("Failed to send email: {}", error.message);
            crate::common::async_bridge::queue_deferred_resolution(ctx, false, move || unsafe {
                error_value(&message)
            });
        }
    }
}

fn settle_verify(ctx: usize, outcome: Outcome) {
    match outcome {
        Outcome::Verified | Outcome::Sent(_) => {
            crate::common::async_bridge::queue_promise_resolution(
                ctx,
                true,
                perry_runtime::JSValue::bool(true).bits(),
            );
        }
        Outcome::Err(error) => {
            let message = format!("Connection test failed: {}", error.message);
            crate::common::async_bridge::queue_deferred_resolution(ctx, false, move || unsafe {
                error_value(&message)
            });
        }
    }
}

/// `{ messageId, response }` — the exact two-field shape this surface has
/// always resolved with, so the migration is invisible from JS.
///
/// # Safety
/// Must run on the thread that owns the JS heap; the deferred-resolution
/// converter guarantees that.
unsafe fn info_object(message_id: &str, response: &str) -> u64 {
    let info = perry_runtime::js_object_alloc(0, 2);
    let id = perry_runtime::js_string_from_bytes(message_id.as_ptr(), message_id.len() as u32);
    perry_runtime::js_object_set_field(info, 0, perry_runtime::JSValue::string_ptr(id));
    let resp = perry_runtime::js_string_from_bytes(response.as_ptr(), response.len() as u32);
    perry_runtime::js_object_set_field(info, 1, perry_runtime::JSValue::string_ptr(resp));
    perry_runtime::JSValue::object_ptr(info as *mut u8).bits()
}

/// # Safety
/// Same contract as `info_object`.
unsafe fn error_value(message: &str) -> u64 {
    let text = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = perry_runtime::error::js_error_new_with_message(text);
    perry_runtime::JSValue::pointer(error as *const u8).bits()
}
