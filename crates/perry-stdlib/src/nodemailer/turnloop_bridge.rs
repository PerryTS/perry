//! Routes `nodemailer` onto the turnloop SMTP engine, and back.
//!
//! The MIME half does not move. The message is rendered by the same `lettre`
//! 0.11 builder as before (`turnloop_smtp::message` re-exports it), so the
//! bytes that reach the wire are produced by the same code and only the
//! transport changed — `turnloop_smtp::Connection` over a turnloop socket
//! instead of `AsyncSmtpTransport<Tokio1Executor>`.
//!
//! There is no longer a second transport. The `AsyncSmtpTransport<
//! Tokio1Executor>` fallback is deleted, which is what lets perry-stdlib drop
//! its direct `lettre` dependency (and with it `tokio1` / `tokio1-rustls-tls` /
//! `pool`) while still rendering the identical MIME bytes through
//! `turnloop_smtp::message`. A refusal — an agent with no loop, an unbuildable
//! message, an unavailable TLS configuration — is returned as the text to
//! reject the promise with, so `try_send` / `try_verify` return
//! `Result<(), String>` instead of an `Accepted`/`Declined` pair that meant
//! "run the other path".

// `::turnloop_smtp` is the CRATE; `crate::turnloop_smtp` (imported below) is
// perry-stdlib's engine module of the same name. The leading `::` is what keeps
// the two apart — without it the module wins and the builder is unreachable.
use ::turnloop_smtp::message::header::ContentType;
use ::turnloop_smtp::message::Message;

use crate::common::get_handle;
use crate::turnloop_smtp::{self, MailJob, Outcome, Sink, SmtpConfig as EngineConfig};

use super::{MailOptions, SmtpTransportHandle};

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

/// Render the message, or the text to reject with. The three error strings are
/// the ones the deleted lettre path produced.
fn build(options: &MailOptions) -> Result<Message, String> {
    let from = options
        .from
        .parse()
        .map_err(|e| format!("Invalid from address: {e}"))?;
    let to = options
        .to
        .parse()
        .map_err(|e| format!("Invalid to address: {e}"))?;
    let builder = Message::builder()
        .from(from)
        .to(to)
        .subject(options.subject.clone());
    let message = if let Some(html) = options.html.clone() {
        builder.header(ContentType::TEXT_HTML).body(html)
    } else if let Some(text) = options.text.clone() {
        builder.header(ContentType::TEXT_PLAIN).body(text)
    } else {
        builder.body(String::new())
    };
    message.map_err(|e| format!("Failed to build email: {e}"))
}

/// What a thread with no `turnloop::Loop` now gets. Named once so `sendMail`
/// and `verify` cannot drift apart.
fn no_transport() -> String {
    "SMTP transport unavailable: this agent has no event loop".to_string()
}

pub(super) fn try_send(
    transporter: crate::common::Handle,
    options: &MailOptions,
    promise_ptr: usize,
) -> Result<(), String> {
    let Some(config) =
        get_handle::<SmtpTransportHandle>(transporter).map(|w| engine_config(&w.config))
    else {
        return Err("Invalid transporter handle".to_string());
    };
    let message = build(options)?;
    let envelope = message.envelope().clone();
    let Some(from) = envelope.from().map(ToString::to_string) else {
        return Err("Invalid from address: no sender in envelope".to_string());
    };
    let to: Vec<String> = envelope.to().iter().map(ToString::to_string).collect();
    if to.is_empty() {
        return Err("Invalid to address: no recipients in envelope".to_string());
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
            Ok(())
        }
        Err(_) => Err(no_transport()),
    }
}

pub(super) fn try_verify(
    transporter: crate::common::Handle,
    promise_ptr: usize,
) -> Result<(), String> {
    let Some(config) =
        get_handle::<SmtpTransportHandle>(transporter).map(|w| engine_config(&w.config))
    else {
        return Err("Invalid transporter handle".to_string());
    };
    let sink = Sink {
        ctx: promise_ptr,
        on_done: settle_verify,
    };
    match turnloop_smtp::verify(&config, sink) {
        Ok(()) => Ok(()),
        Err(_) => Err(no_transport()),
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
    // Every one of these calls can allocate, and therefore collect and MOVE
    // the others. `info` was held as a bare `*mut ObjectHeader` across two
    // `js_string_from_bytes` calls, so each `set_field` could write through a
    // pointer a collection had already relocated -- the #7210 shape, which
    // surfaces cycles later as `TypeError: value is not a function` rather
    // than here. Root all three and re-read through the handles.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let info = scope.root_raw_mut_ptr(perry_runtime::js_object_alloc(0, 2));
    let id = scope.root_raw_mut_ptr(perry_runtime::js_string_from_bytes(
        message_id.as_ptr(),
        message_id.len() as u32,
    ));
    let resp = scope.root_raw_mut_ptr(perry_runtime::js_string_from_bytes(
        response.as_ptr(),
        response.len() as u32,
    ));
    perry_runtime::js_object_set_field(
        info.get_raw_mut_ptr::<perry_runtime::object::ObjectHeader>(),
        0,
        perry_runtime::JSValue::string_ptr(id.get_raw_mut_ptr()),
    );
    perry_runtime::js_object_set_field(
        info.get_raw_mut_ptr::<perry_runtime::object::ObjectHeader>(),
        1,
        perry_runtime::JSValue::string_ptr(resp.get_raw_mut_ptr()),
    );
    perry_runtime::JSValue::object_ptr(
        info.get_raw_mut_ptr::<perry_runtime::object::ObjectHeader>() as *mut u8,
    )
    .bits()
}

/// Reject `promise_ptr` with `message`, through the same deferred queue the
/// engine's own failures use. No tokio task is taken, and the `Error` object is
/// built on the owning thread inside the deferred resolution rather than
/// wherever this happens to be called from (#1824).
pub(super) fn reject(promise_ptr: usize, message: String) {
    crate::common::async_bridge::queue_deferred_resolution(promise_ptr, false, move || unsafe {
        error_value(&message)
    });
}

/// # Safety
/// Same contract as `info_object`.
unsafe fn error_value(message: &str) -> u64 {
    let text = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = perry_runtime::error::js_error_new_with_message(text);
    perry_runtime::JSValue::pointer(error as *const u8).bits()
}
