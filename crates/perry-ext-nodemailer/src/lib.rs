//! Native bindings for the npm `nodemailer` package — SMTP
//! transport via the `lettre` crate. Sync `createTransport`,
//! async `sendMail` / `verify` bridged through `spawn_blocking`
//! + `JsPromise` + `tokio::Handle::current().block_on`.
//!
//! Exercises perry-ffi v0.5's nested-object reading surface
//! (`js_object_get_field` indexed lookups for the user's SMTP
//! config + the auth sub-object) plus the shape-aware object
//! construction surface (`build_object_shape` /
//! `js_object_alloc_with_shape` / `js_object_set_field`) for the
//! info object resolved back to user code on success.

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use perry_ffi::{
    alloc_string, build_object_shape, get_handle, js_object_alloc_with_shape, js_object_get_field,
    js_object_set_field, register_handle, spawn_blocking, Handle, JsPromise, JsValue, ObjectHeader,
    Promise, StringHeader,
};

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

pub struct SmtpTransportHandle {
    pub config: SmtpConfig,
}

impl SmtpTransportHandle {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }
}

unsafe fn jsvalue_to_string(value: JsValue) -> Option<String> {
    if value.is_string() {
        let ptr = value.as_string_ptr();
        if !ptr.is_null() {
            let len = (*ptr).byte_len as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            return std::str::from_utf8(bytes).ok().map(String::from);
        }
    }
    None
}

unsafe fn parse_smtp_config(config: JsValue) -> SmtpConfig {
    let mut result = SmtpConfig::default();
    let obj_ptr = config.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return result;
    }

    if let Some(host) = jsvalue_to_string(js_object_get_field(obj_ptr, 0)) {
        result.host = host;
    }
    let port_val = js_object_get_field(obj_ptr, 1);
    if port_val.is_number() {
        result.port = port_val.to_number() as u16;
    }
    let secure_val = js_object_get_field(obj_ptr, 2);
    if secure_val.is_bool() {
        result.secure = secure_val.to_bool();
    }
    let auth_val = js_object_get_field(obj_ptr, 3);
    let auth_ptr = auth_val.as_pointer::<ObjectHeader>();
    if !auth_ptr.is_null() {
        if let Some(user) = jsvalue_to_string(js_object_get_field(auth_ptr, 0)) {
            result.user = Some(user);
        }
        if let Some(pass) = jsvalue_to_string(js_object_get_field(auth_ptr, 1)) {
            result.pass = Some(pass);
        }
    }
    result
}

/// `nodemailer.createTransport(config) -> Transporter` — sync.
///
/// The config object's positional field layout (host=0, port=1,
/// secure=2, auth={user=0, pass=1}=3) matches perry-runtime's
/// shape-ordered object storage: user code declares the keys in
/// that order in the literal and they hash to the same field
/// indices Perry's runtime emits at the call site.
///
/// # Safety
///
/// `config_f` is the NaN-boxed JsValue (passed as f64 at the FFI
/// boundary to avoid SysV AMD64 ABI mismatches with `JsValue`-typed
/// args — same trick perry-stdlib's existing copy uses).
#[no_mangle]
pub unsafe extern "C" fn js_nodemailer_create_transport(config_f: f64) -> f64 {
    let config = JsValue::from_bits(config_f.to_bits());
    let smtp_config = parse_smtp_config(config);
    register_handle(SmtpTransportHandle::new(smtp_config)) as f64
}

struct MailOptions {
    from: String,
    to: String,
    subject: String,
    text: Option<String>,
    html: Option<String>,
}

unsafe fn parse_mail_options(options: JsValue) -> Option<MailOptions> {
    let obj_ptr = options.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return None;
    }
    let from = jsvalue_to_string(js_object_get_field(obj_ptr, 0))?;
    let to = jsvalue_to_string(js_object_get_field(obj_ptr, 1))?;
    let subject = jsvalue_to_string(js_object_get_field(obj_ptr, 2)).unwrap_or_default();
    let text = jsvalue_to_string(js_object_get_field(obj_ptr, 3));
    let html = jsvalue_to_string(js_object_get_field(obj_ptr, 4));
    Some(MailOptions {
        from,
        to,
        subject,
        text,
        html,
    })
}

fn build_info_object(message_id: &str, response: &str) -> JsValue {
    let (packed, shape_id) = build_object_shape(&["messageId", "response"]);
    let obj =
        unsafe { js_object_alloc_with_shape(shape_id, 2, packed.as_ptr(), packed.len() as u32) };
    let id_str = alloc_string(message_id);
    unsafe { js_object_set_field(obj, 0, JsValue::from_string_ptr(id_str.as_raw())) };
    let resp_str = alloc_string(response);
    unsafe { js_object_set_field(obj, 1, JsValue::from_string_ptr(resp_str.as_raw())) };
    JsValue::from_object_ptr(obj)
}

