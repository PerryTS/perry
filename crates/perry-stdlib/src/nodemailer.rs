//! Nodemailer module (nodemailer compatible)
//!
//! Native implementation of the 'nodemailer' npm package. The SMTP protocol is
//! `turnloop-smtp`'s, driven over a turnloop socket on this agent's own loop
//! (`crate::turnloop_smtp`); the MIME bytes are still lettre's builder, reached
//! through `turnloop_smtp::message` so perry-stdlib no longer declares lettre
//! itself — which is what drops lettre's `tokio1` / `tokio1-rustls-tls` /
//! `pool` features from the graph.
//!
//! The `AsyncSmtpTransport<Tokio1Executor>` fallback is gone. An agent that
//! cannot get a loop now REJECTS rather than quietly taking a second,
//! never-exercised transport.

use perry_runtime::{js_promise_new_cross_thread, JSValue, ObjectHeader, Promise};

use crate::common::{register_handle, Handle};

mod turnloop_bridge;

/// SMTP transporter configuration
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub user: Option<String>,
    pub pass: Option<String>,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 587,
            secure: false,
            user: None,
            pass: None,
        }
    }
}

/// Extract a Rust String from a JSValue that contains a string pointer
fn jsvalue_to_string(value: JSValue) -> Option<String> {
    perry_runtime::string::with_string_value_bytes(f64::from_bits(value.bits()), |bytes| {
        String::from_utf8_lossy(bytes).into_owned()
    })
}

/// Parse SMTP configuration from JSValue
unsafe fn parse_smtp_config(config: JSValue) -> SmtpConfig {
    let mut result = SmtpConfig::default();

    if !config.is_pointer() {
        return result;
    }

    let obj_ptr = config.as_pointer() as *const ObjectHeader;
    if obj_ptr.is_null() {
        return result;
    }

    use perry_runtime::js_object_get_field;

    // Extract host (field 0)
    let host_val = js_object_get_field(obj_ptr, 0);
    if let Some(host) = jsvalue_to_string(host_val) {
        result.host = host;
    }

    // Extract port (field 1)
    let port_val = js_object_get_field(obj_ptr, 1);
    if port_val.is_number() {
        result.port = port_val.to_number() as u16;
    }

    // Extract secure (field 2)
    let secure_val = js_object_get_field(obj_ptr, 2);
    if secure_val.is_bool() {
        result.secure = secure_val.to_bool();
    }

    // Extract auth.user (field 3)
    let auth_val = js_object_get_field(obj_ptr, 3);
    if auth_val.is_pointer() {
        let auth_ptr = auth_val.as_pointer() as *const ObjectHeader;
        if !auth_ptr.is_null() {
            // user is field 0 of auth object
            let user_val = js_object_get_field(auth_ptr, 0);
            if let Some(user) = jsvalue_to_string(user_val) {
                result.user = Some(user);
            }
            // pass is field 1 of auth object
            let pass_val = js_object_get_field(auth_ptr, 1);
            if let Some(pass) = jsvalue_to_string(pass_val) {
                result.pass = Some(pass);
            }
        }
    }

    result
}

/// Wrapper around AsyncSmtpTransport
pub struct SmtpTransportHandle {
    pub config: SmtpConfig,
}

impl SmtpTransportHandle {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }
}

/// nodemailer.createTransport(config) -> Transporter
///
/// Creates a new SMTP transporter with the given configuration.
/// Returns a transporter handle.
///
/// # Safety
/// The config parameter must be a valid JSValue representing a config object.
#[no_mangle]
pub unsafe extern "C" fn js_nodemailer_create_transport(config_f: f64) -> f64 {
    // Take f64 at the FFI boundary to avoid SysV AMD64 ABI mismatch
    // (see js_mysql2_create_pool for details).
    let config = JSValue::from_bits(config_f.to_bits());
    let smtp_config = parse_smtp_config(config);
    let handle = register_handle(SmtpTransportHandle::new(smtp_config));
    // A transporter is a JS object-like handle. Tag the id as POINTER_TAG so
    // method lowering reaches the generic handle dispatcher instead of
    // treating the receiver as a numeric primitive.
    perry_runtime::value::js_nanbox_handle(handle)
}

/// Email message options
pub(crate) struct MailOptions {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) subject: String,
    pub(crate) text: Option<String>,
    pub(crate) html: Option<String>,
}

