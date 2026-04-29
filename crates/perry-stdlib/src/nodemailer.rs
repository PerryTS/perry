//! Nodemailer module (nodemailer compatible)
//!
//! Native implementation of the 'nodemailer' npm package using lettre.
//! Supports sending emails via SMTP.

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use perry_runtime::{
    js_promise_new, js_string_from_bytes, JSValue, ObjectHeader, Promise, StringHeader,
};

use crate::common::{register_handle, Handle};

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
unsafe fn jsvalue_to_string(value: JSValue) -> Option<String> {
    if value.is_pointer() {
        let ptr = value.as_pointer() as *const StringHeader;
        if !ptr.is_null() {
            let len = (*ptr).byte_len as usize;
            let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data_ptr, len);
            return Some(String::from_utf8_lossy(bytes).to_string());
        }
    }
    None
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
    handle as f64
}

/// Email message options
struct MailOptions {
    from: String,
    to: String,
    subject: String,
    text: Option<String>,
    html: Option<String>,
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
    let promise = js_promise_new();

    // Parse mail options
    let mail_opts = match parse_mail_options(options) {
        Some(opts) => opts,
        None => {
            // Return rejected promise for invalid options
            crate::common::spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid mail options".to_string())
            });
            return promise;
        }
    };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::get_handle;

        if let Some(wrapper) = get_handle::<SmtpTransportHandle>(transporter_handle) {
            let config = &wrapper.config;

            // Build the transporter
            let mailer_result = if config.secure {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            };

            let mailer: AsyncSmtpTransport<Tokio1Executor> = match mailer_result {
                Ok(builder) => {
                    let mut builder = builder.port(config.port);

                    // Add credentials if provided
                    if let (Some(user), Some(pass)) = (&config.user, &config.pass) {
                        let creds = Credentials::new(user.clone(), pass.clone());
                        builder = builder.credentials(creds);
                    }

                    builder.build()
                }
                Err(e) => return Err(format!("Failed to create transport: {}", e)),
            };

            // Build the email message
            let mut email_builder = Message::builder()
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

            // Send the email
            match mailer.send(email).await {
                Ok(response) => {
                    // Return info object with messageId
                    let message_id = format!("<{}@perry>", uuid::Uuid::new_v4());
                    let info_obj = perry_runtime::js_object_alloc(0, 2);

                    // Set messageId (field 0)
                    let id_ptr = js_string_from_bytes(message_id.as_ptr(), message_id.len() as u32);
                    perry_runtime::js_object_set_field(info_obj, 0, JSValue::string_ptr(id_ptr));

                    // Set response (field 1)
                    let resp_str = format!("{:?}", response);
                    let resp_ptr = js_string_from_bytes(resp_str.as_ptr(), resp_str.len() as u32);
                    perry_runtime::js_object_set_field(info_obj, 1, JSValue::string_ptr(resp_ptr));

                    Ok(JSValue::object_ptr(info_obj as *mut u8).bits())
                }
                Err(e) => Err(format!("Failed to send email: {}", e)),
            }
        } else {
            Err("Invalid transporter handle".to_string())
        }
    });

    promise
}

/// transporter.verify() -> Promise<boolean>
///
/// Verifies that the transporter can connect to the SMTP server.
#[no_mangle]
pub unsafe extern "C" fn js_nodemailer_verify(transporter_handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        use crate::common::get_handle;

        if let Some(wrapper) = get_handle::<SmtpTransportHandle>(transporter_handle) {
            let config = &wrapper.config;

            // Try to build and test the transporter
            let mailer_result = if config.secure {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            };

            match mailer_result {
                Ok(builder) => {
                    let mut builder = builder.port(config.port);

                    if let (Some(user), Some(pass)) = (&config.user, &config.pass) {
                        let creds = Credentials::new(user.clone(), pass.clone());
                        builder = builder.credentials(creds);
                    }

                    let mailer: AsyncSmtpTransport<Tokio1Executor> = builder.build();

                    match mailer.test_connection().await {
                        Ok(true) => Ok(JSValue::bool(true).bits()),
                        Ok(false) => Ok(JSValue::bool(false).bits()),
                        Err(e) => Err(format!("Connection test failed: {}", e)),
                    }
                }
                Err(e) => Err(format!("Failed to create transport: {}", e)),
            }
        } else {
            Err("Invalid transporter handle".to_string())
        }
    });

    promise
}