/// `transporter.sendMail(mailOptions) -> Promise<info>`.
///
/// # Safety
///
/// `transporter_handle` must be registered via
/// `js_nodemailer_create_transport`. `options_f` is the NaN-boxed
/// JsValue for the mail options object.
#[no_mangle]
pub unsafe extern "C" fn js_nodemailer_send_mail(
    transporter_handle: Handle,
    options_f: f64,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let options = JsValue::from_bits(options_f.to_bits());

    let mail_opts = match parse_mail_options(options) {
        Some(opts) => opts,
        None => {
            promise.reject_string("Invalid mail options");
            return raw;
        }
    };

    spawn_blocking(move || {
        let outcome = (|| -> Result<JsValue, String> {
            let wrapper = get_handle::<SmtpTransportHandle>(transporter_handle)
                .ok_or_else(|| "Invalid transporter handle".to_string())?;
            let config = &wrapper.config;

            let mailer_result = if config.secure {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            };

            let mailer: AsyncSmtpTransport<Tokio1Executor> = match mailer_result {
                Ok(builder) => {
                    let mut builder = builder.port(config.port);
                    if let (Some(user), Some(pass)) = (&config.user, &config.pass) {
                        let creds = Credentials::new(user.clone(), pass.clone());
                        builder = builder.credentials(creds);
                    }
                    builder.build()
                }
                Err(e) => return Err(format!("Failed to create transport: {}", e)),
            };

            let email_builder = Message::builder()
                .from(
                    mail_opts
                        .from
                        .parse()
                        .map_err(|e| format!("Invalid from address: {}", e))?,
                )
                .to(mail_opts
                    .to
                    .parse()
                    .map_err(|e| format!("Invalid to address: {}", e))?)
                .subject(mail_opts.subject);

            let email = if let Some(html) = mail_opts.html {
                email_builder
                    .header(ContentType::TEXT_HTML)
                    .body(html)
                    .map_err(|e| format!("Failed to build email: {}", e))?
            } else if let Some(text) = mail_opts.text {
                email_builder
                    .header(ContentType::TEXT_PLAIN)
                    .body(text)
                    .map_err(|e| format!("Failed to build email: {}", e))?
            } else {
                email_builder
                    .body(String::new())
                    .map_err(|e| format!("Failed to build email: {}", e))?
            };

            let send_result =
                tokio::runtime::Handle::current().block_on(async move { mailer.send(email).await });

            match send_result {
                Ok(response) => {
                    let message_id = format!("<{}@perry>", uuid::Uuid::new_v4());
                    let response_str = format!("{:?}", response);
                    Ok(build_info_object(&message_id, &response_str))
                }
                Err(e) => Err(format!("Failed to send email: {}", e)),
            }
        })();

        match outcome {
            Ok(info) => promise.resolve(info),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `transporter.verify() -> Promise<boolean>`.
#[no_mangle]
pub extern "C" fn js_nodemailer_verify(transporter_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        let outcome = (|| -> Result<bool, String> {
            let wrapper = get_handle::<SmtpTransportHandle>(transporter_handle)
                .ok_or_else(|| "Invalid transporter handle".to_string())?;
            let config = &wrapper.config;

            let mailer_result = if config.secure {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            };

            let mailer: AsyncSmtpTransport<Tokio1Executor> = match mailer_result {
                Ok(builder) => {
                    let mut builder = builder.port(config.port);
                    if let (Some(user), Some(pass)) = (&config.user, &config.pass) {
                        let creds = Credentials::new(user.clone(), pass.clone());
                        builder = builder.credentials(creds);
                    }
                    builder.build()
                }
                Err(e) => return Err(format!("Failed to create transport: {}", e)),
            };

            let test_result = tokio::runtime::Handle::current()
                .block_on(async move { mailer.test_connection().await });

            test_result.map_err(|e| format!("Connection test failed: {}", e))
        })();

        match outcome {
            Ok(b) => promise.resolve(JsValue::from_bool(b)),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_config_defaults() {
        let cfg = SmtpConfig::default();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 587);
        assert!(!cfg.secure);
        assert!(cfg.user.is_none());
        assert!(cfg.pass.is_none());
    }

    #[test]
    fn handle_round_trip() {
        let cfg = SmtpConfig {
            host: "smtp.example.com".to_string(),
            port: 465,
            secure: true,
            user: Some("u".to_string()),
            pass: Some("p".to_string()),
        };
        let h = register_handle(SmtpTransportHandle::new(cfg.clone()));
        let stored = get_handle::<SmtpTransportHandle>(h).expect("registered");
        assert_eq!(stored.config.host, cfg.host);
        assert_eq!(stored.config.port, cfg.port);
        assert!(stored.config.secure);
    }

    #[test]
    fn create_transport_with_null_returns_handle() {
        // Null/undefined config falls back to the SmtpConfig::default()
        // — `js_nodemailer_create_transport` always returns a non-zero
        // handle so the user can still queue mails (which will error
        // at SMTP-connect time).
        let h_f =
            unsafe { js_nodemailer_create_transport(f64::from_bits(JsValue::UNDEFINED.bits())) };
        assert_ne!(h_f, 0.0);
        let stored = get_handle::<SmtpTransportHandle>(h_f as i64).expect("registered");
        assert_eq!(stored.config.host, "localhost");
    }
}