/// Parse mail options from JSValue
unsafe fn parse_mail_options(options: JSValue) -> Option<MailOptions> {
    if !options.is_pointer() {
        return None;
    }

    let obj_ptr = options.as_pointer() as *const ObjectHeader;
    if obj_ptr.is_null() {
        return None;
    }

    use perry_runtime::js_object_get_field;

    // Extract from (field 0)
    let from_val = js_object_get_field(obj_ptr, 0);
    let from = jsvalue_to_string(from_val)?;

    // Extract to (field 1)
    let to_val = js_object_get_field(obj_ptr, 1);
    let to = jsvalue_to_string(to_val)?;

    // Extract subject (field 2)
    let subject_val = js_object_get_field(obj_ptr, 2);
    let subject = jsvalue_to_string(subject_val).unwrap_or_default();

    // Extract text (field 3, optional)
    let text_val = js_object_get_field(obj_ptr, 3);
    let text = jsvalue_to_string(text_val);

    // Extract html (field 4, optional)
    let html_val = js_object_get_field(obj_ptr, 4);
    let html = jsvalue_to_string(html_val);

    Some(MailOptions {
        from,
        to,
        subject,
        text,
        html,
    })
}

/// transporter.sendMail(mailOptions) -> Promise<info>
///
/// Sends an email using the transporter.
///
/// # Safety
/// The transporter_handle must be a valid handle.
/// The options must be a valid JSValue representing mail options.
#[no_mangle]
pub unsafe extern "C" fn js_nodemailer_send_mail(
    transporter_handle: Handle,
    options: JSValue,
) -> *mut Promise {
    let promise = js_promise_new_cross_thread();

    // Parse mail options
    let mail_opts = match parse_mail_options(options) {
        Some(opts) => opts,
        None => {
            // Return rejected promise for invalid options
            crate::common::async_bridge::reject_promise_later(
                promise as *mut u8,
                "Invalid mail options".to_string(),
            );
            return promise;
        }
    };

    // The engine settles the promise exactly once. A refusal comes back as the
    // text to reject with; there is no second transport to fall through to.
    if let Err(message) =
        turnloop_bridge::try_send(transporter_handle, &mail_opts, promise as usize)
    {
        turnloop_bridge::reject(promise as usize, message);
    }

    promise
}

/// transporter.verify() -> Promise<boolean>
///
/// Verifies that the transporter can connect to the SMTP server.
#[no_mangle]
pub unsafe extern "C" fn js_nodemailer_verify(transporter_handle: Handle) -> *mut Promise {
    let promise = js_promise_new_cross_thread();

    if let Err(message) = turnloop_bridge::try_verify(transporter_handle, promise as usize) {
        turnloop_bridge::reject(promise as usize, message);
    }

    promise
}

/// Runtime method dispatch for a transporter handle whose static type was lost.
///
/// `createTransport` returns a pointer-tagged handle id; method calls on the
/// result still use generic dispatch when its static type is lost. See the arm
/// in `common/dispatch/method_dispatch.rs` for why this exists.
///
/// Registry membership is checked first and the name vocabulary second, so an
/// id belonging to another subsystem, or a name this binding does not
/// implement, is never claimed — the runtime then falls through to the
/// prototype chain, as it must for a user object wrapping the transporter.
///
/// # Safety
/// `handle` comes from the runtime's dispatcher and `args` are NaN-boxed.
pub(crate) unsafe fn dispatch_transporter_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if crate::common::get_handle::<SmtpTransportHandle>(handle).is_none() {
        return None;
    }
    let promise = match method {
        "sendMail" => {
            let options = args
                .first()
                .copied()
                .map(|bits| JSValue::from_bits(bits.to_bits()))
                .unwrap_or_else(|| {
                    JSValue::from_bits(crate::common::dispatch::TAG_UNDEFINED_F64.to_bits())
                });
            js_nodemailer_send_mail(handle, options)
        }
        "verify" => js_nodemailer_verify(handle),
        _ => return None,
    };
    Some(f64::from_bits(
        JSValue::pointer(promise as *const u8).bits(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_transport_returns_pointer_tagged_handle() {
        let config = JSValue::from_bits(crate::common::dispatch::TAG_UNDEFINED_F64.to_bits());
        let boxed = unsafe { js_nodemailer_create_transport(f64::from_bits(config.bits())) };
        let value = JSValue::from_bits(boxed.to_bits());
        assert!(value.is_handle());

        let handle = (boxed.to_bits() & 0x0000_FFFF_FFFF_FFFF) as Handle;
        let stored = crate::common::get_handle::<SmtpTransportHandle>(handle)
            .expect("tagged transporter handle remains registered");
        assert_eq!(stored.config.host, "localhost");
    }

    #[test]
    fn string_reader_accepts_inline_short_strings() {
        let short = JSValue::try_short_string(b"hello").expect("fits SSO");
        assert_eq!(jsvalue_to_string(short).as_deref(), Some("hello"));
    }

    #[test]
    fn transporter_dispatch_leaves_unknown_methods_and_foreign_handles_unclaimed() {
        let transporter =
            crate::common::register_handle(SmtpTransportHandle::new(SmtpConfig::default()));
        assert!(
            unsafe { dispatch_transporter_method(transporter, "unknownMethod", &[]) }.is_none()
        );

        let foreign = crate::common::register_handle(String::from("another registry type"));
        assert!(unsafe { dispatch_transporter_method(foreign, "verify", &[]) }.is_none());
    }
}
